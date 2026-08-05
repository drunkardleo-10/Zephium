use std::cell::Cell;
use std::io::{Read, Write};
use std::marker::PhantomData;
use std::rc::Rc;

use thiserror::Error;

std::thread_local! {
    static ACTIVE_MEASUREMENT: Cell<Option<MeasurementState>> = const { Cell::new(None) };
}

/// One completed private-filesystem operation measurement.
///
/// This internal-repository E2E surface is absent unless the dedicated custom
/// configuration is enabled. Counts describe successful native operations and
/// bytes actually transferred on the guarded thread while its scope was active.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "operation measurements must be checked against their resource budgets"]
pub struct PrivateFsOperationSnapshot {
    file_syncs: usize,
    directory_syncs: usize,
    regular_creates: usize,
    regular_unlinks: usize,
    directory_unlinks: usize,
    renames: usize,
    directory_mode_changes: usize,
    bytes_read: usize,
    bytes_written: usize,
}

impl PrivateFsOperationSnapshot {
    /// Returns the number of successful regular-file durability flushes.
    #[must_use]
    pub const fn file_syncs(self) -> usize {
        self.file_syncs
    }

    /// Returns the number of successful directory durability flushes.
    #[must_use]
    pub const fn directory_syncs(self) -> usize {
        self.directory_syncs
    }

    /// Returns the number of successful create-new regular-file operations.
    #[must_use]
    pub const fn regular_creates(self) -> usize {
        self.regular_creates
    }

    /// Returns the number of successful regular-file unlink operations.
    #[must_use]
    pub const fn regular_unlinks(self) -> usize {
        self.regular_unlinks
    }

    /// Returns the number of successful directory unlink operations.
    #[must_use]
    pub const fn directory_unlinks(self) -> usize {
        self.directory_unlinks
    }

    /// Returns the number of successful atomic rename operations.
    #[must_use]
    pub const fn renames(self) -> usize {
        self.renames
    }

    /// Returns the number of successful directory-mode changes.
    #[must_use]
    pub const fn directory_mode_changes(self) -> usize {
        self.directory_mode_changes
    }

    /// Returns the number of bytes successfully read from private regular files.
    #[must_use]
    pub const fn bytes_read(self) -> usize {
        self.bytes_read
    }

    /// Returns the number of bytes successfully written to private regular files.
    #[must_use]
    pub const fn bytes_written(self) -> usize {
        self.bytes_written
    }
}

/// Refusal returned by internal private-filesystem operation measurement.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum PrivateFsOperationMeasurementError {
    /// Another measurement guard is already active on this thread.
    #[error("a private-filesystem operation measurement is already active on this thread")]
    AlreadyActive,
    /// At least one measurement counter exceeded its representable range.
    #[error("a private-filesystem operation measurement counter overflowed")]
    CounterOverflow,
}

/// Exclusive scope for one internal private-filesystem operation measurement.
///
/// Only one guard may exist per thread. The guard is deliberately non-`Send`,
/// and [`Self::finish`] consumes the active state on that same thread. Dropping
/// an unfinished guard discards the state. There is no independent reset or
/// read API, so a later measurement cannot be confused with an earlier
/// generation.
#[derive(Debug)]
#[must_use = "dropping the guard discards its operation measurement"]
pub struct PrivateFsOperationMeasurement {
    active: bool,
    _not_send: PhantomData<Rc<()>>,
}

impl PrivateFsOperationMeasurement {
    /// Starts the only active operation measurement on this thread.
    pub fn begin() -> Result<Self, PrivateFsOperationMeasurementError> {
        let already_active = ACTIVE_MEASUREMENT.with(|active| {
            if active.get().is_some() {
                true
            } else {
                active.set(Some(MeasurementState::default()));
                false
            }
        });
        if already_active {
            return Err(PrivateFsOperationMeasurementError::AlreadyActive);
        }
        Ok(Self {
            active: true,
            _not_send: PhantomData,
        })
    }

    /// Finishes this measurement and returns its immutable snapshot.
    pub fn finish(
        mut self,
    ) -> Result<PrivateFsOperationSnapshot, PrivateFsOperationMeasurementError> {
        let state = ACTIVE_MEASUREMENT
            .with(Cell::take)
            .expect("an active measurement guard must own active state on its thread");
        self.active = false;
        if state.overflowed {
            Err(PrivateFsOperationMeasurementError::CounterOverflow)
        } else {
            Ok(state.snapshot)
        }
    }
}

