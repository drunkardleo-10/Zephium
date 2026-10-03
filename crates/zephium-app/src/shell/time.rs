//! Time on the web and focus sessions. Attention is re-derived from shell
//! state after every command and only changes produce work; tallies reach the
//! store on the existing maintenance tick, and focus wakes once per phase.

use super::*;

use crate::api::TimeCompletion;
use zephium_core::time::{
    Attention, FocusEvent, FocusPhase, FocusPlan, FocusRecord, FocusSession, Ledger, Place,
    TimeQuery, TimeReport, Tracker, HOUR_MS, MAX_BLOCKED_SITES,
};
use zephium_ipc::{
    FocusControl, FocusDayView, FocusPhaseView, FocusStatus, FocusView, IconSurface,
    SiteTimeView, TimeBucketView, TimeCall, TimeError, TimeResponse,
};

/// The running session, kept so a relaunch picks it up where it was.
const SESSION_KEY: &str = "focus.session";
const DEFAULT_RETENTION_DAYS: i64 = 90;

pub(super) struct TimeState {
    tracker: Tracker,
    ledger: Ledger,
    app_active: bool,
    awake: bool,
    enabled: bool,
    retention_days: i64,
    focus: Option<FocusSession>,
    blocked: Vec<String>,
}

impl TimeState {
    pub(super) fn load(store: &SharedStore) -> Self {
        let setting = |key: &str| store.app_setting(key).unwrap_or_default();
        let mut state = Self {
            tracker: Tracker::default(),
            ledger: Ledger::default(),
            app_active: false,
            awake: true,
            enabled: true,
            retention_days: DEFAULT_RETENTION_DAYS,
            focus: serde_json::from_str::<FocusSession>(&setting(SESSION_KEY))
                .ok()
                .filter(FocusSession::valid),
            blocked: Vec::new(),
        };
        for key in ["time.track", "time.retention", "focus.blocked"] {
            state.apply_setting(key, &setting(key));
        }
        state
    }

    fn apply_setting(&mut self, key: &str, value: &str) {
        match key {
            "time.track" => self.enabled = value != "false",
            "time.retention" => {
                self.retention_days = value.parse().unwrap_or(DEFAULT_RETENTION_DAYS);
            }
            "focus.blocked" => self.blocked = blocked_sites(value),
            _ => {}
        }
    }
}

/// The block list as stored: one normalized site per line.
fn blocked_sites(value: &str) -> Vec<String> {
    value
        .lines()
        .filter(|line| !line.is_empty())
        .take(MAX_BLOCKED_SITES)
        .map(str::to_owned)
        .collect()
}

/// Wall-clock milliseconds as the local zone reads them.
fn local_ms(utc_ms: i64) -> i64 {
    use chrono::TimeZone;
    let offset = chrono::Local
        .timestamp_millis_opt(utc_ms)
        .single()
        .map_or(0, |local| i64::from(local.offset().local_minus_utc()));
    utc_ms + offset * 1000
}

fn utc_now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn local_day(utc_ms: i64) -> i64 {
    local_ms(utc_ms).div_euclid(24 * HOUR_MS)
}

/// A page's registrable domain, so `m.youtube.com` and `www.youtube.com`
/// are one site. Hosts without a known suffix, such as `localhost`, stay
/// whole.
fn site_of(url: &url::Url) -> Option<String> {
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    let site = psl::domain(host.as_bytes())
        .filter(|domain| domain.suffix().is_known())
        .and_then(|domain| std::str::from_utf8(domain.as_bytes()).ok())
        .map_or_else(|| host.clone(), str::to_owned);
    (!site.is_empty() && site.len() <= zephium_core::time::MAX_SITE_BYTES).then_some(site)
}

fn seconds(ms: i64) -> u32 {
    u32::try_from(ms.max(0) / 1000).unwrap_or(u32::MAX)
}

fn phase_view(phase: FocusPhase) -> FocusPhaseView {
    match phase {
        FocusPhase::Focus => FocusPhaseView::Focus,
        FocusPhase::Break => FocusPhaseView::Break,
        FocusPhase::LongBreak => FocusPhaseView::LongBreak,
    }
}

