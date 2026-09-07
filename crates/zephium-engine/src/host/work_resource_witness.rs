//! Fixed, release-excluded holder/evidence only. The product resource owns
//! its page independently; this child adds no production rendering authority.
use super::*;
use crate::agent_context_port::resource_witness::{
    Document, Operation as Op, RetentionStamp, Task,
};
use crate::platform::imp::ForegroundRenderingLease;
use std::{cell::RefCell, rc::Rc, sync::Mutex};
use zephium_agentic::ForegroundRenderingState as State;

pub(super) struct RenderingHolder {
    lease: Rc<RefCell<ForegroundRenderingLease>>,
    watchdog: Option<RenderingWatchdog>,
}
/// Original resource-owned read budget survives presentation retirement.
pub(super) struct Admission {
    document: Document,
    samples: u8,
}
impl Admission {
    fn read(&mut self) -> bool {
        if self.samples >= self.document.read_limit() {
            return false;
        }
        self.samples += 1;
        true
    }
}
type WatchdogAction = Box<dyn FnOnce() + Send>;
struct RenderingWatchdog {
    timer: Option<crate::platform::imp::ContentPolicyTimeout>,
    action: Arc<Mutex<Option<WatchdogAction>>>,
}
impl Drop for RenderingWatchdog {
    fn drop(&mut self) {
        self.timer = None;
        // Cancellation consumes the exact never-entered callback authority.
        // A later libdispatch block retains only an empty, inert cell. If the
        // callback already entered, its local original permit still owns drain.
        self.action
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}
impl WorkNativeResource {
    pub(in crate::host) fn witness_visible(&self) -> bool {
        self.witness.as_ref().is_some_and(|holder| {
            holder
                .lease
                .try_borrow()
                .map_or(true, |lease| lease.visible_for_audit())
        })
    }
    pub(super) fn retire_witness(&mut self) -> bool {
        self.retire_witness_state() == State::Retired
    }
    fn retire_witness_state(&mut self) -> State {
        self.witness_attempted = true;
        let Some(holder) = self.witness.as_mut() else {
            return State::Retired;
        };
        holder.watchdog = None;
        let state = holder
            .lease
            .try_borrow_mut()
            .map(|mut lease| lease.retire())
            .unwrap_or(State::Failed);
        if state == State::Retired {
            self.witness = None;
        }
        state
    }
    pub(super) fn witness_ready(&self) -> bool {
        self.witness.as_ref().is_none_or(|holder| {
            holder
                .lease
                .try_borrow_mut()
                .is_ok_and(|mut lease| lease.guard_resource(self.guard.resource()) == State::Ready)
        })
    }
    pub(super) fn admit_witness_read(&mut self) -> bool {
        if !self.witness_ready() {
            return false;
        }
        self.witness_admission.as_mut().is_none_or(Admission::read)
    }
    fn stamp(&self) -> Option<RetentionStamp> {
        if !self.ready() || self.pending() || !self.witness_ready() {
            return None;
        }
        let view = self.view.as_ref()?;
        Some(RetentionStamp::new(
            self.guard.resource().clone(),
            view.semantic()?.witness_identity()?,
            view.work_navigation()?.witness_document()?,
            self.last_invocation,
        ))
    }
}

impl EngineHost {
    pub(crate) fn handle_resource_witness(&mut self, task: Task) {
        let guard = task.guard();
        let request = task.request();
        let Some(resource) = self
            .work_resources
            .get_mut(&request.resource.identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
        else {
            task.complete(State::Failed, None);
            return;
        };
        if request.operation == Op::Retire {
            let state = resource.retire_witness_state();
            task.complete(state, None);
            return;
        }
        if !guard.port_open()
            || !guard.is_healthy()
            || !resource.ready()
            || resource.pending()
            || guard.storage() != ContextProfileStorageClass::Ephemeral
            || !guard
                .document()
                .is_some_and(|target| request.document.admits(target))
            || resource
                .witness_admission
                .as_ref()
                .is_some_and(|admission| admission.document != request.document)
        {
            task.complete(State::Failed, None);
            return;
        }
        let state = match request.operation {
            Op::Acquire => {
                if resource.witness_attempted
                    || resource.last_invocation != 0
                    || guard.execution_reserved()
                {
                    task.complete(State::Failed, None);
                    return;
                }
                resource.witness_attempted = true;
                resource.witness_admission = Some(Admission {
                    document: request.document,
                    samples: 0,
                });
                let Some(deadline) = ForegroundRenderingLease::admission_deadline(Instant::now())
                else {
                    task.complete(State::Failed, None);
                    return;
                };
                let Some(view) = resource.view.as_ref() else {
                    task.complete(State::Failed, None);
                    return;
                };
                let native = match ForegroundRenderingLease::prepare_resource(
                    guard.resource(),
                    view.view(),
                    deadline,
                ) {
                    Ok(native) => Rc::new(RefCell::new(native)),
                    Err(state) => {
                        task.complete(state, None);
                        return;
                    }
                };
                resource.witness = Some(RenderingHolder {
                    lease: native.clone(),
                    watchdog: None,
                });
                native
                    .try_borrow_mut()
                    .map(|mut lease| lease.present_resource(guard.resource()))
                    .unwrap_or(State::Failed)
            }
            Op::Poll | Op::Inspect => resource
                .witness
                .as_ref()
                .and_then(|holder| {
                    holder
                        .lease
                        .try_borrow_mut()
                        .ok()
                        .map(|mut lease| lease.guard_resource(guard.resource()))
                })
                .unwrap_or(State::Failed),
            Op::Retire => State::Failed,
        };
        let stamp = (request.operation == Op::Inspect && state == State::Ready)
            .then(|| resource.stamp())
            .flatten();
        if matches!(state, State::Ready | State::Acquiring)
            && resource
                .witness
                .as_ref()
                .is_some_and(|h| h.watchdog.is_none())
            && !self.arm_resource_witness(&guard)
        {
            task.complete(State::Failed, None);
            return;
        }
        task.complete(state, stamp);
    }
    fn arm_resource_witness(&mut self, guard: &Arc<WorkResourceGuard>) -> bool {
        let Some(permit) = guard.notification_permit() else {
            return false;
        };
        let wake_guard = guard.clone();
        let action: Arc<Mutex<Option<WatchdogAction>>> =
            Arc::new(Mutex::new(Some(Box::new(move || {
                let rejected = wake_guard.clone();
                if !crate::host::try_with_agent_context_terminal(move |host| {
                    host.tick_resource_witness(&wake_guard);
                    drop(permit);
                }) {
                    rejected.fail();
                }
            }))));
        let queued = action.clone();
        let timer = crate::platform::imp::schedule_content_policy_timeout(
            Duration::from_millis(50),
            move || {
                let action = queued
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(action) = action {
                    action();
                }
            },
        );
        let watchdog = RenderingWatchdog { timer, action };
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
        else {
            return false;
        };
        let Some(holder) = resource.witness.as_mut() else {
            return false;
        };
        if watchdog.timer.is_none() {
            resource.retire_witness();
            guard.fail();
            return false;
        }
        holder.watchdog = Some(watchdog);
        true
    }
    fn tick_resource_witness(&mut self, guard: &Arc<WorkResourceGuard>) {
        let Some(resource) = self
            .work_resources
            .get_mut(&guard.resource().identity().context())
            .filter(|resource| Arc::ptr_eq(&resource.guard, guard))
        else {
            return;
        };
        let Some(holder) = resource.witness.as_mut() else {
            return;
        };
        holder.watchdog = None;
        let state = holder
            .lease
            .try_borrow_mut()
            .map(|mut lease| lease.guard_resource(guard.resource()))
            .unwrap_or(State::Failed);
        if matches!(state, State::Ready | State::Acquiring) {
            let _ = self.arm_resource_witness(guard);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_context_port::resource_witness::fixed_fixture;
    use zephium_agentic::ContextNavigationTarget;

    #[test]
    fn rendering_fixture_keeps_its_original_eight_read_bound() {
        let mut admission = Admission {
            document: Document::RenderingFixture,
            samples: 0,
        };
        for _ in 0..8 {
            assert!(admission.read());
        }
        assert!(!admission.read());
        assert_eq!(admission.samples, 8);
    }

    #[cfg(feature = "native-agentic-public-resource-probe")]
    #[test]
    fn public_resource_admission_is_one_shot_without_a_presentation_holder() {
        // This resource-owned budget is not stored in (or renewed by dropping)
        // RenderingHolder. A failed first read consumes the same sole attempt.
        let mut admission = Admission {
            document: Document::PublicProductBrief,
            samples: 0,
        };
        assert!(admission.read());
        for _ in 0..64 {
            assert!(!admission.read());
        }
        assert_eq!(admission.samples, 1);
    }
    #[test]
    fn cancelled_watchdog_consumes_original_authority_before_a_late_dispatch_block() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Owner(Arc<AtomicUsize>);
        impl Drop for Owner {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::AcqRel);
            }
        }
        let owners = Arc::new(AtomicUsize::new(1));
        let owner = Owner(owners.clone());
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let action: Arc<Mutex<Option<WatchdogAction>>> =
            Arc::new(Mutex::new(Some(Box::new(move || {
                called.fetch_add(1, Ordering::AcqRel);
                drop(owner);
            }))));
        let late = action.clone();
        let watchdog = RenderingWatchdog {
            timer: None,
            action,
        };
        drop(watchdog);
        assert_eq!(owners.load(Ordering::Acquire), 0);
        assert!(late.lock().unwrap().take().is_none());
        assert_eq!(calls.load(Ordering::Acquire), 0);
    }
    #[test]
    fn resource_holder_admits_only_the_fixed_anonymous_loopback_route() {
        assert!(fixed_fixture(
            &ContextNavigationTarget::parse("http://127.0.0.1:12345/semantic-rendering-v1.html")
                .unwrap()
        ));
        for url in [
            "https://example.test/semantic-rendering-v1.html",
            "http://localhost:12345/semantic-rendering-v1.html",
            "http://127.0.0.1:12345/semantic-rendering-v1.html?q=1",
            "http://127.0.0.1:12345/other",
        ] {
            assert!(!fixed_fixture(
                &ContextNavigationTarget::parse(url).unwrap()
            ));
        }
    }
}
