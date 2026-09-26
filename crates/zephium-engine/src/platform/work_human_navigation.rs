use super::*;
use std::time::{Duration, Instant};

pub(super) struct HumanNavigation {
    source: ContextNavigationTarget,
    current: ContextNavigationTarget,
    native_id: wry::NavigationId,
    requested: bool,
    loading: Option<wry::NavigationId>,
    committed: bool,
    finished: bool,
    revision: u64,
    deadline: Instant,
}

impl HumanNavigation {
    fn target(&self, raw: &str) -> Option<ContextNavigationTarget> {
        ContextNavigationTarget::parse(raw).ok().filter(|target| {
            target.as_url().as_str() == raw
                && zephium_agentic::same_work_human_site(&self.source, target)
        })
    }
    pub(super) fn allows(&mut self, raw: &str, main_frame: Option<bool>) -> bool {
        if Instant::now() >= self.deadline {
            return false;
        }
        if main_frame == Some(false) {
            return true;
        }
        if main_frame != Some(true) || self.target(raw).is_none() {
            return false;
        }
        if self.loading.is_some() {
            return !self.committed;
        }
        self.requested = true;
        self.finished = false;
        true
    }
    pub(super) fn observe(&mut self, event: wry::NavigationEvent) -> Result<(bool, bool), ()> {
        use wry::NavigationEventPhase as E;
        if Instant::now() >= self.deadline {
            return Err(());
        }
        let target = self.target(&event.url).ok_or(())?;
        match event.phase {
            E::Started if self.requested => {
                self.requested = false;
                self.loading = Some(event.id);
                self.committed = false;
                self.finished = false;
                Ok((false, true))
            }
            E::Redirected if self.loading == Some(event.id) && !self.committed => {
                Ok((false, false))
            }
            E::Committed if self.loading == Some(event.id) && !self.committed => {
                self.native_id = event.id;
                self.current = target;
                self.committed = true;
                self.revision = self.revision.checked_add(1).ok_or(())?;
                Ok((true, true))
            }
            E::Finished if self.loading == Some(event.id) && self.committed => {
                self.current = target;
                self.loading = None;
                self.finished = true;
                Ok((false, true))
            }
            E::Failed if self.loading == Some(event.id) => {
                self.loading = None;
                self.finished = false;
                Ok((false, true))
            }
            _ => Err(()),
        }
    }
    pub(super) fn location_changed(&mut self, raw: Option<&str>) -> Result<bool, ()> {
        if Instant::now() >= self.deadline {
            return Err(());
        }
        let current = self.target(raw.ok_or(())?).ok_or(())?;
        if self.current == current {
            return Ok(false);
        }
        self.current = current;
        self.revision = self.revision.checked_add(1).ok_or(())?;
        Ok(true)
    }
}

impl WorkDocumentNavigation {
    pub(crate) fn human_ready(&self) -> bool {
        self.0.lock().is_ok_and(|state| {
            state.phase == Phase::Human
                && state.human.as_ref().is_some_and(|human| {
                    Instant::now() < human.deadline
                        && human.finished
                        && human.loading.is_none()
                        && !human.requested
                })
        })
    }
    pub(crate) fn human_load_preparation_needed(&self, main_frame: Option<bool>) -> bool {
        main_frame == Some(true)
            && self.0.lock().is_ok_and(|state| {
                state.phase == Phase::Human
                    && state
                        .human
                        .as_ref()
                        .is_some_and(|human| human.loading.is_none() && !human.requested)
            })
    }
    pub(crate) fn begin_human(
        &self,
        source: &ContextNavigationTarget,
        deadline: Instant,
    ) -> Result<(), ()> {
        let now = Instant::now();
        if deadline <= now
            || deadline.duration_since(now)
                > Duration::from_millis(zephium_agentic::MAX_WORK_HUMAN_WAIT_MILLIS)
        {
            return Err(());
        }
        let mut state = self.0.lock().map_err(|_| ())?;
        let site = state.policy == zephium_agentic::WorkBrowserDocumentPolicy::SiteSession;
        let current = state.effective.clone().ok_or(())?;
        if state.phase != Phase::Ready
            || state.human.is_some()
            || (current != *source && !(site && zephium_agentic::same_work_site(source, &current)))
        {
            return Err(());
        }
        state.human = Some(HumanNavigation {
            source: source.clone(),
            current,
            native_id: state.native_id.ok_or(())?,
            requested: false,
            loading: None,
            committed: true,
            finished: true,
            revision: 0,
            deadline,
        });
        state.phase = Phase::Human;
        state.operation = None;
        Ok(())
    }

