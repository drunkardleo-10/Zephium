//! Calendar-day aggregates; native request metadata never crosses this boundary.
use serde::{Deserialize, Serialize};

/// Total awaiting collection, plus a hint that macOS has per-view counts.
pub type BlockedLoadCounter = std::sync::Arc<(
    std::sync::atomic::AtomicU64,
    std::sync::atomic::AtomicBool,
    std::sync::atomic::AtomicBool,
)>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockerStatistics {
    pub day: i32,
    /// Oldest day first; index 6 is `day`.
    pub days: [u64; 7],
}

impl BlockerStatistics {
    pub fn advance(&mut self, day: i32) {
        let shift = i64::from(day) - i64::from(self.day);
        match shift {
            1..=6 => {
                let shift = shift as usize;
                self.days.copy_within(shift.., 0);
                self.days[7 - shift..].fill(0);
            }
            -6..=-1 => {
                let shift = -shift as usize;
                self.days.copy_within(..7 - shift, shift);
                self.days[..shift].fill(0);
            }
            0 => {}
            _ => self.days.fill(0),
        }
        self.day = day;
    }

    pub fn record(&mut self, day: i32, count: u64) {
        self.advance(day);
        self.days[6] = self.days[6].saturating_add(count);
    }

    pub fn today(&self) -> u64 {
        self.days[6]
    }
    pub fn last_seven_days(&self) -> u64 {
        self.days
            .iter()
            .fold(0u64, |total, count| total.saturating_add(*count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_roll_over_and_expire_by_calendar_day() {
        let mut stats = BlockerStatistics::default();
        stats.record(100, 3);
        stats.record(101, 5);
        assert_eq!(stats.today(), 5);
        assert_eq!(stats.last_seven_days(), 8);
        stats.advance(106);
        assert_eq!(stats.days, [3, 5, 0, 0, 0, 0, 0]);
        stats.advance(107);
        assert_eq!(stats.last_seven_days(), 5);
        stats.advance(108);
        assert_eq!(stats.last_seven_days(), 0);
    }
    #[test]
    fn clock_corrections_never_count_future_days_or_resurrect_expired_counts() {
        let mut stats = BlockerStatistics::default();
        stats.record(100, 3);
        stats.record(101, 5);
        stats.advance(100);
        assert_eq!(stats.today(), 3);
        stats.advance(101);
        assert_eq!(stats.today(), 0);
        stats.record(101, u64::MAX);
        stats.record(101, 1);
        assert_eq!(stats.last_seven_days(), u64::MAX);
        stats.advance(i32::MIN);
        assert_eq!(stats.last_seven_days(), 0);
    }
}
