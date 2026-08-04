//! Bounded, delegated package access.

use std::any::Any;
use std::error::Error;
use std::fmt;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::ExtensionRuntimeTarget;

/// Maximum declared size of one runtime-readable package resource.
pub const MAX_EXTENSION_RUNTIME_RESOURCE_BYTES: u64 = 16 * 1024 * 1024;

/// Maximum declared size of an extension manifest.
pub const MAX_EXTENSION_RUNTIME_MANIFEST_BYTES: u64 = 1024 * 1024;

/// Maximum deterministic control-plane memory attributed to one runtime owner,
/// including package access, lifecycle proxy state, and API wrappers.
///
/// Opaque WebView/platform-process memory is governed by the separate native
/// resource ledger and count reservations; this byte ceiling does not replace
/// those hard bounds.
pub const MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES: usize = 16 * 1024 * 1024;

/// Maximum encoded length of a native package-root path passed to a visitor.
pub const MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES: usize = 4 * 1024;

/// Maximum number of consecutive interrupted-read retries performed by one API
/// read operation.
pub const MAX_EXTENSION_RUNTIME_INTERRUPTED_READ_RETRIES: usize = 8;

/// An opaque, bounded package-resource descriptor.
///
/// This value identifies data; it does not authorize access to that data. The
/// delegated provider remains responsible for authenticating the descriptor
/// against the package represented by the access capability.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ExtensionRuntimeResource {
    identifier: [u8; 32],
    declared_bytes: u64,
}

impl ExtensionRuntimeResource {
    /// Constructs a descriptor after enforcing the public resource-size limit.
    pub fn try_new(
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
        })
    }

    /// Returns the opaque identifier a provider must authenticate.
    #[must_use]
    pub const fn identifier(&self) -> &[u8; 32] {
        &self.identifier
    }

    /// Returns the exact byte length committed by the descriptor.
    #[must_use]
    pub const fn declared_bytes(&self) -> u64 {
        self.declared_bytes
    }
}

impl fmt::Debug for ExtensionRuntimeResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionRuntimeResource")
            .field("identifier", &"[redacted]")
            .field("declared_bytes", &self.declared_bytes)
            .finish()
    }
}

/// Why an extension-resource descriptor could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeResourceBuildError {
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
        match self {
            Self::LengthExceeded { .. } => {
                formatter.write_str("extension resource exceeds the runtime size limit")
            }
        }
    }
}

impl Error for ExtensionRuntimeResourceBuildError {}

/// A closed error returned by a synchronous resource or native-root consumer.
///
/// Errors intentionally carry no provider paths, package identifiers, or
/// unbounded strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionRuntimeVisitorError {
    /// The consumer is temporarily unavailable.
    ConsumerUnavailable,
    /// The consumer's own bounded capacity was exceeded.
    CapacityExceeded,
    /// The consumer could not read the supplied stream.
    ReadFailed,
    /// The supplied data was invalid for the consumer.
    InvalidData,
}

impl fmt::Display for ExtensionRuntimeVisitorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ConsumerUnavailable => "extension runtime consumer is unavailable",
            Self::CapacityExceeded => "extension runtime consumer capacity was exceeded",
            Self::ReadFailed => "extension runtime consumer could not read the resource",
            Self::InvalidData => "extension runtime consumer rejected invalid data",
        })
    }
}

impl Error for ExtensionRuntimeVisitorError {}

/// A synchronous visitor for one bounded package resource.
///
/// The reader is valid only for the duration of [`Self::visit`]. The access API
/// caps visible data at the descriptor's declared length and validates exact
/// exhaustion before the provider regains control. Bytes observed here remain
/// provisional until the enclosing access call returns an outer `Ok`: visitors
/// must stage parsing/output and must not publish, inject, or execute it inside
/// the callback because provider postvalidation can still fail afterward.
pub trait ExtensionRuntimeResourceVisitor {
    /// Consumes the resource synchronously.
    fn visit(&mut self, reader: &mut dyn Read) -> Result<(), ExtensionRuntimeVisitorError>;
}

impl<F> ExtensionRuntimeResourceVisitor for F
where
    F: for<'reader> FnMut(&'reader mut dyn Read) -> Result<(), ExtensionRuntimeVisitorError>,
{
    fn visit(&mut self, reader: &mut dyn Read) -> Result<(), ExtensionRuntimeVisitorError> {
        self(reader)
    }
}

/// A synchronous visitor for a provider-validated native package root.
///
/// The borrowed path must not be retained beyond [`Self::visit`]. The API does
/// not expose a return value capable of carrying the borrow out of the callback.
/// Trusted native adapters must also avoid copying the path into longer-lived
/// storage. Native effects initiated inside the callback remain provisional: if
/// provider postvalidation later fails, lifecycle code must classify ownership
/// from observed native state (normally uncertain), never infer absence from the
/// access error alone.
pub trait ExtensionRuntimeNativeRootVisitor {
    /// Uses the native root synchronously.
    fn visit(&mut self, root: &Path) -> Result<(), ExtensionRuntimeVisitorError>;
}

impl<F> ExtensionRuntimeNativeRootVisitor for F
where
    F: for<'root> FnMut(&'root Path) -> Result<(), ExtensionRuntimeVisitorError>,
{
    fn visit(&mut self, root: &Path) -> Result<(), ExtensionRuntimeVisitorError> {
        self(root)
    }
}

/// A closed failure from a delegated package-access provider or API contract
/// validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionPackageAccessError {
    /// The delegated access is no longer active.
    Inactive,
    /// The requested descriptor is not declared by this package.
    ResourceNotDeclared,
    /// The declared resource is temporarily unavailable.
    ResourceUnavailable,
    /// Reading the resource failed.
    ResourceReadFailed,
    /// The provided bytes did not match the descriptor's committed identity or
    /// exact length.
    ResourceIdentityMismatch,
    /// A native package root is unavailable for this runtime target.
    NativeRootUnavailable,
    /// The native root failed provider or API identity validation.
    NativeRootIdentityMismatch,
    /// The delegated provider violated the synchronous callback contract.
    ProviderContractViolation,
}

impl fmt::Display for ExtensionPackageAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Inactive => "delegated extension package access is inactive",
            Self::ResourceNotDeclared => "extension resource is not declared by the package",
            Self::ResourceUnavailable => "extension resource is unavailable",
            Self::ResourceReadFailed => "extension resource could not be read",
            Self::ResourceIdentityMismatch => "extension resource identity validation failed",
            Self::NativeRootUnavailable => "native extension package root is unavailable",
            Self::NativeRootIdentityMismatch => {
                "native extension package root identity validation failed"
            }
            Self::ProviderContractViolation => {
                "delegated extension package provider violated its callback contract"
            }
        })
    }
}

impl Error for ExtensionPackageAccessError {}

