use super::*;
use std::time::{Duration, Instant};

pub(super) struct HumanNavigation {
    source: ContextNavigationTarget,
    /// A sign-in may pass through another site's sign-in pages.
    open: bool,
    current: ContextNavigationTarget,
    native_id: wry::NavigationId,
    requested: bool,
    loading: Option<wry::NavigationId>,
    committed: bool,
    finished: bool,
    revision: u64,
    deadline: Instant,
    #[cfg(target_os = "windows")]
    write_navigation: Option<(wry::NavigationId, u64, ContextNavigationTarget)>,
}

impl HumanNavigation {
    fn target(&self, raw: &str) -> Option<ContextNavigationTarget> {
        ContextNavigationTarget::parse(raw).ok().filter(|target| {
            target.as_url().as_str() == raw
                && (zephium_agentic::same_work_human_site(&self.source, target)
                    || (self.open && target.as_url().scheme() == "https"))
        })
    }
    /// Settled back on the page's own site and off any sign-in path.
    fn clear_of_sign_in(&self) -> bool {
        self.finished
            && self.loading.is_none()
            && zephium_agentic::same_work_human_site(&self.source, &self.current)
            && !self
                .current
                .as_url()
                .path_segments()
                .into_iter()
                .flatten()
                .any(|segment| {
                    let segment = segment.to_ascii_lowercase();
                    [
                        "login",
                        "signin",
                        "sign-in",
                        "sign_in",
                        "log-in",
                        "logon",
                        "sso",
                        "auth",
                        "oauth",
                        "authorize",
                        "2fa",
                        "mfa",
                        "verify",
                        "challenge",
                        "checkpoint",
                    ]
                    .contains(&segment.as_str())
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
        #[cfg(target_os = "windows")]
        if self
            .write_navigation
            .as_ref()
            .is_some_and(|(id, _, _)| *id == event.id)
            && event.phase != E::Started
        {
            self.write_navigation = None;
        }
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
                #[cfg(target_os = "windows")]
                {
                    self.write_navigation = Some((event.id, self.revision, target));
                }
                Ok((false, true))
            }
            E::Redirected if self.loading == Some(event.id) && !self.committed => {
                // A native precommit redirect is a new request in this same
                // Human navigation. Preserve its existing target policy while
                // minting only the exact redirect URI's one-use method join.
                // Work's confirmed-action POST token remains separate.
                #[cfg(target_os = "windows")]
                {
                    self.write_navigation = Some((event.id, self.revision, target));
                }
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
    #[cfg(target_os = "windows")]
    pub(super) fn allows_windows_request(&mut self, raw: &str, main_frame: Option<bool>) -> bool {
        if main_frame == Some(true) {
            return self.allows(raw, Some(false));
        }
        // Headers may not yet include Sec-Fetch-Dest. Correlate only this
        // Human lease's admitted main Started URI/ID and unchanged revision;
        // absence alone is never main-frame authority. The token is consumed
        // once, with the same same-URI subframe availability limit as Work.
        let current = Instant::now() < self.deadline
            && main_frame.is_none()
            && !self.committed
            && self
                .write_navigation
                .as_ref()
                .is_some_and(|(id, revision, target)| {
                    self.loading == Some(*id)
                        && self.revision == *revision
                        && target.as_url().as_str() == raw
                        && self.target(raw).is_some()
                });
        if current {
            self.write_navigation = None;
        }
        current
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
    pub(crate) fn human_clear_of_sign_in(&self) -> bool {
        self.0.lock().is_ok_and(|state| {
            state.phase == Phase::Human
                && state
                    .human
                    .as_ref()
                    .is_some_and(HumanNavigation::clear_of_sign_in)
        })
    }
    pub(crate) fn begin_human(
        &self,
        source: &ContextNavigationTarget,
        deadline: Instant,
        open: bool,
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
            open,
            current,
            native_id: state.native_id.ok_or(())?,
            requested: false,
            loading: None,
            committed: true,
            finished: true,
            revision: 0,
            deadline,
            #[cfg(target_os = "windows")]
            write_navigation: None,
        });
        state.follow = None;
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
            || !zephium_agentic::same_work_human_site(&human.source, &human.current)
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

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::super::tests::{event, ready_gate, URL};
    use super::*;
    use wry::NavigationEventPhase as E;

    fn human(open: bool) -> WorkDocumentNavigation {
        let gate = ready_gate();
        gate.begin_human(
            &ContextNavigationTarget::parse(URL).unwrap(),
            Instant::now() + Duration::from_secs(30),
            open,
        )
        .unwrap();
        gate
    }

    #[test]
    fn human_missing_destination_joins_one_exact_started_post_with_existing_site_policy() {
        for (open, target) in [
            (false, "https://example.test/login"),
            (true, "https://identity.test/login"),
        ] {
            let gate = human(open);
            assert!(!gate.allows_windows_request(target, "POST", None));
            assert!(gate.allows(target));
            assert!(!gate.allows_windows_request(target, "POST", None));
            gate.observe(event(2, E::Started, target)).unwrap();
            assert!(!gate.allows_windows_request("https://example.test/unrelated", "POST", None));
            assert!(!gate.allows_windows_request("https://attacker.test/login", "POST", None));
            assert!(gate.allows_windows_request(target, "POST", None));
            assert!(!gate.allows_windows_request(target, "POST", None));
            gate.observe(event(2, E::Committed, target)).unwrap();
            gate.observe(event(2, E::Finished, target)).unwrap();
            assert!(!gate.allows_windows_request(target, "POST", None));
        }
        assert!(!human(false).allows("https://identity.test/login"));
    }

    #[test]
    fn human_missing_destination_refuses_cancelled_expired_and_changed_identity() {
        let target = "https://example.test/login";
        for failure in 0..4 {
            let gate = human(false);
            assert!(gate.allows(target));
            gate.observe(event(2, E::Started, target)).unwrap();
            match failure {
                0 => {
                    gate.observe(event(2, E::Cancelled, target)).unwrap();
                }
                1 => {
                    gate.0.lock().unwrap().human.as_mut().unwrap().deadline = Instant::now();
                }
                2 => {
                    gate.0.lock().unwrap().human.as_mut().unwrap().revision += 1;
                }
                _ => {
                    gate.0.lock().unwrap().human.as_mut().unwrap().loading =
                        Some(wry::NavigationId::from_raw(3));
                }
            }
            assert!(!gate.allows_windows_request(target, "POST", None));
            gate.retire();
            assert!(!gate.allows_windows_request(target, "POST", None));
        }
    }

    #[test]
    fn human_missing_destination_redirect_post_uses_only_current_native_target() {
        let first = "https://example.test/login";
        let redirected = "https://example.test/signin/continue";
        let gate = human(false);
        assert!(gate.allows(first));
        gate.observe(event(2, E::Started, first)).unwrap();
        assert!(gate.allows_windows_request(first, "POST", None));
        assert!(!gate.allows_windows_request(first, "POST", None));
        assert!(gate.allows(redirected));
        gate.observe(event(2, E::Redirected, redirected)).unwrap();
        assert!(!gate.allows_windows_request(first, "POST", None));
        assert!(gate.allows_windows_request(redirected, "POST", None));
        assert!(!gate.allows_windows_request(redirected, "POST", None));
        let external = "https://identity.test/continue";
        let gate = human(true);
        assert!(gate.allows(first));
        gate.observe(event(2, E::Started, first)).unwrap();
        assert!(gate.allows(external));
        gate.observe(event(2, E::Redirected, external)).unwrap();
        assert!(gate.allows_windows_request(external, "POST", None));
        assert!(!gate.allows_windows_request(external, "POST", None));
        for (id, target) in [(3, redirected), (2, external)] {
            let gate = human(false);
            assert!(gate.allows(first));
            gate.observe(event(2, E::Started, first)).unwrap();
            gate.observe(event(id, E::Redirected, target)).unwrap();
            assert!(gate.failed());
            assert!(!gate.allows_windows_request(target, "POST", None));
        }
    }

    #[test]
    fn human_missing_destination_token_cannot_survive_handoff_end_or_successor() {
        let gate = human(false);
        assert!(gate.allows(URL));
        gate.observe(event(2, E::Started, URL)).unwrap();
        gate.observe(event(2, E::Committed, URL)).unwrap();
        gate.observe(event(2, E::Finished, URL)).unwrap();
        let revision = gate.human_revision().unwrap();
        gate.finish_human(URL, revision).unwrap();
        assert!(!gate.allows_windows_request(URL, "POST", None));
        gate.begin_human(
            &ContextNavigationTarget::parse(URL).unwrap(),
            Instant::now() + Duration::from_secs(30),
            false,
        )
        .unwrap();
        assert!(!gate.allows_windows_request(URL, "POST", None));
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
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30), false)
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
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30), false)
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
        assert!(gate.begin_human(&source, Instant::now(), false).is_err());
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30), false)
            .unwrap();
        assert!(gate.allows_apple_action(URL, apple_action(T::Reload, true)));
        gate.observe(event(2, E::Started, URL)).unwrap();
        gate.observe(event(2, E::Redirected, "https://attacker.test/"))
            .unwrap();
        assert!(gate.failed());
        assert!(gate.finish_human(URL, 0).is_err());
    }

    #[test]
    fn a_sign_in_may_pass_through_another_site_but_must_settle_back() {
        let gate = ready_gate();
        let source = ContextNavigationTarget::parse(URL).unwrap();
        gate.begin_human(&source, Instant::now() + Duration::from_secs(30), true)
            .unwrap();
        let idp = "https://accounts.google.com/signin";
        assert!(gate.allows_apple_action(idp, apple_action(T::LinkActivated, true)));
        assert!(
            !gate.allows_apple_action("http://plain.test/", apple_action(T::LinkActivated, true))
        );
        gate.observe(event(2, E::Started, idp)).unwrap();
        gate.observe(event(2, E::Committed, idp)).unwrap();
        gate.observe(event(2, E::Finished, idp)).unwrap();
        assert!(!gate.human_clear_of_sign_in());
        let revision = gate.human_revision().unwrap();
        assert!(gate.finish_human(idp, revision).is_err());
        let back = "https://example.test/home";
        assert!(gate.allows_apple_action(back, apple_action(T::Other, true)));
        gate.observe(event(3, E::Started, back)).unwrap();
        gate.observe(event(3, E::Committed, back)).unwrap();
        gate.observe(event(3, E::Finished, back)).unwrap();
        assert!(gate.human_clear_of_sign_in());
        gate.finish_human(back, gate.human_revision().unwrap())
            .unwrap();
    }
}
