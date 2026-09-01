//! Platform-neutral native navigation gate for owned agent contexts.
//!
//! Wry exposes the same identity-bearing navigation event contract on WebKit
//! and WebView2. Keeping the authority state machine here prevents the two
//! platform adapters from drifting on bootstrap, redirect, timeout, recovery,
//! and renderer-loss semantics.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use zephium_agentic::{
    ContextNavigationTarget, ContextOperationJoin, ContextOperationKind, ContextPortFailure,
};

/// Closed committed target for one exact native page load.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AgentNavigationCommit {
    /// A validated HTTP(S) document committed.
    Web(ContextNavigationTarget),
    /// The construction/recovery-only empty document committed.
    Bootstrap,
}

/// One exact terminal native observation for a shell-requested page load.
pub(crate) struct AgentNavigationTerminal {
    operation: ContextOperationJoin,
    outcome: Result<AgentNavigationCommit, ContextPortFailure>,
}

impl AgentNavigationTerminal {
    pub(crate) const fn operation(&self) -> ContextOperationJoin {
        self.operation
    }

    pub(crate) fn into_outcome(self) -> Result<AgentNavigationCommit, ContextPortFailure> {
        self.outcome
    }
}

pub(crate) struct AgentNavigationObservation {
    document_committed: bool,
    terminal: Option<AgentNavigationTerminal>,
}

impl AgentNavigationObservation {
    const fn none() -> Self {
        Self {
            document_committed: false,
            terminal: None,
        }
    }

    const fn document_committed() -> Self {
        Self {
            document_committed: true,
            terminal: None,
        }
    }

    fn terminal(_document_committed: bool, terminal: AgentNavigationTerminal) -> Self {
        Self {
            document_committed: _document_committed,
            terminal: Some(terminal),
        }
    }

