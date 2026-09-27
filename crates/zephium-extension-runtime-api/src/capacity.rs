//! Shared extension background-runtime admission policy.

/// Maximum number of extension background runtimes admitted concurrently.
///
/// A pending reservation, an activating runtime, and a live runtime each own
/// one slot until native absence or teardown releases it. Keeping this policy
/// in the backend-neutral boundary prevents the serialized service and native
/// host from admitting different amounts of work.
/// This is independent of the number of profiles: several extensions in one
/// profile each consume a slot. Native resource accounting uses this same bound.
pub const MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES: usize = 12;

const _: () = assert!(MAX_CONCURRENT_EXTENSION_BACKGROUND_RUNTIMES > 0);
