//! One bounded resource reply from an active extension owner to a native
//! isolated document host. This data receipt is not package or grant authority:
//! the native host must rejoin its request id, runtime generation, context,
//! document URL, and live scheme task before publishing bytes to WebKit.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::extensions::ExtensionRuntimeInstance;

pub const MAX_ISOLATED_EXTENSION_RESOURCE_REPLY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ISOLATED_EXTENSION_RESOURCE_PATH_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IsolatedExtensionDocumentKind {
    Offscreen,
    Sandbox,
}

/// A revocation-only token shared by the native task, Shell relay, and
/// existing service actor. Cancellation never grants access or settles a
/// native task by itself; it only prevents queued stale package I/O.
#[derive(Clone)]
pub struct IsolatedExtensionResourceCancel(Arc<AtomicBool>);

impl IsolatedExtensionResourceCancel {
    pub fn new() -> Self { Self(Arc::new(AtomicBool::new(true))) }
    pub fn is_active(&self) -> bool { self.0.load(Ordering::Acquire) }
    pub fn cancel(&self) { self.0.store(false, Ordering::Release); }
}

impl Default for IsolatedExtensionResourceCancel {
    fn default() -> Self { Self::new() }
}
impl PartialEq for IsolatedExtensionResourceCancel {
    fn eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
}
impl Eq for IsolatedExtensionResourceCancel {}
impl fmt::Debug for IsolatedExtensionResourceCancel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IsolatedExtensionResourceCancel(<redacted>)")
    }
}

/// Bounded routing data, never authority. The native broker keeps the exact
/// WKURLSchemeTask and context separately until settlement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IsolatedExtensionResourceRequest {
    runtime: ExtensionRuntimeInstance,
    kind: IsolatedExtensionDocumentKind,
    id: u64,
    path: Box<str>,
    deadline: Instant,
    cancel: IsolatedExtensionResourceCancel,
}

impl IsolatedExtensionResourceRequest {
    pub fn new(
        runtime: ExtensionRuntimeInstance,
        kind: IsolatedExtensionDocumentKind,
        id: u64,
        path: &str,
        deadline: Instant,
        cancel: IsolatedExtensionResourceCancel,
    ) -> Option<Self> {
        if id == 0
            || path.is_empty()
            || path.len() > MAX_ISOLATED_EXTENSION_RESOURCE_PATH_BYTES
            || path.chars().any(char::is_control)
        {
            return None;
        }
        Some(Self {
            runtime,
            kind,
            id,
            path: path.into(),
            deadline,
            cancel,
        })
    }

    pub const fn runtime(&self) -> ExtensionRuntimeInstance {
        self.runtime
    }
    pub const fn kind(&self) -> IsolatedExtensionDocumentKind {
        self.kind
    }
    pub const fn id(&self) -> u64 {
        self.id
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub const fn deadline(&self) -> Instant {
        self.deadline
    }
    pub fn cancel_token(&self) -> IsolatedExtensionResourceCancel { self.cancel.clone() }
}

#[must_use = "the native host must settle or drop verified bytes and their capacity lease"]
pub struct VerifiedIsolatedExtensionResource {
    runtime: ExtensionRuntimeInstance,
    path: Box<str>,
    bytes: Box<[u8]>,
    _capacity_lease: Box<dyn Send>,
}

impl VerifiedIsolatedExtensionResource {
    /// Only an exact actor-owned package read may call this constructor. The
    /// receiver still has to revalidate the native task and runtime identity.
    pub fn from_trusted_service(
        runtime: ExtensionRuntimeInstance,
        path: Box<str>,
        bytes: Box<[u8]>,
        capacity_lease: Box<dyn Send>,
    ) -> Option<Self> {
        if path.is_empty()
            || path.len() > MAX_ISOLATED_EXTENSION_RESOURCE_PATH_BYTES
            || path.chars().any(char::is_control)
            || bytes.len() > MAX_ISOLATED_EXTENSION_RESOURCE_REPLY_BYTES
        {
            return None;
        }
        Some(Self {
            runtime,
            path,
            bytes,
            _capacity_lease: capacity_lease,
        })
    }

    pub const fn runtime(&self) -> ExtensionRuntimeInstance {
        self.runtime
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for VerifiedIsolatedExtensionResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedIsolatedExtensionResource")
            .field("runtime", &"<redacted>")
            .field("path", &"<redacted>")
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

#[derive(Debug)]
pub enum IsolatedExtensionResourceOutcome {
    Verified(VerifiedIsolatedExtensionResource),
    RuntimeUnavailable,
    NotDeclared,
    Capacity,
    ReadFailed,
    Expired,
    Cancelled,
    WorkerUnavailable,
}

pub type IsolatedExtensionResourceCallback =
    Box<dyn FnOnce(IsolatedExtensionResourceOutcome) + Send>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::ExtensionRuntimeGeneration;
    use crate::ids::{ExtensionInstallId, ProfileId};

    #[test]
    fn reply_keeps_exact_identity_and_rejects_oversized_data() {
        let runtime = ExtensionRuntimeInstance::new(
            ProfileId::from(1),
            ExtensionInstallId::from(2),
            ExtensionRuntimeGeneration::INITIAL,
        );
        let value = VerifiedIsolatedExtensionResource::from_trusted_service(
            runtime,
            "document.js".into(),
            Box::from(*b"ok"),
            Box::new(()),
        )
        .unwrap();
        assert_eq!(value.runtime(), runtime);
        assert_eq!(value.path(), "document.js");
        assert_eq!(value.bytes(), b"ok");
        assert!(VerifiedIsolatedExtensionResource::from_trusted_service(
            runtime,
            "document.js".into(),
            vec![0; MAX_ISOLATED_EXTENSION_RESOURCE_REPLY_BYTES + 1].into_boxed_slice(),
            Box::new(()),
        )
        .is_none());
    }
}
