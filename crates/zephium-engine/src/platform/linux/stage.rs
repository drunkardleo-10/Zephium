//! Gtk mirror of the macOS ContentStage. Content webviews live in the
//! composition root's gtk::Fixed with the chrome webview beneath them, so
//! pane gaps show the chrome background. Divider drags are DOM strips in the
//! chrome; the drop indicator is a popup toplevel painted with cairo.
//! Corner rounding is deferred to the UI phase (needs the theme color
//! channel); gaps stay square on Linux until then.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{cairo, glib::Propagation};
use wry::WebViewExtUnix;

use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::split::{self, Pane};

const INDICATOR_RADIUS: f64 = 10.0;
const INDICATOR_BORDER: f64 = 1.5;
const INDICATOR_FILL: f64 = 0.12;
const INDICATOR_STROKE: f64 = 0.42;

struct State {
    fixed: gtk::Fixed,
    gap: f64,
    origin: (f64, f64),
    size: (f64, f64),
    hidden: bool,
    tree: Option<Pane>,
    views: HashMap<ItemId, webkit2gtk::WebView>,
    visible: Vec<ItemId>,
    indicator: Option<gtk::Window>,
    revision: u64,
}

#[derive(Clone)]
pub struct Stage {
    state: Rc<RefCell<State>>,
}

impl Stage {
    pub fn new(fixed: gtk::Fixed, gap: f64) -> Self {
        Self {
            state: Rc::new(RefCell::new(State {
                fixed,
                gap,
                origin: (0.0, 0.0),
                size: (0.0, 0.0),
                hidden: true,
                tree: None,
                views: HashMap::new(),
                visible: Vec::new(),
                indicator: None,
                revision: 0,
            })),
        }
    }

    /// One native pass for frame, tree and visibility; `None` region hides
    /// the whole stage.
    pub fn apply(&self, region: Option<Rect>, tree: Option<Pane>, visible: &[ItemId]) {
        {
            let Ok(mut s) = self.state.try_borrow_mut() else {
                return;
            };
            s.hidden = region.is_none();
            if let Some(r) = region {
                s.origin = (r.x, r.y);
                s.size = (r.width, r.height);
            }
            s.tree = tree;
            s.visible = visible.to_vec();
            s.revision = s.revision.wrapping_add(1).max(1);
        }
        sync(&self.state);
    }

    pub fn has_view(&self, id: ItemId) -> bool {
        self.state
            .try_borrow()
            .is_ok_and(|state| state.views.contains_key(&id))
    }

    pub fn insert_view(&self, id: ItemId, view: &wry::WebView) {
        let widget = view.webview();
        if let Ok(mut state) = self.state.try_borrow_mut() {
            state.views.insert(id, widget);
            state.revision = state.revision.wrapping_add(1).max(1);
        }
    }

    pub fn remove_view(&self, id: ItemId) {
        // wry owns the widget; dropping the webview removes it from the Fixed.
        if let Ok(mut state) = self.state.try_borrow_mut() {
            if state.views.remove(&id).is_some() {
                state.revision = state.revision.wrapping_add(1).max(1);
            }
        }
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        match zone {
            None => {
                let popup = self
                    .state
                    .try_borrow_mut()
                    .ok()
                    .and_then(|mut state| state.indicator.take());
                if let Some(popup) = popup {
                    popup.close();
                }
            }
            Some(zone) => {
                let Some((fixed, origin, existing)) = self
                    .state
                    .try_borrow()
                    .ok()
                    .map(|state| (state.fixed.clone(), state.origin, state.indicator.clone()))
                else {
                    return;
                };
                let popup = if let Some(popup) = existing {
                    popup
                } else {
                    let candidate = make_indicator(&fixed);
                    let Ok(mut state) = self.state.try_borrow_mut() else {
                        candidate.close();
                        return;
                    };
                    if let Some(installed) = state.indicator.as_ref() {
                        let installed = installed.clone();
                        drop(state);
                        candidate.close();
                        installed
                    } else {
                        state.indicator = Some(candidate.clone());
                        candidate
                    }
                };
                let (ox, oy) = fixed
                    .window()
                    .map(|w| {
                        let (_, x, y) = w.origin();
                        (x, y)
                    })
                    .unwrap_or((0, 0));
                popup.move_(
                    ox + (origin.0 + zone.x) as i32,
                    oy + (origin.1 + zone.y) as i32,
                );
                popup.resize(zone.width.max(1.0) as i32, zone.height.max(1.0) as i32);
                popup.show_all();
                popup.queue_draw();
            }
        }
    }
}

