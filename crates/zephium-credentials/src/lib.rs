//! Safe, bounded Windows Credential Manager operations for native secret owners.
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
#![deny(missing_docs)]

/// Content-free native vault failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VaultError {
    /// The exact credential does not exist.
    Missing,
    /// The OS refused access or returned an invalid record.
    Inaccessible,
    /// The target or blob exceeds the native contract.
    Invalid,
    /// A bounded allocation failed.
    Capacity,
}

/// Process-wide serialization shared by provider and connection vault calls.
pub fn turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{delete, list, present, read, write};
