//! The transient Work browser pane: one Space tab laid out in a Rust-owned
//! content rect above full-window Work chrome. Geometry is session-only.

use super::*;
use crate::api::WorkPaneTarget;

pub(super) struct WorkPane {
    pub(super) window: WindowId,
    pub(super) tab: ItemId,
    pub(super) rect: Rect,
    pub(super) generation: u32,
}

impl Shell {
    pub(super) fn work_pane_tab(&self) -> Option<ItemId> {
        self.work_pane
            .as_ref()
            .filter(|_| self.active_browser_page() == Some(crate::BrowserPage::Work))
            .map(|pane| pane.tab)
    }

    pub(super) fn work_pane_shows(&self, id: ItemId) -> bool {
        self.work_pane_tab() == Some(id)
    }

    /// The pane's leaf when the engine can lay it out right now.
    pub(super) fn work_pane_tree(&self) -> Option<Pane> {
        let pane = self.work_pane.as_ref()?;
        let win = self.windows.focused()?;
        (self.active_browser_page() == Some(crate::BrowserPage::Work)
            && pane.window == win.id
            && self.item_in_scope(pane.tab, win.profile, win.space)
            && self.items.tab(pane.tab).is_some_and(TabState::has_view))
        .then_some(Pane::Leaf(pane.tab))
    }

    /// Leaves the engine may show now: the split tree in Browse, the pane in Work.
    pub(super) fn visible_tree(&self) -> Option<Pane> {
        if self.active_browser_page().is_some() {
            self.work_pane_tree()
        } else {
            self.pane_tree()
        }
    }

    pub(super) fn work_pane_layout(&self, presented: bool) -> Option<zephium_ipc::WorkPaneLayout> {
        let pane = self.work_pane.as_ref()?;
        let win = self.windows.focused()?;
        if pane.window != win.id || self.active_browser_page() != Some(crate::BrowserPage::Work) {
            return None;
        }
        let rect = layout::clamp_work_pane_rect(win.size, pane.rect).unwrap_or(pane.rect);
        Some(zephium_ipc::WorkPaneLayout {
            tab: pane.tab.to_string(),
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
            presented,
            generation: pane.generation,
        })
    }

    fn next_work_pane_generation(&mut self) -> u32 {
        self.work_pane_generation = self.work_pane_generation.wrapping_add(1).max(1);
        self.work_pane_generation
    }

    pub(super) fn clear_work_pane(&mut self) -> bool {
        if self.work_pane.take().is_none() {
            return false;
        }
        self.next_work_pane_generation();
        true
    }

    /// Drops pane state whose window, page, scope, or tab no longer exists.
    pub(super) fn prune_work_pane(&mut self) -> bool {
        let Some(pane) = self.work_pane.as_ref() else {
            return false;
        };
        let valid = self.active_browser_page() == Some(crate::BrowserPage::Work)
            && self.windows.focused().is_some_and(|win| {
                win.id == pane.window && self.item_in_scope(pane.tab, win.profile, win.space)
            });
        if valid {
            return false;
        }
        self.clear_work_pane()
    }

    pub(super) fn operation_work_pane_show(
        &mut self,
        target: WorkPaneTarget,
        rect: Rect,
    ) -> OperationDisposition {
        if self.active_browser_page() != Some(crate::BrowserPage::Work) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let Some((window, profile, space, size)) = self
            .windows
            .focused()
            .map(|win| (win.id, win.profile, win.space, win.size))
        else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        let Some(rect) = layout::clamp_work_pane_rect(size, rect) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        };
        let (tab, effects, discard_closing) = match target {
            WorkPaneTarget::Tab(id) => {
                if !self.item_in_scope(id, profile, space) {
                    return operation_result(
                        OperationOutcome::Rejected,
                        OperationReason::InvalidScope,
                    );
                }
                let discard_closing = matches!(
                    self.residency.discard_probes.get(&id),
                    Some(PendingDiscardProbe::Closing { .. })
                );
                self.cancel_discard_probe(id);
                (id, self.items.ensure_view(id), discard_closing)
            }
            WorkPaneTarget::Url(input) => {
                if navigation::classify(&input).is_none() {
                    return operation_result(
                        OperationOutcome::Rejected,
                        OperationReason::InvalidInput,
                    );
                }
                if self.profile_deletion_quarantines(profile) {
                    return operation_result(
                        OperationOutcome::Rejected,
                        OperationReason::InvalidScope,
                    );
                }
                let id = ItemId::generate();
                if !self.items.insert_tab(
                    id,
                    Placement::Space {
                        space,
                        section: SpaceSection::Today,
                    },
                ) {
                    return operation_result(
                        OperationOutcome::Rejected,
                        OperationReason::ItemLimitReached,
                    );
                }
                (id, self.items.navigate(id, &input), false)
            }
        };
        let generation = self.next_work_pane_generation();
        self.work_pane = Some(WorkPane {
            window,
            tab,
            rect,
            generation,
        });
        self.touch(tab);
        self.cancel_page_permission_if_not_foreground();
        let native = self.commit(effects);
        if discard_closing && !native.rejected {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    pub(super) fn operation_work_pane_hide(&mut self) -> OperationDisposition {
        if !self.clear_work_pane() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        self.cancel_page_permission_if_not_foreground();
        let mut native = NativeWork::default();
        native.record(self.relayout());
        self.maintain_views();
        mutation_result(native)
    }

    pub(super) fn work_pane_set_rect(&mut self, rect: Rect, generation: u32) {
        let Some(size) = self.windows.focused().map(|win| win.size) else {
            return;
        };
        let Some(pane) = self
            .work_pane
            .as_mut()
            .filter(|pane| pane.generation == generation)
        else {
            return;
        };
        let Some(rect) = layout::clamp_work_pane_rect(size, rect) else {
            return;
        };
        if pane.rect == rect {
            return;
        }
        pane.rect = rect;
        let _ = self.relayout();
    }
}
