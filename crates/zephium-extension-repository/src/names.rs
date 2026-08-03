//! Fixed private-namespace names and content-addressed object names.

use zephium_private_fs::PrivateComponent;

use crate::state::Digest32;
use crate::ExtensionRepositoryError;

pub(crate) const MAX_ROOT_ENTRIES: usize = 7;
pub(crate) const MAX_CATALOG_OBJECT_ENTRIES: usize = 64;
pub(crate) const MAX_JOURNAL_ENTRIES: usize = 64;

pub(crate) fn component(value: &str) -> Result<PrivateComponent, ExtensionRepositoryError> {
    PrivateComponent::new(value).map_err(|_| ExtensionRepositoryError::StateCorrupt)
}

pub(crate) fn catalogs_directory() -> PrivateComponent {
    component("catalogs").expect("fixed catalogs component is valid")
}

pub(crate) fn journals_directory() -> PrivateComponent {
    component("journals").expect("fixed journals component is valid")
}

pub(crate) fn state_file() -> PrivateComponent {
    component("state.json").expect("fixed state component is valid")
}

pub(crate) fn state_stage() -> PrivateComponent {
    component("state.stage").expect("fixed state-stage component is valid")
}

pub(crate) fn checkpoint_file() -> PrivateComponent {
    component("recovery-checkpoint.json").expect("fixed checkpoint component is valid")
}

pub(crate) fn checkpoint_stage() -> PrivateComponent {
    component("recovery-checkpoint.stage").expect("fixed checkpoint-stage component is valid")
}

pub(crate) fn catalog_file(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.json", digest.to_hex())).expect("digest catalog component is valid")
}

pub(crate) fn catalog_stage(digest: Digest32) -> PrivateComponent {
    component(&format!("{}.stage", digest.to_hex())).expect("digest catalog stage is valid")
}

pub(crate) fn parse_catalog_file(name: &str) -> Option<(Digest32, bool)> {
    let (digest, stage) = if let Some(digest) = name.strip_suffix(".json") {
        (digest, false)
    } else if let Some(digest) = name.strip_suffix(".stage") {
        (digest, true)
    } else {
        return None;
    };
    Digest32::from_lower_hex(digest).map(|digest| (digest, stage))
}

pub(crate) fn journal_file(generation: u64, journal_digest: Digest32) -> PrivateComponent {
    component(&format!(
        "{generation:020}-{}.json",
        journal_digest.to_hex()
    ))
    .expect("journal component is valid")
}

pub(crate) fn journal_stage(generation: u64, journal_digest: Digest32) -> PrivateComponent {
    component(&format!(
        "{generation:020}-{}.stage",
        journal_digest.to_hex()
    ))
    .expect("journal stage component is valid")
}

pub(crate) fn parse_journal_file(name: &str) -> Option<(u64, Digest32, bool)> {
    let (stem, stage) = if let Some(stem) = name.strip_suffix(".json") {
        (stem, false)
    } else if let Some(stem) = name.strip_suffix(".stage") {
        (stem, true)
    } else {
        return None;
    };
    let (generation, digest) = stem.split_once('-')?;
    if generation.len() != 20 || !generation.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let generation = generation.parse().ok()?;
    let digest = Digest32::from_lower_hex(digest)?;
    Some((generation, digest, stage))
}