/// The service-owned implementation behind delegated package access.
///
/// Implementations are trusted native adapters. They must authenticate every
/// resource descriptor and native root against the package represented by the
/// provider. This trait delegates bounded I/O; implementing it does not grant
/// authority and the runtime API never performs package authorization.
///
/// A successful provider call must invoke its visitor exactly once,
/// synchronously. A pre-delivery provider error may invoke it zero times; once
/// delivery begins it may be invoked only once. After the callback returns,
/// including when it returns an error, the provider must complete its own
/// identity and lifecycle postvalidation before returning. Provider or
/// postvalidation errors are returned as the outer error and take precedence
/// over visitor/API errors, while independently observed contract or identity
/// violations still deactivate the access capability. A provider must otherwise
/// return `Ok(())`; the API captures and returns the visitor result separately.
/// Resource consumers must therefore stage side effects until the full outer
/// call succeeds. When a native-root visit necessarily starts native work, a
/// later outer failure requires ownership reconciliation rather than a
/// definitely-absent lifecycle result.
///
/// Provider destruction must be passive: `Drop` must not revoke durable access,
/// unpin package data, mutate native ownership, or claim that ownership has
/// settled. Those transitions belong to the package service and explicit
/// runtime lifecycle protocol.
pub trait ExtensionPackageAccessPort: Any + Send {
    /// Reports deterministic host memory retained exclusively by this provider.
    ///
    /// Implementations must include the concrete provider allocation and every
    /// exclusive deterministic heap/host allocation in a stable upper bound, but must not
    /// include the Rust access wrapper itself. The query must be side-effect-free
    /// so a construction refusal returns unchanged provider state.
    fn retained_bytes(&self) -> usize;

    /// Visits an authenticated package resource.
    fn visit_resource(
        &mut self,
        resource: ExtensionRuntimeResource,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<(), ExtensionPackageAccessError>;

    /// Visits an authenticated native package root for `target`.
    fn visit_native_root(
        &mut self,
        target: ExtensionRuntimeTarget,
        visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
    ) -> Result<(), ExtensionPackageAccessError>;
}

/// Why a delegated provider could not be accepted by the runtime API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExtensionPackageAccessBuildError {
    /// The manifest descriptor exceeds the stricter manifest limit.
    ManifestTooLarge,
    /// Accounting for the provider and wrapper overflowed `usize`.
    RetainedBytesOverflow,
    /// The provider and access wrapper already exceed the per-owner
    /// retained-memory ceiling.
    RetainedBytesExceeded,
}

impl fmt::Display for ExtensionPackageAccessBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ManifestTooLarge => "extension manifest exceeds the runtime size limit",
            Self::RetainedBytesOverflow => "extension package access accounting overflowed",
            Self::RetainedBytesExceeded => {
                "extension package access exceeds the retained-memory limit"
            }
        })
    }
}

impl Error for ExtensionPackageAccessBuildError {}

/// A failed delegated-provider construction that preserves provider ownership.
#[must_use = "the refusal retains the exact delegated provider"]
pub struct ExtensionPackageAccessBuildRefusal {
    reason: ExtensionPackageAccessBuildError,
    provider: Box<dyn ExtensionPackageAccessPort>,
}

impl ExtensionPackageAccessBuildRefusal {
    /// Returns the stable refusal reason.
    #[must_use]
    pub const fn reason(&self) -> ExtensionPackageAccessBuildError {
        self.reason
    }

    /// Returns the same provider supplied to the failed constructor.
    #[must_use]
    pub fn into_provider(self) -> Box<dyn ExtensionPackageAccessPort> {
        self.provider
    }
}

impl fmt::Debug for ExtensionPackageAccessBuildRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackageAccessBuildRefusal")
            .field("reason", &self.reason)
            .field("provider", &"[redacted]")
            .finish()
    }
}

/// Move-only, non-authorizing delegated access to one extension package.
///
/// The package service creates this capability only after its own authorization
/// and durable pinning checks. [`Self::from_delegated_provider`] intentionally
/// performs no authorization: it validates only runtime resource limits and
/// returns the provider unchanged on refusal.
///
/// The capability is `Send` but deliberately neither `Clone` nor `Sync`; all
/// provider I/O requires exclusive access. It has no serialization contract and
/// its debug representation redacts provider identity.
/// Dropping it invokes no API-level release or unpin operation; delegated
/// providers are required to have passive destructors.
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionPackageAccess;
/// fn requires_clone<T: Clone>() {}
/// requires_clone::<ExtensionPackageAccess>();
/// ```
///
/// ```compile_fail
/// use zephium_extension_runtime_api::ExtensionPackageAccess;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ExtensionPackageAccess>();
/// ```
///
/// ```compile_fail
/// use serde::Serialize;
/// use zephium_extension_runtime_api::ExtensionPackageAccess;
/// fn requires_serialize<T: Serialize>() {}
/// requires_serialize::<ExtensionPackageAccess>();
/// ```
///
/// ```compile_fail
/// use serde::de::DeserializeOwned;
/// use zephium_extension_runtime_api::ExtensionPackageAccess;
/// fn requires_deserialize<T: DeserializeOwned>() {}
/// requires_deserialize::<ExtensionPackageAccess>();
/// ```
///
/// A root borrow cannot be returned through the visitor result:
///
/// ```compile_fail
/// use std::path::Path;
/// use zephium_extension_runtime_api::{
///     ExtensionPackageAccessView, ExtensionRuntimeVisitorError,
/// };
/// fn escape<'a>(view: &'a mut ExtensionPackageAccessView<'_>) -> &'a Path {
///     let mut escaped = None;
///     let mut visitor = |root: &Path| {
///         escaped = Some(root);
///         Ok::<(), ExtensionRuntimeVisitorError>(())
///     };
///     let _ = view.visit_native_root(&mut visitor);
///     escaped.expect("visitor must run")
/// }
/// ```
#[must_use = "delegated package access must be settled through the runtime ownership protocol"]
pub struct ExtensionPackageAccess {
    target: ExtensionRuntimeTarget,
    manifest: ExtensionRuntimeResource,
    retained_bytes: usize,
    usable: bool,
    provider: Box<dyn ExtensionPackageAccessPort>,
}

impl ExtensionPackageAccess {
    /// Accepts an already-authorized delegated I/O provider.
    ///
    /// This constructor is explicitly non-authorizing. The caller and provider
    /// are responsible for package authentication, durable pinning, and access
    /// revocation. Any validation refusal returns the original provider in
    /// [`ExtensionPackageAccessBuildRefusal`].
    pub fn from_delegated_provider(
        target: ExtensionRuntimeTarget,
        manifest: ExtensionRuntimeResource,
        provider: Box<dyn ExtensionPackageAccessPort>,
    ) -> Result<Self, ExtensionPackageAccessBuildRefusal> {
        if manifest.declared_bytes > MAX_EXTENSION_RUNTIME_MANIFEST_BYTES {
            return Err(ExtensionPackageAccessBuildRefusal {
                reason: ExtensionPackageAccessBuildError::ManifestTooLarge,
                provider,
            });
        }

        let retained_bytes =
            match std::mem::size_of::<Self>().checked_add(provider.retained_bytes()) {
                Some(retained_bytes) => retained_bytes,
                None => {
                    return Err(ExtensionPackageAccessBuildRefusal {
                        reason: ExtensionPackageAccessBuildError::RetainedBytesOverflow,
                        provider,
                    });
                }
            };
        if retained_bytes > MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES {
            return Err(ExtensionPackageAccessBuildRefusal {
                reason: ExtensionPackageAccessBuildError::RetainedBytesExceeded,
                provider,
            });
        }

        Ok(Self {
            target,
            manifest,
            retained_bytes,
            usable: true,
            provider,
        })
    }

