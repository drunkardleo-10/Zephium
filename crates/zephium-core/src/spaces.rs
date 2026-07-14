//! Spaces (workspaces) group items within a profile; switching spaces swaps
//! the visible item set and the theme.

use std::collections::HashMap;

use crate::ids::{ProfileId, SpaceId};

#[derive(Clone, Debug)]
pub struct Space {
    pub id: SpaceId,
    pub profile: ProfileId,
    pub name: String,
}

/// Unforgeable aggregate proof of the exact spaces removed for one profile.
/// `Items` consumes this proof so it never guesses ownership through a
/// missing/dangling `SpaceId`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedProfileSpaces {
    profile: ProfileId,
    ids: Vec<SpaceId>,
}

impl RemovedProfileSpaces {
    pub fn profile(&self) -> ProfileId {
        self.profile
    }

    pub fn ids(&self) -> &[SpaceId] {
        &self.ids
    }
}

#[derive(Default)]
pub struct Spaces {
    order: Vec<SpaceId>,
    map: HashMap<SpaceId, Space>,
}

impl Spaces {
    pub fn insert(&mut self, space: Space) -> bool {
        if self.map.len() >= crate::session::MAX_SESSION_SPACES || self.map.contains_key(&space.id)
        {
            return false;
        }
        self.order.push(space.id);
        self.map.insert(space.id, space);
        true
    }

    pub fn get(&self, id: SpaceId) -> Option<&Space> {
        self.map.get(&id)
    }

    /// Removes only spaces whose stored ownership exactly matches `profile`.
    /// The returned proof preserves their aggregate order and can be passed to
    /// `Items::remove_for_profile`.
    pub fn remove_for_profile(&mut self, profile: ProfileId) -> RemovedProfileSpaces {
        let ids: Vec<SpaceId> = self
            .order
            .iter()
            .copied()
            .filter(|id| {
                self.map
                    .get(id)
                    .is_some_and(|space| space.profile == profile)
            })
            .collect();
        for id in &ids {
            self.map.remove(id);
        }
        self.order.retain(|id| !ids.contains(id));
        RemovedProfileSpaces { profile, ids }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Space> {
        self.order.iter().filter_map(|id| self.map.get(id))
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn first_for(&self, profile: ProfileId) -> Option<SpaceId> {
        self.iter().find(|s| s.profile == profile).map(|s| s.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_count_is_bounded_at_the_aggregate_boundary() {
        let mut spaces = Spaces::default();
        for value in 0..crate::session::MAX_SESSION_SPACES {
            assert!(spaces.insert(Space {
                id: SpaceId::from(value as u128 + 1),
                profile: ProfileId::from(1),
                name: "Space".into(),
            }));
        }
        assert!(!spaces.insert(Space {
            id: SpaceId::from(10_000),
            profile: ProfileId::from(1),
            name: "Overflow".into(),
        }));
    }

    #[test]
    fn remove_for_profile_returns_only_exact_owned_spaces() {
        let first = ProfileId::from(1);
        let second = ProfileId::from(2);
        let mut spaces = Spaces::default();
        for (id, profile) in [(10, first), (11, second), (12, first)] {
            assert!(spaces.insert(Space {
                id: SpaceId::from(id),
                profile,
                name: format!("Space {id}"),
            }));
        }

        let removed = spaces.remove_for_profile(first);
        assert_eq!(removed.profile(), first);
        assert_eq!(removed.ids(), &[SpaceId::from(10), SpaceId::from(12)]);
        assert_eq!(
            spaces.iter().map(|space| space.id).collect::<Vec<_>>(),
            vec![SpaceId::from(11)]
        );
        assert!(spaces.remove_for_profile(first).ids().is_empty());
    }
}
