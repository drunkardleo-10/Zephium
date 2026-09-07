//! Release-excluded retention selection; original scoped ownership is unchanged.
use super::*;
use std::io::Write as _;

impl RetainedWork {
    pub(in crate::work_resources) fn public_retention_diagnostic(&self) {
        let phase = self
            .owner
            .shared
            .lock_rows()
            .ok()
            .and_then(|rows| rows.phase(&self.resource).ok());
        let resource = self.owner.shared.resource(&self.resource).ok();
        let healthy = resource
            .as_ref()
            .is_some_and(|row| self.owner.shared.current(row).is_ok());
        let idle = resource.as_ref().is_some_and(|row| {
            row.flights.load(Ordering::Acquire) == 0 && row.reads.load(Ordering::Acquire) == 0
        });
        let reusable = resource
            .as_ref()
            .is_some_and(|row| row.reusable.load(Ordering::Acquire));
        let _ = writeln!(std::io::stdout().lock(), "work-retained-product-resource: phase={phase:?} original_row_healthy={healthy} idle={idle} reusable={reusable} destruction_started={} content=redacted", self.destruction.is_some() || self.destroyed);
    }
}

impl StagedActor {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::work_resources) fn for_public_qualification(
        input: AgentWorkRunInput,
        browser: Box<dyn AgentWorkRetainedBrowser>,
        config: AgentRuntimeConfig,
        provider: AgentProviderTransportConfig,
        credential: AgentProviderCredential,
        audit: Arc<dyn AgentAuditPort>,
        task: Box<dyn AgentWorkTask>,
    ) -> Result<Self, AgentWorkFailure> {
        let lease = browser.binding().lease().clone();
        let (controller, handle, scope) = AgentWorkRetainedController::try_new_for_public_probe(
            input,
            browser,
            provider,
            credential,
            audit.clone(),
            task,
        )?;
        let deadline = controller.deadline()?;
        Ok(Self {
            controller: Box::new(controller),
            handle,
            scope,
            lease,
            audit,
            config,
            deadline,
        })
    }
}
