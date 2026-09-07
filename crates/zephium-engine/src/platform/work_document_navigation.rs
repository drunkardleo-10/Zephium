//! Resource-owned one-document navigation gate. No run/legacy operation join,
//! redirect allowance, reload, same-document continuation or model target.

use std::sync::{Arc, Mutex};
use zephium_agentic::ContextNavigationTarget;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Bootstrap,
    Armed,
    Loading,
    Committed,
    Ready,
    Refused,
    Retired,
}
struct State {
    phase: Phase,
    bootstrap_available: bool,
    bootstrap_id: Option<wry::NavigationId>,
    bootstrap_committed: bool,
    bootstrap_finished: bool,
    target: Option<ContextNavigationTarget>,
    native_id: Option<wry::NavigationId>,
    requested: bool,
    #[cfg(feature = "native-agentic-work-resource-probe")]
    evidence: NavigationEvidence,
}

/// Content-free milestones only; descriptive, never navigation authority.
#[cfg(feature = "native-agentic-work-resource-probe")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NavigationEvidence {
    pub bootstrap_started: bool,
    pub bootstrap_committed: bool,
    pub bootstrap_finished: bool,
    pub requested: bool,
    pub started: bool,
    pub committed: bool,
    pub finished: bool,
    pub refused: bool,
    pub last_event: Option<wry::NavigationEventPhase>,
}

/// Immutable source comes only from an admitted Work construction request.
#[derive(Clone)]
pub(crate) struct WorkDocumentNavigation(Arc<Mutex<State>>);
impl Default for WorkDocumentNavigation {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State {
            phase: Phase::Bootstrap,
            bootstrap_available: true,
            bootstrap_id: None,
            bootstrap_committed: false,
            bootstrap_finished: false,
            target: None,
            native_id: None,
            requested: false,
            #[cfg(feature = "native-agentic-work-resource-probe")]
            evidence: NavigationEvidence::default(),
        })))
    }
}

