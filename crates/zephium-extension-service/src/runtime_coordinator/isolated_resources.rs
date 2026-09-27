//! Bounded, actor-owned resource reads for an isolated native isolated extension view.
//! The active runtime owner remains the only package-byte authority. The UI
//! never receives a package path or a clone of that authority.

use std::io::Read;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use zephium_core::extensions::{
    ExtensionGrantBrowsingContext, ExtensionNativeOwnershipKey, ExtensionRuntimeInstance,
};
use zephium_core::ports::extensions::{
    IsolatedExtensionResourceCancel, VerifiedIsolatedExtensionResource,
};
use zephium_extension_runtime_api::ExtensionRuntimeVisitorError;

use super::RuntimeCoordinator;

pub(crate) const MAX_PENDING_ISOLATED_RESOURCE_REQUESTS: usize = 8;
pub(crate) const MAX_PENDING_ISOLATED_RESOURCE_BYTES: usize = 16 * 1024 * 1024;

fn qa_trace_resource_read() {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var("ZEPHIUM_OFFSCREEN_QA_TRACE").as_deref() == Ok("1")) {
        eprintln!("isolated-resource-product: service-resource-authenticated");
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IsolatedResourceFailure {
    RuntimeUnavailable,
    NotDeclared,
    Capacity,
    ReadFailed,
    Expired,
    Cancelled,
}

#[derive(Default)]
struct BudgetState {
    requests: usize,
    bytes: usize,
    failed: bool,
}

/// One process-wide budget shared by all profiles of the extension service.
#[derive(Clone, Default)]
pub(crate) struct IsolatedResourceBudget(Arc<Mutex<BudgetState>>);

impl IsolatedResourceBudget {
    /// Counts an admitted native request before it enters the actor queue.
    pub(crate) fn reserve_request(&self) -> Option<IsolatedResourcePermit> {
        let mut state = self.0.lock().ok()?;
        if state.failed || state.requests >= MAX_PENDING_ISOLATED_RESOURCE_REQUESTS {
            return None;
        }
        state.requests += 1;
        Some(IsolatedResourcePermit {
            budget: self.clone(),
            bytes: 0,
            charged: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn used(&self) -> Option<(usize, usize)> {
        self.0
            .lock()
            .ok()
            .map(|state| (state.requests, state.bytes))
    }
}

pub(crate) struct IsolatedResourcePermit {
    budget: IsolatedResourceBudget,
    bytes: usize,
    charged: bool,
}

impl IsolatedResourcePermit {
    fn reserve_bytes(&mut self, declared_bytes: usize) -> bool {
        if self.charged || declared_bytes > MAX_PENDING_ISOLATED_RESOURCE_BYTES {
            return false;
        }
        let Ok(mut state) = self.budget.0.lock() else {
            return false;
        };
        let Some(next) = state.bytes.checked_add(declared_bytes) else {
            return false;
        };
        if state.failed || next > MAX_PENDING_ISOLATED_RESOURCE_BYTES {
            return false;
        }
        state.bytes = next;
        self.bytes = declared_bytes;
        self.charged = true;
        true
    }
}

impl Drop for IsolatedResourcePermit {
    fn drop(&mut self) {
        let Ok(mut state) = self.budget.0.lock() else {
            return;
        };
        match (
            state.requests.checked_sub(1),
            state.bytes.checked_sub(self.bytes),
        ) {
            (Some(requests), Some(bytes)) => {
                state.requests = requests;
                state.bytes = bytes;
            }
            _ => state.failed = true,
        }
    }
}

/// A complete, provider-validated file and its capacity reservation. Its
/// caller must rejoin this response to the original native scheme task and
/// exact runtime generation before publishing any bytes to WebKit.
#[must_use = "drop or publish the verified resource before settling its scheme task"]
pub(crate) struct VerifiedExtensionResource {
    runtime: ExtensionRuntimeInstance,
    path: Box<str>,
    bytes: Box<[u8]>,
    _permit: IsolatedResourcePermit,
}

impl VerifiedExtensionResource {
    #[cfg(all(test, zephium_internal_repository_e2e))]
    pub(crate) const fn runtime(&self) -> ExtensionRuntimeInstance {
        self.runtime
    }
    #[cfg(all(test, zephium_internal_repository_e2e))]
    pub(crate) fn path(&self) -> &str {
        &self.path
    }
    #[cfg(all(test, zephium_internal_repository_e2e))]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(crate) fn into_core(self) -> Option<VerifiedIsolatedExtensionResource> {
        VerifiedIsolatedExtensionResource::from_trusted_service(
            self.runtime,
            self.path,
            self.bytes,
            Box::new(self._permit),
        )
    }
}

impl RuntimeCoordinator {
    /// Reads one canonical package file from the exact published regular
    /// runtime. The service actor serializes this with retirement and update;
    /// no worker is created for this operation.
    pub(crate) fn read_published_isolated_resource(
        &mut self,
        runtime: ExtensionRuntimeInstance,
        path: &str,
        mut permit: IsolatedResourcePermit,
        deadline: Instant,
        cancel: &IsolatedExtensionResourceCancel,
    ) -> Result<VerifiedExtensionResource, IsolatedResourceFailure> {
        if !cancel.is_active() {
            return Err(IsolatedResourceFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(IsolatedResourceFailure::Expired);
        }
        let key = ExtensionNativeOwnershipKey::new(
            runtime.profile(),
            runtime.install_id(),
            ExtensionGrantBrowsingContext::Regular,
        );
        let slot = self
            .slot_mut(key)
            .ok_or(IsolatedResourceFailure::RuntimeUnavailable)?;
        if slot.generation() != Some(runtime.generation()) {
            return Err(IsolatedResourceFailure::RuntimeUnavailable);
        }
        let owner = slot
            .published_owner_mut()
            .ok_or(IsolatedResourceFailure::RuntimeUnavailable)?;
        let entry = owner
            .resources()
            .entry(path)
            .ok_or(IsolatedResourceFailure::NotDeclared)?;
        let length = usize::try_from(entry.declared_bytes())
            .map_err(|_| IsolatedResourceFailure::Capacity)?;
        let resource = entry.resource();
        if !permit.reserve_bytes(length) {
            return Err(IsolatedResourceFailure::Capacity);
        }
        let mut bytes = Vec::with_capacity(length);
        let mut visitor = |reader: &mut dyn Read| {
            reader
                .take(length as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ExtensionRuntimeVisitorError::ReadFailed)?;
            (bytes.len() == length)
                .then_some(())
                .ok_or(ExtensionRuntimeVisitorError::InvalidData)
        };
        match owner.visit_resource(resource, &mut visitor) {
            Ok(Ok(())) => {}
            _ => return Err(IsolatedResourceFailure::ReadFailed),
        }
        if !cancel.is_active() {
            return Err(IsolatedResourceFailure::Cancelled);
        }
        if Instant::now() >= deadline {
            return Err(IsolatedResourceFailure::Expired);
        }
        qa_trace_resource_read();
        Ok(VerifiedExtensionResource {
            runtime,
            path: path.into(),
            bytes: bytes.into_boxed_slice(),
            _permit: permit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_global_budget_counts_retained_replies_and_releases_on_drop() {
        let budget = IsolatedResourceBudget::default();
        let half = MAX_PENDING_ISOLATED_RESOURCE_BYTES / 2;
        let mut first = budget.reserve_request().expect("first request");
        let mut second = budget.reserve_request().expect("second request");
        assert!(first.reserve_bytes(half));
        assert!(second.reserve_bytes(half));
        let mut over_bytes = budget.reserve_request().expect("third request slot");
        assert!(!over_bytes.reserve_bytes(1));
        drop(over_bytes);
        drop(first);
        let mut third = budget.reserve_request().expect("released request slot");
        assert!(third.reserve_bytes(half));
        drop((second, third));
        let permits = (0..MAX_PENDING_ISOLATED_RESOURCE_REQUESTS)
            .map(|_| budget.reserve_request().expect("request slot"))
            .collect::<Vec<_>>();
        assert!(budget.reserve_request().is_none());
        drop(permits);
        let empty = budget
            .reserve_request()
            .expect("empty file still occupies one request slot");
        drop(empty);
        let mut full = budget.reserve_request().expect("full byte request slot");
        assert!(full.reserve_bytes(MAX_PENDING_ISOLATED_RESOURCE_BYTES));
    }

    #[test]
    fn absent_runtime_never_reads_or_reserves_package_bytes() {
        let mut coordinator = RuntimeCoordinator::new();
        let budget = IsolatedResourceBudget::default();
        let cancel = IsolatedExtensionResourceCancel::new();
        let runtime = ExtensionRuntimeInstance::new(
            zephium_core::ids::ProfileId::from(1),
            zephium_core::ids::ExtensionInstallId::from(1),
            zephium_core::extensions::ExtensionRuntimeGeneration::INITIAL,
        );
        assert!(matches!(
            coordinator.read_published_isolated_resource(
                runtime,
                "manifest.json",
                budget.reserve_request().unwrap(),
                Instant::now() + std::time::Duration::from_secs(1),
                &cancel,
            ),
            Err(IsolatedResourceFailure::RuntimeUnavailable)
        ));
        assert_eq!(budget.used(), Some((0, 0)));
    }
}
