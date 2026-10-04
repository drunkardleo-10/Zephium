use super::*;

const HUMAN_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(all(
    target_os = "macos",
    feature = "agentic-browser-qa",
    feature = "native-agentic-semantic-probe"
))]
impl EngineHost {
    pub(crate) fn navigate_human_fixture(
        &mut self,
        resource: &zephium_agentic::WorkBrowserResourceJoin,
    ) -> bool {
        let Some(resource) = self
            .work_resources
            .get(&resource.identity().context())
            .filter(|owned| owned.guard.resource() == resource)
        else {
            return false;
        };
        let Some(view) = resource.view.as_ref() else {
            return false;
        };
        if !resource
            .human_presentation
            .as_ref()
            .is_some_and(|presentation| presentation.current())
            || !resource.guard.document().is_some_and(|target| {
                target.as_url().host_str() == Some("127.0.0.1")
                    && target.as_url().scheme() == "http"
                    && target.as_url().path() == "/"
            })
        {
            return false;
        }
        if !resource
            .human_presentation
            .as_ref()
            .is_some_and(|presentation| presentation.qualify_geometry_invalidation())
        {
            return false;
        }
        eprintln!("human_probe changed_parent_geometry_refused=true");
        view.view()
            .evaluate_script("location.assign('/verified')")
            .is_ok()
    }
}

impl WorkNativeResource {
    pub(super) fn handle_human(&mut self, task: WorkLifecycleTask) {
        let result = task.request().ok_or(()).and_then(|request| {
            let operation = request.operation();
            let now = work_browser_monotonic_now().ok_or(())?;
            if !self.guard.human_current(operation, now)
                || self.pending()
                || self.frame_in_flight.load(Ordering::Acquire)
            {
                return Err(());
            }
            match operation {
                Operation::PresentHuman => {
                    if self.human_presentation.is_some()
                        || self.human_unpresented
                        || !self.ready()
                        || self.observation_visible()
                    {
                        return Err(());
                    }
                    if request.human_region().is_none() {
                        // The page stays hidden and unchanged; only the
                        // actor lease ends until the person decides.
                        self.human_unpresented = true;
                        return Ok(None);
                    }
                    let remaining = request
                        .human_deadline()
                        .ok_or(())?
                        .millis()
                        .checked_sub(now.millis())
                        .ok_or(())?;
                    let deadline = Instant::now()
                        .checked_add(Duration::from_millis(remaining))
                        .ok_or(())?;
                    let view = self.view.as_ref().ok_or(())?;
                    let presentation = crate::platform::imp::WorkHumanPresentation::prepare(
                        view.view(),
                        request.human_region().ok_or(())?,
                        deadline,
                    )
                    .ok_or(())?;
                    #[cfg(target_os = "windows")]
                    if !view.activate_work_human() {
                        return Err(());
                    }
                    view.work_navigation().ok_or(())?.begin_human(
                        request.human_source().ok_or(())?,
                        deadline,
                        request.human_sign_in(),
                    )?;
                    self.human_presentation = Some(presentation);
                    self.human_progress = request.human_progress();
                    if !self
                        .human_presentation
                        .as_mut()
                        .is_some_and(|presentation| presentation.present())
                        || !self.schedule_human_wake()
                    {
                        return Err(());
                    }
                    Ok(None)
                }
                Operation::ContinueAfterHuman if self.human_unpresented => {
                    self.human_unpresented = false;
                    let view = self.view.as_ref().ok_or(())?;
                    let current = crate::platform::imp::current_url(view.view()).ok_or(())?;
                    let effective = view.work_navigation().ok_or(())?.hand_back(&current)?;
                    if view.semantic_pending_for_audit() != Some(false) {
                        return Err(());
                    }
                    Ok(Some(effective))
                }
                Operation::ContinueAfterHuman => {
                    if !self
                        .human_presentation
                        .as_ref()
                        .is_some_and(|presentation| presentation.current())
                        || !self.retire_human_presentation()
                    {
                        return Err(());
                    }
                    let view = self.view.as_ref().ok_or(())?;
                    let gate = view.work_navigation().ok_or(())?;
                    let revision = gate.human_revision().ok_or(())?;
                    let current = crate::platform::imp::current_url(view.view()).ok_or(())?;
                    let effective = gate.finish_human(&current, revision)?;
                    if view.semantic_pending_for_audit() != Some(false) {
                        return Err(());
                    }
                    Ok(Some(effective))
                }
                _ => Err(()),
            }
        });
        match result {
            Ok(None) => task.complete(Outcome::HumanPresented),
            Ok(Some(document)) => {
                #[cfg(target_os = "windows")]
                if let Some(view) = self.view.as_ref() {
                    let guard = self.guard.clone();
                    view.finish_work_human_activity(move |clean| {
                        if clean {
                            task.complete_human_document(document);
                        } else {
                            guard.fail();
                            task.complete(Outcome::Refused);
                        }
                    });
                    return;
                }
                task.complete_human_document(document);
            }
            Err(()) => {
                self.retire_human_presentation();
                #[cfg(target_os = "windows")]
                if let Some(view) = self.view.as_ref() {
                    view.finish_work_human_activity(|_| {});
                }
                self.guard.fail();
                task.complete(Outcome::Refused);
            }
        }
    }

