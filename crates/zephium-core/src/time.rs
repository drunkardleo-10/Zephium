//! Time: where attention went while Zephium was in front, kept per site in
//! hour buckets, and focus sessions that keep chosen sites shut.
//!
//! Attention is counted from transitions alone: a segment is the monotonic
//! time between two of them, so nothing ticks while a person browses.

use std::collections::HashMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::ids::ProfileId;

pub const HOUR_MS: i64 = 3_600_000;
const MINUTE_MS: i64 = 60_000;
/// A DNS name is at most 253 bytes.
pub const MAX_SITE_BYTES: usize = 253;
pub const MAX_BLOCKED_SITES: usize = 256;
/// Unwritten tallies held between flushes. One flush a minute keeps the real
/// count tiny; the bound only stops a pathological navigation storm.
pub const MAX_PENDING_TALLIES: usize = 4096;
/// Allowances one session holds at once; each is one site let through briefly.
const MAX_ALLOWANCES: usize = 32;
pub const ALLOWANCE_MS: i64 = 5 * MINUTE_MS;
pub const MIN_FOCUS_MINUTES: u16 = 5;
pub const MAX_FOCUS_MINUTES: u16 = 180;
/// A long break follows every fourth focus round.
const ROUNDS_PER_LONG_BREAK: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Place {
    /// A registrable domain, such as `youtube.com`.
    Site(String),
    Work,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attention {
    pub profile: ProfileId,
    pub place: Place,
}

/// Attention held for `duration_ms`, ending at local wall-clock `end_ms`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub profile: ProfileId,
    pub place: Place,
    pub end_ms: i64,
    pub duration_ms: i64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Transition {
    pub closed: Option<Segment>,
    /// A site newly come to, as opposed to returned to after a gap.
    pub opened: Option<(ProfileId, String)>,
}

#[derive(Debug, Default)]
pub struct Tracker {
    current: Option<(Attention, Instant)>,
    last_site: HashMap<ProfileId, String>,
}

impl Tracker {
    pub fn current(&self) -> Option<&Attention> {
        self.current.as_ref().map(|(attention, _)| attention)
    }

    /// Moves attention to `next`. Durations come from the monotonic clock and
    /// only their placement from the wall clock, so a clock change never
    /// stretches or shrinks a segment.
    pub fn set(&mut self, next: Option<Attention>, now: Instant, wall_ms: i64) -> Transition {
        if self.current.as_ref().map(|(attention, _)| attention) == next.as_ref() {
            return Transition::default();
        }
        let closed = self.close(now, wall_ms);
        let opened = match &next {
            Some(Attention {
                profile,
                place: Place::Site(site),
            }) if self.last_site.get(profile) != Some(site) => {
                self.last_site.insert(*profile, site.clone());
                Some((*profile, site.clone()))
            }
            _ => None,
        };
        self.current = next.map(|attention| (attention, now));
        Transition { closed, opened }
    }

    /// Closes the running segment and keeps the same attention from `now`, so
    /// a flush or a report sees time up to this moment.
    pub fn checkpoint(&mut self, now: Instant, wall_ms: i64) -> Option<Segment> {
        let closed = self.close(now, wall_ms);
        if let Some((_, since)) = &mut self.current {
            *since = now;
        }
        closed
    }

    /// Drops what a profile was last on, so its next site counts as new.
    pub fn forget(&mut self, profile: ProfileId) {
        self.last_site.remove(&profile);
        if self
            .current
            .as_ref()
            .is_some_and(|(attention, _)| attention.profile == profile)
        {
            self.current = None;
        }
    }

    fn close(&self, now: Instant, wall_ms: i64) -> Option<Segment> {
        let (attention, since) = self.current.as_ref()?;
        let duration_ms =
            i64::try_from(now.saturating_duration_since(*since).as_millis()).unwrap_or(i64::MAX);
        (duration_ms > 0).then(|| Segment {
            profile: attention.profile,
            place: attention.place.clone(),
            end_ms: wall_ms,
            duration_ms,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub spent_ms: i64,
    pub opens: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HourTally {
    /// Local hour: local wall-clock milliseconds since the epoch divided by
    /// [`HOUR_MS`]. Local, so a day is always 24 consecutive hours, even in a
    /// zone whose offset is not whole hours.
    pub hour: i64,
    pub place: Place,
    pub tally: Tally,
}

/// Tallies not yet written, per profile, hour and place.
#[derive(Debug, Default)]
pub struct Ledger {
    tallies: HashMap<(ProfileId, i64, Place), Tally>,
}

impl Ledger {
    pub fn is_empty(&self) -> bool {
        self.tallies.is_empty()
    }

    /// Spreads a segment across the hours it covers, back from its end.
    pub fn spend(&mut self, segment: &Segment) {
        let mut end = segment.end_ms;
        let mut left = segment.duration_ms;
        while left > 0 {
            let hour = (end - 1).div_euclid(HOUR_MS);
            let within = end - hour * HOUR_MS;
            let taken = left.min(within);
            if let Some(tally) = self.entry(segment.profile, hour, &segment.place) {
                tally.spent_ms = tally.spent_ms.saturating_add(taken);
            }
            left -= taken;
            end -= taken;
        }
    }

    pub fn open(&mut self, profile: ProfileId, site: String, wall_ms: i64) {
        let hour = wall_ms.div_euclid(HOUR_MS);
        if let Some(tally) = self.entry(profile, hour, &Place::Site(site)) {
            tally.opens = tally.opens.saturating_add(1);
        }
    }

    pub fn profiles(&self) -> Vec<ProfileId> {
        let mut profiles: Vec<_> = self.tallies.keys().map(|(profile, ..)| *profile).collect();
        profiles.sort_unstable();
        profiles.dedup();
        profiles
    }

    pub fn take(&mut self, profile: ProfileId) -> Vec<HourTally> {
        let mut taken = Vec::new();
        self.tallies.retain(|(owner, hour, place), tally| {
            if *owner != profile {
                return true;
            }
            taken.push(HourTally {
                hour: *hour,
                place: place.clone(),
                tally: *tally,
            });
            false
        });
        taken.sort_unstable_by(|a, b| (a.hour, &a.place).cmp(&(b.hour, &b.place)));
        taken
    }

    /// Puts back tallies a write did not take, adding to anything newer.
    pub fn restore(&mut self, profile: ProfileId, tallies: Vec<HourTally>) {
        for HourTally { hour, place, tally } in tallies {
            if let Some(kept) = self.entry(profile, hour, &place) {
                kept.spent_ms = kept.spent_ms.saturating_add(tally.spent_ms);
                kept.opens = kept.opens.saturating_add(tally.opens);
            }
        }
    }

    /// Discards a profile's unwritten time, from `since_ms` on when given.
    pub fn forget(&mut self, profile: ProfileId, since_ms: Option<i64>) {
        let since = since_ms.map_or(i64::MIN, |since| since.div_euclid(HOUR_MS));
        self.tallies
            .retain(|(owner, hour, _), _| *owner != profile || *hour < since);
    }

    fn entry(&mut self, profile: ProfileId, hour: i64, place: &Place) -> Option<&mut Tally> {
        let key = (profile, hour, place.clone());
        if !self.tallies.contains_key(&key) && self.tallies.len() >= MAX_PENDING_TALLIES {
            return None;
        }
        Some(self.tallies.entry(key).or_default())
    }
}

/// Buckets one report may hold: a month of days, or a day of hours.
pub const MAX_REPORT_BUCKETS: u32 = 31;
/// Sites a report ranks.
pub const MAX_REPORT_SITES: usize = 50;
/// Leading sites whose time is also given bucket by bucket, for a chart.
pub const HIGHLIGHTED_SITES: usize = 4;

/// Time over `buckets` consecutive spans of `bucket_hours` local hours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeQuery {
    pub from_hour: i64,
    pub bucket_hours: u32,
    pub buckets: u32,
    /// Narrows the report to one site.
    pub site: Option<String>,
}

impl TimeQuery {
    pub fn valid(&self) -> bool {
        matches!(self.bucket_hours, 1 | 24)
            && (1..=MAX_REPORT_BUCKETS).contains(&self.buckets)
            && self
                .from_hour
                .checked_abs()
                .is_some_and(|hour| hour < 1 << 40)
            && self
                .site
                .as_ref()
                .is_none_or(|site| !site.is_empty() && site.len() <= MAX_SITE_BYTES)
    }

    pub fn span_hours(&self) -> i64 {
        i64::from(self.bucket_hours) * i64::from(self.buckets)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BucketTime {
    pub browse_ms: i64,
    pub work_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteTime {
    pub site: String,
    pub spent_ms: i64,
    pub opens: u32,
    /// Per bucket, for highlighted sites; empty for the rest.
    pub series: Vec<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TimeReport {
    pub buckets: Vec<BucketTime>,
    /// The same span just before, for comparison.
    pub previous: BucketTime,
    pub sites: Vec<SiteTime>,
}

/// Focus over one local day.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FocusDay {
    /// Local days since the epoch.
    pub day: i64,
    pub focused_ms: i64,
    pub sessions: u32,
    pub completed: u32,
}

/// Lowercases a typed site and reduces it to its host: `https://www.x.com/a`
/// becomes `x.com`. A leading `www.` names the same site, so it is dropped.
pub fn normalize_site(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return None;
    }
    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };
    let url = url::Url::parse(&candidate).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = match url.host()? {
        url::Host::Domain(domain) => domain.trim_end_matches('.').to_owned(),
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => return None,
    };
    let host = host.strip_prefix("www.").unwrap_or(&host).to_owned();
    (!host.is_empty() && host.len() <= MAX_SITE_BYTES && host.contains('.')).then_some(host)
}

/// Whether `host` is `site` or one of its subdomains.
pub fn site_covers(site: &str, host: &str) -> bool {
    host == site
        || host
            .strip_suffix(site)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

/// What the engine checks before a page loads while a focus round runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FocusGate {
    pub shut: Vec<String>,
    /// Sites let through, each until a wall-clock instant in milliseconds.
    pub allowed: Vec<(String, i64)>,
}

impl FocusGate {
    pub fn blocks(&self, host: &str, now_ms: i64) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.shut.iter().any(|site| site_covers(site, &host))
            && !self
                .allowed
                .iter()
                .any(|(site, until)| *until > now_ms && site_covers(site, &host))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusPhase {
    Focus,
    Break,
    LongBreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusPlan {
    pub minutes: u16,
    /// Repeats focus with breaks until stopped, the Pomodoro way.
    pub breaks: bool,
}

impl FocusPlan {
    pub fn valid(&self) -> bool {
        (MIN_FOCUS_MINUTES..=MAX_FOCUS_MINUTES).contains(&self.minutes)
    }

    /// Short and long break minutes: 25 rests 5 and 15, the classic cadence,
    /// and longer rounds rest in proportion up to half an hour.
    pub fn break_minutes(&self) -> (u16, u16) {
        let short = (self.minutes / 5).clamp(3, 20);
        (short, (short * 3).min(30))
    }

    fn length_ms(&self, phase: FocusPhase) -> i64 {
        let minutes = match phase {
            FocusPhase::Focus => self.minutes,
            FocusPhase::Break => self.break_minutes().0,
            FocusPhase::LongBreak => self.break_minutes().1,
        };
        i64::from(minutes) * MINUTE_MS
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusSession {
    pub plan: FocusPlan,
    pub started_ms: i64,
    pub phase: FocusPhase,
    pub phase_started_ms: i64,
    pub phase_ends_ms: i64,
    /// Focus rounds finished so far.
    pub rounds: u32,
    /// Focus time in finished rounds; the running round is added on demand.
    pub focused_ms: i64,
    /// Sites let through for a moment, with when each closes again.
    pub allowances: Vec<(String, i64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusEvent {
    /// A phase ran out and the next one began.
    Phase {
        ended: FocusPhase,
        began: FocusPhase,
    },
    /// A single round ran out; the session is over.
    Finished,
}

/// A session once it is over, as it is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusRecord {
    pub started_ms: i64,
    pub ended_ms: i64,
    pub focused_ms: i64,
    pub rounds: u32,
    /// Ran its full length rather than being ended early.
    pub completed: bool,
}

impl FocusSession {
    pub fn start(plan: FocusPlan, now_ms: i64) -> Self {
        Self {
            plan,
            started_ms: now_ms,
            phase: FocusPhase::Focus,
            phase_started_ms: now_ms,
            phase_ends_ms: now_ms + plan.length_ms(FocusPhase::Focus),
            rounds: 0,
            focused_ms: 0,
            allowances: Vec::new(),
        }
    }

    /// A session restored from storage is trusted only if it is coherent.
    pub fn valid(&self) -> bool {
        self.plan.valid()
            && self.started_ms <= self.phase_started_ms
            && self.phase_started_ms < self.phase_ends_ms
            && self.focused_ms >= 0
            && self.allowances.len() <= MAX_ALLOWANCES
    }

    pub fn focused_ms(&self, now_ms: i64) -> i64 {
        let running = if self.phase == FocusPhase::Focus {
            (now_ms.min(self.phase_ends_ms) - self.phase_started_ms).max(0)
        } else {
            0
        };
        self.focused_ms + running
    }

    /// Runs the clock forward to `now_ms`. A long sleep may cross several
    /// phases at once; each is reported in order.
    pub fn advance(&mut self, now_ms: i64) -> (Vec<FocusEvent>, Option<FocusRecord>) {
        let mut events = Vec::new();
        self.allowances.retain(|(_, until)| *until > now_ms);
        while now_ms >= self.phase_ends_ms {
            let ended = self.phase;
            let ended_at = self.phase_ends_ms;
            if ended == FocusPhase::Focus {
                self.rounds += 1;
                self.focused_ms += self.phase_ends_ms - self.phase_started_ms;
                if !self.plan.breaks {
                    events.push(FocusEvent::Finished);
                    return (events, Some(self.record(ended_at, true)));
                }
            }
            let began = match ended {
                FocusPhase::Focus if self.rounds.is_multiple_of(ROUNDS_PER_LONG_BREAK) => {
                    FocusPhase::LongBreak
                }
                FocusPhase::Focus => FocusPhase::Break,
                FocusPhase::Break | FocusPhase::LongBreak => FocusPhase::Focus,
            };
            self.phase = began;
            self.phase_started_ms = ended_at;
            self.phase_ends_ms = ended_at + self.plan.length_ms(began);
            events.push(FocusEvent::Phase { ended, began });
        }
        (events, None)
    }

    /// Ends the session now. Rounds of a repeating session count as finished
    /// work, so stopping one is a completion once a round is done.
    pub fn stop(&self, now_ms: i64) -> FocusRecord {
        let mut record = self.record(now_ms, self.plan.breaks && self.rounds > 0);
        record.focused_ms = self.focused_ms(now_ms);
        record
    }

    /// Whether `host` is shut right now. Breaks open everything.
    pub fn blocks(&self, blocked: &[String], host: &str, now_ms: i64) -> bool {
        self.phase == FocusPhase::Focus
            && now_ms < self.phase_ends_ms
            && blocked.iter().any(|site| site_covers(site, host))
            && !self
                .allowances
                .iter()
                .any(|(site, until)| *until > now_ms && site_covers(site, host))
    }

    /// The gate for this moment: shut sites during a round, nothing in a break.
    pub fn gate(&self, shut: &[String], now_ms: i64) -> Option<FocusGate> {
        (self.phase == FocusPhase::Focus && now_ms < self.phase_ends_ms && !shut.is_empty()).then(
            || FocusGate {
                shut: shut.to_vec(),
                allowed: self.allowances.clone(),
            },
        )
    }

    /// Lets `site` through for [`ALLOWANCE_MS`], or until this round ends.
    pub fn allow(&mut self, site: String, now_ms: i64) -> bool {
        if self.phase != FocusPhase::Focus {
            return false;
        }
        self.allowances
            .retain(|(kept, until)| *until > now_ms && *kept != site);
        if self.allowances.len() >= MAX_ALLOWANCES {
            return false;
        }
        let until = (now_ms + ALLOWANCE_MS).min(self.phase_ends_ms);
        self.allowances.push((site, until));
        true
    }

    /// The next moment the session changes on its own.
    pub fn next_change_ms(&self) -> i64 {
        self.allowances
            .iter()
            .map(|(_, until)| *until)
            .fold(self.phase_ends_ms, i64::min)
    }

    fn record(&self, ended_ms: i64, completed: bool) -> FocusRecord {
        FocusRecord {
            started_ms: self.started_ms,
            ended_ms,
            focused_ms: self.focused_ms,
            rounds: self.rounds,
            completed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn profile() -> ProfileId {
        ProfileId::parse("01J00000000000000000000000").unwrap()
    }

    fn site(name: &str) -> Option<Attention> {
        Some(Attention {
            profile: profile(),
            place: Place::Site(name.into()),
        })
    }

    #[test]
    fn segments_measure_monotonic_time_and_land_at_the_wall_clock() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        let first = tracker.set(site("github.com"), start, 1_000);
        assert_eq!(first.closed, None);
        assert_eq!(first.opened, Some((profile(), "github.com".into())));
        // The wall clock jumped back an hour; the segment still lasts 90 s.
        let second = tracker.set(site("x.com"), start + Duration::from_secs(90), -HOUR_MS);
        assert_eq!(
            second.closed,
            Some(Segment {
                profile: profile(),
                place: Place::Site("github.com".into()),
                end_ms: -HOUR_MS,
                duration_ms: 90_000,
            })
        );
    }

    #[test]
    fn returning_to_the_same_site_is_not_a_new_open() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        tracker.set(site("github.com"), start, 0);
        tracker.set(None, start + Duration::from_secs(5), 5_000);
        let back = tracker.set(site("github.com"), start + Duration::from_secs(9), 9_000);
        assert_eq!(back.opened, None);
        let other = tracker.set(site("x.com"), start + Duration::from_secs(12), 12_000);
        assert!(other.opened.is_some());
    }

    #[test]
    fn unchanged_attention_is_not_a_transition() {
        let start = Instant::now();
        let mut tracker = Tracker::default();
        tracker.set(site("github.com"), start, 0);
        let same = tracker.set(site("github.com"), start + Duration::from_secs(30), 30_000);
        assert_eq!(same, Transition::default());
        let checkpoint = tracker.checkpoint(start + Duration::from_secs(40), 40_000);
        assert_eq!(checkpoint.map(|segment| segment.duration_ms), Some(40_000));
        let next = tracker.set(None, start + Duration::from_secs(41), 41_000);
        assert_eq!(next.closed.map(|segment| segment.duration_ms), Some(1_000));
    }

    #[test]
    fn a_segment_spreads_across_the_hours_it_covers() {
        let mut ledger = Ledger::default();
        ledger.spend(&Segment {
            profile: profile(),
            place: Place::Work,
            end_ms: 10 * HOUR_MS + 600_000,
            duration_ms: HOUR_MS + 900_000,
        });
        let tallies = ledger.take(profile());
        let spent: Vec<_> = tallies
            .iter()
            .map(|tally| (tally.hour, tally.tally.spent_ms))
            .collect();
        assert_eq!(spent, vec![(8, 300_000), (9, HOUR_MS), (10, 600_000)]);
        assert!(ledger.is_empty());
    }

    #[test]
    fn a_segment_ending_exactly_on_the_hour_belongs_to_the_hour_before() {
        let mut ledger = Ledger::default();
        ledger.spend(&Segment {
            profile: profile(),
            place: Place::Work,
            end_ms: 3 * HOUR_MS,
            duration_ms: 1_000,
        });
        let tallies = ledger.take(profile());
        assert_eq!(tallies.len(), 1);
        assert_eq!(tallies[0].hour, 2);
        assert_eq!(tallies[0].tally.spent_ms, 1_000);
    }

    #[test]
    fn forgetting_a_range_keeps_older_hours() {
        let mut ledger = Ledger::default();
        for hour in [1, 2, 3] {
            ledger.open(profile(), "x.com".into(), hour * HOUR_MS);
        }
        ledger.forget(profile(), Some(2 * HOUR_MS + 5));
        let hours: Vec<_> = ledger.take(profile()).iter().map(|t| t.hour).collect();
        assert_eq!(hours, vec![1]);
    }

    #[test]
    fn the_ledger_stops_growing_at_its_bound() {
        let mut ledger = Ledger::default();
        for index in 0..MAX_PENDING_TALLIES + 10 {
            ledger.open(profile(), format!("site{index}.com"), 0);
        }
        ledger.open(profile(), "site0.com".into(), 0);
        let tallies = ledger.take(profile());
        assert_eq!(tallies.len(), MAX_PENDING_TALLIES);
        assert!(tallies
            .iter()
            .any(|t| t.place == Place::Site("site0.com".into()) && t.tally.opens == 2));
    }

    #[test]
    fn typed_sites_reduce_to_their_host() {
        assert_eq!(
            normalize_site("https://www.YouTube.com/watch?v=1").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(normalize_site(" x.com ").as_deref(), Some("x.com"));
        assert_eq!(
            normalize_site("mail.google.com/inbox").as_deref(),
            Some("mail.google.com")
        );
        assert_eq!(normalize_site("localhost"), None);
        assert_eq!(normalize_site("192.168.1.1"), None);
        assert_eq!(normalize_site("file:///etc"), None);
        assert_eq!(normalize_site(""), None);
    }

    #[test]
    fn a_site_covers_its_subdomains_only() {
        assert!(site_covers("youtube.com", "youtube.com"));
        assert!(site_covers("youtube.com", "m.youtube.com"));
        assert!(!site_covers("youtube.com", "notyoutube.com"));
        assert!(!site_covers("mail.google.com", "google.com"));
    }

    #[test]
    fn breaks_follow_the_classic_cadence() {
        let plan = |minutes| FocusPlan {
            minutes,
            breaks: true,
        };
        assert_eq!(plan(25).break_minutes(), (5, 15));
        assert_eq!(plan(50).break_minutes(), (10, 30));
        assert_eq!(plan(90).break_minutes(), (18, 30));
        assert_eq!(plan(5).break_minutes(), (3, 9));
    }

    #[test]
    fn a_single_round_finishes_with_its_full_length() {
        let plan = FocusPlan {
            minutes: 25,
            breaks: false,
        };
        let mut session = FocusSession::start(plan, 0);
        assert_eq!(session.advance(24 * MINUTE_MS), (vec![], None));
        assert_eq!(session.focused_ms(10 * MINUTE_MS), 10 * MINUTE_MS);
        let (events, record) = session.advance(30 * MINUTE_MS);
        assert_eq!(events, vec![FocusEvent::Finished]);
        assert_eq!(
            record,
            Some(FocusRecord {
                started_ms: 0,
                ended_ms: 25 * MINUTE_MS,
                focused_ms: 25 * MINUTE_MS,
                rounds: 1,
                completed: true,
            })
        );
    }

    #[test]
    fn repeating_rounds_take_a_long_break_after_the_fourth() {
        let plan = FocusPlan {
            minutes: 25,
            breaks: true,
        };
        let mut session = FocusSession::start(plan, 0);
        // Four rounds and three short breaks: 4 * 25 + 3 * 5 = 115 minutes.
        let (events, record) = session.advance(115 * MINUTE_MS);
        assert_eq!(record, None);
        assert_eq!(events.len(), 7);
        assert_eq!(
            events.last(),
            Some(&FocusEvent::Phase {
                ended: FocusPhase::Focus,
                began: FocusPhase::LongBreak,
            })
        );
        assert_eq!(session.rounds, 4);
        assert_eq!(session.phase_ends_ms, 130 * MINUTE_MS);
        let stopped = session.stop(120 * MINUTE_MS);
        assert!(stopped.completed);
        assert_eq!(stopped.focused_ms, 100 * MINUTE_MS);
    }

    #[test]
    fn stopping_early_counts_the_time_focused_so_far() {
        let plan = FocusPlan {
            minutes: 50,
            breaks: false,
        };
        let session = FocusSession::start(plan, 1_000);
        let record = session.stop(1_000 + 12 * MINUTE_MS);
        assert!(!record.completed);
        assert_eq!(record.focused_ms, 12 * MINUTE_MS);
    }

    #[test]
    fn blocking_holds_during_focus_and_yields_to_allowances_and_breaks() {
        let blocked = vec!["youtube.com".to_owned()];
        let plan = FocusPlan {
            minutes: 25,
            breaks: true,
        };
        let mut session = FocusSession::start(plan, 0);
        assert!(session.blocks(&blocked, "m.youtube.com", 1_000));
        assert!(!session.blocks(&blocked, "github.com", 1_000));
        assert!(session.allow("youtube.com".into(), 1_000));
        assert!(!session.blocks(&blocked, "youtube.com", 2_000));
        assert_eq!(session.next_change_ms(), 1_000 + ALLOWANCE_MS);
        assert!(session.blocks(&blocked, "youtube.com", 1_000 + ALLOWANCE_MS));
        session.advance(26 * MINUTE_MS);
        assert_eq!(session.phase, FocusPhase::Break);
        assert!(!session.blocks(&blocked, "youtube.com", 26 * MINUTE_MS));
        assert!(!session.allow("youtube.com".into(), 26 * MINUTE_MS));
    }

    #[test]
    fn the_gate_holds_only_during_a_round() {
        let shut = vec!["youtube.com".to_owned()];
        let plan = FocusPlan {
            minutes: 25,
            breaks: true,
        };
        let mut session = FocusSession::start(plan, 0);
        let gate = session.gate(&shut, 1_000).unwrap();
        assert!(gate.blocks("WWW.YouTube.com.", 1_000));
        assert!(!gate.blocks("github.com", 1_000));
        session.allow("youtube.com".into(), 1_000);
        assert!(!session
            .gate(&shut, 2_000)
            .unwrap()
            .blocks("youtube.com", 2_000));
        assert!(session.gate(&[], 2_000).is_none());
        session.advance(26 * MINUTE_MS);
        assert!(session.gate(&shut, 26 * MINUTE_MS).is_none());
    }

    #[test]
    fn an_allowance_never_outlasts_its_round() {
        let plan = FocusPlan {
            minutes: 25,
            breaks: false,
        };
        let mut session = FocusSession::start(plan, 0);
        session.allow("x.com".into(), 23 * MINUTE_MS);
        assert_eq!(session.next_change_ms(), 25 * MINUTE_MS);
    }

    #[test]
    fn a_session_survives_storage() {
        let mut session = FocusSession::start(
            FocusPlan {
                minutes: 50,
                breaks: true,
            },
            7,
        );
        session.allow("x.com".into(), 10);
        let raw = serde_json::to_string(&session).unwrap();
        let back: FocusSession = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, session);
        assert!(back.valid());
        let mut broken = back;
        broken.phase_ends_ms = broken.phase_started_ms;
        assert!(!broken.valid());
    }
}
