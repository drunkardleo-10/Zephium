//! Canonical, bounded package-resource plans.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::mem::size_of;
use std::ops::Bound::{Included, Unbounded};

use sha2::{Digest, Sha256};

/// Maximum declared size of one runtime-readable package resource.
///
/// Resource access is synchronous and streaming. Consumers remain responsible
/// for their own smaller retained-memory limits; this ceiling only bounds the
/// exact package resource that can be exposed through a reader.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_BYTES: u64 = 32 * 1024 * 1024;

/// Maximum declared size of an extension manifest.
pub const MAX_EXTENSION_RUNTIME_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Maximum number of files in one runtime package plan.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES: usize = 4_096;

/// Maximum UTF-8 bytes in one canonical package-relative runtime path.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_PATH_BYTES: usize = 512;

/// Maximum bytes in one canonical runtime path component.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_PATH_COMPONENT_BYTES: usize = 128;

/// Maximum components in one canonical runtime resource path.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_PATH_DEPTH: usize = 32;

/// Maximum deterministic memory retained by one runtime resource plan.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES: usize = 4 * 1024 * 1024;

const RESOURCE_PLAN_DOMAIN: &[u8] = b"zephium:extension-runtime-resource-plan:v1\0";
const RESOURCE_DOMAIN: &[u8] = b"zephium:extension-runtime-resource:v1\0";

/// An opaque, bounded package-resource descriptor.
///
/// This value identifies data; it does not authorize access to that data. Its
/// identifier commits to a complete plan digest and to this entry's internally
/// assigned ordinal, canonical path, exact length, and SHA-256. The delegated
/// provider must independently authenticate every field against its package.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ExtensionRuntimeResource {
    identifier: [u8; 32],
    declared_bytes: u64,
    ordinal: u32,
}

impl ExtensionRuntimeResource {
    fn from_binding(
        plan_digest: [u8; 32],
        ordinal: u32,
        binding: &ExtensionRuntimeResourceBinding,
    ) -> Self {
        Self {
            identifier: resource_identifier(
                plan_digest,
                ordinal,
                binding.path(),
                binding.declared_bytes,
                binding.sha256,
            ),
            declared_bytes: binding.declared_bytes,
            ordinal,
        }
    }

    #[cfg(test)]
    pub(crate) fn try_new(
        identifier: [u8; 32],
        declared_bytes: u64,
    ) -> Result<Self, ExtensionRuntimeResourceBuildError> {
        if declared_bytes > MAX_EXTENSION_RUNTIME_RESOURCE_BYTES {
            return Err(ExtensionRuntimeResourceBuildError::LengthExceeded {
                declared_bytes,
                maximum_bytes: MAX_EXTENSION_RUNTIME_RESOURCE_BYTES,
            });
        }
        Ok(Self {
            identifier,
            declared_bytes,
            ordinal: u32::MAX,
        })
    }

    /// Returns the opaque, domain-separated resource identifier.
    #[must_use]
    pub const fn identifier(&self) -> &[u8; 32] {
        &self.identifier
    }

    /// Returns the exact byte length committed by the descriptor.
    #[must_use]
    pub const fn declared_bytes(&self) -> u64 {
        self.declared_bytes
    }

    /// Returns the plan-assigned canonical inventory ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Verifies this descriptor against one exact canonical plan binding.
    ///
    /// Delegated providers use this after selecting their immutable package
    /// inventory entry by [`Self::ordinal`]. Invalid or oversized paths fail
    /// without allocation.
    #[must_use]
    pub fn authenticates(
        &self,
        plan_digest: [u8; 32],
        ordinal: u32,
        canonical_path: &str,
        declared_bytes: u64,
        sha256: [u8; 32],
    ) -> bool {
        self.ordinal == ordinal
            && self.declared_bytes == declared_bytes
            && is_valid_resource_path(canonical_path)
            && self.identifier
                == resource_identifier(plan_digest, ordinal, canonical_path, declared_bytes, sha256)
    }
}