    pub(crate) const fn did_commit_document(&self) -> bool {
        self.document_committed
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn into_terminal(self) -> Option<AgentNavigationTerminal> {
        self.terminal
    }

    #[cfg(test)]
    pub(crate) fn is_none(&self) -> bool {
        self.terminal.is_none()
    }

    #[cfg(test)]
    pub(crate) fn expect(self, message: &str) -> AgentNavigationTerminal {
        self.terminal.expect(message)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use zephium_agentic::{
        ContextCapabilities, ContextId, ContextIdentity, ContextOperationId, ContextRegistry,
        ContextRunId,
    };
    use zephium_core::ids::ProfileId;

    fn operation(
        kind: zephium_agentic::ContextOperationKind,
    ) -> zephium_agentic::ContextOperationJoin {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(44),
            zephium_agentic::ContextKind::Owned,
        );
        let capability = match kind {
            zephium_agentic::ContextOperationKind::Navigate => {
                zephium_agentic::ContextCapability::Navigate
            }
            zephium_agentic::ContextOperationKind::Recover => {
                zephium_agentic::ContextCapability::Recover
            }
            _ => panic!("test helper supports only page-load operations"),
        };
        let capabilities =
            ContextCapabilities::try_new(zephium_agentic::ContextKind::Owned, &[capability])
                .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(
                identity.id(),
                construction,
                zephium_agentic::ContextSettlement::Applied,
            )
            .expect("settle construction");
        match kind {
            zephium_agentic::ContextOperationKind::Navigate => registry
                .begin_navigation(
                    identity.id(),
                    ContextOperationId::new(2).expect("operation"),
                )
                .expect("navigation"),
            zephium_agentic::ContextOperationKind::Recover => {
                let prior = registry.join(identity.id()).expect("join");
                registry
                    .renderer_lost(identity.id(), prior)
                    .expect("renderer loss");
                registry
                    .begin_recovery(
                        identity.id(),
                        ContextOperationId::new(2).expect("operation"),
                    )
                    .expect("recovery")
            }
            _ => unreachable!(),
        }
    }

    fn event(id: u64, phase: wry::NavigationEventPhase, url: &str) -> wry::NavigationEvent {
        wry::NavigationEvent {
            id: wry::NavigationId::from_raw(id),
            phase,
            url: url.to_owned(),
        }
    }

    #[test]
    fn bootstrap_is_one_shot_and_never_emits_a_shell_terminal() {
        let gate = super::AgentNavigationController::default();
        assert!(gate.allows("about:blank"));
        assert!(!gate.allows("about:blank"));
        assert!(!gate.allows("https://example.test/"));
        assert!(gate
            .observe(event(70, wry::NavigationEventPhase::Started, "about:blank",))
            .expect("bootstrap start")
            .is_none());
        let commit = gate
            .observe(event(
                70,
                wry::NavigationEventPhase::Committed,
                "about:blank",
            ))
            .expect("bootstrap commit");
        assert!(commit.did_commit_document());
        assert!(commit.is_none());
    }

    #[test]
    fn armed_navigation_matches_exact_target_and_native_id_once() {
        let gate = super::AgentNavigationController::default();
        let operation = operation(zephium_agentic::ContextOperationKind::Navigate);
        let target = zephium_agentic::ContextNavigationTarget::parse("https://example.test/exact")
            .expect("target");
        gate.arm(operation, target.clone(), Arc::new(AtomicBool::new(false)))
            .expect("arm");
        assert!(gate.allows(target.as_url().as_str()));
        assert!(!gate.allows("https://example.test/redirect"));
        assert!(gate
            .observe(event(
                8,
                wry::NavigationEventPhase::Started,
                target.as_url().as_str(),
            ))
            .expect("start")
            .is_none());
        assert!(gate
            .observe(event(
                9,
                wry::NavigationEventPhase::Committed,
                target.as_url().as_str(),
            ))
            .expect("wrong native id")
            .is_none());
        let terminal = gate
            .observe(event(
                8,
                wry::NavigationEventPhase::Committed,
                target.as_url().as_str(),
            ))
            .expect("commit")
            .expect("terminal");
        assert_eq!(terminal.operation(), operation);
        assert_eq!(
            terminal.into_outcome(),
            Ok(super::AgentNavigationCommit::Web(target))
        );
        assert!(gate
            .observe(event(
                8,
                wry::NavigationEventPhase::Failed,
                "https://example.test/exact",
            ))
            .expect("late failure")
            .is_none());
    }

    #[test]
    fn timeout_claim_blocks_a_late_native_commit() {
        let gate = super::AgentNavigationController::default();
        let operation = operation(zephium_agentic::ContextOperationKind::Navigate);
        let target = zephium_agentic::ContextNavigationTarget::parse("https://example.test/late")
            .expect("target");
        let claimed = Arc::new(AtomicBool::new(false));
        gate.arm(operation, target.clone(), claimed.clone())
            .expect("arm");
        gate.observe(event(
            10,
            wry::NavigationEventPhase::Started,
            target.as_url().as_str(),
        ))
        .expect("start");
        assert!(!claimed.swap(true, Ordering::AcqRel));
        assert!(gate
            .observe(event(
                10,
                wry::NavigationEventPhase::Committed,
                target.as_url().as_str(),
            ))
            .expect("late commit")
            .is_none());
        assert!(gate.disarm(operation));
    }

    #[test]
    fn recovery_requires_loss_and_failed_recovery_restores_loss() {
        let gate = super::AgentNavigationController::default();
        let operation = operation(zephium_agentic::ContextOperationKind::Recover);
        let target =
            zephium_agentic::ContextNavigationTarget::parse("https://example.test/recover")
                .expect("target");
        assert!(gate
            .arm_recovery(
                operation,
                Some(target.clone()),
                Arc::new(AtomicBool::new(false)),
            )
            .is_err());
        assert_eq!(gate.claim_renderer_loss(), Ok(true));
        assert_eq!(gate.claim_renderer_loss(), Ok(false));
        gate.arm_recovery(
            operation,
            Some(target.clone()),
            Arc::new(AtomicBool::new(false)),
        )
        .expect("recovery arm");
        assert!(gate.allows(target.as_url().as_str()));
        assert!(!gate.allows("https://example.test/redirect"));
        gate.observe(event(
            11,
            wry::NavigationEventPhase::Started,
            target.as_url().as_str(),
        ))
        .expect("start");
        let terminal = gate
            .observe(event(
                11,
                wry::NavigationEventPhase::Failed,
                target.as_url().as_str(),
            ))
            .expect("failure")
            .expect("terminal");
        assert_eq!(
            terminal.into_outcome(),
            Err(zephium_agentic::ContextPortFailure::NativeRefused)
        );
        assert!(gate.settle_recovery(operation, false));
        assert!(gate.matches_for_audit(None, true));
    }
}

#[derive(Clone)]
enum AgentLoadExpectation {
    Web(ContextNavigationTarget),
    Bootstrap,
}

impl AgentLoadExpectation {
    fn matches(&self, candidate: &str) -> bool {
        match self {
            Self::Web(expected) => ContextNavigationTarget::parse(candidate)
                .ok()
                .is_some_and(|candidate| candidate == *expected),
            Self::Bootstrap => candidate == "about:blank",
        }
    }

