//! Resource-owned document gate. Construction and explicit policy-bound
//! successor loads share one gate; unsolicited transitions, redirects and
//! same-document continuation never acquire authority from native callbacks.

use std::sync::{Arc, Mutex};
use zephium_agentic::{
    ContextJoin, ContextNavigationRequest, ContextNavigationTarget, ContextOperationJoin,
    ContextPortFailure,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Bootstrap,
    Armed,
    Loading,
    Committed,
    Finalizing,
    Sampling,
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
    effective: Option<ContextNavigationTarget>,
    policy: zephium_agentic::WorkBrowserDocumentPolicy,
    location_revision: u64,
    native_id: Option<wry::NavigationId>,
    requested: bool,
    navigation_epoch: u64,
    operation: Option<ContextOperationJoin>,
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
    pub location_callback_after_commit_before_ready: bool,
}

/// Component relations only: no URL, digest, query key/value, or native handle.
/// Equality here is diagnostic; only the original exact gate admits a document.
#[cfg(feature = "native-agentic-work-resource-probe")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentDocumentEvidence {
    MissingNativeUrl,
    MissingAbsoluteString,
    Utf16Limit,
    Utf8Limit,
    InvalidUrl,
    Compared {
        raw_equal: bool,
        canonical_equal: bool,
        scheme_equal: bool,
        host_equal: bool,
        port_equal: bool,
        path_equal: bool,
        query_equal: bool,
        fragment_equal: bool,
        credentials_equal: bool,
        query_present: bool,
        query_empty: bool,
        fragment_present: bool,
        credentials_present: bool,
    },
}
#[cfg(feature = "native-agentic-work-resource-probe")]
impl CurrentDocumentEvidence {
    pub(crate) fn compare(expected: &ContextNavigationTarget, current: &str) -> Self {
        let Ok(actual) = url::Url::parse(current) else {
            return Self::InvalidUrl;
        };
        let expected = expected.as_url();
        Self::Compared {
            raw_equal: expected.as_str() == current,
            canonical_equal: expected == &actual,
            scheme_equal: expected.scheme() == actual.scheme(),
            host_equal: expected.host() == actual.host(),
            port_equal: expected.port_or_known_default() == actual.port_or_known_default(),
            path_equal: expected.path() == actual.path(),
            query_equal: expected.query() == actual.query(),
            fragment_equal: expected.fragment() == actual.fragment(),
            credentials_equal: expected.username() == actual.username()
                && expected.password() == actual.password(),
            query_present: actual.query().is_some(),
            query_empty: actual.query() == Some(""),
            fragment_present: actual.fragment().is_some(),
            credentials_present: !actual.username().is_empty() || actual.password().is_some(),
        }
    }
}

/// One resource's document authority. Construction fixes the initial source;
/// only an admitted execution-lease operation can retire it for a successor.
#[derive(Clone)]
pub(crate) struct WorkDocumentNavigation(Arc<Mutex<State>>);
/// Private, descriptive stamp of the exact committed native document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkDocumentStamp {
    native_id: wry::NavigationId,
    epoch: u64,
}
#[cfg(test)]
impl WorkDocumentStamp {
    pub(crate) fn for_test(epoch: u64) -> Self {
        Self {
            native_id: wry::NavigationId::from_raw(1),
            epoch,
        }
    }
}
impl Default for WorkDocumentNavigation {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(State {
            phase: Phase::Bootstrap,
            bootstrap_available: true,
            bootstrap_id: None,
            bootstrap_committed: false,
            bootstrap_finished: false,
            target: None,
            effective: None,
            policy: zephium_agentic::WorkBrowserDocumentPolicy::Exact,
            location_revision: 0,
            native_id: None,
            requested: false,
            navigation_epoch: 1,
            operation: None,
            #[cfg(feature = "native-agentic-work-resource-probe")]
            evidence: NavigationEvidence::default(),
        })))
    }
}

