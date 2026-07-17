//! Accepted user-operation interpretation and completion disposition.

use super::*;

impl Shell {
    pub(super) fn handle_operation(&mut self, command: Command) -> OperationDisposition {
        match command {
            Command::Open => self.operation_open(),
            Command::Activate(id) => self.operation_activate(id),
            Command::Close(id) => self.operation_close(id),
            Command::Navigate { id, input } => self.operation_navigate(id, input),
            Command::Reload(id) => self.operation_reload(id),
            Command::GoBack(id) => self.operation_history(id, false),
            Command::GoForward(id) => self.operation_history(id, true),
            Command::SplitWith { other, axis } => self.operation_split(other, axis),
            Command::Unsplit => self.operation_unsplit(),
            Command::DropTab { id, x, y } => self.operation_drop_tab(id, x, y),
            Command::DividerRelease { x, y } => self.operation_divider_release(x.zip(y)),
            Command::Run(id) => self.operation_run_command(&id),
            Command::OpenUrl(input) => self.operation_open_url(input),
            Command::SetAppSetting { key, value } => self.operation_set_app_setting(key, value),
            _ => operation_result(
                OperationOutcome::Rejected,
                OperationReason::UnsupportedCommand,
            ),
        }
    }

    pub(super) fn operation_open(&mut self) -> OperationDisposition {
        if self.windows.focused().is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        }
        let Some((_id, effects)) = self.open_tab_with_id() else {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::ItemLimitReached,
            );
        };
        mutation_result(self.commit(effects))
    }

    pub(super) fn operation_open_url(&mut self, input: String) -> OperationDisposition {
        if navigation::classify(&input).is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        if self.windows.focused().is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        }
        let Some((id, mut effects)) = self.open_tab_with_id() else {
            // Reaching the bounded item limit must not repurpose and navigate
            // the caller's existing active tab.
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::ItemLimitReached,
            );
        };
        effects.extend(self.items.navigate(id, &input));
        mutation_result(self.commit(effects))
    }

    pub(super) fn operation_activate(&mut self, id: ItemId) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let active = self.windows.focused().and_then(|window| window.active);
        let has_view = self.items.tab(id).is_some_and(TabState::has_view);
        let discard_closing = matches!(
            self.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { .. })
        );
        if active == Some(id) && has_view && !discard_closing {
            self.touch(id);
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let effects = self.focus_tab(id);
        let native = self.commit(effects);
        if discard_closing {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    pub(super) fn operation_close(&mut self, id: ItemId) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let discard_closing = matches!(
            self.discard_probes.get(&id),
            Some(PendingDiscardProbe::Closing { .. })
        );
        let native = self.close(id);
        if discard_closing && !native.rejected {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    pub(super) fn operation_navigate(&mut self, id: ItemId, input: String) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if navigation::classify(&input).is_none() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        if let Some(PendingDiscardProbe::Closing {
            recreate,
            deferred_navigation,
            ..
        }) = self.discard_probes.get_mut(&id)
        {
            *recreate = true;
            *deferred_navigation = Some(input);
            return operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            );
        }
        self.cancel_discard_probe(id);
        let effects = self.items.navigate(id, &input);
        if effects.is_empty() {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        mutation_result(self.commit(effects))
    }

    pub(super) fn operation_reload(&mut self, id: ItemId) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if self.recreate_after_inflight_discard(id) {
            return operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            );
        }
        self.cancel_discard_probe(id);
        if !self.items.tab(id).is_some_and(TabState::has_view) {
            let effects = self.items.ensure_view(id);
            if effects.is_empty() {
                return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
            }
            return mutation_result(self.commit(effects));
        }
        let mut native = NativeWork::default();
        native.record(self.engine.reload(id));
        mutation_result(native)
    }

    pub(super) fn operation_history(&mut self, id: ItemId, forward: bool) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let Some(tab) = self.items.tab(id) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        let available = if forward {
            tab.can_go_forward
        } else {
            tab.can_go_back
        };
        if !available || !tab.has_view() {
            return operation_result(OperationOutcome::NoOp, OperationReason::HistoryUnavailable);
        }
        if self.recreate_after_inflight_discard(id) {
            // Native back/forward history belongs to the controller that is
            // already closing and cannot be reconstructed by URL alone.
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::HistoryUnavailable,
            );
        }
        self.cancel_discard_probe(id);
        let admission = if forward {
            self.engine.go_forward(id)
        } else {
            self.engine.go_back(id)
        };
        let mut native = NativeWork::default();
        native.record(admission);
        mutation_result(native)
    }

    pub(super) fn operation_split(&mut self, other: ItemId, axis: Axis) -> OperationDisposition {
        let Some((active, profile, space)) = self
            .windows
            .focused()
            .and_then(|win| win.active.map(|active| (active, win.profile, win.space)))
        else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if !self.item_in_scope(active, profile, space) || !self.item_in_scope(other, profile, space)
        {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        if active == other {
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
        if tree.contains(other) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let closing = [active, other].into_iter().any(|id| {
            matches!(
                self.discard_probes.get(&id),
                Some(PendingDiscardProbe::Closing { .. })
            )
        });
        let effects = self.items.ensure_view(other);
        let mut native = self.apply(effects);
        if !self.items.tab(other).is_some_and(TabState::has_view) {
            // A synchronous create-dispatch refusal already rolled back the
            // optimistic view bit. Do not install a split whose new leaf can
            // never be represented by the native layout admitted below.
            return mutation_result(native);
        }
        self.touch(other);
        if !tree.split(active, other, axis, false) {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        if let Some(win) = self.windows.focused_mut() {
            win.splits = Some(tree);
        }
        native.merge(self.commit(Vec::new()));
        if closing && !native.rejected {
            operation_result(
                OperationOutcome::Deferred,
                OperationReason::DiscardCompletionPending,
            )
        } else {
            mutation_result(native)
        }
    }

    pub(super) fn operation_unsplit(&mut self) -> OperationDisposition {
        let Some(win) = self.windows.focused_mut() else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        if win.splits.take().is_none() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        mutation_result(self.commit(Vec::new()))
    }

    pub(super) fn operation_drop_tab(
        &mut self,
        id: ItemId,
        x: f64,
        y: f64,
    ) -> OperationDisposition {
        if !self.item_in_focused_scope(id) {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        }
        let Some(window) = self.windows.focused().map(|window| window.id) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::NoFocusedWindow);
        };
        let Some(drop) = self.resolve_drop(x, y) else {
            let _ = self.engine.set_drop_indicator(window, None);
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        };
        let result = self.apply_drop(drop.tab, id, drop.edge);
        let _ = self.engine.set_drop_indicator(window, None);
        result
    }

    pub(super) fn operation_divider_release(
        &mut self,
        final_pointer: Option<(f64, f64)>,
    ) -> OperationDisposition {
        if self.divider.is_none() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        if let Some((x, y)) = final_pointer {
            self.divider_drag(x, y);
        }
        if self.divider.take().is_none() {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        self.schedule_persist();
        operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
    }

    pub(super) fn operation_set_app_setting(
        &mut self,
        key: String,
        value: String,
    ) -> OperationDisposition {
        if key != "appearance" || !matches!(value.as_str(), "system" | "light" | "dark") {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidInput);
        }
        if !self.store.set_app_setting(key, value.clone()) {
            return operation_result(
                OperationOutcome::Rejected,
                OperationReason::StoreAdmissionRejected,
            );
        }
        // This projection is downstream of truthful store-queue admission.
        // The desktop composition root applies native theme state from this
        // signal, never optimistically from the IPC request itself.
        (self.emit)(Projection::UiCommand(format!("theme.{value}")));
        operation_result(
            OperationOutcome::Deferred,
            OperationReason::StoreWorkPending,
        )
    }

    pub(super) fn operation_run_command(&mut self, id: &str) -> OperationDisposition {
        let active = self.windows.focused().and_then(|window| window.active);
        match id {
            "tab.new" => self.operation_open(),
            "tab.close" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_close(id),
            ),
            "tab.next" => self.operation_cycle_tab(1),
            "tab.previous" => self.operation_cycle_tab(-1),
            "nav.back" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_history(id, false),
            ),
            "nav.forward" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_history(id, true),
            ),
            "nav.reload" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| self.operation_reload(id),
            ),
            "nav.stop" => active.map_or_else(
                || operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow),
                |id| {
                    let mut native = NativeWork::default();
                    native.record(self.engine.stop(id));
                    mutation_result(native)
                },
            ),
            "zoom.in" => self.operation_adjust_zoom(Some(0.1)),
            "zoom.out" => self.operation_adjust_zoom(Some(-0.1)),
            "zoom.reset" => self.operation_adjust_zoom(None),
            "url.focus" => {
                (self.emit)(Projection::UiCommand("url.focus".into()));
                operation_result(OperationOutcome::Applied, OperationReason::MutationApplied)
            }
            _ => operation_result(
                OperationOutcome::Rejected,
                OperationReason::UnsupportedCommand,
            ),
        }
    }

    fn operation_cycle_tab(&mut self, step: isize) -> OperationDisposition {
        let Some(win) = self.windows.focused() else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let Some(active) = win.active else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let tabs = self.today_tabs(win.space);
        let Some(position) = tabs.iter().position(|id| *id == active) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        if tabs.len() < 2 {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        let next = (position as isize + step).rem_euclid(tabs.len() as isize) as usize;
        self.operation_activate(tabs[next])
    }

    fn operation_adjust_zoom(&mut self, delta: Option<f64>) -> OperationDisposition {
        let Some(active) = self.windows.focused().and_then(|window| window.active) else {
            return operation_result(OperationOutcome::NoOp, OperationReason::NoFocusedWindow);
        };
        let Some(settled) = self.items.tab(active).map(|tab| tab.zoom) else {
            return operation_result(OperationOutcome::Rejected, OperationReason::InvalidScope);
        };
        let current = self
            .zoom
            .pending
            .get(&active)
            .map_or(settled, |pending| pending.desired_scale);
        let zoom = match delta {
            Some(delta) => (current + delta).clamp(0.3, 3.0),
            None => 1.0,
        };
        if (zoom - current).abs() < f64::EPSILON {
            return operation_result(OperationOutcome::NoOp, OperationReason::StateUnchanged);
        }
        // `Items.zoom` is durable authoritative state. Keep the responsive
        // desired value in a separate bounded map until the exact native view
        // generation reports what it actually applied; otherwise any
        // unrelated session save could persist an optimistic lie.
        let admission = self.request_zoom(active, zoom);
        let mut native = NativeWork::default();
        native.record(admission);
        mutation_result(native)
    }
}