    fn commit(&self, candidate: &str) -> Result<AgentNavigationCommit, ContextPortFailure> {
        match self {
            Self::Web(expected) => ContextNavigationTarget::parse(candidate)
                .ok()
                .filter(|candidate| candidate == expected)
                .map(AgentNavigationCommit::Web)
                .ok_or(ContextPortFailure::NativeRefused),
            Self::Bootstrap if candidate == "about:blank" => Ok(AgentNavigationCommit::Bootstrap),
            Self::Bootstrap => Err(ContextPortFailure::NativeRefused),
        }
    }
}

struct AgentNavigationArm {
    operation: ContextOperationJoin,
    expected: AgentLoadExpectation,
    terminal_claimed: Arc<AtomicBool>,
    native_id: Option<wry::NavigationId>,
}

struct AgentNavigationState {
    bootstrap_available: bool,
    bootstrap_pending: bool,
    bootstrap_native_id: Option<wry::NavigationId>,
    renderer_lost: bool,
    armed: Option<AgentNavigationArm>,
}

impl Default for AgentNavigationState {
    fn default() -> Self {
        Self {
            bootstrap_available: true,
            bootstrap_pending: false,
            bootstrap_native_id: None,
            renderer_lost: false,
            armed: None,
        }
    }
}

/// Exact, one-at-a-time navigation policy shared with Wry's native delegates.
///
/// Unarmed page navigation is denied. The one bootstrap `about:blank` permit
/// exists solely for construction and is permanently consumed or sealed by
/// the first shell arm. Redirects are deliberately denied until the policy
/// port can authorize their exact destination.
#[derive(Clone, Default)]
pub(crate) struct AgentNavigationController {
    state: Arc<Mutex<AgentNavigationState>>,
}

impl AgentNavigationController {
    pub(crate) fn arm(
        &self,
        operation: ContextOperationJoin,
        target: ContextNavigationTarget,
        terminal_claimed: Arc<AtomicBool>,
    ) -> Result<(), ()> {
        if operation.kind() != ContextOperationKind::Navigate {
            return Err(());
        }
        self.arm_exact(
            operation,
            AgentLoadExpectation::Web(target),
            terminal_claimed,
            false,
        )
    }

    /// Arms the sole page load permitted after exact renderer loss.
    pub(crate) fn arm_recovery(
        &self,
        operation: ContextOperationJoin,
        target: Option<ContextNavigationTarget>,
        terminal_claimed: Arc<AtomicBool>,
    ) -> Result<(), ()> {
        if operation.kind() != ContextOperationKind::Recover {
            return Err(());
        }
        self.arm_exact(
            operation,
            target.map_or(AgentLoadExpectation::Bootstrap, AgentLoadExpectation::Web),
            terminal_claimed,
            true,
        )
    }

    fn arm_exact(
        &self,
        operation: ContextOperationJoin,
        expected: AgentLoadExpectation,
        terminal_claimed: Arc<AtomicBool>,
        requires_renderer_loss: bool,
    ) -> Result<(), ()> {
        if terminal_claimed.load(Ordering::Acquire) {
            return Err(());
        }
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost != requires_renderer_loss || state.armed.is_some() {
            return Err(());
        }
        state.bootstrap_available = false;
        state.bootstrap_pending = false;
        state.bootstrap_native_id = None;
        state.renderer_lost = false;
        state.armed = Some(AgentNavigationArm {
            operation,
            expected,
            terminal_claimed,
            native_id: None,
        });
        Ok(())
    }

    pub(crate) fn disarm(&self, operation: ContextOperationJoin) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state
            .armed
            .as_ref()
            .is_some_and(|armed| armed.operation == operation)
        {
            state.armed = None;
            true
        } else {
            false
        }
    }

