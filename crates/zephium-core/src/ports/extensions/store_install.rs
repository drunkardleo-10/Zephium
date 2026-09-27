//! Bounded native-to-service transport for a user-selected store package.
use super::{ExtensionInstallCandidateEntry, MAX_EXTENSION_ACQUIRED_CRX_BYTES};
use std::mem::size_of;

/// Original downloaded bytes and the requested store ID. This is untrusted
/// transport data; the service must authenticate and assess it before review.
pub struct ExtensionStorePackageRequest {
    id: Box<str>,
    bytes: Box<[u8]>,
    update: Option<super::ExtensionInstallSelector>,
    background: bool,
}
impl ExtensionStorePackageRequest {
    /// Bounds the mailbox payload; performs no package or origin authentication.
    pub fn new(id: String, bytes: Box<[u8]>) -> Option<Self> {
        if id.len() != 32
            || !id.bytes().all(|b| (b'a'..=b'p').contains(&b))
            || bytes.is_empty()
            || bytes.len() > MAX_EXTENSION_ACQUIRED_CRX_BYTES
        {
            return None;
        }
        Some(Self {
            id: id.into_boxed_str(),
            bytes,
            update: None,
            background: false,
        })
    }
    /// Binds an explicit update request to the exact installed revision.
    pub fn for_update(
        id: String,
        bytes: Box<[u8]>,
        selector: super::ExtensionInstallSelector,
    ) -> Option<Self> {
        let mut request = Self::new(id, bytes)?;
        request.update = Some(selector);
        Some(request)
    }
    /// Automatic checks may never replace an outstanding interactive review.
    pub fn for_background_update(
        id: String,
        bytes: Box<[u8]>,
        selector: super::ExtensionInstallSelector,
    ) -> Option<Self> {
        let mut request = Self::for_update(id, bytes, selector)?;
        request.background = true;
        Some(request)
    }
    pub const fn is_background_update(&self) -> bool {
        self.background
    }
    /// An update selector is structural; Store independently rechecks it.
    pub const fn update_selector(&self) -> Option<super::ExtensionInstallSelector> {
        self.update
    }
    /// Exact requested Chromium identifier.
    pub fn extension_id(&self) -> &str {
        &self.id
    }
    /// Original immutable response for independent service authentication.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// Retained mailbox charge, including owned original bytes.
    pub fn retained_bytes(&self) -> usize {
        size_of::<Self>() + self.id.len() + self.bytes.len() + 4 * size_of::<usize>()
    }
}
impl std::fmt::Debug for ExtensionStorePackageRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtensionStorePackageRequest")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}
/// Preparation only. No variant claims installation or native activation.
#[derive(Debug)]
pub enum ExtensionStorePackagePreparationOutcome {
    /// Authenticated package and permissions awaiting an explicit user decision.
    Prepared(Box<ExtensionInstallCandidateEntry>),
    /// This profile already has an install from this publisher's update line.
    AlreadyInstalled,
    /// Latest authenticated source matches the installed upstream checkpoint.
    UpToDate,
    /// Installed packages plus retained recovery data exceed the local storage budget.
    StorageLimit,
    /// A verified replacement needs review before new access can be granted.
    UpdateAvailable,
    /// The replacement was durably committed, with the actual runtime state.
    UpdateSettled(Box<super::ExtensionManagementSettlement<super::ExtensionUpdateOutcome>>),
    /// A required manifest capability is not supported by the selected runtime.
    Unsupported(ExtensionUnsupportedFeatures),
    /// The signature, requested identity or archive contents were rejected.
    InvalidPackage,
    /// Capacity, deadline or service availability prevented preparation.
    Unavailable,
    /// Storage or ownership integrity requires recovery.
    FailedClosed,
}
/// Exactly-once settlement for an admitted native store-preparation request.
pub type ExtensionStorePackagePreparationCallback =
    Box<dyn FnOnce(ExtensionStorePackagePreparationOutcome) + Send>;

/// Bounded compatibility details for a refused package, not an install witness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionUnsupportedFeatures {
    declarations: Vec<crate::extensions::ExtensionManifestDeclaration>,
    more: bool,
}
impl ExtensionUnsupportedFeatures {
    /// Retains at most eight declaration keys, with an explicit truncation flag.
    pub fn new(
        declarations: impl IntoIterator<Item = crate::extensions::ExtensionManifestDeclaration>,
    ) -> Self {
        let mut declarations: Vec<_> = declarations.into_iter().take(9).collect();
        let more = declarations.len() > 8;
        declarations.truncate(8);
        Self { declarations, more }
    }
    /// Exact bounded declarations that the current installer cannot honor.
    pub fn declarations(&self) -> &[crate::extensions::ExtensionManifestDeclaration] {
        &self.declarations
    }
    /// Whether further unsupported declarations were omitted from this summary.
    pub const fn has_more(&self) -> bool {
        self.more
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_report_is_bounded_and_indicates_omissions() {
        let report = ExtensionUnsupportedFeatures::new(std::iter::repeat_n(
            crate::extensions::ExtensionManifestDeclaration::Background,
            100,
        ));
        assert_eq!(report.declarations().len(), 8);
        assert!(report.has_more());
        let empty = ExtensionUnsupportedFeatures::new([]);
        assert!(empty.declarations().is_empty());
        assert!(!empty.has_more());
    }
}