fn sync(state: &Rc<RefCell<State>>) {
    // GTK geometry/visibility calls may synchronously pump callbacks. Clone
    // GObject handles and immutable model data first, then retry from the
    // latest revision if re-entry changed ownership while this pass ran.
    for _ in 0..4 {
        let Some((revision, fixed, gap, origin, size, hidden, tree, views, visible)) =
            state.try_borrow().ok().map(|state| {
                (
                    state.revision,
                    state.fixed.clone(),
                    state.gap,
                    state.origin,
                    state.size,
                    state.hidden,
                    state.tree.clone(),
                    state
                        .views
                        .iter()
                        .map(|(id, view)| (*id, view.clone()))
                        .collect::<Vec<_>>(),
                    state.visible.clone(),
                )
            })
        else {
            return;
        };
        let local = Rect::new(0.0, 0.0, size.0, size.1);
        let panes = tree
            .as_ref()
            .map_or_else(Vec::new, |tree| split::layout(tree, local, gap));
        for (id, view) in views {
            let pane = panes.iter().find(|(pid, _)| *pid == id).map(|(_, r)| *r);
            let show = !hidden && pane.is_some() && visible.contains(&id);
            if let Some(r) = pane {
                fixed.move_(&view, (origin.0 + r.x) as i32, (origin.1 + r.y) as i32);
                view.set_size_request(r.width.max(1.0) as i32, r.height.max(1.0) as i32);
            }
            if show == view.is_visible() {
                continue;
            }
            if show {
                // wry's own visibility path is show_all; a remapped webkitgtk
                // view keeps a stale compositing surface until forced to relayout
                view.show_all();
                view.queue_resize();
                view.queue_draw();
            } else {
                view.hide();
            }
        }
        if state
            .try_borrow()
            .is_ok_and(|state| state.revision == revision)
        {
            return;
        }
    }
}

// A popup toplevel, not a child widget: gtk child windows do not alpha-blend
// over sibling native windows, the compositor blends toplevels.
fn make_indicator(fixed: &gtk::Fixed) -> gtk::Window {
    let popup = gtk::Window::new(gtk::WindowType::Popup);
    popup.set_app_paintable(true);
    popup.set_accept_focus(false);
    if let Some(screen) = WidgetExt::screen(fixed) {
        popup.set_visual(screen.rgba_visual().as_ref());
    }
    if let Some(top) = fixed
        .toplevel()
        .and_then(|t| t.downcast::<gtk::Window>().ok())
    {
        popup.set_transient_for(Some(&top));
    }
    popup.connect_draw(|area, cr| {
        let w = area.allocated_width() as f64;
        let h = area.allocated_height() as f64;
        let _ = cr.save();
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        let _ = cr.restore();
        let inset = INDICATOR_BORDER / 2.0;
        rounded_rect(
            cr,
            inset,
            inset,
            w - INDICATOR_BORDER,
            h - INDICATOR_BORDER,
            INDICATOR_RADIUS,
        );
        cr.set_source_rgba(1.0, 1.0, 1.0, INDICATOR_FILL);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(1.0, 1.0, 1.0, INDICATOR_STROKE);
        cr.set_line_width(INDICATOR_BORDER);
        let _ = cr.stroke();
        Propagation::Proceed
    });
    popup
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let (right, bottom) = (x + w, y + h);
    cr.new_sub_path();
    cr.arc(right - r, y + r, r, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(right - r, bottom - r, r, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(
        x + r,
        bottom - r,
        r,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}
