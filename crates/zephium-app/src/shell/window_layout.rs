//! Authoritative window geometry, split layout, and divider capture.

use super::*;

#[derive(Clone, Debug)]
pub(super) struct GrabbedDivider {
    window: WindowId,
    topology: Pane,
    divider: split::Divider,
}

impl Shell {
    pub(super) fn resolve_drop(&self, x: f64, y: f64) -> Option<split::Drop> {
        let win = self.windows.focused()?;
        let tree = self.pane_tree()?;
        let region =
            layout::compute(win.size, win.mode, win.metrics, self.present(&tree)).content?;
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        split::drop_target(&tree, local, win.metrics.gap, x - region.x, y - region.y)
    }

    pub(super) fn relayout(&self) -> NativeDispatch {
        let Some(win) = self.windows.focused() else {
            return NativeDispatch::Rejected;
        };
        let tree = self.pane_tree();
        let present = tree.as_ref().is_some_and(|t| self.present(t));
        let mut l = layout::compute(win.size, win.mode, win.metrics, present);
        if !self.window_visible {
            l.content = None;
        }
        // Raw native children still receive their final geometry while a
        // first navigation is provisional, but macOS must not shrink the
        // privileged chrome away from a fresh New Tab surface until at least
        // one visible leaf completed its exact chrome-verification transition.
        // The content stage itself is transparent and presentation-gated, so
        // it can sit above this real UI without painting an artificial box.
        let chrome_present = self.window_visible
            && tree.as_ref().is_some_and(|tree| {
                tree.tabs().iter().any(|id| {
                    self.items.tab(*id).is_some_and(|tab| {
                        tab.has_view()
                            && tab.url.is_some()
                            && !self.deferred_first_content_layout.contains(id)
                    })
                })
            });
        let chrome_layout = layout::compute(win.size, win.mode, win.metrics, chrome_present);
        if !self.chrome.position(ChromeFrame {
            rect: chrome_layout.chrome,
            fill_width: chrome_layout.content.is_none(),
        }) {
            return NativeDispatch::Rejected;
        }
        let dividers = match (&tree, l.content) {
            (Some(tree), Some(region)) => {
                let local = Rect::new(0.0, 0.0, region.width, region.height);
                split::dividers(tree, local, win.metrics.gap)
                    .into_iter()
                    .map(|d| DividerView {
                        x: region.x + d.strip.x,
                        y: region.y + d.strip.y,
                        width: d.strip.width,
                        height: d.strip.height,
                        vertical: d.axis == Axis::Row,
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        (self.emit)(Projection::Layout(LayoutState { dividers }));
        self.engine.set_content(win.id, tree, l.content)
    }

    pub(super) fn locate_divider(&self, x: f64, y: f64) -> Option<GrabbedDivider> {
        let win = self.windows.focused()?;
        let tree = self.pane_tree()?;
        let region =
            layout::compute(win.size, win.mode, win.metrics, self.present(&tree)).content?;
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        let divider = split::divider_at(&tree, local, win.metrics.gap, x - region.x, y - region.y)?;
        Some(GrabbedDivider {
            window: win.id,
            topology: tree,
            divider,
        })
    }

    pub(super) fn divider_drag(&mut self, x: f64, y: f64) {
        let Some(grabbed) = self.divider.as_ref() else {
            return;
        };
        let grabbed_window = grabbed.window;
        let grabbed_path = grabbed.divider.path.clone();
        let Some(win) = self.windows.focused() else {
            self.divider = None;
            return;
        };
        let current_tree = self.pane_tree();
        let topology_is_current = win.id == grabbed_window
            && current_tree
                .as_ref()
                .is_some_and(|tree| grabbed.topology.same_topology(tree));
        if !topology_is_current {
            // Focus/topology changed while the pointer was captured. The same
            // binary path may now identify a different live branch.
            self.divider = None;
            return;
        }
        let Some(region) = layout::compute(win.size, win.mode, win.metrics, true).content else {
            return;
        };
        let gap = win.metrics.gap;
        let Some(tree) = current_tree.as_ref() else {
            self.divider = None;
            return;
        };
        let local = Rect::new(0.0, 0.0, region.width, region.height);
        let Some(current) = split::divider_at_path(tree, local, gap, &grabbed_path) else {
            // The split tree changed while the pointer was captured. Its old
            // path is no longer authority for any live branch.
            self.divider = None;
            return;
        };
        let ratio = split::ratio_for(current.axis, current.rect, gap, x - region.x, y - region.y);
        if let Some(win) = self.windows.focused_mut() {
            if let Some(tree) = win.splits.as_mut() {
                tree.set_ratio(&current.path, ratio);
            }
        }
        let _ = self.relayout();
    }

    fn present(&self, tree: &Pane) -> bool {
        tree.tabs()
            .iter()
            .any(|id| self.items.tab(*id).is_some_and(TabState::has_view))
    }

    // The split group persists across tab switches (Arc model): members show
    // the whole group, other tabs show alone, the group is a tab away.
    pub(super) fn pane_tree(&self) -> Option<Pane> {
        let win = self.windows.focused()?;
        let active = win.active?;
        if !self.item_in_scope(active, win.profile, win.space) {
            return None;
        }
        if let Some(tree) = win.splits.clone() {
            if tree.contains(active)
                && self.pane_in_scope(&tree, win.profile, win.space)
                && tree
                    .tabs()
                    .into_iter()
                    .all(|id| self.items.tab(id).is_some_and(TabState::has_view))
            {
                return Some(tree);
            }
        }
        self.items
            .tab(active)
            .is_some_and(TabState::has_view)
            .then_some(Pane::Leaf(active))
    }

    pub(super) fn content_region(&self) -> Rect {
        let Some(win) = self.windows.focused() else {
            return Rect::default();
        };
        layout::compute(win.size, win.mode, win.metrics, true)
            .content
            .unwrap_or_default()
    }
}