impl fmt::Debug for ExtensionRuntimeResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeResource")
            .field("identifier", &"[redacted]")
            .field("declared_bytes", &self.declared_bytes)
            .field("ordinal", &self.ordinal)
            .finish()
    }
}

/// One exact canonical package-file binding supplied to plan construction.
#[derive(Clone, Eq, PartialEq)]
pub struct ExtensionRuntimeResourceBinding {
    path: Box<str>,
    declared_bytes: u64,
    sha256: [u8; 32],
}

impl ExtensionRuntimeResourceBinding {
    /// Validates and owns one canonical package-relative file identity.
    pub fn try_new(
        canonical_path: &str,
        declared_bytes: u64,
        sha256: [u8; 32],
    ) -> Result<Self, ExtensionRuntimeResourceBuildError> {
        if !is_valid_resource_path(canonical_path) {
            return Err(ExtensionRuntimeResourceBuildError::InvalidPath);
        }
        if declared_bytes > MAX_EXTENSION_RUNTIME_RESOURCE_BYTES {
            return Err(ExtensionRuntimeResourceBuildError::LengthExceeded {
                declared_bytes,
                maximum_bytes: MAX_EXTENSION_RUNTIME_RESOURCE_BYTES,
            });
        }
        Ok(Self {
            path: canonical_path.into(),
            declared_bytes,
            sha256,
        })
    }

    /// Returns the exact canonical package-relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the exact indexed file length.
    #[must_use]
    pub const fn declared_bytes(&self) -> u64 {
        self.declared_bytes
    }

    /// Returns SHA-256 of the exact indexed file bytes.
    #[must_use]
    pub const fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}

impl fmt::Debug for ExtensionRuntimeResourceBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeResourceBinding")
            .field("path", &"[redacted]")
            .field("declared_bytes", &self.declared_bytes)
            .field("sha256", &"[redacted]")
            .finish()
    }
}

/// Why one runtime-resource binding could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeResourceBuildError {
    /// The path is not in the closed portable package grammar.
    InvalidPath,
    /// The declared resource length exceeded the process-wide policy limit.
    LengthExceeded {
        /// The rejected length.
        declared_bytes: u64,
        /// The applicable limit.
        maximum_bytes: u64,
    },
}

impl fmt::Display for ExtensionRuntimeResourceBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "extension runtime resource path is invalid",
            Self::LengthExceeded { .. } => "extension resource exceeds the runtime size limit",
        })
    }
}

impl Error for ExtensionRuntimeResourceBuildError {}

/// One immutable entry in a complete runtime package-resource plan.
#[derive(Eq, PartialEq)]
pub struct ExtensionRuntimeResourcePlanEntry {
    binding: ExtensionRuntimeResourceBinding,
    resource: ExtensionRuntimeResource,
}

impl ExtensionRuntimeResourcePlanEntry {
    /// Returns the exact canonical package-relative path.
    #[must_use]
    pub fn path(&self) -> &str {
        self.binding.path()
    }

    /// Returns the exact file length.
    #[must_use]
    pub const fn declared_bytes(&self) -> u64 {
        self.binding.declared_bytes()
    }

    /// Returns SHA-256 of the exact file bytes.
    #[must_use]
    pub const fn sha256(&self) -> [u8; 32] {
        self.binding.sha256()
    }

    /// Returns the opaque descriptor accepted by delegated package access.
    #[must_use]
    pub const fn resource(&self) -> ExtensionRuntimeResource {
        self.resource
    }
}

impl fmt::Debug for ExtensionRuntimeResourcePlanEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeResourcePlanEntry")
            .field("binding", &"[redacted]")
            .field("resource", &self.resource)
            .finish()
    }
}

/// Complete canonical file inventory delegated to a backend-neutral runtime.
///
/// Entries remain in exact canonical package order. Construction rejects all
/// duplicate, case-aliasing, file/directory-aliasing, oversized, or unsorted
/// inventories before computing the plan digest and per-entry descriptors.
pub struct ExtensionRuntimeResourcePlan {
    entries: Box<[ExtensionRuntimeResourcePlanEntry]>,
    digest: [u8; 32],
    manifest_ordinal: u32,
    retained_bytes: usize,
}

