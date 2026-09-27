//! Process-local extension runtime identity.
//!
//! These values are deliberately non-persistent and non-authorizing. They
//! let the shell and native engine correlate one exact runtime generation.

use crate::ids::{ExtensionInstallId, ProfileId};

/// Process-local, non-wrapping identity for one extension runtime generation.
///
/// A late native callback from a disabled, updated, or restarted extension
/// must never alias a replacement generation. Exhaustion therefore requires
/// retiring extension execution for the process instead of wrapping.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeGeneration(u64);

impl ExtensionRuntimeGeneration {
    /// First legal process-local runtime generation.
    pub const INITIAL: Self = Self(1);

    /// Constructs a nonzero process-local generation.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Returns the opaque numeric generation for native correlation only.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next generation, refusing counter wrap.
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

/// Compact map key for one profile-scoped runtime generation.
///
/// This value is freely copyable because it is identity, not authority.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExtensionRuntimeInstance {
    profile: ProfileId,
    install_id: ExtensionInstallId,
    generation: ExtensionRuntimeGeneration,
}

impl ExtensionRuntimeInstance {
    /// Constructs a non-authorizing runtime identity.
    pub const fn new(
        profile: ProfileId,
        install_id: ExtensionInstallId,
        generation: ExtensionRuntimeGeneration,
    ) -> Self {
        Self {
            profile,
            install_id,
            generation,
        }
    }

    /// Exact profile that owns the runtime.
    pub const fn profile(self) -> ProfileId {
        self.profile
    }

    /// Stable profile-scoped installation identity.
    pub const fn install_id(self) -> ExtensionInstallId {
        self.install_id
    }

    /// Exact process-local runtime generation.
    pub const fn generation(self) -> ExtensionRuntimeGeneration {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_generations_are_nonzero_and_never_wrap() {
        assert_eq!(ExtensionRuntimeGeneration::new(0), None);
        assert_eq!(ExtensionRuntimeGeneration::INITIAL.get(), 1);
        assert_eq!(
            ExtensionRuntimeGeneration::INITIAL
                .next()
                .map(|value| value.get()),
            Some(2)
        );
        assert_eq!(
            ExtensionRuntimeGeneration::new(u64::MAX)
                .expect("maximum nonzero generation")
                .next(),
            None
        );
    }

    #[test]
    fn runtime_instance_is_only_the_exact_compact_key() {
        let instance = ExtensionRuntimeInstance::new(
            ProfileId::from(7),
            ExtensionInstallId::from(9),
            ExtensionRuntimeGeneration::new(11).unwrap(),
        );
        assert_eq!(instance.profile(), ProfileId::from(7));
        assert_eq!(instance.install_id(), ExtensionInstallId::from(9));
        assert_eq!(instance.generation().get(), 11);
    }
}