    /// Returns the selected runtime family.
    #[must_use]
    pub const fn target(&self) -> ExtensionRuntimeTarget {
        self.target
    }

    /// Returns the authenticated manifest descriptor.
    #[must_use]
    pub const fn manifest(&self) -> ExtensionRuntimeResource {
        self.manifest
    }

    /// Returns bounded retained bytes attributed to this access object.
    #[must_use]
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Recovers the exact delegated provider after native absence is known.
    ///
    /// This method is deliberately unavailable from runtime-owner and
    /// uncertain-owner values. The lifecycle protocol returns package access
    /// only before activation or after it has proved native ownership absent.
    /// The exact same provider allocation is returned without invoking provider
    /// code. The service may trait-upcast it to `Any` and downcast to its private
    /// lease-backed type; a failed downcast must retain the returned box
    /// fail-safe rather than treating the mismatch as settlement.
    #[must_use = "the delegated provider retains package authority"]
    pub fn into_provider(self) -> Box<dyn ExtensionPackageAccessPort> {
        self.provider
    }

    /// Visits the package manifest through the same authenticated resource path
    /// used for every other package file.
    pub fn visit_manifest(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        self.visit_resource(self.manifest, visitor)
    }

    /// Visits one authenticated, exactly-sized package resource.
    ///
    /// Provider and provider-postvalidation failures are returned as the outer
    /// `Err` and take precedence over consumer failures. The inner result is the
    /// synchronous visitor result.
    pub fn visit_resource(
        &mut self,
        resource: ExtensionRuntimeResource,
        visitor: &mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        if !self.usable {
            return Err(ExtensionPackageAccessError::Inactive);
        }
        let mut guard = ResourceVisitGuard::new(resource.declared_bytes, visitor);
        let provider_result = self.provider.visit_resource(resource, &mut guard);
        let observed_terminal_error = guard.observed_terminal_error();
        let result = match provider_result {
            Ok(()) => guard.finish(),
            Err(error) => Err(error),
        };
        self.finish_operation(result, observed_terminal_error)
    }

    /// Visits a provider-validated native package root synchronously.
    ///
    /// The root must be absolute, non-root, and lexically normalized. Provider
    /// and provider-postvalidation failures take precedence over consumer
    /// failures.
    pub(crate) fn visit_native_root(
        &mut self,
        visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        if !self.usable {
            return Err(ExtensionPackageAccessError::Inactive);
        }
        let mut guard = NativeRootVisitGuard::new(visitor);
        let provider_result = self.provider.visit_native_root(self.target, &mut guard);
        let observed_terminal_error = guard.observed_terminal_error();
        let result = match provider_result {
            Ok(()) => guard.finish(),
            Err(error) => Err(error),
        };
        self.finish_operation(result, observed_terminal_error)
    }

    fn finish_operation(
        &mut self,
        result: Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError>,
        observed_terminal_error: Option<ExtensionPackageAccessError>,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        if observed_terminal_error.is_some()
            || result
                .as_ref()
                .err()
                .is_some_and(|error| access_error_is_terminal(*error))
        {
            self.usable = false;
        }
        result
    }
}

const fn access_error_is_terminal(error: ExtensionPackageAccessError) -> bool {
    match error {
        ExtensionPackageAccessError::Inactive
        | ExtensionPackageAccessError::ResourceIdentityMismatch
        | ExtensionPackageAccessError::NativeRootIdentityMismatch
        | ExtensionPackageAccessError::ProviderContractViolation => true,
        ExtensionPackageAccessError::ResourceNotDeclared
        | ExtensionPackageAccessError::ResourceUnavailable
        | ExtensionPackageAccessError::ResourceReadFailed
        | ExtensionPackageAccessError::NativeRootUnavailable => false,
    }
}

impl fmt::Debug for ExtensionPackageAccess {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtensionPackageAccess")
            .field("target", &self.target)
            .field("manifest", &self.manifest)
            .field("retained_bytes", &self.retained_bytes)
            .field("provider", &"[redacted]")
            .finish()
    }
}

struct ResourceVisitGuard<'visitor> {
    visitor: &'visitor mut dyn ExtensionRuntimeResourceVisitor,
    calls: usize,
    visitor_result: Option<Result<(), ExtensionRuntimeVisitorError>>,
    validation_error: Option<ExtensionPackageAccessError>,
    declared_bytes: u64,
}

impl<'visitor> ResourceVisitGuard<'visitor> {
    fn new(
        declared_bytes: u64,
        visitor: &'visitor mut dyn ExtensionRuntimeResourceVisitor,
    ) -> Self {
        Self {
            visitor,
            calls: 0,
            visitor_result: None,
            validation_error: None,
            declared_bytes,
        }
    }

    fn finish(
        self,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        if self.calls != 1 {
            return Err(ExtensionPackageAccessError::ProviderContractViolation);
        }
        if let Some(error) = self.validation_error {
            return Err(error);
        }
        self.visitor_result
            .ok_or(ExtensionPackageAccessError::ProviderContractViolation)
    }

    fn observed_terminal_error(&self) -> Option<ExtensionPackageAccessError> {
        if self.calls > 1 {
            Some(ExtensionPackageAccessError::ProviderContractViolation)
        } else {
            self.validation_error
                .filter(|error| access_error_is_terminal(*error))
        }
    }
}

impl ExtensionRuntimeResourceVisitor for ResourceVisitGuard<'_> {
    fn visit(&mut self, reader: &mut dyn Read) -> Result<(), ExtensionRuntimeVisitorError> {
        self.calls = self.calls.saturating_add(1);
        if self.calls != 1 {
            return Err(ExtensionRuntimeVisitorError::InvalidData);
        }

        let mut bounded_reader = ExactLengthReader::new(reader, self.declared_bytes);
        let visitor_result = self.visitor.visit(&mut bounded_reader);
        self.validation_error = bounded_reader.finish().err();
        self.visitor_result = Some(visitor_result);
        visitor_result
    }
}

struct ExactLengthReader<'reader> {
    reader: &'reader mut dyn Read,
    remaining: u64,
    observed_read_failure: bool,
    observed_contract_violation: bool,
    observed_premature_eof: bool,
}

impl<'reader> ExactLengthReader<'reader> {
    fn new(reader: &'reader mut dyn Read, declared_bytes: u64) -> Self {
        Self {
            reader,
            remaining: declared_bytes,
            observed_read_failure: false,
            observed_contract_violation: false,
            observed_premature_eof: false,
        }
    }