impl Drop for PrivateFsOperationMeasurement {
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
enum OperationCounter {
    FileSync,
    DirectorySync,
    RegularCreate,
    RegularUnlink,
    #[cfg(unix)]
    DirectoryUnlink,
    Rename,
    #[cfg(unix)]
    DirectoryModeChange,
    BytesRead,
    BytesWritten,
}

#[derive(Clone, Copy, Debug, Default)]
struct MeasurementState {
    snapshot: PrivateFsOperationSnapshot,
    overflowed: bool,
}

impl MeasurementState {
    fn record(&mut self, counter: OperationCounter, amount: usize) {
        let target = match counter {
            OperationCounter::FileSync => &mut self.snapshot.file_syncs,
            OperationCounter::DirectorySync => &mut self.snapshot.directory_syncs,
            OperationCounter::RegularCreate => &mut self.snapshot.regular_creates,
            OperationCounter::RegularUnlink => &mut self.snapshot.regular_unlinks,
            #[cfg(unix)]
            OperationCounter::DirectoryUnlink => &mut self.snapshot.directory_unlinks,
            OperationCounter::Rename => &mut self.snapshot.renames,
            #[cfg(unix)]
            OperationCounter::DirectoryModeChange => &mut self.snapshot.directory_mode_changes,
            OperationCounter::BytesRead => &mut self.snapshot.bytes_read,
            OperationCounter::BytesWritten => &mut self.snapshot.bytes_written,
        };
        if let Some(sum) = target.checked_add(amount) {
            *target = sum;
        } else {
            self.overflowed = true;
        }
    }
}

fn record(counter: OperationCounter, amount: usize) {
    ACTIVE_MEASUREMENT.with(|active| {
        let Some(mut state) = active.get() else {
            return;
        };
        state.record(counter, amount);
        active.set(Some(state));
    });
}

pub(crate) fn record_file_sync() {
    record(OperationCounter::FileSync, 1);
}

pub(crate) fn record_directory_sync() {
    record(OperationCounter::DirectorySync, 1);
}

pub(crate) fn record_regular_create() {
    record(OperationCounter::RegularCreate, 1);
}

pub(crate) fn record_regular_unlink() {
    record(OperationCounter::RegularUnlink, 1);
}

#[cfg(unix)]
pub(crate) fn record_directory_unlink() {
    record(OperationCounter::DirectoryUnlink, 1);
}

pub(crate) fn record_rename() {
    record(OperationCounter::Rename, 1);
}

#[cfg(unix)]
pub(crate) fn record_directory_mode_change() {
    record(OperationCounter::DirectoryModeChange, 1);
}

pub(crate) struct MeasuredReader<R> {
    inner: R,
}

impl<R> MeasuredReader<R> {
    pub(crate) const fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: Read> Read for MeasuredReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        record(OperationCounter::BytesRead, read);
        Ok(read)
    }
}

pub(crate) struct MeasuredWriter<W> {
    inner: W,
}

impl<W> MeasuredWriter<W> {
    pub(crate) const fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for MeasuredWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buffer)?;
        record(OperationCounter::BytesWritten, written);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_checked_and_never_wrap() {
        let mut state = MeasurementState::default();
        for counter in [
            OperationCounter::FileSync,
            OperationCounter::DirectorySync,
            OperationCounter::RegularCreate,
            OperationCounter::RegularUnlink,
            OperationCounter::Rename,
            OperationCounter::BytesRead,
            OperationCounter::BytesWritten,
        ] {
            state.record(counter, 1);
        }
        #[cfg(unix)]
        for counter in [
            OperationCounter::DirectoryUnlink,
            OperationCounter::DirectoryModeChange,
        ] {
            state.record(counter, 1);
        }
        assert_eq!(
            state.snapshot,
            PrivateFsOperationSnapshot {
                file_syncs: 1,
                directory_syncs: 1,
                regular_creates: 1,
                regular_unlinks: 1,
                directory_unlinks: usize::from(cfg!(unix)),
                renames: 1,
                directory_mode_changes: usize::from(cfg!(unix)),
                bytes_read: 1,
                bytes_written: 1,
            }
        );

        state.snapshot.bytes_written = usize::MAX;
        state.record(OperationCounter::BytesWritten, 1);
        assert_eq!(state.snapshot.bytes_written, usize::MAX);
        assert!(state.overflowed);
    }

    #[test]
    fn measurement_refuses_nesting_and_releases_on_drop_or_finish() {
        let measurement = PrivateFsOperationMeasurement::begin().unwrap();
        assert_eq!(
            PrivateFsOperationMeasurement::begin().unwrap_err(),
            PrivateFsOperationMeasurementError::AlreadyActive
        );
        drop(measurement);

        let measurement = PrivateFsOperationMeasurement::begin().unwrap();
        let _snapshot = measurement.finish().unwrap();
        let measurement = PrivateFsOperationMeasurement::begin().unwrap();
        drop(measurement);
    }

    #[test]
    fn overlapping_threads_measure_independently() {
        let rendezvous = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let rendezvous = rendezvous.clone();
                std::thread::spawn(move || {
                    let measurement = PrivateFsOperationMeasurement::begin();
                    rendezvous.wait();
                    let measurement = measurement.unwrap();
                    record_file_sync();
                    measurement.finish().unwrap()
                })
            })
            .collect::<Vec<_>>();

        rendezvous.wait();
        for worker in workers {
            let snapshot = worker.join().unwrap();
            assert_eq!(snapshot.file_syncs(), 1);
            assert_eq!(snapshot.directory_syncs(), 0);
        }
    }
}
