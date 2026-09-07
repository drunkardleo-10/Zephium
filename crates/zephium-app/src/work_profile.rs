//! Read-only trusted browser-state binding; never a profile creator or policy installer.
use std::sync::mpsc::{Receiver, TryRecvError};
use zephium_agentic::ContextProfileStorageClass;
use zephium_core::{ids::ProfileId, profiles::ProfileKind};

/// Actor-selected profile/session identity. Only the application actor can mint
/// it; consumers cannot submit a profile selector. Native construction still
/// independently checks profile retirement, policy and persistence class.
///
/// ```compile_fail
/// let _ = zephium_app::AgentWorkProfileBinding {
///     profile: 1_u128.into(),
///     storage: zephium_agentic::ContextProfileStorageClass::Durable,
/// };
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AgentWorkProfileBinding {
    profile: ProfileId,
    storage: ContextProfileStorageClass,
}
impl AgentWorkProfileBinding {
    pub(crate) fn from_profile(profile: &zephium_core::profiles::Profile) -> Self {
        Self {
            profile: profile.id,
            storage: match profile.kind {
                ProfileKind::Default | ProfileKind::Named => ContextProfileStorageClass::Durable,
                ProfileKind::Incognito => ContextProfileStorageClass::Ephemeral,
            },
        }
    }
    pub fn profile(self) -> ProfileId {
        self.profile
    }
    pub fn storage_class(self) -> ContextProfileStorageClass {
        self.storage
    }
}
impl std::fmt::Debug for AgentWorkProfileBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentWorkProfileBinding([actor-selected])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentWorkProfileReadiness {
    Ready(AgentWorkProfileBinding),
    PolicyPending(AgentWorkProfileBinding),
    ProfileMissing,
    PolicyMissing,
    PolicyFailed,
    Unavailable,
}

#[must_use = "retain this bounded query until its reply or the original caller deadline"]
pub struct AgentWorkProfileRequest(pub(crate) Receiver<AgentWorkProfileReadiness>);
impl AgentWorkProfileRequest {
    pub fn try_recv(&self) -> Option<AgentWorkProfileReadiness> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(AgentWorkProfileReadiness::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_profile_kind_preserves_session_storage_and_debug_is_content_free() {
        for (kind, storage) in [
            (ProfileKind::Default, ContextProfileStorageClass::Durable),
            (ProfileKind::Named, ContextProfileStorageClass::Durable),
            (
                ProfileKind::Incognito,
                ContextProfileStorageClass::Ephemeral,
            ),
        ] {
            let profile = zephium_core::profiles::Profile {
                id: 123_u128.into(),
                name: "private profile name".into(),
                kind,
            };
            let binding = AgentWorkProfileBinding::from_profile(&profile);
            assert_eq!(binding.profile(), profile.id);
            assert_eq!(binding.storage_class(), storage);
            assert_eq!(
                format!("{binding:?}"),
                "AgentWorkProfileBinding([actor-selected])"
            );
        }
    }
}