    fn finish(&mut self) -> Result<(), ExtensionPackageAccessError> {
        if self.observed_contract_violation {
            return Err(ExtensionPackageAccessError::ProviderContractViolation);
        }

        let mut buffer = [0_u8; 8 * 1024];
        while self.remaining != 0 {
            let requested = bounded_read_length(self.remaining, buffer.len());
            match read_with_bounded_interrupts(self.reader, &mut buffer[..requested]) {
                Ok(0) => return Err(ExtensionPackageAccessError::ResourceIdentityMismatch),
                Ok(read) if read <= requested => self.remaining -= read as u64,
                Ok(_) => return Err(ExtensionPackageAccessError::ProviderContractViolation),
                Err(_) => return Err(ExtensionPackageAccessError::ResourceReadFailed),
            }
        }

        let has_trailing_data = match read_with_bounded_interrupts(self.reader, &mut buffer[..1]) {
            Ok(0) => false,
            Ok(1) => true,
            Ok(_) => return Err(ExtensionPackageAccessError::ProviderContractViolation),
            Err(_) => return Err(ExtensionPackageAccessError::ResourceReadFailed),
        };

        if self.observed_read_failure {
            return Err(ExtensionPackageAccessError::ResourceReadFailed);
        }
        if self.observed_premature_eof || has_trailing_data {
            return Err(ExtensionPackageAccessError::ResourceIdentityMismatch);
        }
        Ok(())
    }
}

impl Read for ExactLengthReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.remaining == 0 {
            return Ok(0);
        }

        let maximum = bounded_read_length(self.remaining, buffer.len());
        match read_with_bounded_interrupts(self.reader, &mut buffer[..maximum]) {
            Ok(0) => {
                self.observed_premature_eof = true;
                Ok(0)
            }
            Ok(read) if read <= maximum => {
                self.remaining -= read as u64;
                Ok(read)
            }
            Ok(_) => {
                self.observed_contract_violation = true;
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "extension provider over-reported a read length",
                ))
            }
            Err(error) => {
                self.observed_read_failure = true;
                Err(error)
            }
        }
    }
}

struct NativeRootVisitGuard<'visitor> {
    visitor: &'visitor mut dyn ExtensionRuntimeNativeRootVisitor,
    calls: usize,
    visitor_result: Option<Result<(), ExtensionRuntimeVisitorError>>,
    validation_error: Option<ExtensionPackageAccessError>,
}

impl<'visitor> NativeRootVisitGuard<'visitor> {
    fn new(visitor: &'visitor mut dyn ExtensionRuntimeNativeRootVisitor) -> Self {
        Self {
            visitor,
            calls: 0,
            visitor_result: None,
            validation_error: None,
        }
    }

    fn finish(
        self,
    ) -> Result<Result<(), ExtensionRuntimeVisitorError>, ExtensionPackageAccessError> {
        if self.calls != 1 {
            return Err(ExtensionPackageAccessError::ProviderContractViolation);
        }
        if let Some(error) = self.validation_error {
            return Err(error);
        }
        self.visitor_result
            .ok_or(ExtensionPackageAccessError::ProviderContractViolation)
    }

    fn observed_terminal_error(&self) -> Option<ExtensionPackageAccessError> {
        if self.calls > 1 {
            Some(ExtensionPackageAccessError::ProviderContractViolation)
        } else {
            self.validation_error
                .filter(|error| access_error_is_terminal(*error))
        }
    }
}

impl ExtensionRuntimeNativeRootVisitor for NativeRootVisitGuard<'_> {
    fn visit(&mut self, root: &Path) -> Result<(), ExtensionRuntimeVisitorError> {
        self.calls = self.calls.saturating_add(1);
        if self.calls != 1 {
            return Err(ExtensionRuntimeVisitorError::InvalidData);
        }
        if !is_valid_native_root(root) {
            self.validation_error = Some(ExtensionPackageAccessError::NativeRootIdentityMismatch);
            return Err(ExtensionRuntimeVisitorError::InvalidData);
        }

        let visitor_result = self.visitor.visit(root);
        self.visitor_result = Some(visitor_result);
        visitor_result
    }
}

fn is_valid_native_root(root: &Path) -> bool {
    if !root.is_absolute() || root.as_os_str().len() > MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES {
        return false;
    }

    let mut has_normal_component = false;
    for component in root.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(_) => has_normal_component = true,
            Component::CurDir | Component::ParentDir => return false,
        }
    }

    let normalized = root.components().collect::<PathBuf>();
    has_normal_component && normalized.as_os_str() == root.as_os_str()
}

fn bounded_read_length(remaining: u64, buffer_length: usize) -> usize {
    usize::try_from(remaining).map_or(buffer_length, |remaining| remaining.min(buffer_length))
}

