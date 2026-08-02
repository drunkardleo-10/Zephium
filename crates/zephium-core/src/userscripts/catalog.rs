//! Persistent userscript aggregate and compare-and-swap mutation contracts.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use crate::ids::UserscriptId;
use crate::ports::engine::MAX_USER_SCRIPT_BYTES;

use super::{
    assess_userscript_compatibility, parse_userscript_metadata, ParsedUserscriptMetadata,
    UserscriptCompatibility, UserscriptMetadataError, UserscriptSourceDigest,
};

pub const CURRENT_USERSCRIPT_METADATA_FORMAT: u32 = 1;
pub const MAX_USERSCRIPTS_PER_PROFILE: usize = 64;
pub const MAX_USERSCRIPT_CATALOG_SOURCE_BYTES: usize = 16 * 1024 * 1024;

macro_rules! nonzero_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            pub const INITIAL: Self = Self(1);

            pub const fn new(value: u64) -> Option<Self> {
                if value == 0 {
                    None
                } else {
                    Some(Self(value))
                }
            }

            pub const fn get(self) -> u64 {
                self.0
            }

            pub const fn next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(next) => Some(Self(next)),
                    None => None,
                }
            }
        }
    };
}

nonzero_revision!(UserscriptRevision);
nonzero_revision!(UserscriptCatalogRevision);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Userscript {
    pub id: UserscriptId,
    pub revision: UserscriptRevision,
    pub enabled: bool,
    pub metadata_format: u32,
    pub source: Arc<str>,
    pub digest: UserscriptSourceDigest,
    pub metadata: ParsedUserscriptMetadata,
    pub compatibility: UserscriptCompatibility,
}

impl Userscript {
    pub fn from_source(
        id: UserscriptId,
        revision: UserscriptRevision,
        enabled: bool,
        source: Arc<str>,
    ) -> Result<Self, UserscriptCatalogError> {
        let digest = UserscriptSourceDigest::for_source(&source);
        Self::from_persisted(
            id,
            revision,
            enabled,
            CURRENT_USERSCRIPT_METADATA_FORMAT,
            source,
            digest,
        )
    }

