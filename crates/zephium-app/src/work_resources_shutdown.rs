//! Original retained registry and original port enter the unchanged global
//! native seal/audit protocol. Local absence is not a native-zero substitute.

use super::*;
use std::time::Instant;

pub(super) struct RetainedNativeShutdown {
    shared: Arc<Shared>,
    coordinator: Option<AgentNativeShutdownCoordinator>,
    proof: Option<AgentNativeShutdownProof>,
    failed: bool,
    retry_at: Option<Instant>,
}

impl RetainedNativeShutdown {
    pub(super) fn new(owner: &WorkResourceOwner) -> Result<Self, Refusal> {
        #[cfg(feature = "work-runtime")]
        crate::work_commands::shutdown();
        if !owner.locally_retired() {
            return Err(Refusal::Busy);
        }
        let coordinator = owner.shared.lock_rows()?.begin_native_shutdown()?;
        Ok(Self {
            shared: owner.shared.clone(),
            coordinator: Some(coordinator),
            proof: None,
            failed: false,
            retry_at: None,
        })
    }

    /// Runs at most one native dispatch per application poll. All eight audit
    /// attempts, replay/type checks and zero-count rules belong to the same
    /// core coordinator used by complete-browser shutdown.
    pub(super) fn poll(&mut self) -> Result<bool, Refusal> {
        if self.failed || !self.shared.global_current() {
            return Err(Refusal::Uncertain);
        }
        if self.proof.is_some() {
            return Ok(true);
        }
        let coordinator = self.coordinator.as_mut().ok_or(Refusal::Uncertain)?;
        let audit = ContextResourceAuditId::new(u64::from(coordinator.status().attempts()) + 1)
            .ok_or(Refusal::Uncertain)?;
        let result = match coordinator.status().stage() {
            AgentNativeShutdownStage::ReadyToSeal => coordinator
                .begin_port_seal(audit)
                .and_then(|()| {
                    coordinator.account_port_seal(audit, self.shared.port.seal_for_shutdown(audit))
                })
                .map(|_| false),
            AgentNativeShutdownStage::ResourceAuditRequired => {
                let retry_at = *self.retry_at.get_or_insert_with(|| {
                    Instant::now()
                        + agent_native_shutdown_retry_delay(coordinator.status().attempts())
                });
                if Instant::now() < retry_at {
                    return Ok(false);
                }
                self.retry_at = None;
                coordinator
                    .begin_resource_audit(audit)
                    .and_then(|()| {
                        coordinator
                            .account_resource_audit(audit, self.shared.port.audit_resources(audit))
                    })
                    .map(|_| false)
            }
            AgentNativeShutdownStage::ZeroProven => {
                let coordinator = self.coordinator.take().ok_or(Refusal::Uncertain)?;
                match coordinator.finish() {
                    Ok(proof) => {
                        self.proof = Some(proof);
                        return Ok(true);
                    }
                    Err(refusal) => {
                        self.coordinator = Some(refusal.into_coordinator());
                        self.failed = true;
                        return Err(Refusal::Uncertain);
                    }
                }
            }
            AgentNativeShutdownStage::Exhausted => {
                self.failed = true;
                return Err(Refusal::Uncertain);
            }
            AgentNativeShutdownStage::ShutdownAuditPending
            | AgentNativeShutdownStage::ResourceAuditPending => return Ok(false),
            _ => {
                self.failed = true;
                return Err(Refusal::Uncertain);
            }
        };
        result.map_err(|_| {
            self.failed = true;
            Refusal::Uncertain
        })?;
        // Synchronous dispatch refusal has no event wake. Publish its retry
        // deadline in this same poll, without issuing a second native call.
        if coordinator.status().stage() == AgentNativeShutdownStage::ResourceAuditRequired {
            self.retry_at = Some(
                Instant::now() + agent_native_shutdown_retry_delay(coordinator.status().attempts()),
            );
        } else if coordinator.status().stage() == AgentNativeShutdownStage::Exhausted {
            // The final synchronous refusal has neither a callback nor another
            // retry deadline. Return its terminal failure in this same poll.
            self.failed = true;
            return Err(Refusal::Uncertain);
        }
        Ok(false)
    }

    /// An unexpected terminal is returned losslessly. Only the original owner's
    /// global lane may call this; no actor/runtime callback is rebound here.
    pub(super) fn settle(
        &mut self,
        event: ContextNativeEvent,
    ) -> Result<(), Box<ContextNativeEvent>> {
        let Some(coordinator) = self.coordinator.as_mut().filter(|_| !self.failed) else {
            return Err(Box::new(event));
        };
        let result = match &event {
            ContextNativeEvent::ShutdownAuditSettled(settlement) => {
                coordinator.settle_shutdown_audit(*settlement)
            }
            ContextNativeEvent::ResourceAuditSettled(settlement) => {
                coordinator.settle_resource_audit(*settlement)
            }
            _ => {
                self.failed = true;
                return Err(Box::new(event));
            }
        };
        match result {
            Ok(_) => Ok(()),
            Err(_) => {
                self.failed = true;
                Err(Box::new(event))
            }
        }
    }

    pub(super) fn next_deadline(&self) -> Option<Instant> {
        self.retry_at.filter(|_| !self.failed)
    }
}