impl WorkDocumentNavigation {
    pub(crate) fn observation_stamp(&self, context: ContextJoin) -> Option<WorkDocumentStamp> {
        let state = self.0.lock().ok()?;
        (state.phase == Phase::Ready
            && state.operation.is_none()
            && state.navigation_epoch == context.navigation_epoch().get()
            && state.navigation_epoch == context.frame_generation().get())
        .then_some(WorkDocumentStamp {
            native_id: state.native_id?,
            epoch: state.navigation_epoch,
        })
    }
    /// Reuse the exact resource gate only under an admitted successor operation.
    /// The source was freshly observed under the same execution lease; the host
    /// additionally checks the current native URL and exact ingress reservation.
    pub(crate) fn arm_successor(
        &self,
        source: ContextJoin,
        request: &ContextNavigationRequest,
    ) -> Result<(), ()> {
        let mut state = self.0.lock().map_err(|_| ())?;
        let next = request.operation().context();
        if state.phase != Phase::Ready
            || state.operation.is_some()
            || state.native_id.is_none()
            || state.navigation_epoch != source.navigation_epoch().get()
            || source.identity() != next.identity()
            || source.context_generation() != next.context_generation()
            || source.cancellation_generation() != next.cancellation_generation()
            || source.frame() != zephium_agentic::FrameId::MAIN
            || next.frame() != zephium_agentic::FrameId::MAIN
            || source.navigation_epoch().get().checked_add(1) != Some(next.navigation_epoch().get())
            || source.frame_generation().get().checked_add(1) != Some(next.frame_generation().get())
            || request.redirect_policy().is_some()
        {
            return Err(());
        }
        state.operation = Some(request.operation());
        state.target = Some(request.target().clone());
        state.effective = None;
        state.policy = zephium_agentic::WorkBrowserDocumentPolicy::Exact;
        state.native_id = None;
        state.requested = false;
        state.phase = Phase::Armed;
        Ok(())
    }
    /// One exact terminal after original native event callbacks have returned.
    pub(crate) fn take_successor_terminal(
        &self,
    ) -> Option<(
        ContextOperationJoin,
        Result<ContextNavigationTarget, ContextPortFailure>,
    )> {
        let mut state = self.0.lock().ok()?;
        let outcome = match state.phase {
            Phase::Ready => Ok(state.effective.clone()?),
            Phase::Refused | Phase::Retired => Err(ContextPortFailure::NativeRefused),
            _ => return None,
        };
        let operation = state.operation.take()?;
        if outcome.is_ok() {
            state.navigation_epoch = operation.context().navigation_epoch().get();
        }
        Some((operation, outcome))
    }
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
    #[cfg(test)]
    pub(crate) fn arm(&self, target: ContextNavigationTarget) -> Result<(), ()> {
        self.arm_with_policy(target, zephium_agentic::WorkBrowserDocumentPolicy::Exact)
    }
    pub(crate) fn arm_with_policy(
        &self,
        target: ContextNavigationTarget,
        policy: zephium_agentic::WorkBrowserDocumentPolicy,
    ) -> Result<(), ()> {
        let mut state = self.0.lock().map_err(|_| ())?;
        if state.phase != Phase::Bootstrap
            || !state.bootstrap_finished
            || state.target.is_some()
            || !policy.admits_request(&target)
        {
            return Err(());
        }
        state.bootstrap_available = false;
        state.bootstrap_id = None;
        state.target = Some(target);
        state.policy = policy;
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
                state.phase = if state.policy == zephium_agentic::WorkBrowserDocumentPolicy::Exact {
                    state.effective = state.target.clone();
                    Phase::Ready
                } else {
                    Phase::Finalizing
                };
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
        #[cfg(feature = "native-agentic-work-resource-probe")]
        if state.phase == Phase::Committed {
            // This observer also includes back/forward availability. It is
            // evidence of a callback, not proof of a URL or History API change.
            state.evidence.location_callback_after_commit_before_ready = true;
        }
        if state.phase == Phase::Ready {
            state.phase = Phase::Refused;
            return Ok(true);
        }
        if matches!(state.phase, Phase::Finalizing | Phase::Sampling) {
            let Some(revision) = state.location_revision.checked_add(1) else {
                state.phase = Phase::Refused;
                return Err(());
            };
            state.location_revision = revision;
        }
        Ok(false)
    }
    pub(crate) fn finalization_pending(&self) -> bool {
        self.0
            .lock()
            .is_ok_and(|state| state.phase == Phase::Finalizing)
    }
    /// One native sample outside the lock. A callback racing the getter closes
    /// this attempt; it never authorizes a retry or a second location sample.
    pub(crate) fn finalize(
        &self,
        sample: impl FnOnce() -> Option<String>,
    ) -> Result<ContextNavigationTarget, ()> {
        let revision = {
            let mut state = self.0.lock().map_err(|_| ())?;
            if state.phase != Phase::Finalizing {
                return Err(());
            }
            state.phase = Phase::Sampling;
            state.location_revision
        };
        let sampled = sample();
        let mut state = self.0.lock().map_err(|_| ())?;
        let effective = sampled.as_deref().and_then(|raw| {
            ContextNavigationTarget::parse(raw)
                .ok()
                .filter(|target| target.as_url().as_str() == raw)
        });
        if state.phase != Phase::Sampling
            || state.location_revision != revision
            || !effective.as_ref().zip(state.target.as_ref()).is_some_and(
                |(effective, requested)| state.policy.admits_final_document(requested, effective),
            )
        {
            state.phase = Phase::Refused;
            return Err(());
        }
        let effective = effective.ok_or(())?;
        state.effective = Some(effective.clone());
        state.phase = Phase::Ready;
        Ok(effective)
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
                    .effective
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
    fn next_request() -> (ContextJoin, ContextNavigationRequest) {
        use zephium_agentic::*;
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            zephium_core::ids::ProfileId::generate(),
            ContextKind::Owned,
        );
        let mut registry = ContextRegistry::new();
        registry
            .reserve(
                identity,
                ContextCapabilities::try_new(
                    ContextKind::Owned,
                    &[ContextCapability::Navigate, ContextCapability::Observe],
                )
                .unwrap(),
            )
            .unwrap();
        let construction = registry
            .begin_context(identity.id(), ContextOperationId::new(1).unwrap())
            .unwrap();
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .unwrap();
        let source = registry.join(identity.id()).unwrap();
        registry
            .acknowledge_observation(identity.id(), source)
            .unwrap();
        let operation = registry
            .begin_navigation(identity.id(), ContextOperationId::new(2).unwrap())
            .unwrap();
        (
            source,
            ContextNavigationRequest::try_new(
                operation,
                ContextNavigationTarget::parse("https://example.test/next").unwrap(),
            )
            .unwrap(),
        )
    }
    fn ready_gate() -> WorkDocumentNavigation {
        let gate = armed();
        for phase in [E::Started, E::Committed, E::Finished] {
            gate.observe(event(1, phase, URL)).unwrap();
        }
        gate
    }
    #[test]
    fn successor_uses_same_gate_exact_lineage_and_one_terminal_without_bootstrap() {
        let gate = ready_gate();
        let (source, request) = next_request();
        let prior_stamp = gate.observation_stamp(source).unwrap();
        let next = request.target().as_url().as_str();
        gate.arm_successor(source, &request).unwrap();
        assert!(gate.observation_stamp(source).is_none());
        assert!(!gate.ready(Some(URL)));
        assert!(!gate.allows("about:blank"));
        assert!(!gate.allows(URL));
        assert!(gate.allows(next));
        assert!(!gate.allows(next));
        assert!(gate.take_successor_terminal().is_none());
        gate.observe(event(2, E::Started, next)).unwrap();
        gate.location_changed().unwrap();
        assert_eq!(
            gate.observe(event(2, E::Committed, next)),
            Ok((true, false))
        );
        assert!(gate.take_successor_terminal().is_none());
        gate.observe(event(2, E::Finished, next)).unwrap();
        assert!(gate.ready(Some(next)));
        assert!(gate.arm_successor(source, &request).is_err());
        let (operation, outcome) = gate.take_successor_terminal().unwrap();
        assert_eq!(operation, request.operation());
        assert_eq!(outcome.as_ref(), Ok(request.target()));
        assert_eq!(gate.0.lock().unwrap().navigation_epoch, 2);
        assert!(gate.observation_stamp(source).is_none());
        assert_ne!(
            gate.observation_stamp(operation.context()),
            Some(prior_stamp)
        );
        assert!(gate.observation_stamp(operation.context()).is_some());
        assert!(gate.take_successor_terminal().is_none());
        assert!(gate.arm_successor(source, &request).is_err());
        assert_eq!(gate.location_changed(), Ok(true));
        assert!(!gate.ready(Some(next)));
        assert!(gate.observation_stamp(operation.context()).is_none());
    }
    #[test]
    fn successor_rejects_old_document_events_redirects_wrong_ids_and_post_ready_drift() {
        for fault in 0..6 {
            let gate = ready_gate();
            let (source, request) = next_request();
            let next = request.target().as_url().as_str();
            gate.arm_successor(source, &request).unwrap();
            assert!(gate.allows(next));
            gate.observe(event(2, E::Started, next)).unwrap();
            let hostile = match fault {
                0 => event(1, E::Finished, URL),
                1 => event(2, E::Redirected, next),
                2 => event(3, E::Committed, next),
                3 => event(2, E::Committed, URL),
                4 => event(2, E::Finished, next),
                _ => event(2, E::Failed, next),
            };
            gate.observe(hostile).unwrap();
            assert!(gate.failed());
            let (operation, outcome) = gate.take_successor_terminal().unwrap();
            assert_eq!(operation, request.operation());
            assert!(outcome.is_err());
            gate.observe(event(2, E::Committed, next)).unwrap();
            gate.observe(event(2, E::Finished, next)).unwrap();
            assert!(!gate.ready(Some(next)));
            assert!(!gate.ready(Some(URL)));
            assert!(gate.arm_successor(source, &request).is_err());
            assert_eq!(gate.0.lock().unwrap().navigation_epoch, 1);
        }
    }
    #[cfg(feature = "native-agentic-work-resource-probe")]
    #[test]
    fn current_components_distinguish_normalization_and_drift_without_granting_authority() {
        use CurrentDocumentEvidence as C;
        let expected = ContextNavigationTarget::parse(URL).unwrap();
        let gate = armed();
        for phase in [E::Started, E::Committed, E::Finished] {
            gate.observe(event(1, phase, URL)).unwrap();
        }
        let normalized = "https://EXAMPLE.TEST:443/frozen";
        assert!(matches!(
            C::compare(&expected, normalized),
            C::Compared {
                raw_equal: false,
                canonical_equal: true,
                ..
            }
        ));
        assert!(!gate.ready(Some(normalized)));
        assert!(gate.ready(Some(URL)));
        let query = C::compare(&expected, "https://example.test/frozen?opaque=one");
        assert!(matches!(
            query,
            C::Compared {
                raw_equal: false,
                canonical_equal: false,
                scheme_equal: true,
                host_equal: true,
                port_equal: true,
                path_equal: true,
                query_equal: false,
                fragment_equal: true,
                query_present: true,
                query_empty: false,
                credentials_present: false,
                ..
            }
        ));
        assert_eq!(
            query,
            C::compare(&expected, "https://example.test/frozen?different=two")
        );
        let safe = format!("{query:?}");
        for secret in [
            "example.test",
            "opaque",
            "one",
            "different",
            "two",
            "/frozen",
        ] {
            assert!(!safe.contains(secret));
        }
        assert!(matches!(
            C::compare(&expected, "https://example.test/frozen?"),
            C::Compared {
                query_present: true,
                query_empty: true,
                ..
            }
        ));
        assert!(matches!(
            C::compare(&expected, "https://example.test/frozen#private"),
            C::Compared {
                query_equal: true,
                fragment_equal: false,
                fragment_present: true,
                ..
            }
        ));
        assert!(matches!(
            C::compare(&expected, "https://example.test/changed"),
            C::Compared {
                path_equal: false,
                ..
            }
        ));
        assert!(matches!(
            C::compare(&expected, "https://other.test/frozen"),
            C::Compared {
                host_equal: false,
                ..
            }
        ));
        assert!(matches!(
            C::compare(&expected, "https://user:secret@example.test/frozen"),
            C::Compared {
                credentials_equal: false,
                credentials_present: true,
                ..
            }
        ));
        assert!(matches!(
            C::compare(&expected, "http://example.test/frozen"),
            C::Compared {
                scheme_equal: false,
                port_equal: false,
                ..
            }
        ));
        assert!(matches!(
            C::compare(&expected, "https://example.test:8443/frozen"),
            C::Compared {
                port_equal: false,
                ..
            }
        ));
        assert_eq!(C::compare(&expected, "not a URL"), C::InvalidUrl);
    }
    #[cfg(feature = "native-agentic-work-resource-probe")]
    #[test]
    fn finished_commit_identity_does_not_authorize_a_pre_finish_location_change() {
        let gate = armed();
        gate.observe(event(1, E::Started, URL)).unwrap();
        gate.observe(event(1, E::Committed, URL)).unwrap();
        assert_eq!(gate.location_changed(), Ok(false));
        gate.observe(event(1, E::Finished, URL)).unwrap();
        let evidence = gate.construction_evidence().unwrap();
        assert!(
            evidence.finished
                && evidence.location_callback_after_commit_before_ready
                && !evidence.refused
        );
        assert!(!gate.ready(Some("https://example.test/frozen?changed=1")));
        assert!(!gate.ready(None));
    }
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
        armed_policy(zephium_agentic::WorkBrowserDocumentPolicy::Exact)
    }
    fn armed_policy(policy: zephium_agentic::WorkBrowserDocumentPolicy) -> WorkDocumentNavigation {
        let gate = WorkDocumentNavigation::default();
        assert!(gate.allows("about:blank"));
        for phase in [E::Started, E::Committed, E::Finished] {
            gate.observe(event(99, phase, "about:blank")).unwrap();
        }
        gate.arm_with_policy(ContextNavigationTarget::parse(URL).unwrap(), policy)
            .unwrap();
        assert!(gate.allows(URL));
        gate
    }
    fn finalizing() -> WorkDocumentNavigation {
        let gate =
            armed_policy(zephium_agentic::WorkBrowserDocumentPolicy::InitialQueryFinalization);
        for phase in [E::Started, E::Committed] {
            gate.observe(event(1, phase, URL)).unwrap();
        }
        assert_eq!(gate.location_changed(), Ok(false));
        gate.observe(event(1, E::Finished, URL)).unwrap();
        assert!(gate.finalization_pending());
        assert!(!gate.ready(Some(URL)));
        gate
    }
    #[test]
    fn startup_finalization_freezes_original_navigation_before_any_dispatch() {
        let current = "https://example.test/frozen?opaque=one";
        let gate = finalizing();
        assert!(!gate.allows(current));
        let effective = gate.finalize(|| Some(current.into())).unwrap();
        assert_eq!(effective.as_url().as_str(), current);
        assert!(gate.ready(Some(current)));
        assert!(!gate.ready(Some(URL)));
        assert!(!gate.allows(current));
        assert!(!gate.finalization_pending());
        assert!(gate
            .finalize(|| panic!("never resample a sealed document"))
            .is_err());
        // This is the same permanent fence used before dispatch and after the
        // next-main-queue result barrier. Returning to the URL cannot revive refs.
        assert_eq!(gate.location_changed(), Ok(true));
        assert!(!gate.ready(Some(current)));
        assert!(gate.failed());
        gate.observe(event(1, E::Finished, URL)).unwrap();
        assert!(!gate.ready(Some(current)));
    }
    #[test]
    fn startup_sampling_is_first_attempt_only_and_revision_fenced() {
        let gate = finalizing();
        let count = std::cell::Cell::new(0);
        assert!(gate
            .finalize(|| {
                count.set(count.get() + 1);
                assert!(!gate.ready(Some(URL)));
                assert!(gate.finalize(|| panic!("reentrant sample")).is_err());
                gate.location_changed().unwrap();
                Some(URL.into())
            })
            .is_err());
        assert_eq!(count.get(), 1);
        assert!(gate.failed());
        assert!(gate.finalize(|| panic!("retry sample")).is_err());
        assert!(!gate.ready(Some(URL)));
    }
    #[test]
    fn startup_refuses_missing_noncanonical_and_changed_documents() {
        for current in [
            None,
            Some("https://EXAMPLE.TEST/frozen?x=1"),
            Some("https://example.test/other?x=1"),
            Some("https://else.test/frozen?x=1"),
            Some("https://example.test/frozen?"),
            Some("https://example.test/frozen?x=1#fragment"),
        ] {
            let gate = finalizing();
            assert!(gate.finalize(|| current.map(str::to_owned)).is_err());
            assert!(gate.failed());
        }
    }
    #[test]
    fn startup_never_finalizes_missing_foreign_or_redirected_native_lineage() {
        for events in [
            vec![event(1, E::Finished, URL)],
            vec![event(1, E::Started, URL), event(2, E::Committed, URL)],
            vec![event(1, E::Started, URL), event(1, E::Redirected, URL)],
            vec![
                event(1, E::Started, URL),
                event(1, E::Committed, URL),
                event(2, E::Finished, URL),
            ],
        ] {
            let gate =
                armed_policy(zephium_agentic::WorkBrowserDocumentPolicy::InitialQueryFinalization);
            for event in events {
                gate.observe(event).unwrap();
            }
            assert!(gate.failed());
            assert!(gate
                .finalize(|| panic!("unproved lineage cannot sample"))
                .is_err());
        }
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