    pub fn from_persisted(
        id: UserscriptId,
        revision: UserscriptRevision,
        enabled: bool,
        metadata_format: u32,
        source: Arc<str>,
        digest: UserscriptSourceDigest,
    ) -> Result<Self, UserscriptCatalogError> {
        if metadata_format != CURRENT_USERSCRIPT_METADATA_FORMAT {
            return Err(UserscriptCatalogError::UnsupportedMetadataFormat {
                found: metadata_format,
                supported: CURRENT_USERSCRIPT_METADATA_FORMAT,
            });
        }
        if source.len() > MAX_USER_SCRIPT_BYTES {
            return Err(UserscriptCatalogError::SourceTooLarge {
                length: source.len(),
                max: MAX_USER_SCRIPT_BYTES,
            });
        }
        if UserscriptSourceDigest::for_source(&source) != digest {
            return Err(UserscriptCatalogError::DigestMismatch { id });
        }
        let metadata = parse_userscript_metadata(&source)
            .map_err(|error| UserscriptCatalogError::InvalidMetadata { id, error })?;
        let compatibility = assess_userscript_compatibility(&metadata);
        Ok(Self {
            id,
            revision,
            enabled,
            metadata_format,
            source,
            digest,
            metadata,
            compatibility,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserscriptCatalog {
    revision: UserscriptCatalogRevision,
    scripts: Vec<Userscript>,
    source_bytes: usize,
}

impl UserscriptCatalog {
    pub fn new(
        revision: UserscriptCatalogRevision,
        mut scripts: Vec<Userscript>,
    ) -> Result<Self, UserscriptCatalogError> {
        if scripts.len() > MAX_USERSCRIPTS_PER_PROFILE {
            return Err(UserscriptCatalogError::TooManyScripts {
                count: scripts.len(),
                max: MAX_USERSCRIPTS_PER_PROFILE,
            });
        }
        scripts.sort_unstable_by_key(|script| script.id);
        let mut ids = HashSet::with_capacity(scripts.len());
        let mut source_bytes = 0_usize;
        for script in &scripts {
            if !ids.insert(script.id) {
                return Err(UserscriptCatalogError::DuplicateId(script.id));
            }
            source_bytes = source_bytes.checked_add(script.source.len()).ok_or(
                UserscriptCatalogError::CatalogSourceTooLarge {
                    length: usize::MAX,
                    max: MAX_USERSCRIPT_CATALOG_SOURCE_BYTES,
                },
            )?;
            if source_bytes > MAX_USERSCRIPT_CATALOG_SOURCE_BYTES {
                return Err(UserscriptCatalogError::CatalogSourceTooLarge {
                    length: source_bytes,
                    max: MAX_USERSCRIPT_CATALOG_SOURCE_BYTES,
                });
            }
        }
        Ok(Self {
            revision,
            scripts,
            source_bytes,
        })
    }

    pub const fn revision(&self) -> UserscriptCatalogRevision {
        self.revision
    }

    pub fn scripts(&self) -> &[Userscript] {
        &self.scripts
    }

    pub const fn source_bytes(&self) -> usize {
        self.source_bytes
    }

    pub fn get(&self, id: UserscriptId) -> Option<&Userscript> {
        self.scripts
            .binary_search_by_key(&id, |script| script.id)
            .ok()
            .map(|index| &self.scripts[index])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptCatalogMutation {
    Install {
        id: UserscriptId,
        enabled: bool,
        source: Arc<str>,
    },
    UpdateSource {
        id: UserscriptId,
        expected: UserscriptRevision,
        source: Arc<str>,
    },
    SetEnabled {
        id: UserscriptId,
        expected: UserscriptRevision,
        enabled: bool,
    },
    Delete {
        id: UserscriptId,
        expected: UserscriptRevision,
    },
}

impl UserscriptCatalogMutation {
    pub fn source_bytes(&self) -> usize {
        match self {
            Self::Install { source, .. } | Self::UpdateSource { source, .. } => source.len(),
            Self::SetEnabled { .. } | Self::Delete { .. } => 0,
        }
    }

    pub fn validate_source(&self) -> Result<(), UserscriptCatalogError> {
        match self {
            Self::Install {
                id,
                enabled,
                source,
            } => {
                Userscript::from_source(*id, UserscriptRevision::INITIAL, *enabled, source.clone())
                    .map(|_| ())
            }
            Self::UpdateSource {
                id,
                expected,
                source,
            } => Userscript::from_source(*id, *expected, false, source.clone()).map(|_| ()),
            Self::SetEnabled { .. } | Self::Delete { .. } => Ok(()),
        }
    }

    /// Cheap admission check for a caller-facing mailbox. Syntax and digest
    /// work belongs on the storage actor, not the UI/application thread.
    pub fn source_envelope_is_valid(&self) -> bool {
        match self {
            Self::Install { source, .. } | Self::UpdateSource { source, .. } => {
                !source.is_empty()
                    && source.len() <= MAX_USER_SCRIPT_BYTES
                    && !source.as_bytes().contains(&0)
            }
            Self::SetEnabled { .. } | Self::Delete { .. } => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UserscriptCatalogError {
    SourceTooLarge {
        length: usize,
        max: usize,
    },
    InvalidMetadata {
        id: UserscriptId,
        error: UserscriptMetadataError,
    },
    UnsupportedMetadataFormat {
        found: u32,
        supported: u32,
    },
    DigestMismatch {
        id: UserscriptId,
    },
    TooManyScripts {
        count: usize,
        max: usize,
    },
    CatalogSourceTooLarge {
        length: usize,
        max: usize,
    },
    DuplicateId(UserscriptId),
}

impl fmt::Display for UserscriptCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceTooLarge { length, max } => {
                write!(
                    formatter,
                    "userscript source is {length} bytes; limit is {max}"
                )
            }
            Self::InvalidMetadata { id, error } => {
                write!(formatter, "userscript {id} metadata is invalid: {error}")
            }
            Self::UnsupportedMetadataFormat { found, supported } => write!(
                formatter,
                "userscript metadata format {found} is unsupported; supported format is {supported}"
            ),
            Self::DigestMismatch { id } => write!(formatter, "userscript {id} digest mismatches"),
            Self::TooManyScripts { count, max } => {
                write!(
                    formatter,
                    "userscript catalog has {count} scripts; limit is {max}"
                )
            }
            Self::CatalogSourceTooLarge { length, max } => write!(
                formatter,
                "userscript catalog retains {length} source bytes; limit is {max}"
            ),
            Self::DuplicateId(id) => write!(formatter, "userscript catalog duplicates id {id}"),
        }
    }
}

impl Error for UserscriptCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidMetadata { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str) -> Arc<str> {
        format!(
            "// ==UserScript==\n// @name {name}\n// @match https://example.com/*\n// ==/UserScript==\n"
        )
        .into()
    }

    #[test]
    fn catalog_is_canonical_and_owner_ids_are_unique() {
        let high = Userscript::from_source(
            UserscriptId::from(2),
            UserscriptRevision::INITIAL,
            true,
            source("High"),
        )
        .unwrap();
        let low = Userscript::from_source(
            UserscriptId::from(1),
            UserscriptRevision::INITIAL,
            false,
            source("Low"),
        )
        .unwrap();
        let catalog = UserscriptCatalog::new(
            UserscriptCatalogRevision::INITIAL,
            vec![high.clone(), low.clone()],
        )
        .unwrap();
        assert_eq!(catalog.scripts()[0].id, low.id);
        assert_eq!(catalog.scripts()[1].id, high.id);
        assert_eq!(catalog.source_bytes(), low.source.len() + high.source.len());
        assert_eq!(
            UserscriptCatalog::new(UserscriptCatalogRevision::INITIAL, vec![low.clone(), low]),
            Err(UserscriptCatalogError::DuplicateId(UserscriptId::from(1)))
        );
    }

    #[test]
    fn persisted_digest_and_format_are_exact_authority() {
        let source = source("A");
        assert!(matches!(
            Userscript::from_persisted(
                UserscriptId::from(1),
                UserscriptRevision::INITIAL,
                true,
                CURRENT_USERSCRIPT_METADATA_FORMAT + 1,
                source.clone(),
                UserscriptSourceDigest::for_source(&source),
            ),
            Err(UserscriptCatalogError::UnsupportedMetadataFormat { .. })
        ));
        assert!(matches!(
            Userscript::from_persisted(
                UserscriptId::from(1),
                UserscriptRevision::INITIAL,
                true,
                CURRENT_USERSCRIPT_METADATA_FORMAT,
                source,
                UserscriptSourceDigest::from_bytes([0; 32]),
            ),
            Err(UserscriptCatalogError::DigestMismatch { .. })
        ));

        let nul_source: Arc<str> =
            "// ==UserScript==\n// @name A\n// @match <all_urls>\n// ==/UserScript==\n\0".into();
        let mutation = UserscriptCatalogMutation::Install {
            id: UserscriptId::from(2),
            enabled: true,
            source: nul_source.clone(),
        };
        assert!(!mutation.source_envelope_is_valid());
        assert!(matches!(
            Userscript::from_source(
                UserscriptId::from(2),
                UserscriptRevision::INITIAL,
                true,
                nul_source,
            ),
            Err(UserscriptCatalogError::InvalidMetadata {
                error: UserscriptMetadataError::SourceContainsNul,
                ..
            })
        ));
    }

    #[test]
    fn revisions_never_wrap_or_accept_zero() {
        assert!(UserscriptRevision::new(0).is_none());
        assert!(UserscriptCatalogRevision::new(0).is_none());
        assert!(UserscriptRevision::new(u64::MAX).unwrap().next().is_none());
        assert!(UserscriptCatalogRevision::new(u64::MAX)
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn catalog_count_and_aggregate_source_budgets_are_exact() {
        let scripts: Vec<_> = (0..=MAX_USERSCRIPTS_PER_PROFILE)
            .map(|index| {
                Userscript::from_source(
                    UserscriptId::from(index as u128 + 1),
                    UserscriptRevision::INITIAL,
                    true,
                    source("Bounded"),
                )
                .unwrap()
            })
            .collect();
        assert!(matches!(
            UserscriptCatalog::new(UserscriptCatalogRevision::INITIAL, scripts),
            Err(UserscriptCatalogError::TooManyScripts { count, max })
                if count == MAX_USERSCRIPTS_PER_PROFILE + 1
                    && max == MAX_USERSCRIPTS_PER_PROFILE
        ));

        let prefix = source("Large");
        let mut large_source = String::with_capacity(MAX_USER_SCRIPT_BYTES);
        large_source.push_str(&prefix);
        large_source.push_str(&"x".repeat(MAX_USER_SCRIPT_BYTES - prefix.len()));
        let large_source: Arc<str> = large_source.into();
        let scripts: Vec<_> = (0..9)
            .map(|index| {
                Userscript::from_source(
                    UserscriptId::from(index + 1),
                    UserscriptRevision::INITIAL,
                    true,
                    large_source.clone(),
                )
                .unwrap()
            })
            .collect();
        assert!(matches!(
            UserscriptCatalog::new(UserscriptCatalogRevision::INITIAL, scripts),
            Err(UserscriptCatalogError::CatalogSourceTooLarge { length, max })
                if length > MAX_USERSCRIPT_CATALOG_SOURCE_BYTES
                    && max == MAX_USERSCRIPT_CATALOG_SOURCE_BYTES
        ));
    }
}
