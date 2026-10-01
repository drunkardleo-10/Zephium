//! Original application-owned preparation/terminal slot for retained navigation.
use super::*;

pub(super) struct PendingNavigation {
    shared: Arc<Shared>,
    resource: Arc<Resource>,
    slot: Arc<Mutex<NavigationOperation>>,
}
pub(super) struct NavigationOperation {
    pub(super) flight: Flight<WorkBrowserNavigationCompletion>,
    preparation: Option<WorkBrowserNavigationPreparation>,
    callback: Option<WorkBrowserNavigationCompletionCallback>,
}
impl PendingNavigation {
    pub(super) fn prepare(
        browser: &LeaseBrowser,
        source: ContextJoin,
        now: AgentPolicyInstant,
    ) -> Result<(Self, ContextOperationJoin), Refusal> {
        browser.health(now)?;
        let preparation =
            browser
                .shared
                .lock_rows()?
                .prepare_navigation(&browser.lease, source, now)?;
        let operation = preparation.operation();
        let (mut flight, callback) = Flight::new(&browser.shared, &browser.resource, false);
        flight.navigation = true;
        browser.resource.navigations.fetch_add(1, Ordering::AcqRel);
        let slot = Arc::new(Mutex::new(NavigationOperation {
            flight,
            preparation: Some(preparation),
            callback: Some(callback),
        }));
        browser
            .resource
            .retain(OwnedSlot::Navigation(slot.clone()))?;
        Ok((
            Self {
                shared: browser.shared.clone(),
                resource: browser.resource.clone(),
                slot,
            },
            operation,
        ))
    }
    pub(super) fn dispatch(&mut self, active: &AgentActiveNavigation) -> ContextDispatch {
        let Ok(mut slot) = self.resource.lock_local(&self.slot) else {
            return ContextDispatch::Rejected(ContextPortFailure::Shutdown);
        };
        let Some(preparation) = slot.preparation.take() else {
            self.resource.fail();
            return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
        };
        let request = match preparation.bind(active) {
            Ok(request) => request,
            Err(preparation) => {
                slot.preparation = Some(*preparation);
                let _ = slot.refuse_preparation(&self.shared, &self.resource);
                return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
            }
        };
        let Some(callback) = slot.callback.take() else {
            self.resource.fail();
            let _ = self.shared.lock_rows().and_then(|mut rows| {
                rows.navigation_dispatch_refused(request)
                    .map_err(Into::into)
            });
            return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
        };
        drop(slot);
        let result = if !self.shared.global_current() || !self.resource.current() {
            drop(callback);
            Ok(WorkBrowserNavigationDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::Shutdown,
            })
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.shared.port.work_resource_navigate(request, callback)
            }))
        };
        match result {
            Ok(WorkBrowserNavigationDispatch::Scheduled) => ContextDispatch::Scheduled,
            Ok(WorkBrowserNavigationDispatch::Rejected { request, failure }) => {
                if let Ok(mut slot) = self.resource.lock_local(&self.slot) {
                    slot.flight.rejected(&self.resource);
                    if self
                        .shared
                        .lock_rows()
                        .and_then(|mut rows| {
                            rows.navigation_dispatch_refused(*request)
                                .map_err(Into::into)
                        })
                        .is_err()
                    {
                        self.resource.fail();
                    }
                    if !slot.flight.contradictory {
                        slot.flight.finish(&self.resource);
                    }
                }
                ContextDispatch::Rejected(failure)
            }
            Err(_) => {
                // Admission is uncertain. Keep the exact original callback debt;
                // never manufacture a synchronous refusal or restore old refs.
                self.resource.fail();
                ContextDispatch::Scheduled
            }
        }
    }
    pub(super) fn dispatch_history_back(
        &mut self,
        active: &AgentActiveNavigation,
    ) -> ContextDispatch {
        let Ok(mut slot) = self.resource.lock_local(&self.slot) else {
            return ContextDispatch::Rejected(ContextPortFailure::Shutdown);
        };
        let Some(preparation) = slot.preparation.take() else {
            self.resource.fail();
            return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
        };
        let request = match preparation.bind_history_back(active) {
            Ok(request) => request,
            Err(preparation) => {
                slot.preparation = Some(*preparation);
                let _ = slot.refuse_preparation(&self.shared, &self.resource);
                return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
            }
        };
        let Some(callback) = slot.callback.take() else {
            self.resource.fail();
            let _ = self.shared.lock_rows().and_then(|mut rows| {
                rows.history_back_dispatch_refused(request)
                    .map_err(Into::into)
            });
            return ContextDispatch::Rejected(ContextPortFailure::NativeRefused);
        };
        drop(slot);
        let result = if !self.shared.global_current() || !self.resource.current() {
            drop(callback);
            Ok(WorkBrowserHistoryBackDispatch::Rejected {
                request: Box::new(request),
                failure: ContextPortFailure::Shutdown,
            })
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.shared.port.work_resource_back(request, callback)
            }))
        };
        match result {
            Ok(WorkBrowserHistoryBackDispatch::Scheduled) => ContextDispatch::Scheduled,
            Ok(WorkBrowserHistoryBackDispatch::Rejected { request, failure }) => {
                if let Ok(mut slot) = self.resource.lock_local(&self.slot) {
                    slot.flight.rejected(&self.resource);
                    if self
                        .shared
                        .lock_rows()
                        .and_then(|mut rows| {
                            rows.history_back_dispatch_refused(*request)
                                .map_err(Into::into)
                        })
                        .is_err()
                    {
                        self.resource.fail();
                    }
                    if !slot.flight.contradictory {
                        slot.flight.finish(&self.resource);
                    }
                }
                ContextDispatch::Rejected(failure)
            }
            Err(_) => {
                self.resource.fail();
                ContextDispatch::Scheduled
            }
        }
    }
    pub(super) fn cancel_preparation(&mut self) -> Result<(), Refusal> {
        self.resource
            .lock_local(&self.slot)?
            .refuse_preparation(&self.shared, &self.resource)
    }
    pub(super) fn finished(&self) -> bool {
        self.resource
            .lock_local(&self.slot)
            .is_ok_and(|slot| slot.flight.finished)
    }
    pub(super) fn poll(
        &mut self,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserNavigationEvent>, Refusal> {
        self.resource
            .lock_local(&self.slot)?
            .poll(&self.shared, &self.resource, now)
    }
}
impl Drop for PendingNavigation {
    fn drop(&mut self) {
        match self.slot.lock() {
            Ok(mut slot) if !slot.flight.finished => {
                slot.flight.abandoned = true;
                self.resource.fail();
            }
            Err(_) => self.resource.fail(),
            _ => {}
        }
    }
}
impl NavigationOperation {
    fn refuse_preparation(&mut self, shared: &Shared, resource: &Resource) -> Result<(), Refusal> {
        let preparation = self.preparation.take().ok_or(Refusal::Consumed)?;
        self.callback.take();
        let result = shared
            .lock_rows()?
            .navigation_preparation_refused(preparation);
        self.flight.finish(resource);
        result.map_err(Into::into)
    }
    pub(super) fn poll(
        &mut self,
        shared: &Shared,
        resource: &Resource,
        now: AgentPolicyInstant,
    ) -> Result<Option<WorkBrowserNavigationEvent>, Refusal> {
        if self.preparation.is_some() {
            if self.flight.abandoned {
                self.refuse_preparation(shared, resource)?;
            }
            return Err(Refusal::Consumed);
        }
        let terminal = match self.flight.take(resource) {
            Ok(Some(terminal)) => terminal,
            Ok(None) => return Ok(None),
            Err(error) => {
                if self.flight.contradictory {
                    self.flight.finish(resource);
                }
                return Err(error);
            }
        };
        if shared.current(resource).is_err()
            && shared.lock_rows()?.phase(&resource.join)? != WorkBrowserResourcePhase::Destroyed
        {
            shared.lock_rows()?.quarantine(&resource.join)?;
        }
        let result = shared.lock_rows()?.settle_navigation(terminal, now);
        self.flight.finish(resource);
        if self.flight.contradictory {
            return Err(Refusal::Uncertain);
        }
        Ok(Some(result?))
    }
}
