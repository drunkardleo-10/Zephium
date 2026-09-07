//! Debug-only fixed resource rendering/evidence transport. Shares original
//! native admission and task accounting; not an AgentBrowserPort capability.

use super::*;
use zephium_agentic::{ForegroundRenderingState, WorkBrowserResourceJoin};

/// First construction failure on the original native guard. No URL, page data,
/// native identity, or retry/cleanup authority is carried by this snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConstructionEvidence {
    pub cause: &'static str,
    pub port_failure: Option<ContextPortFailure>,
    pub navigation: Option<crate::platform::work_document_navigation::NavigationEvidence>,
    pub document_started: bool,
    pub deadline_expired: bool,
    pub guard_healthy: bool,
    pub current_document: bool,
    pub current_components:
        Option<crate::platform::work_document_navigation::CurrentDocumentEvidence>,
    pub semantic_pending: Option<bool>,
}

impl WorkResourceGuard {
    pub(crate) fn record_construction_evidence(
        &self,
        sample: impl FnOnce() -> ConstructionEvidence,
    ) {
        // Reserve before any native getter or evidence construction. Sampling
        // runs outside the publication lock, including reentrant callbacks.
        // A lost/panicking sample remains unavailable; it never permits retry.
        if self
            .construction_evidence_claimed
            .swap(true, Ordering::AcqRel)
        {
            return;
        }
        let evidence = sample();
        if let Ok(mut first) = self.construction_evidence.lock() {
            *first = Some(evidence);
        }
    }
}

/// Closed diagnostic purpose, bound to the exact original resource request.
/// Public rendering is absent unless its separate excluded feature is selected.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Document {
    RenderingFixture,
    #[cfg(feature = "native-agentic-public-resource-probe")]
    PublicProductBrief,
}
impl Document {
    pub(crate) fn admits(self, target: &ContextNavigationTarget) -> bool {
        match self {
            Self::RenderingFixture => fixed_fixture(target),
            #[cfg(feature = "native-agentic-public-resource-probe")]
            Self::PublicProductBrief => {
                target.as_url().as_str() == "https://shop.pimoroni.com/products/raspberry-pi-pico-2"
            }
        }
    }
    pub(crate) const fn read_limit(self) -> u8 {
        match self {
            Self::RenderingFixture => 8,
            #[cfg(feature = "native-agentic-public-resource-probe")]
            Self::PublicProductBrief => 1,
        }
    }
}
pub(crate) fn fixed_fixture(target: &ContextNavigationTarget) -> bool {
    let url = target.as_url();
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/semantic-rendering-v1.html"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Operation {
    Acquire,
    Poll,
    Inspect,
    Retire,
}
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Request {
    pub(crate) resource: WorkBrowserResourceJoin,
    pub(crate) operation: Operation,
    pub(crate) document: Document,
}

/// In-memory comparison only: deliberately no Debug/Serialize or raw getters.
pub(crate) struct RetentionStamp {
    resource: WorkBrowserResourceJoin,
    view: usize,
    world: usize,
    document: wry::NavigationId,
    completed: u16,
    invocation: u64,
    presented_observations: u16,
}
impl RetentionStamp {
    pub(crate) fn new(
        resource: WorkBrowserResourceJoin,
        identity: (usize, usize, u16),
        document: wry::NavigationId,
        invocation: u64,
        presented_observations: u16,
    ) -> Self {
        Self {
            resource,
            view: identity.0,
            world: identity.1,
            completed: identity.2,
            document,
            invocation,
            presented_observations,
        }
    }
    pub(crate) fn retained_after_one_read(&self, next: &Self) -> bool {
        self.resource == next.resource
            && self.view != 0
            && self.world != 0
            && self.view == next.view
            && self.world == next.world
            && self.document == next.document
            && self.completed.checked_add(1) == Some(next.completed)
            && next.invocation > self.invocation
    }
    pub(crate) fn completed(&self) -> u16 {
        self.completed
    }
    pub(crate) fn presented_observations(&self) -> u16 {
        self.presented_observations
    }
}
pub(crate) struct Evidence {
    pub(crate) state: ForegroundRenderingState,
    pub(crate) stamp: Option<RetentionStamp>,
}
type Completion = Box<dyn FnOnce(Request, Evidence) + Send>;