    fn schedule_human_wake(&mut self) -> bool {
        if self.human_wake.is_some() {
            return true;
        }
        let guard = self.guard.clone();
        let rejected = guard.clone();
        self.human_wake =
            crate::platform::imp::schedule_content_policy_timeout(HUMAN_POLL_INTERVAL, move || {
                if !crate::host::try_with_agent_context_terminal(move |host| {
                    if let Some(resource) = host
                        .work_resources
                        .get_mut(&guard.resource().identity().context())
                        .filter(|resource| Arc::ptr_eq(&resource.guard, &guard))
                    {
                        resource.human_wake = None;
                    }
                    host.progress_work_resource(&guard);
                }) {
                    rejected.fail();
                }
            });
        self.human_wake.is_some()
    }

    pub(super) fn progress_human(&mut self, erased: bool) {
        let Some(presentation) = self.human_presentation.as_ref() else {
            return;
        };
        if let Some(progress) = &self.human_progress {
            progress.record_ready(
                self.view
                    .as_ref()
                    .and_then(|view| view.work_navigation())
                    .is_some_and(|gate| gate.human_ready()),
            );
        }
        if let Some(progress) = &self.human_progress {
            progress.record_clear_of_sign_in(
                self.view
                    .as_ref()
                    .and_then(|view| view.work_navigation())
                    .is_some_and(|gate| gate.human_clear_of_sign_in()),
            );
        }
        if let Some((progress, revision)) = self.human_progress.as_ref().zip(
            self.view
                .as_ref()
                .and_then(|view| view.work_navigation())
                .and_then(|gate| gate.human_revision()),
        ) {
            progress.record_revision(revision);
        }
        if erased
            || !presentation.current()
            || !work_browser_monotonic_now().is_some_and(|now| {
                self.guard.human_current(Operation::PresentHuman, now)
                    || self.guard.human_current(Operation::ContinueAfterHuman, now)
            })
            || !self.schedule_human_wake()
        {
            self.retire_human_presentation();
            #[cfg(target_os = "windows")]
            if let Some(view) = self.view.as_ref() {
                view.finish_work_human_activity(|_| {});
            }
            self.guard.fail();
        }
    }

    pub(super) fn retire_human_presentation(&mut self) -> bool {
        self.human_wake = None;
        self.human_progress = None;
        let clean = self
            .human_presentation
            .as_mut()
            .is_none_or(|presentation| presentation.retire());
        if clean {
            self.human_presentation = None;
        } else {
            self.retirement_clean = false;
            self.guard.fail();
        }
        clean
    }
}