    pub(crate) fn human_revision(&self) -> Option<u64> {
        let state = self.0.lock().ok()?;
        (state.phase == Phase::Human)
            .then(|| state.human.as_ref().map(|human| human.revision))
            .flatten()
    }

    /// The caller must hide and retire native input before freezing this gate.
    pub(crate) fn finish_human(
        &self,
        current: &str,
        revision: u64,
    ) -> Result<ContextNavigationTarget, ()> {
        let mut state = self.0.lock().map_err(|_| ())?;
        let human = state.human.as_ref().ok_or(())?;
        if state.phase != Phase::Human
            || Instant::now() >= human.deadline
            || !human.finished
            || human.loading.is_some()
            || human.requested
            || human.revision != revision
            || human.current.as_url().as_str() != current
        {
            return Err(());
        }
        let target = human.current.clone();
        let id = human.native_id;
        let epoch = state.navigation_epoch.checked_add(1).ok_or(())?;
        state.human = None;
        state.navigation_epoch = epoch;
        state.native_id = Some(id);
        state.target = Some(target.clone());
        state.effective = Some(target.clone());
        state.operation = None;
        state.requested = false;
        state.admission_kind = None;
        state.phase = Phase::Ready;
        Ok(target)
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod tests {
    use super::super::tests::{apple_action, event, ready_gate, URL};
    use super::*;
    use wry::{AppleNavigationType as T, NavigationEventPhase as E};

    #[test]
    fn human_navigation_seals_agent_stamps_and_accepts_only_the_original_origin() {
        let gate = ready_gate();
        let source = ContextNavigationTarget::parse(URL).unwrap();
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30))
            .unwrap();
        assert!(gate.ready_target().is_none());
        assert!(!gate.ready(Some(URL)));
        for foreign in [
            "https://example.test.attacker.test/",
            "https://attacker.test/",
            "http://example.test/",
            "https://example.test:8443/",
        ] {
            assert!(!gate.allows_apple_action(foreign, apple_action(T::LinkActivated, true)));
        }
        let current = "https://example.test/en.html";
        assert!(gate.allows_apple_action(current, apple_action(T::FormSubmitted, false)));
        gate.observe(event(2, E::Started, current)).unwrap();
        assert!(gate.finish_human(current, 0).is_err());
        gate.observe(event(2, E::Committed, current)).unwrap();
        let revision = gate.human_revision().unwrap();
        gate.observe(event(2, E::Finished, current)).unwrap();
        assert!(gate.finish_human(current, revision - 1).is_err());
        gate.finish_human(current, revision).unwrap();
        assert!(gate.ready(Some(current)));
        assert!(!gate.allows_apple_action(current, apple_action(T::Reload, true)));
        assert!(gate.finish_human(current, revision).is_err());
    }

    #[test]
    fn human_form_followup_before_start_keeps_one_navigation_episode() {
        let gate = ready_gate();
        let source = ContextNavigationTarget::parse(URL).unwrap();
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30))
            .unwrap();
        assert!(gate.allows_apple_action(URL, apple_action(T::FormSubmitted, false)));
        assert!(gate.allows_apple_action(URL, apple_action(T::FormSubmitted, true)));
        assert!(!gate.allows_apple_action(
            "https://attacker.test/",
            apple_action(T::FormSubmitted, true)
        ));
        assert!(!gate.human_ready());
        assert!(!gate.ready(Some(URL)));
        assert!(!gate.human_load_preparation_needed(Some(true)));
        assert!(gate.finish_human(URL, 0).is_err());
        gate.observe(event(2, E::Started, URL)).unwrap();
        gate.observe(event(2, E::Committed, URL)).unwrap();
        assert!(gate.finish_human(URL, 1).is_err());
        gate.observe(event(2, E::Finished, URL)).unwrap();
        assert!(gate.human_ready());
        assert_eq!(gate.human_revision(), Some(1));
        gate.finish_human(URL, 1).unwrap();
        assert!(gate.ready(Some(URL)));
        assert!(!gate.allows_apple_action(URL, apple_action(T::FormSubmitted, true)));
    }

    #[test]
    fn foreign_redirect_and_expired_human_window_cannot_be_frozen() {
        let gate = ready_gate();
        let source = ContextNavigationTarget::parse(URL).unwrap();
        assert!(gate.begin_human(&source, Instant::now()).is_err());
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30))
            .unwrap();
        assert!(gate.allows_apple_action(URL, apple_action(T::Reload, true)));
        gate.observe(event(2, E::Started, URL)).unwrap();
        gate.observe(event(2, E::Redirected, "https://attacker.test/"))
            .unwrap();
        assert!(gate.failed());
        assert!(gate.finish_human(URL, 0).is_err());
    }
}
