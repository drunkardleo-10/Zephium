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

#[derive(Default)]
pub struct Spaces {
    order: Vec<SpaceId>,
    map: HashMap<SpaceId, Space>,
}

impl Spaces {
    pub fn insert(&mut self, space: Space) -> bool {
        if self.map.contains_key(&space.id) {
            return false;
        }
        self.order.push(space.id);
        self.map.insert(space.id, space);
        true
    }

    pub fn get(&self, id: SpaceId) -> Option<&Space> {
        self.map.get(&id)
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