    /// Retires one exact recovery arm and restores loss state on refusal.
    pub(crate) fn settle_recovery(&self, operation: ContextOperationJoin, applied: bool) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if operation.kind() != ContextOperationKind::Recover
            || !state
                .armed
                .as_ref()
                .is_some_and(|armed| armed.operation == operation)
        {
            return false;
        }
        state.armed = None;
        // A termination callback can win after the commit callback has
        // claimed the terminal but before the host consumes either queued
        // callback. Successful settlement must not erase that newer loss.
        if !applied {
            state.renderer_lost = true;
        }
        true
    }

    pub(crate) fn allows(&self, candidate: &str) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.renderer_lost {
            return false;
        }
        if let Some(armed) = state.armed.as_ref() {
            if armed.terminal_claimed.load(Ordering::Acquire) {
                return false;
            }
            return armed.expected.matches(candidate);
        }
        if state.bootstrap_available && candidate == "about:blank" {
            state.bootstrap_available = false;
            state.bootstrap_pending = true;
            return true;
        }
        false
    }

    pub(crate) fn observe(
        &self,
        event: wry::NavigationEvent,
    ) -> Result<AgentNavigationObservation, ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost {
            return Ok(AgentNavigationObservation::none());
        }
        let Some(armed) = state.armed.as_mut() else {
            if !state.bootstrap_pending {
                return Ok(AgentNavigationObservation::none());
            }
            if event.phase == wry::NavigationEventPhase::Started {
                if event.url == "about:blank" && state.bootstrap_native_id.is_none() {
                    state.bootstrap_native_id = Some(event.id);
                }
                return Ok(AgentNavigationObservation::none());
            }
            if !matches!(
                event.phase,
                wry::NavigationEventPhase::Committed | wry::NavigationEventPhase::Failed
            ) || state.bootstrap_native_id != Some(event.id)
            {
                return Ok(AgentNavigationObservation::none());
            }
            state.bootstrap_pending = false;
            state.bootstrap_native_id = None;
            return match event.phase {
                wry::NavigationEventPhase::Committed if event.url == "about:blank" => {
                    Ok(AgentNavigationObservation::document_committed())
                }
                wry::NavigationEventPhase::Failed => Ok(AgentNavigationObservation::none()),
                wry::NavigationEventPhase::Committed => Err(()),
                wry::NavigationEventPhase::Started
                | wry::NavigationEventPhase::Redirected
                | wry::NavigationEventPhase::Finished => Ok(AgentNavigationObservation::none()),
            };
        };
        if event.phase == wry::NavigationEventPhase::Started {
            let matches_target = armed.expected.matches(&event.url);
            if matches_target && armed.native_id.is_none() {
                armed.native_id = Some(event.id);
            }
            return Ok(AgentNavigationObservation::none());
        }
        if !matches!(
            event.phase,
            wry::NavigationEventPhase::Committed | wry::NavigationEventPhase::Failed
        ) || armed.native_id != Some(event.id)
        {
            return Ok(AgentNavigationObservation::none());
        }
        if armed
            .terminal_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(AgentNavigationObservation::none());
        }
        let operation = armed.operation;
        let expected = armed.expected.clone();
        let outcome = match event.phase {
            wry::NavigationEventPhase::Committed => expected.commit(&event.url),
            wry::NavigationEventPhase::Failed => Err(ContextPortFailure::NativeRefused),
            wry::NavigationEventPhase::Started
            | wry::NavigationEventPhase::Redirected
            | wry::NavigationEventPhase::Finished => return Ok(AgentNavigationObservation::none()),
        };
        let document_committed = outcome.is_ok();
        Ok(AgentNavigationObservation::terminal(
            document_committed,
            AgentNavigationTerminal { operation, outcome },
        ))
    }

    pub(crate) fn claim_renderer_loss(&self) -> Result<bool, ()> {
        let mut state = self.state.lock().map_err(|_| ())?;
        if state.renderer_lost {
            return Ok(false);
        }
        state.renderer_lost = true;
        if let Some(armed) = state.armed.as_ref() {
            armed.terminal_claimed.store(true, Ordering::Release);
        }
        Ok(true)
    }

    pub(crate) fn matches_for_audit(
        &self,
        pending: Option<ContextOperationJoin>,
        renderer_lost: bool,
    ) -> bool {
        self.state.lock().is_ok_and(|state| {
            state.renderer_lost == renderer_lost
                && state.armed.as_ref().map(|armed| armed.operation) == pending
        })
    }
}