impl ExtensionRuntimeResourcePlan {
    /// Constructs a complete canonical resource plan.
    pub fn try_new(
        bindings: Vec<ExtensionRuntimeResourceBinding>,
    ) -> Result<Self, ExtensionRuntimeResourcePlanBuildError> {
        if bindings.is_empty() {
            return Err(ExtensionRuntimeResourcePlanBuildError::Empty);
        }
        if bindings.len() > MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES {
            return Err(ExtensionRuntimeResourcePlanBuildError::TooManyEntries {
                count: bindings.len(),
                maximum: MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES,
            });
        }

        let mut collision_keys = BTreeSet::new();
        let mut previous_path: Option<&str> = None;
        let mut manifest_ordinal = None;
        for (ordinal, binding) in bindings.iter().enumerate() {
            if previous_path.is_some_and(|previous| previous >= binding.path()) {
                return Err(ExtensionRuntimeResourcePlanBuildError::NonCanonicalOrder);
            }
            previous_path = Some(binding.path());
            let collision_key = binding.path().to_ascii_lowercase().into_boxed_str();
            if portable_path_shape_conflicts(&collision_keys, &collision_key) {
                return Err(ExtensionRuntimeResourcePlanBuildError::PathCollision);
            }
            collision_keys.insert(collision_key);
            if binding.path() == "manifest.json" {
                if binding.declared_bytes == 0
                    || binding.declared_bytes > MAX_EXTENSION_RUNTIME_MANIFEST_BYTES
                {
                    return Err(ExtensionRuntimeResourcePlanBuildError::ManifestLength);
                }
                manifest_ordinal = Some(
                    u32::try_from(ordinal)
                        .map_err(|_| ExtensionRuntimeResourcePlanBuildError::AccountingOverflow)?,
                );
            }
        }
        let manifest_ordinal =
            manifest_ordinal.ok_or(ExtensionRuntimeResourcePlanBuildError::ManifestMissing)?;

        let digest = plan_digest(&bindings);
        let mut entries = Vec::with_capacity(bindings.len());
        for (ordinal, binding) in bindings.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| ExtensionRuntimeResourcePlanBuildError::AccountingOverflow)?;
            let resource = ExtensionRuntimeResource::from_binding(digest, ordinal, &binding);
            entries.push(ExtensionRuntimeResourcePlanEntry { binding, resource });
        }
        let entries = entries.into_boxed_slice();
        let retained_bytes = entries.iter().try_fold(
            size_of::<Self>()
                .checked_add(
                    entries
                        .len()
                        .checked_mul(size_of::<ExtensionRuntimeResourcePlanEntry>())
                        .ok_or(ExtensionRuntimeResourcePlanBuildError::AccountingOverflow)?,
                )
                .ok_or(ExtensionRuntimeResourcePlanBuildError::AccountingOverflow)?,
            |total, entry| {
                total
                    .checked_add(entry.path().len())
                    .ok_or(ExtensionRuntimeResourcePlanBuildError::AccountingOverflow)
            },
        )?;
        if retained_bytes > MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES {
            return Err(
                ExtensionRuntimeResourcePlanBuildError::RetainedBytesExceeded {
                    retained_bytes,
                    maximum: MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES,
                },
            );
        }

        Ok(Self {
            entries,
            digest,
            manifest_ordinal,
            retained_bytes,
        })
    }

    /// Returns the exact canonical sorted inventory.
    #[must_use]
    pub fn entries(&self) -> &[ExtensionRuntimeResourcePlanEntry] {
        &self.entries
    }

    /// Finds one exact canonical path without allocation or regular expressions.
    #[must_use]
    pub fn entry(&self, canonical_path: &str) -> Option<&ExtensionRuntimeResourcePlanEntry> {
        if !is_valid_resource_path(canonical_path) {
            return None;
        }
        self.entries
            .binary_search_by(|entry| entry.path().cmp(canonical_path))
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// Returns the exact root `manifest.json` descriptor.
    #[must_use]
    pub fn manifest(&self) -> ExtensionRuntimeResource {
        self.entries[self.manifest_ordinal as usize].resource()
    }

    /// Returns the domain-separated digest of the complete canonical plan.
    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Returns exact deterministic bytes retained by this plan.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub(crate) fn exclusive_heap_bytes(&self) -> usize {
        self.retained_bytes - size_of::<Self>()
    }
}

