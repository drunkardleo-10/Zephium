//! Private-filesystem publication and bounded control-file primitives.

use std::collections::{BTreeMap, BTreeSet};

use zephium_extension_package::{
    ExtensionReleaseCatalog, ExtensionReleaseCatalogError, MAX_EXTENSION_RELEASE_CATALOG_BYTES,
};
use zephium_private_fs::{
    ByteLimit, FileIdentity, PrivateComponent, PrivateDirectory, PrivateFsError,
};

use crate::codec;
use crate::names::{
    self, catalog_file, catalog_stage, checkpoint_file, checkpoint_stage, journal_file,
    journal_stage,
};
use crate::state::{
    Digest32, RecoveryCheckpoint, TransitionJournal, CHECKPOINT_SCHEMA_VERSION,
    MAX_CHECKPOINT_BYTES, MAX_JOURNAL_BYTES,
};
use crate::ExtensionRepositoryError;

pub(crate) type CatalogObjectIdentities = BTreeMap<Digest32, FileIdentity>;

pub(crate) struct ValidatedCatalogObject {
    pub(crate) identity: FileIdentity,
    pub(crate) length: u64,
    pub(crate) catalog: ExtensionReleaseCatalog,
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct CatalogValidationCounts {
    pub(crate) inventory_passes: usize,
    pub(crate) content_revalidated_catalogs: usize,
    pub(crate) content_revalidated_catalog_bytes: usize,
    pub(crate) fully_validated_catalogs: usize,
    pub(crate) fully_validated_catalog_bytes: usize,
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
std::thread_local! {
    static CATALOG_VALIDATION_MEASUREMENT: std::cell::Cell<Option<CatalogValidationCounts>> =
        const { std::cell::Cell::new(None) };
}

/// One-thread measurement scope for internal repository performance assertions.
///
/// The instrumentation and this guard do not exist in shipping builds. A
/// thread-local scope keeps parallel test workers from contaminating one
/// another while rejecting accidentally nested measurements on one worker.
#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
#[must_use = "dropping the guard ends the catalog validation measurement"]
pub(crate) struct CatalogValidationMeasurement {
    _not_send: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
impl CatalogValidationMeasurement {
    pub(crate) fn begin() -> Self {
        CATALOG_VALIDATION_MEASUREMENT.with(|measurement| {
            assert!(
                measurement.get().is_none(),
                "catalog validation measurement scopes may not nest"
            );
            measurement.set(Some(CatalogValidationCounts::default()));
        });
        Self {
            _not_send: std::marker::PhantomData,
        }
    }

    pub(crate) fn snapshot(&self) -> CatalogValidationCounts {
        CATALOG_VALIDATION_MEASUREMENT.with(|measurement| {
            measurement
                .get()
                .expect("catalog validation measurement is active")
        })
    }
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
impl Drop for CatalogValidationMeasurement {
    fn drop(&mut self) {
        CATALOG_VALIDATION_MEASUREMENT.with(|measurement| {
            assert!(
                measurement.replace(None).is_some(),
                "catalog validation measurement was already inactive"
            );
        });
    }
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
pub(crate) fn note_catalog_inventory_pass() {
    CATALOG_VALIDATION_MEASUREMENT.with(|measurement| {
        let Some(mut counts) = measurement.get() else {
            return;
        };
        counts.inventory_passes = counts
            .inventory_passes
            .checked_add(1)
            .expect("test-only catalog inventory counter overflowed");
        measurement.set(Some(counts));
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn note_full_catalog_validation(bytes: usize) {
    CATALOG_VALIDATION_MEASUREMENT.with(|measurement| {
        let Some(mut counts) = measurement.get() else {
            return;
        };
        counts.fully_validated_catalogs = counts
            .fully_validated_catalogs
            .checked_add(1)
            .expect("test-only catalog validation counter overflowed");
        counts.fully_validated_catalog_bytes = counts
            .fully_validated_catalog_bytes
            .checked_add(bytes)
            .expect("test-only catalog byte counter overflowed");
        measurement.set(Some(counts));
    });
}

#[cfg(all(
    test,
    zephium_internal_repository_e2e,
    any(target_os = "macos", target_os = "linux")
))]
fn note_catalog_content_revalidation(bytes: usize) {
    CATALOG_VALIDATION_MEASUREMENT.with(|measurement| {
        let Some(mut counts) = measurement.get() else {
            return;
        };
        counts.content_revalidated_catalogs = counts
            .content_revalidated_catalogs
            .checked_add(1)
            .expect("test-only catalog content revalidation counter overflowed");
        counts.content_revalidated_catalog_bytes = counts
            .content_revalidated_catalog_bytes
            .checked_add(bytes)
            .expect("test-only catalog content revalidation byte counter overflowed");
        measurement.set(Some(counts));
    });
}

/// The sole shipping structural catalog parse boundary in this crate.
///
/// Keeping every production parse behind one helper makes the internal CPU
/// amplification gate exhaustive instead of relying on individual callers to
/// remember instrumentation.
pub(crate) fn parse_catalog_canonical(
    bytes: &[u8],
) -> Result<ExtensionReleaseCatalog, ExtensionReleaseCatalogError> {
    let catalog = ExtensionReleaseCatalog::parse_canonical(bytes)?;
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    note_full_catalog_validation(bytes.len());
    Ok(catalog)
}

pub(crate) fn ensure_catalog_object(
    catalogs: &PrivateDirectory,
    digest: Digest32,
    bytes: &[u8],
) -> Result<FileIdentity, ExtensionRepositoryError> {
    let destination = catalog_file(digest);
    if let Some(identity) = catalogs.regular_identity(&destination)? {
        let stored = read_required(catalogs, &destination, MAX_EXTENSION_RELEASE_CATALOG_BYTES)?;
        if stored != bytes || catalogs.regular_identity(&destination)? != Some(identity) {
            return Err(ExtensionRepositoryError::StateCorrupt);
        }
        return Ok(identity);
    }
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    note_catalog_inventory_pass();
    let entries = catalogs
        .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
        .map_err(map_recovery_fs)?;
    if entries.len() >= names::MAX_CATALOG_OBJECT_ENTRIES {
        return Err(ExtensionRepositoryError::CatalogObjectLimit);
    }
    let stage = catalog_stage(digest);
    if catalogs.regular_exists(&stage)? {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let stage_identity = match catalogs.write_new_synced(
        &stage,
        bytes,
        ByteLimit::new(MAX_EXTENSION_RELEASE_CATALOG_BYTES)?,
    ) {
        Ok(identity) => identity,
        Err(PrivateFsError::AlreadyExists) => {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        Err(error) => return Err(ExtensionRepositoryError::FileSystem(error)),
    };
    let published_identity = catalogs
        .publish_noreplace_verified_regular(&stage, &destination)
        .map_err(|_| ExtensionRepositoryError::SettlementAmbiguous)?;
    if published_identity != stage_identity {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    let stored = read_required(catalogs, &destination, MAX_EXTENSION_RELEASE_CATALOG_BYTES)
        .map_err(|_| ExtensionRepositoryError::SettlementAmbiguous)?;
    if stored != bytes
        || catalogs
            .regular_identity(&destination)
            .map_err(|_| ExtensionRepositoryError::SettlementAmbiguous)?
            != Some(published_identity)
    {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    Ok(published_identity)
}

pub(crate) fn validate_named_catalog_object(
    catalogs: &PrivateDirectory,
    name: &PrivateComponent,
    expected_digest: Digest32,
) -> Result<ValidatedCatalogObject, ExtensionRepositoryError> {
    let identity = catalogs
        .regular_identity(name)
        .map_err(map_recovery_fs)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
    let bytes = read_required_recovery(catalogs, name, MAX_EXTENSION_RELEASE_CATALOG_BYTES)?;
    if catalogs.regular_identity(name).map_err(map_recovery_fs)? != Some(identity) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    if codec::digest(&bytes) != expected_digest {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let catalog =
        parse_catalog_canonical(&bytes).map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    let length =
        u64::try_from(bytes.len()).map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    Ok(ValidatedCatalogObject {
        identity,
        length,
        catalog,
    })
}

pub(crate) fn validate_catalog_object_cache(
    ids: &BTreeSet<Digest32>,
    identities: &CatalogObjectIdentities,
) -> Result<(), ExtensionRepositoryError> {
    if ids.len() != identities.len() || !ids.iter().eq(identities.keys()) {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

/// Revalidates the exact settled catalog namespace and content-addressed bytes.
///
/// Parsed product authority may remain cached, but opaque file identity is
/// never treated as proof that bytes are unchanged. Every entry is bounded,
/// read through the private-filesystem capability, checked against its digest
/// name, and identity-checked both before and after the read.
pub(crate) fn validate_catalog_object_inventory(
    catalogs: &PrivateDirectory,
    ids: &BTreeSet<Digest32>,
    identities: &CatalogObjectIdentities,
) -> Result<(), ExtensionRepositoryError> {
    validate_catalog_object_cache(ids, identities)?;
    #[cfg(all(
        test,
        zephium_internal_repository_e2e,
        any(target_os = "macos", target_os = "linux")
    ))]
    note_catalog_inventory_pass();
    let entries = catalogs
        .list_components(names::MAX_CATALOG_OBJECT_ENTRIES)
        .map_err(map_recovery_fs)?;
    if entries.len() != ids.len() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let mut observed = BTreeSet::new();
    for entry in entries {
        let (digest, stage) = names::parse_catalog_file(entry.as_str())
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        let expected_identity = identities
            .get(&digest)
            .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)?;
        if stage || !observed.insert(digest) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        if catalogs.regular_identity(&entry).map_err(map_recovery_fs)? != Some(*expected_identity) {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        let bytes = read_required_recovery(catalogs, &entry, MAX_EXTENSION_RELEASE_CATALOG_BYTES)?;
        if catalogs.regular_identity(&entry).map_err(map_recovery_fs)? != Some(*expected_identity)
            || codec::digest(&bytes) != digest
        {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous);
        }
        #[cfg(all(
            test,
            zephium_internal_repository_e2e,
            any(target_os = "macos", target_os = "linux")
        ))]
        note_catalog_content_revalidation(bytes.len());
    }
    if observed != *ids {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

pub(crate) fn insert_catalog_object_identity(
    ids: &mut BTreeSet<Digest32>,
    identities: &mut CatalogObjectIdentities,
    digest: Digest32,
    identity: FileIdentity,
) -> Result<(), ExtensionRepositoryError> {
    validate_catalog_object_cache(ids, identities)?;
    match (ids.contains(&digest), identities.get(&digest)) {
        (true, Some(existing)) if *existing == identity => return Ok(()),
        (false, None) => {}
        _ => return Err(ExtensionRepositoryError::RecoveryAmbiguous),
    }
    if !ids.insert(digest) || identities.insert(digest, identity).is_some() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    validate_catalog_object_cache(ids, identities)
}

pub(crate) fn forget_catalog_object_identity(
    ids: &mut BTreeSet<Digest32>,
    identities: &mut CatalogObjectIdentities,
    digest: Digest32,
) -> Result<(), ExtensionRepositoryError> {
    validate_catalog_object_cache(ids, identities)?;
    if !ids.remove(&digest) || identities.remove(&digest).is_none() {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    validate_catalog_object_cache(ids, identities)
}

pub(crate) fn publish_journal(
    journals: &PrivateDirectory,
    journal: &TransitionJournal,
    bytes: &[u8],
) -> Result<PrivateComponent, ExtensionRepositoryError> {
    if !journals
        .list_components(names::MAX_JOURNAL_ENTRIES)
        .map_err(map_recovery_fs)?
        .is_empty()
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let journal_digest = codec::digest(bytes);
    let stage = journal_stage(journal.generation, journal_digest);
    let destination = journal_file(journal.generation, journal_digest);
    match journals.write_new_synced(&stage, bytes, ByteLimit::new(MAX_JOURNAL_BYTES)?) {
        Ok(_) => {}
        Err(PrivateFsError::AlreadyExists) => {
            return Err(ExtensionRepositoryError::RecoveryAmbiguous)
        }
        Err(error) => return Err(ExtensionRepositoryError::FileSystem(error)),
    }
    if journals
        .publish_noreplace_verified_regular(&stage, &destination)
        .is_err()
    {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    let stored = read_required(journals, &destination, MAX_JOURNAL_BYTES)
        .map_err(|_| ExtensionRepositoryError::SettlementAmbiguous)?;
    if stored != bytes {
        return Err(ExtensionRepositoryError::SettlementAmbiguous);
    }
    Ok(destination)
}

pub(crate) fn write_checkpoint(
    root: &PrivateDirectory,
    checkpoint: &RecoveryCheckpoint,
) -> Result<(), ExtensionRepositoryError> {
    if checkpoint.schema_version != CHECKPOINT_SCHEMA_VERSION {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    let bytes = codec::encode(checkpoint, MAX_CHECKPOINT_BYTES)
        .map_err(|_| ExtensionRepositoryError::RecoveryAmbiguous)?;
    atomic_write_control(
        root,
        &checkpoint_file(),
        &checkpoint_stage(),
        &bytes,
        MAX_CHECKPOINT_BYTES,
    )
}

pub(crate) fn atomic_write_control(
    directory: &PrivateDirectory,
    destination: &PrivateComponent,
    stage: &PrivateComponent,
    bytes: &[u8],
    maximum: usize,
) -> Result<(), ExtensionRepositoryError> {
    if directory.regular_exists(stage)? {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    directory.write_new_synced(stage, bytes, ByteLimit::new(maximum)?)?;
    if directory.regular_exists(destination)? {
        directory.replace_verified_regular(stage, destination)?;
    } else {
        directory.publish_noreplace_verified_regular(stage, destination)?;
    }
    let stored = read_required(directory, destination, maximum)?;
    if stored != bytes {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

fn read_optional(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(ExtensionRepositoryError::FileSystem)
}

pub(crate) fn read_optional_state(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(|error| match error {
            PrivateFsError::BoundExceeded => ExtensionRepositoryError::StateCorrupt,
            other => ExtensionRepositoryError::FileSystem(other),
        })
}

pub(crate) fn read_optional_recovery(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Option<Vec<u8>>, ExtensionRepositoryError> {
    directory
        .read_bounded_regular(name, ByteLimit::new(maximum)?)
        .map_err(map_recovery_fs)
}

pub(crate) fn read_required(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional(directory, name, maximum)?.ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
}

pub(crate) fn read_required_state(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional_state(directory, name, maximum)?.ok_or(ExtensionRepositoryError::StateCorrupt)
}

pub(crate) fn read_required_recovery(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
    maximum: usize,
) -> Result<Vec<u8>, ExtensionRepositoryError> {
    read_optional_recovery(directory, name, maximum)?
        .ok_or(ExtensionRepositoryError::RecoveryAmbiguous)
}

pub(crate) fn remove_required(
    directory: &PrivateDirectory,
    name: &PrivateComponent,
) -> Result<(), ExtensionRepositoryError> {
    if !directory
        .remove_verified_regular(name)
        .map_err(map_recovery_fs)?
    {
        return Err(ExtensionRepositoryError::RecoveryAmbiguous);
    }
    Ok(())
}

pub(crate) fn map_recovery_fs(error: PrivateFsError) -> ExtensionRepositoryError {
    match error {
        PrivateFsError::BoundExceeded => ExtensionRepositoryError::RecoveryAmbiguous,
        other => ExtensionRepositoryError::FileSystem(other),
    }
}
