//! Internal measurement of materialization-recovery CPU amplification.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

std::thread_local! {
    static ACTIVE_MEASUREMENT: Cell<Option<MeasurementState>> = const { Cell::new(None) };
}

/// One completed materialization-recovery measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "recovery measurements must be checked against their CPU budgets"]
pub(crate) struct MaterializationRecoverySnapshot {
    /// Successful production `open_or_recover` traversals.
    pub(crate) completed_passes: usize,
    /// Successful catalog-object reads performed by generation recognition.
    pub(crate) catalog_reads: usize,
    /// Bytes returned by those catalog-object reads.
    pub(crate) catalog_read_bytes: usize,
    /// Full authority admission calls attempted by generation recognition.
    pub(crate) catalog_admission_attempts: usize,
    /// Bytes supplied to those attempted admission calls.
    pub(crate) catalog_admission_bytes: usize,
    /// Exact identity-bound parsed catalog cache hits.
    pub(crate) catalog_cache_hits: usize,
}

/// Refusal returned by internal materialization-recovery measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaterializationRecoveryMeasurementError {
    /// Another measurement guard is already active on this thread.
    AlreadyActive,
    /// At least one measurement counter exceeded its representable range.
    CounterOverflow,
}

/// Exclusive scope for one internal materialization-recovery measurement.
///
/// Only one guard may exist per thread. The guard is deliberately non-`Send`,
/// and [`Self::finish`] consumes the active state on that same thread. Dropping
/// an unfinished guard discards the state, so a later measurement cannot be
/// confused with an earlier generation.
#[derive(Debug)]
#[must_use = "dropping the guard discards its recovery measurement"]
pub(crate) struct MaterializationRecoveryMeasurement {
    active: bool,
    _not_send: PhantomData<Rc<()>>,
}

impl MaterializationRecoveryMeasurement {
    /// Starts the only active recovery measurement on this thread.
    pub(crate) fn begin() -> Result<Self, MaterializationRecoveryMeasurementError> {
        let already_active = ACTIVE_MEASUREMENT.with(|active| {
            if active.get().is_some() {
                true
            } else {
                active.set(Some(MeasurementState::default()));
                false
            }
        });
        if already_active {
            return Err(MaterializationRecoveryMeasurementError::AlreadyActive);
        }
        Ok(Self {
            active: true,
            _not_send: PhantomData,
        })
    }

    /// Finishes this measurement and returns its immutable snapshot.
    pub(crate) fn finish(
        mut self,
    ) -> Result<MaterializationRecoverySnapshot, MaterializationRecoveryMeasurementError> {
        let state = ACTIVE_MEASUREMENT
            .with(Cell::take)
            .expect("an active recovery measurement guard must own active state on its thread");
        self.active = false;
        if state.overflowed {
            Err(MaterializationRecoveryMeasurementError::CounterOverflow)
        } else {
            Ok(state.snapshot)
        }
    }
}

impl Drop for MaterializationRecoveryMeasurement {
    fn drop(&mut self) {
        if self.active {
            ACTIVE_MEASUREMENT.with(|active| {
                active.take();
            });
            self.active = false;
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RecoveryCounter {
    CompletedPasses,
    CatalogReads,
    CatalogReadBytes,
    CatalogAdmissionAttempts,
    CatalogAdmissionBytes,
    CatalogCacheHits,
}

#[derive(Clone, Copy, Debug, Default)]
struct MeasurementState {
    snapshot: MaterializationRecoverySnapshot,
    overflowed: bool,
}

impl MeasurementState {
    fn record(&mut self, counter: RecoveryCounter, amount: usize) {
        let target = match counter {
            RecoveryCounter::CompletedPasses => &mut self.snapshot.completed_passes,
            RecoveryCounter::CatalogReads => &mut self.snapshot.catalog_reads,
            RecoveryCounter::CatalogReadBytes => &mut self.snapshot.catalog_read_bytes,
            RecoveryCounter::CatalogAdmissionAttempts => {
                &mut self.snapshot.catalog_admission_attempts
            }
            RecoveryCounter::CatalogAdmissionBytes => &mut self.snapshot.catalog_admission_bytes,
            RecoveryCounter::CatalogCacheHits => &mut self.snapshot.catalog_cache_hits,
        };
        if let Some(sum) = target.checked_add(amount) {
            *target = sum;
        } else {
            self.overflowed = true;
        }
    }
}

fn record(counter: RecoveryCounter, amount: usize) {
    ACTIVE_MEASUREMENT.with(|active| {
        let Some(mut state) = active.get() else {
            return;
        };
        state.record(counter, amount);
        active.set(Some(state));
    });
}

pub(super) fn note_completed_pass() {
    record(RecoveryCounter::CompletedPasses, 1);
}

pub(super) fn note_catalog_read(bytes: usize) {
    record(RecoveryCounter::CatalogReads, 1);
    record(RecoveryCounter::CatalogReadBytes, bytes);
}

pub(super) fn note_catalog_admission_attempt(bytes: usize) {
    record(RecoveryCounter::CatalogAdmissionAttempts, 1);
    record(RecoveryCounter::CatalogAdmissionBytes, bytes);
}

pub(super) fn note_catalog_cache_hit() {
    record(RecoveryCounter::CatalogCacheHits, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_refuses_nesting_and_releases_on_drop_or_finish() {
        let measurement = MaterializationRecoveryMeasurement::begin().unwrap();
        assert_eq!(
            MaterializationRecoveryMeasurement::begin().unwrap_err(),
            MaterializationRecoveryMeasurementError::AlreadyActive
        );
        drop(measurement);

        let measurement = MaterializationRecoveryMeasurement::begin().unwrap();
        let _snapshot = measurement.finish().unwrap();
        let measurement = MaterializationRecoveryMeasurement::begin().unwrap();
        drop(measurement);
    }

    #[test]
    fn recovery_boundaries_have_independent_checked_counters() {
        let measurement = MaterializationRecoveryMeasurement::begin().unwrap();
        note_completed_pass();
        note_catalog_read(13);
        note_catalog_admission_attempt(11);
        note_catalog_cache_hit();
        let snapshot = measurement.finish().unwrap();
        assert_eq!(
            snapshot,
            MaterializationRecoverySnapshot {
                completed_passes: 1,
                catalog_reads: 1,
                catalog_read_bytes: 13,
                catalog_admission_attempts: 1,
                catalog_admission_bytes: 11,
                catalog_cache_hits: 1,
            }
        );

        let mut state = MeasurementState::default();
        state.snapshot.catalog_read_bytes = usize::MAX;
        state.record(RecoveryCounter::CatalogReadBytes, 1);
        assert_eq!(state.snapshot.catalog_read_bytes, usize::MAX);
        assert!(state.overflowed);
    }

    #[test]
    fn overlapping_threads_measure_independently() {
        let rendezvous = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers = (0..2)
            .map(|worker| {
                let rendezvous = rendezvous.clone();
                std::thread::spawn(move || {
                    let measurement = MaterializationRecoveryMeasurement::begin();
                    rendezvous.wait();
                    let measurement = measurement.unwrap();
                    note_catalog_read(worker + 1);
                    measurement.finish().unwrap()
                })
            })
            .collect::<Vec<_>>();

        rendezvous.wait();
        for (worker, handle) in workers.into_iter().enumerate() {
            let snapshot = handle.join().unwrap();
            assert_eq!(snapshot.catalog_reads, 1);
            assert_eq!(snapshot.catalog_read_bytes, worker + 1);
            assert_eq!(snapshot.completed_passes, 0);
        }
    }
}
