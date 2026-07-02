//! Profiles are isolation roots: each owns a store partition and a webview
//! data partition. Incognito is ephemeral and never persisted.

use std::collections::HashMap;

use crate::ids::ProfileId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        if self.map.contains_key(&profile.id) {
            return false;
        }
        self.order.push(profile.id);
        self.map.insert(profile.id, profile);
        true
    }

    pub fn get(&self, id: ProfileId) -> Option<&Profile> {
        self.map.get(&id)
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