fn read_with_bounded_interrupts(reader: &mut dyn Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut interrupted_retries = 0;
    loop {
        match reader.read(buffer) {
            Err(error)
                if error.kind() == io::ErrorKind::Interrupted
                    && interrupted_retries < MAX_EXTENSION_RUNTIME_INTERRUPTED_READ_RETRIES =>
            {
                interrupted_retries += 1;
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{self, Cursor, Read};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Copy)]
    enum ResourceBehavior {
        Exact,
        NeverCalls,
        CallsTwice,
        ProviderFailure,
        ProviderUnavailableBeforeDelivery,
        ProviderFailureWithTrailingByte,
        ProviderUnavailableWithTrailingByte,
        ProviderFailureWithOverReportedRead,
        TrailingByte,
        ShortByte,
        ReadFailure,
        OverReportedRead,
        OverReportedEof,
        InterruptedForever,
    }

    struct TestProvider {
        identity: u64,
        retained_bytes: usize,
        bytes: Vec<u8>,
        behavior: ResourceBehavior,
        root: PathBuf,
        root_calls: usize,
        root_provider_error: Option<ExtensionPackageAccessError>,
        recovered_identity: Arc<Mutex<Option<u64>>>,
        read_calls: Arc<AtomicUsize>,
    }

    impl Drop for TestProvider {
        fn drop(&mut self) {
            *self.recovered_identity.lock().expect("lock") = Some(self.identity);
        }
    }

    impl ExtensionPackageAccessPort for TestProvider {
        fn retained_bytes(&self) -> usize {
            self.retained_bytes
        }

        fn visit_resource(
            &mut self,
            _resource: ExtensionRuntimeResource,
            visitor: &mut dyn ExtensionRuntimeResourceVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            match self.behavior {
                ResourceBehavior::Exact => {
                    let _ = visitor.visit(&mut Cursor::new(self.bytes.as_slice()));
                    Ok(())
                }
                ResourceBehavior::NeverCalls => Ok(()),
                ResourceBehavior::CallsTwice => {
                    let _ = visitor.visit(&mut Cursor::new(self.bytes.as_slice()));
                    let _ = visitor.visit(&mut Cursor::new(self.bytes.as_slice()));
                    Ok(())
                }
                ResourceBehavior::ProviderFailure => {
                    let _ = visitor.visit(&mut Cursor::new(self.bytes.as_slice()));
                    Err(ExtensionPackageAccessError::Inactive)
                }
                ResourceBehavior::ProviderUnavailableBeforeDelivery => {
                    Err(ExtensionPackageAccessError::ResourceUnavailable)
                }
                ResourceBehavior::ProviderFailureWithTrailingByte => {
                    let mut bytes = self.bytes.clone();
                    bytes.push(0xff);
                    let _ = visitor.visit(&mut Cursor::new(bytes));
                    Err(ExtensionPackageAccessError::Inactive)
                }
                ResourceBehavior::ProviderUnavailableWithTrailingByte => {
                    let mut bytes = self.bytes.clone();
                    bytes.push(0xff);
                    let _ = visitor.visit(&mut Cursor::new(bytes));
                    Err(ExtensionPackageAccessError::ResourceUnavailable)
                }
                ResourceBehavior::ProviderFailureWithOverReportedRead => {
                    let _ = visitor.visit(&mut OverReportingReader {
                        calls: Arc::clone(&self.read_calls),
                    });
                    Err(ExtensionPackageAccessError::Inactive)
                }
                ResourceBehavior::TrailingByte => {
                    let mut bytes = self.bytes.clone();
                    bytes.push(0xff);
                    let _ = visitor.visit(&mut Cursor::new(bytes));
                    Ok(())
                }
                ResourceBehavior::ShortByte => {
                    let length = self.bytes.len().saturating_sub(1);
                    let _ = visitor.visit(&mut Cursor::new(&self.bytes[..length]));
                    Ok(())
                }
                ResourceBehavior::ReadFailure => {
                    let mut reader = FailingReader::new(self.bytes.clone(), 1);
                    let _ = visitor.visit(&mut reader);
                    Ok(())
                }
                ResourceBehavior::OverReportedRead => {
                    let _ = visitor.visit(&mut OverReportingReader {
                        calls: Arc::clone(&self.read_calls),
                    });
                    Ok(())
                }
                ResourceBehavior::OverReportedEof => {
                    let mut reader = OverReportingEofReader {
                        bytes: Cursor::new(self.bytes.as_slice()),
                        calls: Arc::clone(&self.read_calls),
                    };
                    let _ = visitor.visit(&mut reader);
                    Ok(())
                }
                ResourceBehavior::InterruptedForever => {
                    let _ = visitor.visit(&mut AlwaysInterruptedReader {
                        calls: Arc::clone(&self.read_calls),
                    });
                    Ok(())
                }
            }
        }

        fn visit_native_root(
            &mut self,
            _target: ExtensionRuntimeTarget,
            visitor: &mut dyn ExtensionRuntimeNativeRootVisitor,
        ) -> Result<(), ExtensionPackageAccessError> {
            for _ in 0..self.root_calls {
                let _ = visitor.visit(&self.root);
            }
            self.root_provider_error.map_or(Ok(()), Err)
        }
    }

    struct FailingReader {
        bytes: VecDeque<u8>,
        reads_before_failure: usize,
        failed: bool,
    }

    impl FailingReader {
        fn new(bytes: Vec<u8>, reads_before_failure: usize) -> Self {
            Self {
                bytes: bytes.into(),
                reads_before_failure,
                failed: false,
            }
        }
    }

    impl Read for FailingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if !self.failed && self.reads_before_failure == 0 {
                self.failed = true;
                return Err(io::Error::other("injected read failure"));
            }
            self.reads_before_failure = self.reads_before_failure.saturating_sub(1);
            let count = buffer.len().min(self.bytes.len()).min(1);
            for output in buffer.iter_mut().take(count) {
                *output = self.bytes.pop_front().expect("length checked");
            }
            Ok(count)
        }
    }

    struct OverReportingReader {
        calls: Arc<AtomicUsize>,
    }

    impl Read for OverReportingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(buffer.len().saturating_add(1))
        }
    }

    struct OverReportingEofReader<'bytes> {
        bytes: Cursor<&'bytes [u8]>,
        calls: Arc<AtomicUsize>,
    }

    impl Read for OverReportingEofReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let read = self.bytes.read(buffer)?;
            if read == 0 {
                Ok(buffer.len().saturating_add(1))
            } else {
                Ok(read)
            }
        }
    }

    struct AlwaysInterruptedReader {
        calls: Arc<AtomicUsize>,
    }

    impl Read for AlwaysInterruptedReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "injected interruption",
            ))
        }
    }

    fn descriptor(length: usize) -> ExtensionRuntimeResource {
        ExtensionRuntimeResource::try_new([0x5a; 32], length as u64).expect("descriptor")
    }

    fn provider(
        behavior: ResourceBehavior,
        bytes: Vec<u8>,
        root: PathBuf,
    ) -> (Box<dyn ExtensionPackageAccessPort>, Arc<Mutex<Option<u64>>>) {
        let recovered_identity = Arc::new(Mutex::new(None));
        (
            Box::new(TestProvider {
                identity: 91,
                retained_bytes: bytes.len(),
                bytes,
                behavior,
                root,
                root_calls: 1,
                root_provider_error: None,
                recovered_identity: Arc::clone(&recovered_identity),
                read_calls: Arc::new(AtomicUsize::new(0)),
            }),
            recovered_identity,
        )
    }

    fn access_with(behavior: ResourceBehavior, bytes: Vec<u8>) -> ExtensionPackageAccess {
        let manifest = descriptor(bytes.len());
        let (provider, _) = provider(behavior, bytes, absolute_test_root());
        ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            manifest,
            provider,
        )
        .expect("access")
    }

    fn access_with_observed_reads(
        behavior: ResourceBehavior,
        bytes: Vec<u8>,
    ) -> (ExtensionPackageAccess, Arc<AtomicUsize>) {
        let read_calls = Arc::new(AtomicUsize::new(0));
        let provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 96,
            retained_bytes: bytes.len(),
            bytes: bytes.clone(),
            behavior,
            root: absolute_test_root(),
            root_calls: 1,
            root_provider_error: None,
            recovered_identity: Arc::new(Mutex::new(None)),
            read_calls: Arc::clone(&read_calls),
        });
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            descriptor(bytes.len()),
            provider,
        )
        .expect("access");
        (access, read_calls)
    }

    fn access_with_root_calls(root_calls: usize) -> ExtensionPackageAccess {
        let provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 97,
            retained_bytes: 0,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: absolute_test_root(),
            root_calls,
            root_provider_error: None,
            recovered_identity: Arc::new(Mutex::new(None)),
            read_calls: Arc::new(AtomicUsize::new(0)),
        });
        ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            descriptor(0),
            provider,
        )
        .expect("access")
    }

    #[cfg(unix)]
    fn absolute_test_root() -> PathBuf {
        PathBuf::from("/private/var/zephium/extensions/package")
    }

    #[cfg(windows)]
    fn absolute_test_root() -> PathBuf {
        PathBuf::from(r"C:\Zephium\extensions\package")
    }

    #[test]
    fn resource_limit_accepts_maximum_and_rejects_next_byte() {
        assert!(
            ExtensionRuntimeResource::try_new([0; 32], MAX_EXTENSION_RUNTIME_RESOURCE_BYTES)
                .is_ok()
        );
        assert_eq!(
            ExtensionRuntimeResource::try_new([0; 32], MAX_EXTENSION_RUNTIME_RESOURCE_BYTES + 1),
            Err(ExtensionRuntimeResourceBuildError::LengthExceeded {
                declared_bytes: MAX_EXTENSION_RUNTIME_RESOURCE_BYTES + 1,
                maximum_bytes: MAX_EXTENSION_RUNTIME_RESOURCE_BYTES,
            })
        );
    }

    #[test]
    fn construction_refusal_returns_the_same_provider() {
        let manifest =
            ExtensionRuntimeResource::try_new([1; 32], MAX_EXTENSION_RUNTIME_MANIFEST_BYTES + 1)
                .expect("within general resource limit");
        let (provider, recovered_identity) =
            provider(ResourceBehavior::Exact, Vec::new(), absolute_test_root());

        let refusal = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            manifest,
            provider,
        )
        .expect_err("manifest must be refused");

        assert_eq!(
            refusal.reason(),
            ExtensionPackageAccessBuildError::ManifestTooLarge
        );
        assert_eq!(*recovered_identity.lock().expect("lock"), None);
        drop(refusal.into_provider());
        assert_eq!(*recovered_identity.lock().expect("lock"), Some(91));
    }

    #[test]
    fn retained_memory_is_bounded_and_provider_is_returned() {
        let manifest = descriptor(0);
        let recovered_identity = Arc::new(Mutex::new(None));
        let provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 92,
            retained_bytes: MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: absolute_test_root(),
            root_calls: 1,
            root_provider_error: None,
            recovered_identity: Arc::clone(&recovered_identity),
            read_calls: Arc::new(AtomicUsize::new(0)),
        });

        let refusal = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            manifest,
            provider,
        )
        .expect_err("wrapper overhead must make this too large");
        assert_eq!(
            refusal.reason(),
            ExtensionPackageAccessBuildError::RetainedBytesExceeded
        );
        drop(refusal.into_provider());
        assert_eq!(*recovered_identity.lock().expect("lock"), Some(92));
    }

    #[test]
    fn retained_memory_accepts_the_exact_limit_and_refuses_overflow() {
        let wrapper_bytes = std::mem::size_of::<ExtensionPackageAccess>();
        let recovered_identity = Arc::new(Mutex::new(None));
        let accepted_provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 94,
            retained_bytes: MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES - wrapper_bytes,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: absolute_test_root(),
            root_calls: 1,
            root_provider_error: None,
            recovered_identity: Arc::clone(&recovered_identity),
            read_calls: Arc::new(AtomicUsize::new(0)),
        });
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            descriptor(0),
            accepted_provider,
        )
        .expect("exact retained limit must be accepted");
        assert_eq!(
            access.retained_bytes(),
            MAX_EXTENSION_RUNTIME_OWNER_RETAINED_BYTES
        );
        drop(access);
        assert_eq!(*recovered_identity.lock().expect("lock"), Some(94));

        let recovered_identity = Arc::new(Mutex::new(None));
        let overflowing_provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 95,
            retained_bytes: usize::MAX,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: absolute_test_root(),
            root_calls: 1,
            root_provider_error: None,
            recovered_identity: Arc::clone(&recovered_identity),
            read_calls: Arc::new(AtomicUsize::new(0)),
        });
        let refusal = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            descriptor(0),
            overflowing_provider,
        )
        .expect_err("overflow must be refused");
        assert_eq!(
            refusal.reason(),
            ExtensionPackageAccessBuildError::RetainedBytesOverflow
        );
        drop(refusal.into_provider());
        assert_eq!(*recovered_identity.lock().expect("lock"), Some(95));
    }

    #[test]
    fn successful_access_recovers_the_exact_provider_payload() {
        let manifest = descriptor(0);
        let (provider, recovered_identity) =
            provider(ResourceBehavior::Exact, Vec::new(), absolute_test_root());
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            manifest,
            provider,
        )
        .expect("access");

        let provider = access.into_provider();
        let payload: Box<dyn Any + Send> = provider;
        let provider = match payload.downcast::<TestProvider>() {
            Ok(provider) => provider,
            Err(payload) => {
                drop(payload);
                panic!("recovery payload type changed")
            }
        };
        assert_eq!(provider.identity, 91);
        assert_eq!(*recovered_identity.lock().expect("lock"), None);
        drop(provider);
        assert_eq!(*recovered_identity.lock().expect("lock"), Some(91));
    }

    #[test]
    fn exact_empty_and_partial_resource_consumption_succeed() {
        for bytes in [Vec::new(), b"manifest".to_vec()] {
            let mut access = access_with(ResourceBehavior::Exact, bytes.clone());
            let mut observed = Vec::new();
            let result = access.visit_manifest(&mut |reader: &mut dyn Read| {
                let mut prefix = [0_u8; 2];
                let count = reader
                    .read(&mut prefix)
                    .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
                observed.extend_from_slice(&prefix[..count]);
                Ok(())
            });
            assert_eq!(result, Ok(Ok(())));
            assert_eq!(observed, bytes[..observed.len()]);
        }
    }

    #[test]
    fn reader_never_exposes_trailing_data() {
        let mut access = access_with(ResourceBehavior::TrailingByte, b"abc".to_vec());
        let mut observed = Vec::new();
        let result = access.visit_manifest(&mut |reader: &mut dyn Read| {
            reader
                .read_to_end(&mut observed)
                .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
            Ok(())
        });
        assert_eq!(observed, b"abc");
        assert_eq!(
            result,
            Err(ExtensionPackageAccessError::ResourceIdentityMismatch)
        );
    }

    #[test]
    fn short_and_read_failure_are_outer_errors() {
        for (behavior, expected) in [
            (
                ResourceBehavior::ShortByte,
                ExtensionPackageAccessError::ResourceIdentityMismatch,
            ),
            (
                ResourceBehavior::ReadFailure,
                ExtensionPackageAccessError::ResourceReadFailed,
            ),
        ] {
            let mut access = access_with(behavior, b"abc".to_vec());
            let result = access.visit_manifest(&mut |reader: &mut dyn Read| {
                let mut bytes = Vec::new();
                reader
                    .read_to_end(&mut bytes)
                    .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
                Ok(())
            });
            assert_eq!(result, Err(expected));
        }
    }

    #[test]
    fn provider_failure_outranks_visitor_failure() {
        for behavior in [
            ResourceBehavior::ProviderFailure,
            ResourceBehavior::ProviderFailureWithTrailingByte,
        ] {
            let mut access = access_with(behavior, b"abc".to_vec());
            let result = access.visit_manifest(&mut |_reader: &mut dyn Read| {
                Err(ExtensionRuntimeVisitorError::CapacityExceeded)
            });
            assert_eq!(result, Err(ExtensionPackageAccessError::Inactive));
        }
    }

    #[test]
    fn visitor_failure_is_nested_after_successful_validation() {
        let mut access = access_with(ResourceBehavior::Exact, b"abc".to_vec());
        let result = access.visit_manifest(&mut |_reader: &mut dyn Read| {
            Err(ExtensionRuntimeVisitorError::InvalidData)
        });
        assert_eq!(result, Ok(Err(ExtensionRuntimeVisitorError::InvalidData)));
    }

    #[test]
    fn visitor_failure_cannot_bypass_drain_and_eof_validation() {
        let mut access = access_with(ResourceBehavior::TrailingByte, b"abc".to_vec());
        let result = access.visit_manifest(&mut |_reader: &mut dyn Read| {
            Err(ExtensionRuntimeVisitorError::CapacityExceeded)
        });
        assert_eq!(
            result,
            Err(ExtensionPackageAccessError::ResourceIdentityMismatch)
        );
    }

    #[test]
    fn over_reported_reads_are_rejected_without_arithmetic_failure() {
        for visitor_reads in [false, true] {
            let (mut access, calls) =
                access_with_observed_reads(ResourceBehavior::OverReportedRead, b"abc".to_vec());
            let result = access.visit_manifest(&mut |reader: &mut dyn Read| {
                if visitor_reads {
                    let _ = reader.read(&mut [0_u8; 1]);
                }
                Ok(())
            });
            assert_eq!(
                result,
                Err(ExtensionPackageAccessError::ProviderContractViolation)
            );
            assert_eq!(calls.load(Ordering::Relaxed), 1);
        }

        let (mut access, calls) =
            access_with_observed_reads(ResourceBehavior::OverReportedEof, b"abc".to_vec());
        let result = access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(()));
        assert_eq!(
            result,
            Err(ExtensionPackageAccessError::ProviderContractViolation)
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn interrupted_reads_are_bounded_in_visitor_drain_and_eof_paths() {
        let maximum_attempts = MAX_EXTENSION_RUNTIME_INTERRUPTED_READ_RETRIES + 1;

        let (mut access, calls) =
            access_with_observed_reads(ResourceBehavior::InterruptedForever, b"abc".to_vec());
        let result = access.visit_manifest(&mut |reader: &mut dyn Read| {
            let _ = reader.read(&mut [0_u8; 1]);
            Err(ExtensionRuntimeVisitorError::ReadFailed)
        });
        assert_eq!(result, Err(ExtensionPackageAccessError::ResourceReadFailed));
        assert_eq!(calls.load(Ordering::Relaxed), maximum_attempts * 2);

        let (mut access, calls) =
            access_with_observed_reads(ResourceBehavior::InterruptedForever, b"abc".to_vec());
        let result = access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(()));
        assert_eq!(result, Err(ExtensionPackageAccessError::ResourceReadFailed));
        assert_eq!(calls.load(Ordering::Relaxed), maximum_attempts);

        let (mut access, calls) =
            access_with_observed_reads(ResourceBehavior::InterruptedForever, Vec::new());
        let result = access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(()));
        assert_eq!(result, Err(ExtensionPackageAccessError::ResourceReadFailed));
        assert_eq!(calls.load(Ordering::Relaxed), maximum_attempts);
    }

    #[test]
    fn provider_postvalidation_outranks_reader_contract_failure() {
        let mut access = access_with(
            ResourceBehavior::ProviderFailureWithOverReportedRead,
            b"abc".to_vec(),
        );
        let result = access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(()));
        assert_eq!(result, Err(ExtensionPackageAccessError::Inactive));
    }

    #[test]
    fn resource_callback_contract_is_enforced() {
        for behavior in [ResourceBehavior::NeverCalls, ResourceBehavior::CallsTwice] {
            let mut access = access_with(behavior, b"abc".to_vec());
            let mut calls = 0;
            let result = access.visit_manifest(&mut |_reader: &mut dyn Read| {
                calls += 1;
                Ok(())
            });
            assert_eq!(
                result,
                Err(ExtensionPackageAccessError::ProviderContractViolation)
            );
            assert_eq!(
                calls,
                usize::from(matches!(behavior, ResourceBehavior::CallsTwice))
            );
        }
    }

    #[test]
    fn native_root_callback_contract_is_enforced() {
        for root_calls in [0, 2] {
            let mut access = access_with_root_calls(root_calls);
            let mut consumer_calls = 0;
            let result = access.visit_native_root(&mut |_root: &Path| {
                consumer_calls += 1;
                Ok(())
            });
            assert_eq!(
                result,
                Err(ExtensionPackageAccessError::ProviderContractViolation)
            );
            assert_eq!(consumer_calls, usize::from(root_calls == 2));
            assert_eq!(
                access.visit_native_root(&mut |_root: &Path| Ok(())),
                Err(ExtensionPackageAccessError::Inactive)
            );
        }
    }

    #[test]
    fn terminal_identity_failures_deactivate_local_access() {
        let mut resource_access = access_with(ResourceBehavior::TrailingByte, b"abc".to_vec());
        assert_eq!(
            resource_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
            Err(ExtensionPackageAccessError::ResourceIdentityMismatch)
        );
        assert_eq!(
            resource_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
            Err(ExtensionPackageAccessError::Inactive)
        );

        let manifest = descriptor(0);
        let (provider, _) = provider(
            ResourceBehavior::Exact,
            Vec::new(),
            PathBuf::from("relative"),
        );
        let mut root_access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            manifest,
            provider,
        )
        .expect("access");
        assert_eq!(
            root_access.visit_native_root(&mut |_root: &Path| Ok(())),
            Err(ExtensionPackageAccessError::NativeRootIdentityMismatch)
        );
        assert_eq!(
            root_access.visit_native_root(&mut |_root: &Path| Ok(())),
            Err(ExtensionPackageAccessError::Inactive)
        );
    }

    #[test]
    fn provider_error_precedence_does_not_hide_terminal_local_validation() {
        let mut resource_access = access_with(
            ResourceBehavior::ProviderUnavailableWithTrailingByte,
            b"abc".to_vec(),
        );
        assert_eq!(
            resource_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
            Err(ExtensionPackageAccessError::ResourceUnavailable)
        );
        assert_eq!(
            resource_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
            Err(ExtensionPackageAccessError::Inactive)
        );

        let provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 98,
            retained_bytes: 0,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: PathBuf::from("relative"),
            root_calls: 1,
            root_provider_error: Some(ExtensionPackageAccessError::NativeRootUnavailable),
            recovered_identity: Arc::new(Mutex::new(None)),
            read_calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut root_access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            descriptor(0),
            provider,
        )
        .expect("access");
        assert_eq!(
            root_access.visit_native_root(&mut |_root: &Path| Ok(())),
            Err(ExtensionPackageAccessError::NativeRootUnavailable)
        );
        assert_eq!(
            root_access.visit_native_root(&mut |_root: &Path| Ok(())),
            Err(ExtensionPackageAccessError::Inactive)
        );
    }

    #[test]
    fn pre_delivery_transient_provider_failures_remain_retryable() {
        let mut resource_access = access_with(
            ResourceBehavior::ProviderUnavailableBeforeDelivery,
            b"abc".to_vec(),
        );
        for _ in 0..2 {
            assert_eq!(
                resource_access.visit_manifest(&mut |_reader: &mut dyn Read| Ok(())),
                Err(ExtensionPackageAccessError::ResourceUnavailable)
            );
        }

        let provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 99,
            retained_bytes: 0,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: absolute_test_root(),
            root_calls: 0,
            root_provider_error: Some(ExtensionPackageAccessError::NativeRootUnavailable),
            recovered_identity: Arc::new(Mutex::new(None)),
            read_calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut root_access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            descriptor(0),
            provider,
        )
        .expect("access");
        for _ in 0..2 {
            assert_eq!(
                root_access.visit_native_root(&mut |_root: &Path| Ok(())),
                Err(ExtensionPackageAccessError::NativeRootUnavailable)
            );
        }
    }

    #[test]
    fn read_failures_follow_service_retry_policy_without_local_poisoning() {
        let mut access = access_with(ResourceBehavior::ReadFailure, b"abc".to_vec());
        for _ in 0..2 {
            assert_eq!(
                access.visit_manifest(&mut |reader: &mut dyn Read| {
                    let mut bytes = Vec::new();
                    reader
                        .read_to_end(&mut bytes)
                        .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
                    Ok(())
                }),
                Err(ExtensionPackageAccessError::ResourceReadFailed)
            );
        }
    }

    #[test]
    fn native_root_is_borrowed_once_and_validated() {
        let mut access = access_with(ResourceBehavior::Exact, Vec::new());
        let expected = absolute_test_root();
        let mut calls = 0;
        let result = access.visit_native_root(&mut |root: &Path| {
            calls += 1;
            assert_eq!(root, expected);
            Ok(())
        });
        assert_eq!(result, Ok(Ok(())));
        assert_eq!(calls, 1);
    }

    #[test]
    fn invalid_root_never_reaches_consumer() {
        let manifest = descriptor(0);
        let (provider, _) = provider(
            ResourceBehavior::Exact,
            Vec::new(),
            PathBuf::from("relative"),
        );
        let mut access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            manifest,
            provider,
        )
        .expect("access");
        let mut calls = 0;
        let result = access.visit_native_root(&mut |_root: &Path| {
            calls += 1;
            Ok(())
        });
        assert_eq!(
            result,
            Err(ExtensionPackageAccessError::NativeRootIdentityMismatch)
        );
        assert_eq!(calls, 0);
    }

    #[test]
    fn lexically_noncanonical_roots_never_reach_consumer() {
        #[cfg(unix)]
        let roots = [
            PathBuf::from("/private//var/zephium/extensions"),
            PathBuf::from("/private/./var/zephium/extensions"),
            PathBuf::from("/private/var/zephium/extensions/"),
        ];
        #[cfg(windows)]
        let roots = [
            PathBuf::from(r"C:\\Zephium\extensions"),
            PathBuf::from(r"C:\Zephium\.\extensions"),
            PathBuf::from("C:\\Zephium\\extensions\\"),
        ];

        for root in roots {
            let manifest = descriptor(0);
            let (provider, _) = provider(ResourceBehavior::Exact, Vec::new(), root);
            let mut access = ExtensionPackageAccess::from_delegated_provider(
                ExtensionRuntimeTarget::NativeWebExtension,
                manifest,
                provider,
            )
            .expect("access");
            let mut calls = 0;
            let result = access.visit_native_root(&mut |_root: &Path| {
                calls += 1;
                Ok(())
            });
            assert_eq!(
                result,
                Err(ExtensionPackageAccessError::NativeRootIdentityMismatch)
            );
            assert_eq!(calls, 0);
        }
    }

    #[test]
    fn oversized_root_never_reaches_consumer() {
        #[cfg(unix)]
        let root = PathBuf::from(format!(
            "/{}",
            "a".repeat(MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES)
        ));
        #[cfg(windows)]
        let root = PathBuf::from(format!(
            r"C:\{}",
            "a".repeat(MAX_EXTENSION_RUNTIME_NATIVE_ROOT_BYTES)
        ));
        let manifest = descriptor(0);
        let (provider, _) = provider(ResourceBehavior::Exact, Vec::new(), root);
        let mut access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            manifest,
            provider,
        )
        .expect("access");
        let mut calls = 0;
        let result = access.visit_native_root(&mut |_root: &Path| {
            calls += 1;
            Ok(())
        });
        assert_eq!(
            result,
            Err(ExtensionPackageAccessError::NativeRootIdentityMismatch)
        );
        assert_eq!(calls, 0);
    }

    #[test]
    fn native_root_provider_failure_outranks_consumer_failure() {
        let manifest = descriptor(0);
        let recovered_identity = Arc::new(Mutex::new(None));
        let provider: Box<dyn ExtensionPackageAccessPort> = Box::new(TestProvider {
            identity: 93,
            retained_bytes: 0,
            bytes: Vec::new(),
            behavior: ResourceBehavior::Exact,
            root: absolute_test_root(),
            root_calls: 1,
            root_provider_error: Some(ExtensionPackageAccessError::Inactive),
            recovered_identity,
            read_calls: Arc::new(AtomicUsize::new(0)),
        });
        let mut access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::NativeWebExtension,
            manifest,
            provider,
        )
        .expect("access");
        let result = access.visit_native_root(&mut |_root: &Path| {
            Err(ExtensionRuntimeVisitorError::ConsumerUnavailable)
        });
        assert_eq!(result, Err(ExtensionPackageAccessError::Inactive));
    }

    #[test]
    fn debug_output_redacts_identifiers_and_providers() {
        let descriptor = ExtensionRuntimeResource::try_new([0xab; 32], 0).expect("descriptor");
        let (provider, _) = provider(ResourceBehavior::Exact, Vec::new(), absolute_test_root());
        let access = ExtensionPackageAccess::from_delegated_provider(
            ExtensionRuntimeTarget::Compatibility,
            descriptor,
            provider,
        )
        .expect("access");

        let descriptor_debug = format!("{descriptor:?}");
        let access_debug = format!("{access:?}");
        assert!(descriptor_debug.contains("[redacted]"));
        assert!(!descriptor_debug.contains("abab"));
        assert!(access_debug.contains("[redacted]"));
        assert!(!access_debug.contains("TestProvider"));
    }

    #[test]
    fn capability_and_ports_are_send_and_object_safe() {
        fn assert_send<T: Send>() {}
        assert_send::<ExtensionPackageAccess>();
        let _: Option<Box<dyn ExtensionPackageAccessPort>> = None;
        let _: Option<Box<dyn ExtensionRuntimeResourceVisitor>> = None;
        let _: Option<Box<dyn ExtensionRuntimeNativeRootVisitor>> = None;
    }
}