impl fmt::Debug for ExtensionRuntimeResourcePlan {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeResourcePlan")
            .field("entries", &self.entries.len())
            .field("digest", &"[redacted]")
            .field("manifest", &"[redacted]")
            .field("retained_bytes", &self.retained_bytes)
            .finish()
    }
}

/// Why a complete runtime-resource plan could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeResourcePlanBuildError {
    /// A package plan must contain at least `manifest.json`.
    Empty,
    /// The file count exceeded the fixed runtime inventory ceiling.
    TooManyEntries {
        /// Observed number of entries.
        count: usize,
        /// Maximum number of entries.
        maximum: usize,
    },
    /// Input paths were not in strictly increasing canonical byte order.
    NonCanonicalOrder,
    /// Paths collide by case or file/directory shape on a supported host.
    PathCollision,
    /// The exact root `manifest.json` entry was absent.
    ManifestMissing,
    /// The manifest length was zero or exceeded its stricter limit.
    ManifestLength,
    /// Deterministic retained-memory accounting overflowed.
    AccountingOverflow,
    /// The complete plan exceeded its retained-memory ceiling.
    RetainedBytesExceeded {
        /// Exact deterministic retained bytes.
        retained_bytes: usize,
        /// Applicable retained-memory limit.
        maximum: usize,
    },
}

impl fmt::Display for ExtensionRuntimeResourcePlanBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "extension runtime resource plan is empty",
            Self::TooManyEntries { .. } => "extension runtime resource plan has too many entries",
            Self::NonCanonicalOrder => "extension runtime resource plan is not canonically ordered",
            Self::PathCollision => "extension runtime resource plan paths collide",
            Self::ManifestMissing => "extension runtime resource plan has no manifest",
            Self::ManifestLength => "extension runtime manifest length is invalid",
            Self::AccountingOverflow => "extension runtime resource plan accounting overflowed",
            Self::RetainedBytesExceeded { .. } => {
                "extension runtime resource plan exceeds its memory limit"
            }
        })
    }
}

impl Error for ExtensionRuntimeResourcePlanBuildError {}

fn plan_digest(bindings: &[ExtensionRuntimeResourceBinding]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(RESOURCE_PLAN_DOMAIN);
    digest.update((bindings.len() as u64).to_be_bytes());
    for binding in bindings {
        update_binding(
            &mut digest,
            binding.path(),
            binding.declared_bytes,
            binding.sha256,
        );
    }
    digest.finalize().into()
}

fn resource_identifier(
    plan_digest: [u8; 32],
    ordinal: u32,
    canonical_path: &str,
    declared_bytes: u64,
    sha256: [u8; 32],
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(RESOURCE_DOMAIN);
    digest.update(plan_digest);
    digest.update(ordinal.to_be_bytes());
    update_binding(&mut digest, canonical_path, declared_bytes, sha256);
    digest.finalize().into()
}

fn update_binding(
    digest: &mut Sha256,
    canonical_path: &str,
    declared_bytes: u64,
    sha256: [u8; 32],
) {
    digest.update((canonical_path.len() as u64).to_be_bytes());
    digest.update(canonical_path.as_bytes());
    digest.update(declared_bytes.to_be_bytes());
    digest.update(sha256);
}