impl WorkDocumentNavigation {
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn construction_evidence(&self) -> Option<NavigationEvidence> {
        let state = self.0.lock().ok()?;
        Some(NavigationEvidence {
            bootstrap_started: state.bootstrap_id.is_some() || state.bootstrap_finished,
            bootstrap_committed: state.bootstrap_committed,
            bootstrap_finished: state.bootstrap_finished,
            requested: state.requested,
            started: state.native_id.is_some(),
            refused: state.phase == Phase::Refused,
            ..state.evidence
        })
    }
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn witness_document(&self) -> Option<wry::NavigationId> {
        let state = self.0.lock().ok()?;
        (state.phase == Phase::Ready)
            .then_some(state.native_id)
            .flatten()
    }
    /// Called once after selected-profile policy and native owner publication.
    pub(crate) fn arm(&self, target: ContextNavigationTarget) -> Result<(), ()> {
        let mut state = self.0.lock().map_err(|_| ())?;
        if state.phase != Phase::Bootstrap || !state.bootstrap_finished || state.target.is_some() {
            return Err(());
        }
        state.bootstrap_available = false;
        state.bootstrap_id = None;
        state.target = Some(target);
        state.phase = Phase::Armed;
        Ok(())
    }
    pub(crate) fn allows(&self, target: &str) -> bool {
        let Ok(mut state) = self.0.lock() else {
            return false;
        };
        if state.phase == Phase::Bootstrap && target == "about:blank" && state.bootstrap_available {
            state.bootstrap_available = false;
            return true;
        }
        if state.phase == Phase::Armed
            && !state.requested
            && state
                .target
                .as_ref()
                .is_some_and(|expected| expected.as_url().as_str() == target)
        {
            state.requested = true;
            return true;
        }
        // A refused unsolicited navigation grants no replacement document.
        false
    }
    /// Returns (actual document committed, resource owner must reconcile).
    pub(crate) fn observe(&self, event: wry::NavigationEvent) -> Result<(bool, bool), ()> {
        use wry::NavigationEventPhase as E;
        let mut state = self.0.lock().map_err(|_| ())?;
        if matches!(state.phase, Phase::Refused | Phase::Retired) {
            return Ok((false, false));
        }
        #[cfg(feature = "native-agentic-work-resource-probe")]
        {
            state.evidence.last_event = Some(event.phase);
        }
        if state.phase == Phase::Bootstrap {
            if event.url != "about:blank" {
                state.phase = Phase::Refused;
                return Ok((false, true));
            }
            return match event.phase {
                E::Started if !state.bootstrap_available && state.bootstrap_id.is_none() => {
                    state.bootstrap_id = Some(event.id);
                    Ok((false, false))
                }
                E::Committed
                    if state.bootstrap_id == Some(event.id) && !state.bootstrap_committed =>
                {
                    state.bootstrap_committed = true;
                    Ok((true, false))
                }
                E::Finished
                    if state.bootstrap_id == Some(event.id)
                        && state.bootstrap_committed
                        && !state.bootstrap_finished =>
                {
                    state.bootstrap_finished = true;
                    Ok((false, true))
                }
                _ => {
                    state.phase = Phase::Refused;
                    Ok((false, true))
                }
            };
        }
        let exact = state
            .target
            .as_ref()
            .is_some_and(|target| target.as_url().as_str() == event.url);
        match (state.phase, event.phase) {
            (Phase::Armed, E::Started) if exact && state.requested && state.native_id.is_none() => {
                state.native_id = Some(event.id);
                state.phase = Phase::Loading;
                Ok((false, false))
            }
            (Phase::Loading, E::Committed) if exact && state.native_id == Some(event.id) => {
                #[cfg(feature = "native-agentic-work-resource-probe")]
                {
                    state.evidence.committed = true;
                }
                state.phase = Phase::Committed;
                Ok((true, false))
            }
            (Phase::Committed, E::Finished) if exact && state.native_id == Some(event.id) => {
                #[cfg(feature = "native-agentic-work-resource-probe")]
                {
                    state.evidence.finished = true;
                }
                state.phase = Phase::Ready;
                Ok((false, true))
            }
            _ => {
                state.phase = Phase::Refused;
                Ok((false, true))
            }
        }
    }
    /// Initial load KVO does not establish readiness. Any later location event
    /// invalidates this fixed-document slice, even if its URL is unchanged.
    pub(crate) fn location_changed(&self) -> Result<bool, ()> {
        let mut state = self.0.lock().map_err(|_| ())?;
        if state.phase == Phase::Ready {
            state.phase = Phase::Refused;
            return Ok(true);
        }
        Ok(false)
    }
    pub(crate) fn refuse(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.phase = Phase::Refused;
        }
    }
    pub(crate) fn ready(&self, current: Option<&str>) -> bool {
        self.0.lock().is_ok_and(|state| {
            state.phase == Phase::Ready
                && state
                    .target
                    .as_ref()
                    .is_some_and(|target| Some(target.as_url().as_str()) == current)
        })
    }
    pub(crate) fn failed(&self) -> bool {
        self.0.lock().map_or(true, |state| {
            matches!(state.phase, Phase::Refused | Phase::Retired)
        })
    }
    pub(crate) fn bootstrap_ready(&self) -> bool {
        self.0
            .lock()
            .is_ok_and(|state| state.phase == Phase::Bootstrap && state.bootstrap_finished)
    }
    pub(crate) fn retire(&self) -> bool {
        match self.0.lock() {
            Ok(mut state) => {
                state.phase = Phase::Retired;
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wry::NavigationEventPhase as E;
    const URL: &str = "https://example.test/frozen";
    #[cfg(feature = "native-agentic-work-resource-probe")]
    #[test]
    fn construction_milestones_distinguish_waits_and_freeze_first_navigation_refusal() {
        let empty = WorkDocumentNavigation::default()
            .construction_evidence()
            .unwrap();
        assert_eq!(empty, NavigationEvidence::default());
        let gate = armed();
        let bootstrap = gate.construction_evidence().unwrap();
        assert!(
            bootstrap.bootstrap_started
                && bootstrap.bootstrap_committed
                && bootstrap.bootstrap_finished
                && bootstrap.requested
        );
        assert!(
            !bootstrap.started && !bootstrap.committed && !bootstrap.finished && !bootstrap.refused
        );
        gate.observe(event(1, E::Started, URL)).unwrap();
        assert!(gate.construction_evidence().unwrap().started);
        gate.observe(event(1, E::Committed, URL)).unwrap();
        let committed = gate.construction_evidence().unwrap();
        assert!(committed.committed && !committed.finished && !committed.refused);
        gate.observe(event(1, E::Redirected, URL)).unwrap();
        let refused = gate.construction_evidence().unwrap();
        assert!(refused.refused && refused.committed && !refused.finished);
        assert_eq!(refused.last_event, Some(E::Redirected));
        gate.observe(event(1, E::Finished, URL)).unwrap();
        assert_eq!(gate.construction_evidence(), Some(refused));
        assert!(!gate.ready(Some(URL)));
    }
    fn event(id: u64, phase: E, url: &str) -> wry::NavigationEvent {
        wry::NavigationEvent {
            id: wry::NavigationId::from_raw(id),
            phase,
            url: url.into(),
        }
    }
    fn armed() -> WorkDocumentNavigation {
        let gate = WorkDocumentNavigation::default();
        assert!(gate.allows("about:blank"));
        for phase in [E::Started, E::Committed, E::Finished] {
            gate.observe(event(99, phase, "about:blank")).unwrap();
        }
        gate.arm(ContextNavigationTarget::parse(URL).unwrap())
            .unwrap();
        assert!(gate.allows(URL));
        gate
    }
    #[test]
    fn exact_frozen_source_requires_start_commit_finish_and_never_rearms() {
        let gate = armed();
        assert!(!gate.allows(URL));
        assert!(!gate.ready(Some(URL)));
        assert_eq!(gate.observe(event(1, E::Started, URL)), Ok((false, false)));
        assert_eq!(gate.observe(event(1, E::Committed, URL)), Ok((true, false)));
        assert!(!gate.ready(Some(URL)));
        assert_eq!(gate.observe(event(1, E::Finished, URL)), Ok((false, true)));
        assert!(gate.ready(Some(URL)));
        assert!(!gate.ready(Some("https://example.test/other")));
        assert!(gate
            .arm(ContextNavigationTarget::parse(URL).unwrap())
            .is_err());
        assert!(!gate.allows(URL));
    }
    #[test]
    fn redirects_substitution_foreign_events_reloads_and_same_document_reveal_refuse() {
        for hostile in [
            event(1, E::Redirected, URL),
            event(1, E::Committed, "https://example.test/other"),
            event(2, E::Committed, URL),
            event(1, E::Finished, URL),
            event(1, E::Failed, URL),
        ] {
            let gate = armed();
            gate.observe(event(1, E::Started, URL)).unwrap();
            assert_eq!(gate.observe(hostile), Ok((false, true)));
            assert!(gate.failed());
            assert!(!gate.ready(Some(URL)));
        }
        for reload in [false, true] {
            let gate = armed();
            for phase in [E::Started, E::Committed, E::Finished] {
                gate.observe(event(1, phase, URL)).unwrap();
            }
            if reload {
                gate.observe(event(2, E::Started, URL)).unwrap();
            } else {
                assert_eq!(gate.location_changed(), Ok(true));
            }
            assert!(gate.failed());
        }
    }
    #[test]
    fn about_blank_is_construction_only_and_retirement_never_reopens_it() {
        let gate = WorkDocumentNavigation::default();
        assert!(!gate.allows(URL));
        assert!(gate.allows("about:blank"));
        assert!(!gate.allows("about:blank"));
        gate.observe(event(1, E::Started, "about:blank")).unwrap();
        assert_eq!(
            gate.observe(event(1, E::Committed, "about:blank")),
            Ok((true, false))
        );
        gate.observe(event(1, E::Finished, "about:blank")).unwrap();
        gate.arm(ContextNavigationTarget::parse(URL).unwrap())
            .unwrap();
        assert!(!gate.allows("about:blank"));
        assert!(gate.retire());
        assert!(!gate.allows(URL));
        assert!(gate.failed());
    }
}
