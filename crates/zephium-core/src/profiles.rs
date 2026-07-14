//! Profiles are isolation roots: each owns a store partition and a webview
//! data partition. Incognito is ephemeral and never persisted.

use std::collections::HashMap;

use crate::ids::ProfileId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProfileKind {
    Default,
    Named,
    Incognito,
}

#[derive(Clone, Debug)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub kind: ProfileKind,
}

#[derive(Default)]
pub struct Profiles {
    order: Vec<ProfileId>,
    map: HashMap<ProfileId, Profile>,
}

impl Profiles {
    pub fn insert(&mut self, profile: Profile) -> bool {
        if self.map.len() >= crate::session::MAX_SESSION_PROFILES
            || self.map.contains_key(&profile.id)
        {
            return false;
        }
        self.order.push(profile.id);
        self.map.insert(profile.id, profile);
        true
    }

    pub fn get(&self, id: ProfileId) -> Option<&Profile> {
        self.map.get(&id)
    }

    /// Removes exactly one isolation root. Selection/default-profile policy is
    /// deliberately left to the application layer.
    pub fn remove(&mut self, id: ProfileId) -> Option<Profile> {
        let profile = self.map.remove(&id)?;
        self.order.retain(|candidate| *candidate != id);
        Some(profile)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Profile> {
        self.order.iter().filter_map(|id| self.map.get(id))
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn default_profile(&self) -> Option<ProfileId> {
        self.iter()
            .find(|p| p.kind == ProfileKind::Default)
            .map(|p| p.id)
            .or(self.order.first().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_count_is_bounded_at_the_aggregate_boundary() {
        let mut profiles = Profiles::default();
        for value in 0..crate::session::MAX_SESSION_PROFILES {
            assert!(profiles.insert(Profile {
                id: ProfileId::from(value as u128 + 1),
                name: "Profile".into(),
                kind: ProfileKind::Named,
            }));
        }
        assert!(!profiles.insert(Profile {
            id: ProfileId::from(10_000),
            name: "Overflow".into(),
            kind: ProfileKind::Named,
        }));
    }

    #[test]
    fn removal_is_exact_order_preserving_and_idempotent() {
        let mut profiles = Profiles::default();
        for value in 1..=3 {
            assert!(profiles.insert(Profile {
                id: ProfileId::from(value),
                name: format!("Profile {value}"),
                kind: ProfileKind::Named,
            }));
        }

        let removed = profiles.remove(ProfileId::from(2)).unwrap();
        assert_eq!(removed.id, ProfileId::from(2));
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.id)
                .collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3)]
        );
        assert!(profiles.remove(ProfileId::from(2)).is_none());
    }
}