pub(crate) struct ResourceWitnessPort {
    dispatch: MainThreadDispatch,
    admission: Arc<AgentPortAdmission>,
}
impl AgentContextPortSlot {
    pub(crate) fn construction_evidence(
        &self,
        resource: &WorkBrowserResourceJoin,
    ) -> Option<ConstructionEvidence> {
        let admission = self.state.lock().ok()?.admission.clone()?;
        let guard = admission.witness_resource(resource)?;
        let evidence = *guard.construction_evidence.lock().ok()?;
        evidence
    }
    pub(crate) fn resource_witness_port(&self) -> Option<ResourceWitnessPort> {
        let state = self.state.lock().ok()?;
        if !state.taken || state.sealed || state.factory.is_some() {
            return None;
        }
        Some(ResourceWitnessPort {
            dispatch: self.dispatch.clone(),
            admission: state.admission.clone()?,
        })
    }
}
pub(crate) struct Task {
    request: Request,
    guard: Arc<WorkResourceGuard>,
    completion: Option<Completion>,
    permit: AgentTaskPermit,
}
impl Task {
    pub(crate) fn request(&self) -> &Request {
        &self.request
    }
    pub(crate) fn guard(&self) -> Arc<WorkResourceGuard> {
        self.guard.clone()
    }
    pub(crate) fn complete(
        mut self,
        state: ForegroundRenderingState,
        stamp: Option<RetentionStamp>,
    ) {
        self.deliver(Evidence { state, stamp });
    }
    fn deliver(&mut self, evidence: Evidence) {
        if let Some(completion) = self.completion.take() {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                completion(self.request.clone(), evidence)
            }))
            .is_err()
            {
                self.guard.fail();
            }
        }
        self.permit.release();
    }
}
impl Drop for Task {
    fn drop(&mut self) {
        if self.completion.is_some() {
            self.deliver(Evidence {
                state: ForegroundRenderingState::Failed,
                stamp: None,
            });
        }
    }
}
impl ResourceWitnessPort {
    pub(crate) fn schedule(&self, request: Request, completion: Completion) -> bool {
        let Some(guard) = self.admission.witness_resource(&request.resource) else {
            return false;
        };
        let Ok(permit) = self.admission.reserve() else {
            return false;
        };
        let slot = Arc::new(Mutex::new(Some(Task {
            request,
            guard,
            completion: Some(completion),
            permit,
        })));
        let queued = slot.clone();
        let executed = Arc::new(AtomicBool::new(false));
        let ran = executed.clone();
        let admitted = contain_agent_port_panic(&self.admission, || {
            (self.dispatch)(Box::new(move || {
                ran.store(true, Ordering::Release);
                let task = queued
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                let Some(task) = task else {
                    return;
                };
                let owned = Arc::new(Mutex::new(Some(task)));
                let for_host = owned.clone();
                if !crate::host::try_with_agent_context(move |host| {
                    if let Some(task) = for_host
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        host.handle_resource_witness(task);
                    }
                }) {
                    if let Some(task) = owned
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .take()
                    {
                        task.complete(ForegroundRenderingState::Failed, None);
                    }
                }
            }))
        })
        .unwrap_or(false);
        if admitted || executed.load(Ordering::Acquire) {
            return true;
        }
        if let Some(mut task) = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            task.completion = None;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "native-agentic-public-resource-probe")]
    #[test]
    fn public_document_admission_is_exact_and_disjoint_from_the_fixture() {
        use super::*;
        let public = ContextNavigationTarget::parse(
            "https://shop.pimoroni.com/products/raspberry-pi-pico-2",
        )
        .unwrap();
        assert!(Document::PublicProductBrief.admits(&public));
        assert!(!Document::RenderingFixture.admits(&public));
        for url in [
            "http://127.0.0.1:12345/semantic-rendering-v1.html",
            "http://shop.pimoroni.com/products/raspberry-pi-pico-2",
            "https://pimoroni.com/products/raspberry-pi-pico-2",
            "https://shop.pimoroni.com.evil.test/products/raspberry-pi-pico-2",
            "https://shop.pimoroni.com:8443/products/raspberry-pi-pico-2",
            "https://shop.pimoroni.com/products/raspberry-pi-pico-2/",
            "https://shop.pimoroni.com/products/raspberry-pi-pico-2-w",
            "https://shop.pimoroni.com/products/raspberry-pi-pico-2?q=1",
            "https://shop.pimoroni.com/products/raspberry-pi-pico-2#fragment",
        ] {
            assert!(
                !Document::PublicProductBrief.admits(&ContextNavigationTarget::parse(url).unwrap()),
                "{url}"
            );
        }
        // Parsing may itself reject credentials; neither path can admit them.
        for url in [
            "https://user@shop.pimoroni.com/products/raspberry-pi-pico-2",
            "https://user:pass@shop.pimoroni.com/products/raspberry-pi-pico-2",
        ] {
            assert!(ContextNavigationTarget::parse(url)
                .map_or(true, |target| !Document::PublicProductBrief.admits(&target)));
        }
    }
    use super::*;
    use zephium_agentic::*;
    fn source() -> (WorkBrowserResources, WorkBrowserResourceRequest) {
        let mut rows =
            WorkBrowserResources::new(WorkId::generate(), AgentWorkProfileId::generate());
        let request = rows
            .construct_document(
                WorkBrowserResourceId::generate(),
                ContextId::generate(),
                ContextProfileStorageClass::Ephemeral,
                ContextNavigationTarget::parse("http://127.0.0.1:12345/semantic-rendering-v1.html")
                    .unwrap(),
                AgentPolicyInstant::from_millis(0),
            )
            .unwrap();
        (rows, request)
    }
    #[test]
    fn construction_evidence_is_first_wins_exact_and_available_before_retained_or_holder() {
        let (mut rows, construction) = source();
        let resource = construction.resource().clone();
        let slot = AgentContextPortSlot::new(Arc::new(|_| true), Arc::new(|_| {}));
        let port = slot.take(Arc::new(|_| {})).unwrap();
        assert!(matches!(
            port.work_resource_lifecycle(
                construction,
                Box::new(move |completion| {
                    let _ = rows
                        .settle_at(completion, AgentPolicyInstant::from_millis(1))
                        .unwrap();
                })
            ),
            WorkBrowserResourceDispatch::Scheduled
        ));
        let admission = slot.state.lock().unwrap().admission.clone().unwrap();
        let guard = admission.witness_resource(&resource).unwrap();
        assert!(slot.construction_evidence(&resource).is_none());
        let first = ConstructionEvidence {
            cause: "construction_deadline",
            port_failure: None,
            navigation: None,
            document_started: false,
            deadline_expired: true,
            guard_healthy: false,
            current_document: false,
            current_components: None,
            semantic_pending: None,
        };
        let samples = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = samples.clone();
        let sampling_guard = guard.clone();
        let (entered, entry) = std::sync::mpsc::sync_channel(1);
        let (release, proceed) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            sampling_guard.record_construction_evidence(|| {
                counted.fetch_add(1, Ordering::AcqRel);
                assert!(sampling_guard.construction_evidence.try_lock().is_ok());
                sampling_guard
                    .record_construction_evidence(|| panic!("reentrant sample must not run"));
                entered.send(()).unwrap();
                proceed
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .unwrap();
                first
            })
        });
        entry
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(slot.construction_evidence(&resource).is_none());
        guard.record_construction_evidence(|| {
            samples.fetch_add(1, Ordering::AcqRel);
            ConstructionEvidence {
                cause: "native_health",
                ..first
            }
        });
        assert_eq!(samples.load(Ordering::Acquire), 1);
        release.send(()).unwrap();
        worker.join().unwrap();
        guard.record_construction_evidence(|| panic!("late cleanup must not resample"));
        assert_eq!(slot.construction_evidence(&resource), Some(first));
        assert!(slot.construction_evidence(source().1.resource()).is_none());
        slot.seal();
        assert_eq!(slot.construction_evidence(&resource), Some(first));
        assert!(!guard.port_open());
        assert!(!guard.is_healthy());
        assert_eq!(admission.pending(), Some(0));
    }
    #[test]
    fn retention_stamp_rejects_resource_view_document_world_counter_and_invocation_substitution() {
        let (_, request) = source();
        let join = request.resource().clone();
        let stamp = || {
            RetentionStamp::new(
                join.clone(),
                (10, 20, 1),
                wry::NavigationId::from_raw(7),
                1,
                0,
            )
        };
        let next = || {
            RetentionStamp::new(
                join.clone(),
                (10, 20, 2),
                wry::NavigationId::from_raw(7),
                3,
                0,
            )
        };
        assert!(stamp().retained_after_one_read(&next()));
        for change in 0..8 {
            let mut wrong = next();
            match change {
                0 => wrong.resource = source().1.resource().clone(),
                1 => wrong.view = 11,
                2 => wrong.world = 21,
                3 => wrong.document = wry::NavigationId::from_raw(8),
                4 => wrong.completed = 1,
                5 => wrong.completed = 3,
                6 => wrong.completed = 0,
                _ => wrong.invocation = 1,
            }
            assert!(!stamp().retained_after_one_read(&wrong));
        }
        let mut invalid = stamp();
        invalid.world = 0;
        assert!(!invalid.retained_after_one_read(&next()));
        let mut invalid = stamp();
        invalid.completed = u16::MAX;
        assert!(!invalid.retained_after_one_read(&next()));
    }
    #[test]
    fn diagnostic_terminal_holds_original_global_permit_through_callback_and_drops_once() {
        let (mut rows, construction) = source();
        let resource = construction.resource().clone();
        let slot = AgentContextPortSlot::new(Arc::new(|_| true), Arc::new(|_| {}));
        assert!(slot.resource_witness_port().is_none());
        let port = slot.take(Arc::new(|_| {})).unwrap();
        assert!(matches!(
            port.work_resource_lifecycle(
                construction,
                Box::new(move |completion| {
                    let _ = rows
                        .settle_at(completion, AgentPolicyInstant::from_millis(1))
                        .unwrap();
                })
            ),
            WorkBrowserResourceDispatch::Scheduled
        ));
        let admission = slot.state.lock().unwrap().admission.clone().unwrap();
        let guard = admission.witness_resource(&resource).unwrap();
        let outputs = Arc::new(Mutex::new(Vec::new()));
        let delivered = outputs.clone();
        let callback_admission = admission.clone();
        let expected = resource.clone();
        let task = Task {
            request: Request {
                resource,
                operation: Operation::Inspect,
                document: Document::RenderingFixture,
            },
            guard,
            permit: admission.reserve().unwrap(),
            completion: Some(Box::new(move |request, evidence| {
                assert_eq!(request.resource, expected);
                assert_eq!(callback_admission.pending(), Some(1));
                assert!(!callback_admission.work_is_absent());
                assert!(evidence.stamp.is_none());
                delivered.lock().unwrap().push(evidence.state);
            })),
        };
        drop(task);
        assert_eq!(*outputs.lock().unwrap(), [ForegroundRenderingState::Failed]);
        assert_eq!(admission.pending(), Some(0));
        slot.seal();
        assert!(slot.resource_witness_port().is_none());
    }
}
