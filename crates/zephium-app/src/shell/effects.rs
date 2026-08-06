//! Authoritative core-effect application to native engine work.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct NativeWork {
    pub(super) scheduled: bool,
    pub(super) rejected: bool,
    unsupported: bool,
}

impl NativeWork {
    pub(super) fn record(&mut self, admission: NativeDispatch) {
        match admission {
            NativeDispatch::Scheduled => self.scheduled = true,
            NativeDispatch::Rejected => self.rejected = true,
            NativeDispatch::Unsupported => self.unsupported = true,
        }
    }

    pub(super) fn merge(&mut self, other: Self) {
        self.scheduled |= other.scheduled;
        self.rejected |= other.rejected;
        self.unsupported |= other.unsupported;
    }
}

pub(super) fn operation_result(
    outcome: OperationOutcome,
    reason: OperationReason,
) -> OperationDisposition {
    OperationDisposition {
        operation_id: String::new(),
        outcome,
        reason,
    }
}

pub(super) fn mutation_result(native: NativeWork) -> OperationDisposition {
    if native.rejected {
        operation_result(
            OperationOutcome::NativeAdmissionFailed,
            OperationReason::NativeDispatchRejected,
        )
    } else if native.unsupported {
        operation_result(
            OperationOutcome::Rejected,
            OperationReason::UnsupportedCommand,
        )
    } else if native.scheduled {
        operation_result(
            OperationOutcome::Deferred,
            OperationReason::NativeWorkPending,
        )
    } else {
        operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
    }
}

impl Shell {
    pub(super) fn commit(&mut self, effects: Vec<Effect>) -> NativeWork {
        // Every ordinary committed mutation may change focus, view
        // residency, or split topology. A captured divider path cannot cross
        // that boundary; resize/sidebar geometry updates deliberately bypass
        // `commit` and remain draggable through path-based recomputation.
        self.divider = None;
        let mut native = self.apply(effects);
        self.schedule_persist();
        // Visibility has to land before dormancy. WebView2 only accepts a
        // suspend request for an already-hidden controller; recording the
        // request first would permanently lose the transition.
        native.record(self.relayout());
        self.maintain_views();
        self.project_items();
        native
    }

    pub(super) fn apply(&mut self, effects: Vec<Effect>) -> NativeWork {
        // Deletion quarantine dominates every other profile policy. Filter
        // before the blocker gate so an effect can never be retained there
        // and replayed after the extension worker has installed its monotonic
        // retirement fence.
        let mut admitted = Vec::with_capacity(effects.len());
        let mut native = NativeWork::default();
        for effect in effects {
            match effect {
                Effect::CreateView { id, .. } if self.profile_deletion_quarantines_item(id) => {
                    self.zoom.pending.remove(&id);
                    self.items.view_creation_failed(id);
                    native.rejected = true;
                    crate::diagnostic!(
                        "profile deletion: refused native view creation for quarantined profile"
                    );
                }
                Effect::Navigate { id, request, .. }
                    if self.profile_deletion_quarantines_item(id) =>
                {
                    self.items.navigation_failed(id, request);
                    native.rejected = true;
                    crate::diagnostic!(
                        "profile deletion: refused native navigation for quarantined profile"
                    );
                }
                effect => admitted.push(effect),
            }
        }
        let (effects, blocker_native) = self.blocker_gate_effects(admitted);
        native.merge(blocker_native);
        if effects.is_empty() {
            return native;
        }
        // `Items` sets a tab's logical view bit before returning CreateView.
        // A batch (restore or multi-leaf process recovery) can therefore make
        // every prospective view appear resident before the first native
        // create is considered. Subtract this exact, unique optimistic set to
        // recover the pre-batch resident count, then account only successful
        // creates in actor order. This keeps the ceiling exact without
        // rejecting an early visible leaf because a later leaf pre-set its
        // bit.
        let optimistic_creates: std::collections::HashSet<ItemId> = effects
            .iter()
            .filter_map(|effect| match effect {
                Effect::CreateView { id, .. }
                    if self.items.tab(*id).is_some_and(TabState::has_view) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        let mut logical_residents = self
            .items
            .view_ids()
            .len()
            .saturating_sub(optimistic_creates.len());
        let mut rejected_creates = std::collections::HashSet::new();
        let bounds = self.content_region();
        for effect in effects {
            match effect {
                Effect::CreateView { id, url } => {
                    if logical_residents >= LIVE_VIEW_ABSOLUTE_LIMIT {
                        self.zoom.pending.remove(&id);
                        self.items.view_creation_failed(id);
                        rejected_creates.insert(id);
                        native.rejected = true;
                        continue;
                    }
                    if !self
                        .engine
                        .create_view(id, self.partition_of(id), &url, bounds)
                    {
                        // Dispatch rejection is synchronous and must not rely
                        // on a callback entering an already-overloaded queue.
                        self.zoom.pending.remove(&id);
                        self.items.view_creation_failed(id);
                        rejected_creates.insert(id);
                        native.rejected = true;
                        continue;
                    }
                    logical_residents += 1;
                    native.scheduled = true;
                    // restored or revived views keep their zoom
                    let zoom = self.items.tab(id).map(|t| t.zoom).unwrap_or(1.0);
                    if zoom != 1.0 {
                        let admission = self.request_zoom(id, zoom);
                        native.record(admission);
                        if admission != NativeDispatch::Scheduled {
                            // A newly constructed view starts at 1.0. If its
                            // restore request never reached the native queue,
                            // do not retain or later persist the stale scale.
                            self.items.set_zoom(id, 1.0);
                            self.schedule_persist();
                            self.project_tab(id);
                        }
                    }
                }
                Effect::Navigate { id, url, request } => {
                    if rejected_creates.contains(&id) {
                        // `view_creation_failed` already revoked this pending
                        // request. Do not dispatch navigation to a controller
                        // that this batch deliberately did not create.
                        continue;
                    }
                    if !self.engine.navigate(id, &url, request) {
                        self.items.navigation_failed(id, request);
                        native.rejected = true;
                    } else {
                        native.scheduled = true;
                    }
                }
                Effect::Close { id } => {
                    self.zoom.pending.remove(&id);
                    native.record(self.engine.close(id));
                }
            }
        }
        native
    }
}