fn is_valid_resource_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_EXTENSION_RUNTIME_RESOURCE_PATH_BYTES
        || !value.is_ascii()
        || value.starts_with('/')
        || value.ends_with('/')
    {
        return false;
    }

    let mut depth = 0_usize;
    for component in value.split('/') {
        let Some(next_depth) = depth.checked_add(1) else {
            return false;
        };
        depth = next_depth;
        if !is_valid_resource_path_component(component) {
            return false;
        }
    }
    depth <= MAX_EXTENSION_RUNTIME_RESOURCE_PATH_DEPTH
}

fn is_valid_resource_path_component(component: &str) -> bool {
    if component.is_empty()
        || matches!(component, "." | "..")
        || component.len() > MAX_EXTENSION_RUNTIME_RESOURCE_PATH_COMPONENT_BYTES
    {
        return false;
    }
    let bytes = component.as_bytes();
    if bytes.iter().any(|byte| {
        byte.is_ascii_control()
            || matches!(
                *byte,
                b'\\' | b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'%' | b'#'
            )
    }) || bytes
        .last()
        .is_some_and(|byte| matches!(*byte, b' ' | b'.'))
    {
        return false;
    }

    let device_stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem);
    let reserved_numbered = device_stem.len() == 4
        && (device_stem[..3].eq_ignore_ascii_case("COM")
            || device_stem[..3].eq_ignore_ascii_case("LPT"))
        && matches!(device_stem.as_bytes()[3], b'1'..=b'9');
    !device_stem.eq_ignore_ascii_case("CON")
        && !device_stem.eq_ignore_ascii_case("PRN")
        && !device_stem.eq_ignore_ascii_case("AUX")
        && !device_stem.eq_ignore_ascii_case("NUL")
        && !device_stem.eq_ignore_ascii_case("CONIN$")
        && !device_stem.eq_ignore_ascii_case("CONOUT$")
        && !reserved_numbered
}