impl Shell {
    fn time_attention(&self) -> Option<Attention> {
        let time = &self.time;
        if !time.enabled || !time.app_active || !time.awake || !self.window_visible {
            return None;
        }
        let window = self.windows.focused()?;
        let profile = window.profile;
        let counted = self
            .profiles
            .get(profile)
            .is_some_and(|p| p.kind != ProfileKind::Incognito)
            && !self.degraded_storage_profiles.contains(&profile)
            && !self.profile_deletion_quarantines(profile);
        if !counted {
            return None;
        }
        let place = match self.active_browser_page() {
            Some(crate::BrowserPage::Work) => Place::Work,
            Some(_) => return None,
            None => {
                let tab = self.items.tab(window.active?)?;
                if tab.content != zephium_core::item::TabContent::Web {
                    return None;
                }
                Place::Site(site_of(tab.url.as_ref()?)?)
            }
        };
        Some(Attention { profile, place })
    }

    /// Follows attention after a command. Unchanged attention costs one
    /// comparison and no write.
    pub(super) fn refresh_time(&mut self) {
        let next = self.time_attention();
        if self.time.tracker.current() == next.as_ref() {
            return;
        }
        let wall = local_ms(utc_now_ms());
        let transition = self.time.tracker.set(next, std::time::Instant::now(), wall);
        if let Some(segment) = transition.closed {
            self.time.ledger.spend(&segment);
        }
        if let Some((profile, site)) = transition.opened {
            self.time.ledger.open(profile, site, wall);
        }
    }

    /// Writes what has been counted, up to now, for every profile or one.
    pub(super) fn flush_time(&mut self, only: Option<ProfileId>) {
        let wall = local_ms(utc_now_ms());
        if let Some(segment) = self
            .time
            .tracker
            .checkpoint(std::time::Instant::now(), wall)
        {
            self.time.ledger.spend(&segment);
        }
        let keep_from_hour = wall.div_euclid(HOUR_MS) - self.time.retention_days * 24;
        for profile in self.time.ledger.profiles() {
            if only.is_some_and(|only| only != profile) {
                continue;
            }
            let tallies = self.time.ledger.take(profile);
            if !self
                .store
                .record_time(profile, tallies.clone(), keep_from_hour)
            {
                self.time.ledger.restore(profile, tallies);
            }
        }
    }

    pub(super) fn set_app_active(&mut self, active: bool) {
        self.time.app_active = active;
    }

    /// Sleep, a locked screen or a dark display stop the clock; waking runs
    /// the focus clock forward, since a timer may have slept through its end.
    pub(super) fn set_system_awake(&mut self, awake: bool) {
        self.time.awake = awake;
        if awake {
            self.focus_wake();
        }
    }

    pub(super) fn apply_time_setting(&mut self, key: &str, value: &str) {
        let was_enabled = self.time.enabled;
        let was_blocked = self.time.blocked.clone();
        self.time.apply_setting(key, value);
        if was_enabled && !self.time.enabled {
            self.flush_time(None);
        }
        if was_blocked != self.time.blocked {
            self.project_focus();
        }
    }

    /// Clearing history clears the time spent over the same span.
    pub(super) fn clear_time(&mut self, profile: ProfileId, since_seconds: Option<i64>) {
        let since_hour =
            since_seconds.map(|since| local_ms(since.saturating_mul(1000)).div_euclid(HOUR_MS));
        self.flush_time(Some(profile));
        self.time.tracker.forget(profile);
        self.time
            .ledger
            .forget(profile, since_hour.map(|hour| hour * HOUR_MS));
        let _ = self.store.clear_time(profile, since_hour);
    }

