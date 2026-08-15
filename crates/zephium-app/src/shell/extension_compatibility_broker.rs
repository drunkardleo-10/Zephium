//! Shell settlement for capability-limited extension compatibility requests.

use super::*;
use zephium_core::extensions::{
    ExtensionCompatibilityBrokerOperation, ExtensionCompatibilityBrokerRejection,
    ExtensionCompatibilityBrokerRequest, ExtensionCompatibilityBrokerResult,
    ExtensionCompatibilityBrokerSettlement, ExtensionCompatibilityHistoryEntry,
};

impl Shell {
    pub(super) fn on_extension_compatibility_broker_request(
        &mut self,
        request: ExtensionCompatibilityBrokerRequest,
    ) {
        let runtime = request.runtime();
        let id = request.id();
        if !self.bootstrapped
            || !self.extension_browser_surfaces.is_active(runtime.profile())
            || self.profile_deletion_quarantines(runtime.profile())
        {
            self.settle_extension_compatibility_broker(
                runtime,
                id,
                ExtensionCompatibilityBrokerSettlement::Rejected(
                    ExtensionCompatibilityBrokerRejection::InvalidContext,
                ),
            );
            return;
        }
        let admitted = match request.operation() {
            ExtensionCompatibilityBrokerOperation::RecentHistory { limit } => self
                .store_reads
                .as_ref()
                .is_some_and(|reads| reads.request_extension_recent_history(runtime, id, limit)),
        };
        if !admitted {
            self.settle_extension_compatibility_broker(
                runtime,
                id,
                ExtensionCompatibilityBrokerSettlement::Rejected(
                    ExtensionCompatibilityBrokerRejection::BackendUnavailable,
                ),
            );
        }
    }

    pub(super) fn on_extension_recent_history_read(
        &mut self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        hits: Vec<zephium_core::ports::store::HistoryHit>,
    ) {
        let entries = hits
            .into_iter()
            .map(|hit| ExtensionCompatibilityHistoryEntry {
                url: hit.url,
                title: hit.title,
                last_visit: hit.last_visit,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.settle_extension_compatibility_broker(
            runtime,
            request,
            ExtensionCompatibilityBrokerSettlement::Applied(
                ExtensionCompatibilityBrokerResult::RecentHistory(entries),
            ),
        );
    }

    fn settle_extension_compatibility_broker(
        &self,
        runtime: zephium_core::extensions::ExtensionRuntimeInstance,
        request: zephium_core::extensions::ExtensionCompatibilityBrokerRequestId,
        settlement: ExtensionCompatibilityBrokerSettlement,
    ) {
        if self
            .engine
            .settle_extension_compatibility_broker_request(runtime, request, settlement)
            != NativeDispatch::Scheduled
        {
            crate::diagnostic!("extensions: compatibility-broker settlement was not admitted");
        }
    }
}
