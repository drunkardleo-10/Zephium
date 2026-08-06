//! Generation-correlated native zoom requests and settlement.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PendingZoom {
    request: ZoomRequestId,
    pub(super) desired_scale: f64,
}

#[derive(Default)]
pub(super) struct ZoomState {
    pub(super) next_request: u64,
    pub(super) pending: std::collections::HashMap<ItemId, PendingZoom>,
}

impl Shell {
    pub(super) fn request_zoom(&mut self, id: ItemId, desired_scale: f64) -> NativeDispatch {
        let Some(next) = self.zoom.next_request.checked_add(1) else {
            // Reusing an identity could let a late result settle a newer
            // request. Saturation permanently rejects new zoom work.
            return NativeDispatch::Rejected;
        };
        self.zoom.next_request = next;
        let request = ZoomRequestId(next);
        let admission = self.engine.zoom(id, desired_scale, request);
        if admission == NativeDispatch::Scheduled {
            self.zoom.pending.insert(
                id,
                PendingZoom {
                    request,
                    desired_scale,
                },
            );
        }
        admission
    }

    pub(super) fn on_zoom_settled(
        &mut self,
        id: ItemId,
        request: ZoomRequestId,
        applied_scale: f64,
        succeeded: bool,
    ) {
        let Some(pending) = self.zoom.pending.get(&id).copied() else {
            return;
        };
        if pending.request != request {
            // A newest-per-item native settlement contains the cumulative
            // applied scale. Older results cannot settle a newer desired
            // value and are deliberately ignored.
            return;
        }
        // This exact terminal result owns the obligation even when its native
        // payload is malformed. Retire it before validation so a corrupt or
        // incompatible engine response cannot leave all future zoom input
        // based on a value that will never settle.
        self.zoom.pending.remove(&id);
        if !applied_scale.is_finite() || !(0.3..=3.0).contains(&applied_scale) {
            crate::diagnostic!("engine: rejected malformed native zoom settlement");
            return;
        }
        if !succeeded {
            crate::diagnostic!("engine: native zoom request was not applied");
        } else if (applied_scale - pending.desired_scale).abs() > f64::EPSILON {
            crate::diagnostic!("engine: native zoom settled at an unexpected scale");
        }
        let Some(previous) = self.items.tab(id).map(|tab| tab.zoom) else {
            return;
        };
        if (previous - applied_scale).abs() <= f64::EPSILON {
            return;
        }
        self.items.set_zoom(id, applied_scale);
        self.schedule_persist();
        self.project_tab(id);
    }
}