    pub(super) fn time_call(
        &mut self,
        expected_profile: ProfileId,
        call: TimeCall,
        done: TimeCompletion,
    ) {
        let focused = self
            .windows
            .focused()
            .is_some_and(|window| window.profile == expected_profile);
        if !focused || !call.validate() {
            done.finish(TimeResponse::Error {
                error: TimeError::Invalid,
            });
            return;
        }
        let unavailable = |done: TimeCompletion| {
            done.finish(TimeResponse::Error {
                error: TimeError::Unavailable,
            })
        };
        match call {
            TimeCall::Report {
                from_hour,
                bucket_hours,
                buckets,
                site,
            } => {
                let private = self
                    .profiles
                    .get(expected_profile)
                    .is_none_or(|p| p.kind == ProfileKind::Incognito);
                if private || self.degraded_storage_profiles.contains(&expected_profile) {
                    unavailable(done);
                    return;
                }
                // The store runs writes and reads in one queue, so this report
                // sees everything counted up to now.
                self.flush_time(Some(expected_profile));
                let callback = self.self_queue.as_ref().map(|queue| CallbackHandle {
                    queue: Arc::downgrade(&queue.inner),
                });
                let returned = done.clone();
                let accepted = self.store.time_report(
                    expected_profile,
                    TimeQuery {
                        from_hour: i64::from(from_hour),
                        bucket_hours,
                        buckets,
                        site,
                    },
                    Box::new(move |report| {
                        let delivered = callback.is_some_and(|callback| {
                            callback.dispatch(Command::TimeReportRead {
                                profile: expected_profile,
                                report,
                                done: returned.clone(),
                            })
                        });
                        if !delivered {
                            unavailable(returned);
                        }
                    }),
                );
                if !accepted {
                    unavailable(done);
                }
            }
            TimeCall::FocusDays { from_day, days } => {
                let returned = done.clone();
                let accepted = self.store.focus_days(
                    i64::from(from_day),
                    days,
                    Box::new(move |days| match days {
                        Some(days) => returned.finish(TimeResponse::FocusDays {
                            days: days
                                .into_iter()
                                .filter_map(|day| {
                                    Some(FocusDayView {
                                        day: i32::try_from(day.day).ok()?,
                                        seconds: seconds(day.focused_ms),
                                        sessions: day.sessions,
                                        completed: day.completed,
                                    })
                                })
                                .collect(),
                        }),
                        None => unavailable(returned),
                    }),
                );
                if !accepted {
                    unavailable(done);
                }
            }
        }
    }

    pub(super) fn on_time_report(
        &mut self,
        profile: ProfileId,
        report: Option<TimeReport>,
        done: TimeCompletion,
    ) {
        let Some(report) = report else {
            done.finish(TimeResponse::Error {
                error: TimeError::Unavailable,
            });
            return;
        };
        let bucket = |bucket: zephium_core::time::BucketTime| TimeBucketView {
            browse: seconds(bucket.browse_ms),
            work: seconds(bucket.work_ms),
        };
        let pages: Vec<String> = report
            .sites
            .iter()
            .map(|entry| format!("https://{}/", entry.site))
            .collect();
        let sites = report
            .sites
            .into_iter()
            .zip(&pages)
            .map(|(entry, page)| SiteTimeView {
                icon: self.icon_ref_for_url(IconSurface::Chrome, profile, page),
                site: entry.site,
                seconds: seconds(entry.spent_ms),
                opens: entry.opens,
                series: entry.series.into_iter().map(seconds).collect(),
            })
            .collect::<Vec<_>>();
        self.want_icons(
            IconSurface::Chrome,
            profile,
            sites
                .iter()
                .zip(&pages)
                .filter(|(site, _)| site.icon.is_none())
                .map(|(_, page)| page.as_str()),
        );
        self.publish_icons();
        done.finish(TimeResponse::Report {
            buckets: report.buckets.into_iter().map(bucket).collect(),
            previous: bucket(report.previous),
            sites,
        });
    }

    pub(super) fn operation_focus(&mut self, control: FocusControl) -> OperationDisposition {
        if !control.validate() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        let now = utc_now_ms();
        match control {
            FocusControl::Start { minutes, breaks } => {
                if let Some(running) = self.time.focus.take() {
                    self.record_focus(running.stop(now));
                }
                let plan = FocusPlan {
                    minutes: u16::try_from(minutes).unwrap_or(25),
                    breaks,
                };
                self.time.focus = Some(FocusSession::start(plan, now));
            }
            FocusControl::Stop => {
                let Some(running) = self.time.focus.take() else {
                    return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
                };
                self.record_focus(running.stop(now));
            }
            FocusControl::Allow { site } => {
                let allowed = self.time.focus.as_mut().is_some_and(|session| {
                    zephium_core::time::normalize_site(&site)
                        .is_some_and(|site| session.allow(site, now))
                });
                if !allowed {
                    return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
                }
            }
            FocusControl::Skip => {
                let Some(session) = self.time.focus.as_mut() else {
                    return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
                };
                session.phase_ends_ms = now.max(session.phase_started_ms + 1);
                self.focus_wake();
                return operation_result(OperationOutcome::Applied, OperationReason::MutationApplied);
            }
        }
        self.focus_changed();
        operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
    }

