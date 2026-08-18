//! Shell settlement for capability-limited extension compatibility requests.

use super::*;
use zephium_core::extensions::{
    ExtensionCompatibilityBrokerOperation, ExtensionCompatibilityBrokerRejection,
    ExtensionCompatibilityBrokerRequest, ExtensionCompatibilityBrokerResult,
    ExtensionCompatibilityBrokerSettlement, ExtensionCompatibilityHistoryEntry,
    ExtensionCompatibilitySearchDisposition,
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
        let operation = request.operation().clone();
        let admitted = match operation {
            ExtensionCompatibilityBrokerOperation::RecentHistory { limit } => self
                .store_reads
                .as_ref()
                .is_some_and(|reads| reads.request_extension_recent_history(runtime, id, limit)),
            ExtensionCompatibilityBrokerOperation::DefaultSearch { disposition, query } => {
                let result =
                    self.extension_default_search(runtime.profile(), disposition, query.into());
                self.settle_extension_compatibility_broker(
                    runtime,
                    id,
                    ExtensionCompatibilityBrokerSettlement::Applied(
                        ExtensionCompatibilityBrokerResult::DefaultSearch { opened: result },
                    ),
                );
                return;
            }
            ExtensionCompatibilityBrokerOperation::RestoreRecentSession => {
                let restored = self
                    .restore_recently_closed_tab(runtime.profile())
                    .is_some();
                self.settle_extension_compatibility_broker(
                    runtime,
                    id,
                    ExtensionCompatibilityBrokerSettlement::Applied(
                        ExtensionCompatibilityBrokerResult::RecentSessionRestore { restored },
                    ),
                );
                return;
            }
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

    pub(super) fn extension_default_search(
        &mut self,
        profile: ProfileId,
        disposition: ExtensionCompatibilitySearchDisposition,
        query: String,
    ) -> bool {
        let Some(target) = navigation::search_query(&query) else {
            return false;
        };
        let Some(window) = self
            .windows
            .focused()
            .filter(|window| window.profile == profile)
        else {
            return false;
        };
        let active = window.active;
        let outcome = match disposition {
            ExtensionCompatibilitySearchDisposition::CurrentTab => active
                .map(|item| self.operation_navigate(item, target.to_string()))
                .unwrap_or_else(|| {
                    operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope)
                }),
            ExtensionCompatibilitySearchDisposition::NewTab => {
                self.operation_open_url(target.to_string())
            }
        };
        matches!(
            outcome.outcome,
            OperationOutcome::Applied | OperationOutcome::NoOp | OperationOutcome::Deferred
        )
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