fn portable_path_shape_conflicts(paths: &BTreeSet<Box<str>>, candidate: &str) -> bool {
    if paths.contains(candidate) {
        return true;
    }
    let mut ancestor = candidate;
    while let Some(separator) = ancestor.rfind('/') {
        ancestor = &ancestor[..separator];
        if paths.contains(ancestor) {
            return true;
        }
    }
    let mut descendant_prefix = String::with_capacity(candidate.len() + 1);
    descendant_prefix.push_str(candidate);
    descendant_prefix.push('/');
    paths
        .range::<str, _>((Included(descendant_prefix.as_str()), Unbounded))
        .next()
        .is_some_and(|path| path.starts_with(descendant_prefix.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LARGE_WASM_RESOURCE_BYTES: u64 = 17 * 1024 * 1024;

    fn binding(path: &str, byte: u8, length: u64) -> ExtensionRuntimeResourceBinding {
        ExtensionRuntimeResourceBinding::try_new(path, length, [byte; 32]).unwrap()
    }

    #[test]
    fn plan_binds_digest_before_ordinal_path_length_and_sha() {
        let plan = ExtensionRuntimeResourcePlan::try_new(vec![
            binding("a.js", 1, 3),
            binding("manifest.json", 2, 7),
        ])
        .unwrap();
        let first = plan.entry("a.js").unwrap();
        assert!(first.resource().authenticates(
            plan.digest(),
            0,
            first.path(),
            first.declared_bytes(),
            first.sha256(),
        ));
        assert!(!first.resource().authenticates(
            plan.digest(),
            1,
            first.path(),
            first.declared_bytes(),
            first.sha256(),
        ));
        assert!(!first.resource().authenticates(
            plan.digest(),
            0,
            "b.js",
            first.declared_bytes(),
            first.sha256(),
        ));
        assert!(!first.resource().authenticates(
            plan.digest(),
            0,
            first.path(),
            first.declared_bytes() + 1,
            first.sha256(),
        ));
        assert!(!first.resource().authenticates(
            plan.digest(),
            0,
            first.path(),
            first.declared_bytes(),
            [9; 32],
        ));
        assert_eq!(
            plan.manifest(),
            plan.entry("manifest.json").unwrap().resource()
        );
    }

    #[test]
    fn plan_admits_large_resources_without_retaining_their_bytes() {
        let plan = ExtensionRuntimeResourcePlan::try_new(vec![
            binding("assets/runtime.wasm", 1, LARGE_WASM_RESOURCE_BYTES),
            binding("manifest.json", 2, 7),
        ])
        .unwrap();

        let resource = plan.entry("assets/runtime.wasm").unwrap();
        assert_eq!(resource.declared_bytes(), LARGE_WASM_RESOURCE_BYTES);
        assert!(plan.retained_bytes() < 1_024);
    }

    #[test]
    fn plan_rejects_order_duplicates_aliases_shapes_and_manifest_errors() {
        for (bindings, expected) in [
            (
                vec![binding("manifest.json", 1, 1), binding("a.js", 2, 1)],
                ExtensionRuntimeResourcePlanBuildError::NonCanonicalOrder,
            ),
            (
                vec![
                    binding("A.js", 1, 1),
                    binding("a.js", 2, 1),
                    binding("manifest.json", 3, 1),
                ],
                ExtensionRuntimeResourcePlanBuildError::PathCollision,
            ),
            (
                vec![
                    binding("a", 1, 1),
                    binding("a/b", 2, 1),
                    binding("manifest.json", 3, 1),
                ],
                ExtensionRuntimeResourcePlanBuildError::PathCollision,
            ),
            (
                vec![binding("a.js", 1, 1)],
                ExtensionRuntimeResourcePlanBuildError::ManifestMissing,
            ),
            (
                vec![binding("manifest.json", 1, 0)],
                ExtensionRuntimeResourcePlanBuildError::ManifestLength,
            ),
        ] {
            assert_eq!(
                ExtensionRuntimeResourcePlan::try_new(bindings).unwrap_err(),
                expected
            );
        }
    }

    #[test]
    fn path_grammar_and_accounting_are_bounded_and_redacted() {
        for invalid in [
            "", "/a", "a/", "a//b", ".", "..", "a/../b", "a\\b", "a%b", "CON", "lpt9.txt", "a.",
            "é",
        ] {
            assert!(matches!(
                ExtensionRuntimeResourceBinding::try_new(invalid, 1, [0; 32]),
                Err(ExtensionRuntimeResourceBuildError::InvalidPath)
            ));
        }
        let plan =
            ExtensionRuntimeResourcePlan::try_new(vec![binding("manifest.json", 1, 1)]).unwrap();
        assert_eq!(
            plan.retained_bytes(),
            size_of::<ExtensionRuntimeResourcePlan>()
                + size_of::<ExtensionRuntimeResourcePlanEntry>()
                + "manifest.json".len()
        );
        let debug = format!("{plan:?}");
        assert!(!debug.contains("manifest.json"));
        assert!(!format!("{:?}", plan.entries()[0]).contains("manifest.json"));
    }

    #[test]
    fn maximum_inventory_is_accepted_and_next_entry_is_rejected_first() {
        let mut bindings = Vec::with_capacity(MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES);
        for ordinal in 0..MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES - 1 {
            bindings.push(binding(&format!("file-{ordinal:04}.bin"), 1, 0));
        }
        bindings.push(binding("manifest.json", 2, 1));
        let plan = ExtensionRuntimeResourcePlan::try_new(bindings).expect("maximum inventory");
        assert_eq!(
            plan.entries().len(),
            MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES
        );
        assert!(plan.retained_bytes() <= MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_RETAINED_BYTES);

        let too_many =
            vec![binding("manifest.json", 3, 1); MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES + 1];
        assert_eq!(
            ExtensionRuntimeResourcePlan::try_new(too_many).unwrap_err(),
            ExtensionRuntimeResourcePlanBuildError::TooManyEntries {
                count: MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES + 1,
                maximum: MAX_EXTENSION_RUNTIME_RESOURCE_PLAN_ENTRIES,
            }
        );
    }
}