    /// Runs the focus clock to now: phases that ended move on, and a single
    /// round that ran out is kept and closed.
    pub(super) fn focus_wake(&mut self) {
        let Some(session) = self.time.focus.as_mut() else {
            self.schedule_focus_wake();
            return;
        };
        let (events, finished) = session.advance(utc_now_ms());
        if events.is_empty() {
            self.schedule_focus_wake();
            return;
        }
        if let Some(record) = finished {
            self.time.focus = None;
            self.record_focus(record);
        }
        if let Some(last) = events.last() {
            let alert = match last {
                FocusEvent::Finished => "finished",
                FocusEvent::Phase {
                    began: FocusPhase::Focus,
                    ..
                } => "focus",
                FocusEvent::Phase { .. } => "break",
            };
            (self.emit)(Projection::UiCommand(format!("focus.alert={alert}")));
        }
        self.focus_changed();
    }

    fn record_focus(&mut self, record: FocusRecord) {
        if record.focused_ms > 0 {
            let _ = self
                .store
                .record_focus(record, local_day(record.started_ms));
        }
    }

    fn focus_changed(&mut self) {
        let saved = self
            .time
            .focus
            .as_ref()
            .and_then(|session| serde_json::to_string(session).ok())
            .unwrap_or_default();
        let _ = self.store.set_app_setting(SESSION_KEY.to_owned(), saved);
        self.schedule_focus_wake();
        self.project_focus();
    }

    fn schedule_focus_wake(&self) {
        let Some(queue) = &self.self_queue else {
            return;
        };
        let deadline = self.time.focus.as_ref().map(|session| {
            let wait = (session.next_change_ms() - utc_now_ms()).max(0);
            std::time::Instant::now()
                + std::time::Duration::from_millis(u64::try_from(wait).unwrap_or(0))
        });
        queue.schedule_focus(deadline);
    }

    pub(super) fn project_focus(&self) {
        let session = self.time.focus.as_ref().map(|session| {
            let (short, long) = session.plan.break_minutes();
            FocusView {
                phase: phase_view(session.phase),
                started_at: session.started_ms,
                phase_started_at: session.phase_started_ms,
                phase_ends_at: session.phase_ends_ms,
                minutes: u32::from(session.plan.minutes),
                breaks: session.plan.breaks,
                break_minutes: u32::from(short),
                long_break_minutes: u32::from(long),
                rounds: session.rounds,
                focused: seconds(session.focused_ms),
                allowed: session
                    .allowances
                    .iter()
                    .map(|(site, _)| site.clone())
                    .collect(),
            }
        });
        (self.emit)(Projection::Focus(FocusStatus {
            session,
            blocked: self.time.blocked.clone(),
        }));
    }

    /// Picks a restored session back up once the actor can schedule it.
    pub(super) fn resume_focus(&mut self) {
        if self.time.focus.is_some() {
            self.focus_wake();
            self.schedule_focus_wake();
        }
        self.project_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_group_by_registrable_domain() {
        let site = |url: &str| site_of(&url::Url::parse(url).unwrap());
        assert_eq!(site("https://m.youtube.com/watch").as_deref(), Some("youtube.com"));
        assert_eq!(site("https://www.bbc.co.uk/news").as_deref(), Some("bbc.co.uk"));
        assert_eq!(site("https://docs.google.com/").as_deref(), Some("google.com"));
        assert_eq!(site("http://localhost:5173/").as_deref(), Some("localhost"));
        assert_eq!(site("about:blank"), None);
    }

    #[test]
    fn the_block_list_is_one_site_per_line() {
        assert_eq!(
            blocked_sites("x.com\n\nyoutube.com\n"),
            vec!["x.com".to_owned(), "youtube.com".to_owned()]
        );
    }
}
