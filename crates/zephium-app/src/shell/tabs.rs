//! Tab creation, focus, close, split mutation, and linked navigation.

use super::*;

impl Shell {
    pub(super) fn open_tab(&mut self) -> Vec<Effect> {
        self.open_tab_with_id()
            .map(|(_, effects)| effects)
            .unwrap_or_default()
    }

    pub(super) fn open_tab_with_id(&mut self) -> Option<(ItemId, Vec<Effect>)> {
        if self
            .windows
            .focused()
            .is_some_and(|window| self.profile_deletion_quarantines(window.profile))
        {
            return None;
        }
        let win = self.windows.focused_mut()?;
        let space = win.space;
        let id = ItemId::generate();
        if !self.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ) {
            return None;
        }
        Some((id, self.focus_tab(id)))
    }

    /// Moves window focus to `id`: lifecycle bookkeeping plus a lazy view.
    pub(super) fn focus_tab(&mut self, id: ItemId) -> Vec<Effect> {
        let Some((profile, space)) = self.windows.focused().map(|win| (win.profile, win.space))
        else {
            return Vec::new();
        };
        if !self.item_in_scope(id, profile, space) {
            return Vec::new();
        }
        let Some(win) = self.windows.focused_mut() else {
            return Vec::new();
        };
        let prev = win.active.replace(id).filter(|p| *p != id);
        if let Some(prev) = prev {
            self.items.set_lifecycle(prev, Lifecycle::Inactive);
        }
        self.items.set_lifecycle(id, Lifecycle::Active);
        self.cancel_page_permission_if_not_foreground();
        self.residency.recent.retain(|r| *r != id);
        self.residency.recent.push(id);
        self.touch(id);
        self.items.ensure_view(id)
    }

    pub(super) fn close(&mut self, id: ItemId) -> NativeWork {
        if !self.item_in_focused_scope(id) {
            return NativeWork::default();
        }
        let closed = self.windows.focused().and_then(|window| {
            self.items.tab(id).and_then(|tab| {
                tab.url
                    .as_ref()
                    .map(|url| zephium_core::session::PersistedClosedTab {
                        profile: window.profile,
                        space: window.space,
                        url: url.to_string(),
                        title: tab.title.clone(),
                        zoom: tab.zoom,
                    })
            })
        });
        self.cancel_page_permission_for_item(id);
        self.cancel_pending_presentation(id);
        self.cancel_favicon_attempt(id);
        if matches!(
            self.residency.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { .. })
        ) {
            // Native destruction is already admitted. Clear the logical view
            // before `remove` so it does not issue a second same-id close.
            self.items.mark_view_discarded(id);
            self.residency.discard_probes.remove(&id);
            if let Some(queue) = &self.self_queue {
                queue.cancel_discard_probe(id);
            }
        } else {
            self.cancel_discard_probe(id);
        }
        let Some(win) = self.windows.focused_mut() else {
            return NativeWork::default();
        };
        if let Some(tree) = win.splits.take() {
            win.splits = tree.remove(id);
        }
        let space = win.space;
        let was_active = win.active == Some(id);
        if was_active {
            win.active = None;
        }
        let tabs_before = self.today_tabs(space);
        let pos = tabs_before.iter().position(|x| *x == id);
        let mut fx = self.items.remove(id);
        if let Some(closed) = closed {
            if self.recently_closed.len() == zephium_core::session::MAX_RECENTLY_CLOSED_TABS {
                self.recently_closed.remove(0);
            }
            self.recently_closed.push(closed);
        }
        if was_active {
            let tabs = self.today_tabs(space);
            if let (Some(pos), false) = (pos, tabs.is_empty()) {
                fx.extend(self.focus_tab(tabs[pos.min(tabs.len() - 1)]));
            }
        }
        self.commit(fx)
    }

    /// Restores the newest closed tab owned by the focused profile and space.
    /// A fresh item/native identity is always allocated; the closed record is
    /// removed only after the logical item has been inserted successfully.
    pub(super) fn restore_recently_closed_tab(
        &mut self,
        profile: ProfileId,
    ) -> Option<(ItemId, NativeWork)> {
        let space = self
            .windows
            .focused()
            .filter(|window| window.profile == profile)
            .map(|window| window.space)?;
        let position = self
            .recently_closed
            .iter()
            .rposition(|entry| entry.profile == profile && entry.space == space)?;
        let entry = self.recently_closed[position].clone();
        let id = (0..8).find_map(|_| {
            let candidate = ItemId::generate();
            self.items
                .insert_tab(
                    candidate,
                    Placement::Space {
                        space,
                        section: SpaceSection::Today,
                    },
                )
                .then_some(candidate)
        })?;
        if !self.items.set_committed_url_str(id, &entry.url) {
            let _ = self.items.remove(id);
            return None;
        }
        self.items.set_title(id, entry.title);
        self.items.set_zoom(id, entry.zoom);
        self.recently_closed.remove(position);
        let effects = self.focus_tab(id);
        Some((id, self.commit(effects)))
    }

    /// Removes a failed native leaf from the retained split immediately. A
    /// create failure may arrive after `operation_split`/`apply_drop` has
    /// committed its optimistic topology; retaining that leaf would let a
    /// later single-tab retry silently resurrect the old group.
    pub(super) fn collapse_failed_split_leaf(&mut self, id: ItemId) -> bool {
        let Some((tree, failed_was_active)) = self.windows.focused().and_then(|window| {
            window
                .splits
                .as_ref()
                .filter(|tree| tree.contains(id))
                .cloned()
                .map(|tree| (tree, window.active == Some(id)))
        }) else {
            return false;
        };
        self.divider = None;
        let remaining = tree.remove(id);
        let replacement = failed_was_active
            .then(|| {
                remaining.as_ref().and_then(|tree| {
                    tree.tabs().into_iter().find(|candidate| {
                        self.items.tab(*candidate).is_some_and(TabState::has_view)
                    })
                })
            })
            .flatten();
        if let Some(window) = self.windows.focused_mut() {
            window.splits = remaining;
            if replacement.is_some() {
                // `focus_tab` owns lifecycle/recent bookkeeping below.
                window.active = None;
            }
        }
        if let Some(replacement) = replacement {
            // The replacement was selected from live split leaves, so this
            // normally emits no construction effect. Keep the invariant even
            // if future lifecycle states add another recoverable resident.
            let effects = self.focus_tab(replacement);
            let _ = self.apply(effects);
        }
        true
    }

    pub(super) fn apply_drop(
        &mut self,
        target: ItemId,
        dropped: ItemId,
        edge: Edge,
    ) -> OperationDisposition {
        let Some((profile, space)) = self.windows.focused().map(|win| (win.profile, win.space))
        else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if !self.item_in_scope(target, profile, space)
            || !self.item_in_scope(dropped, profile, space)
        {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if target == dropped {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let Some(mut tree) = self.pane_tree() else {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::LayoutUnavailable,
            );
        };
        if !self.pane_in_scope(&tree, profile, space) || tree.tabs().len() >= MAX_VISIBLE_PANES {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::LayoutUnavailable,
            );
        }
        if tree.contains(dropped) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let effects = self.items.ensure_view(dropped);
        let mut native = self.apply(effects);
        if !self.items.tab(dropped).is_some_and(TabState::has_view) {
            // Match `operation_split`: synchronous native refusal must leave
            // the previously rendered topology authoritative.
            return mutation_result(native);
        }
        self.touch(dropped);
        if !tree.split(target, dropped, edge.axis(), edge.before()) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        if let Some(win) = self.windows.focused_mut() {
            win.splits = Some(tree);
        }
        native.merge(self.commit(Vec::new()));
        mutation_result(native)
    }

    /// `x`/`y` are window coords (the desktop layer normalizes per platform).
    /// window.open / target=_blank lands as a new Today tab next to its
    /// source, routed through the same navigation policy. The split group
    /// survives: the new tab shows alone, the group stays a tab away.
    pub(super) fn open_linked_tab(&mut self, source: ItemId, url: &str) {
        let Some((profile, space)) = self.windows.focused().map(|win| (win.profile, win.space))
        else {
            return;
        };
        if !self.item_in_scope(source, profile, space) {
            return;
        }
        let id = ItemId::generate();
        if !self.items.insert_tab(
            id,
            Placement::Space {
                space,
                section: SpaceSection::Today,
            },
        ) {
            return;
        }
        let mut fx = self.focus_tab(id);
        fx.extend(self.items.navigate(id, url));
        self.commit(fx);
    }
}
