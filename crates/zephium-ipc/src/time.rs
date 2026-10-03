//! Time surfaces: time per site, and focus sessions. Durations cross as
//! whole seconds; instants as Unix milliseconds.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::IconRef;
use zephium_core::time::{MAX_REPORT_BUCKETS, MAX_SITE_BYTES};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimeCall {
    /// Time over `buckets` spans of `bucket_hours` (1 or 24) local hours,
    /// from local hour `from_hour`, narrowed to `site` when given.
    Report {
        from_hour: i32,
        bucket_hours: u32,
        buckets: u32,
        site: Option<String>,
    },
    /// Focus per local day, from local day `from_day`.
    FocusDays { from_day: i32, days: u32 },
}

impl TimeCall {
    pub fn validate(&self) -> bool {
        match self {
            Self::Report {
                bucket_hours,
                buckets,
                site,
                ..
            } => {
                matches!(bucket_hours, 1 | 24)
                    && (1..=MAX_REPORT_BUCKETS).contains(buckets)
                    && site
                        .as_deref()
                        .is_none_or(|site| !site.is_empty() && site.len() <= MAX_SITE_BYTES)
            }
            Self::FocusDays { days, .. } => (1..=MAX_REPORT_BUCKETS).contains(days),
        }
    }
}

/// Seconds in one bucket, on the web and in Work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TimeBucketView {
    pub browse: u32,
    pub work: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SiteTimeView {
    pub site: String,
    pub icon: Option<IconRef>,
    pub seconds: u32,
    /// Times the site was come to, rather than returned to.
    pub opens: u32,
    /// Seconds per bucket, for the few leading sites; empty for the rest.
    pub series: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FocusDayView {
    pub day: i32,
    pub seconds: u32,
    pub sessions: u32,
    pub completed: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimeResponse {
    Report {
        buckets: Vec<TimeBucketView>,
        /// The same span just before, for comparison.
        previous: TimeBucketView,
        sites: Vec<SiteTimeView>,
    },
    FocusDays {
        days: Vec<FocusDayView>,
    },
    Error {
        error: TimeError,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TimeError {
    Invalid,
    Unavailable,
    Capacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FocusPhaseView {
    Focus,
    Break,
    LongBreak,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FocusView {
    pub phase: FocusPhaseView,
    #[specta(type = f64)]
    pub started_at: i64,
    #[specta(type = f64)]
    pub phase_started_at: i64,
    #[specta(type = f64)]
    pub phase_ends_at: i64,
    pub minutes: u32,
    pub breaks: bool,
    pub break_minutes: u32,
    pub long_break_minutes: u32,
    /// Focus rounds finished so far.
    pub rounds: u32,
    /// Seconds focused in finished rounds; the running round adds to it.
    pub focused: u32,
    /// Sites let through for a moment.
    pub allowed: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ShutSiteView {
    pub site: String,
    pub icon: Option<IconRef>,
}

/// Focus as chrome shows it: the running session, if any, and the sites a
/// focus round keeps shut.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct FocusStatus {
    pub session: Option<FocusView>,
    pub shut: Vec<ShutSiteView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FocusControl {
    Start { minutes: u32, breaks: bool },
    Stop,
    /// Lets one shut site through for a few minutes.
    Allow { site: String },
    /// Ends the running phase now: a break, or a round.
    Skip,
}

impl FocusControl {
    pub fn validate(&self) -> bool {
        match self {
            Self::Start { minutes, .. } => u16::try_from(*minutes).is_ok_and(|minutes| {
                (zephium_core::time::MIN_FOCUS_MINUTES..=zephium_core::time::MAX_FOCUS_MINUTES)
                    .contains(&minutes)
            }),
            Self::Allow { site } => !site.is_empty() && site.len() <= MAX_SITE_BYTES,
            Self::Stop | Self::Skip => true,
        }
    }
}
