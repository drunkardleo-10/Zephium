//! Fixed-size projection of the profiles that run extensions.

use crate::ids::ProfileId;

/// At most this many profiles run extensions at once. Keep the projection
/// fixed-size and allocation-free.
pub const MAX_EXTENSION_ACTIVE_PROFILES: usize = 3;

/// Exact profiles whose extension runtimes are active.
///
/// This value is routing data, not authority. It exists solely so the Shell
/// can publish logical browser tabs to the native runtimes that need them
/// without broadcasting every profile.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtensionActiveProfiles {
    entries: [[u8; 16]; MAX_EXTENSION_ACTIVE_PROFILES],
    length: u8,
}

const _: () = assert!(std::mem::size_of::<ExtensionActiveProfiles>() <= 64);

impl ExtensionActiveProfiles {
    pub const EMPTY: Self = Self {
        entries: [[0; 16]; MAX_EXTENSION_ACTIVE_PROFILES],
        length: 0,
    };

    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub const fn len(&self) -> usize {
        self.length as usize
    }

    pub fn contains(&self, profile: ProfileId) -> bool {
        self.entries[..self.len()].contains(&profile.bytes())
    }

    /// Inserts one unique profile without allocation. Re-inserting an exact
    /// identity succeeds; exceeding the ceiling is refused.
    pub fn try_insert(&mut self, profile: ProfileId) -> bool {
        if self.contains(profile) {
            return true;
        }
        let index = self.len();
        if index == MAX_EXTENSION_ACTIVE_PROFILES {
            return false;
        }
        self.entries[index] = profile.bytes();
        self.length += 1;
        true
    }

    pub fn remove(&mut self, profile: ProfileId) -> bool {
        let length = self.len();
        let Some(index) = self.entries[..length]
            .iter()
            .position(|entry| *entry == profile.bytes())
        else {
            return false;
        };
        self.entries.copy_within(index + 1..length, index);
        self.entries[length - 1] = [0; 16];
        self.length -= 1;
        true
    }

    pub fn iter(self) -> impl Iterator<Item = ProfileId> {
        self.entries
            .into_iter()
            .take(self.len())
            .map(|bytes| ProfileId::from(u128::from_be_bytes(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_profile_projection_is_unique_compact_and_bounded() {
        let mut profiles = ExtensionActiveProfiles::EMPTY;
        for value in 1..=MAX_EXTENSION_ACTIVE_PROFILES {
            assert!(profiles.try_insert(ProfileId::from(value as u128)));
        }
        assert!(profiles.try_insert(ProfileId::from(2)));
        assert!(!profiles.try_insert(ProfileId::from(99)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(2), ProfileId::from(3)]
        );
        assert!(profiles.remove(ProfileId::from(2)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3)]
        );
        assert!(!profiles.remove(ProfileId::from(2)));
        assert!(profiles.try_insert(ProfileId::from(4)));
        assert_eq!(
            profiles.iter().collect::<Vec<_>>(),
            vec![ProfileId::from(1), ProfileId::from(3), ProfileId::from(4)]
        );
    }
}
