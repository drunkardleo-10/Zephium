//! Gtk mirror of the macOS ContentStage. Content webviews live in the
//! composition root's gtk::Fixed with the chrome webview beneath them, so
//! pane gaps show the chrome background. Divider drags are DOM strips in the
//! chrome; the drop indicator is a DrawingArea painted with cairo.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{cairo, glib::Propagation};
use wry::WebViewExtUnix;

use zephium_core::geometry::Rect;
use zephium_core::ids::ItemId;
use zephium_core::split::{self, Pane};

const RADIUS: i32 = 12;
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
    rings: HashMap<ItemId, gtk::DrawingArea>,
    visible: Vec<ItemId>,
    indicator: Option<gtk::Window>,
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
                rings: HashMap::new(),
                visible: Vec::new(),
                indicator: None,
            })),
        }
    }

    pub fn set_hidden(&self, hidden: bool) {
        self.state.borrow_mut().hidden = hidden;
        sync(&self.state);
    }

    pub fn set_frame(&self, rect: Rect) {
        {
            let mut s = self.state.borrow_mut();
            s.origin = (rect.x, rect.y);
            s.size = (rect.width, rect.height);
        }
        sync(&self.state);
    }

    pub fn set_tree(&self, tree: Option<Pane>) {
        self.state.borrow_mut().tree = tree;
        sync(&self.state);
    }

    pub fn has_view(&self, id: ItemId) -> bool {
        self.state.borrow().views.contains_key(&id)
    }

    pub fn insert_view(&self, id: ItemId, view: &wry::WebView) {
        self.state.borrow_mut().views.insert(id, view.webview());
    }

    pub fn remove_view(&self, id: ItemId) {
        // wry owns the widget; dropping the webview removes it from the Fixed.
        let mut s = self.state.borrow_mut();
        s.views.remove(&id);
        if let Some(ring) = s.rings.remove(&id) {
            s.fixed.remove(&ring);
        }
    }

    pub fn set_visible(&self, visible: &[ItemId]) {
        self.state.borrow_mut().visible = visible.to_vec();
        sync(&self.state);
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        let mut s = self.state.borrow_mut();
        match zone {
            None => {
                if let Some(popup) = s.indicator.take() {
                    popup.close();
                }
            }
            Some(zone) => {
                let popup = match &s.indicator {
                    Some(popup) => popup.clone(),
                    None => {
                        let popup = make_indicator(&s.fixed);
                        s.indicator = Some(popup.clone());
                        popup
                    }
                };
                let (ox, oy) = s
                    .fixed
                    .window()
                    .map(|w| {
                        let (_, x, y) = w.origin();
                        (x, y)
                    })
                    .unwrap_or((0, 0));
                popup.move_(
                    ox + (s.origin.0 + zone.x) as i32,
                    oy + (s.origin.1 + zone.y) as i32,
                );
                popup.resize(zone.width.max(1.0) as i32, zone.height.max(1.0) as i32);
                popup.show_all();
                popup.queue_draw();
            }
        }
    }
}

fn sync(state: &Rc<RefCell<State>>) {
    let mut s = state.borrow_mut();
    let local = Rect::new(0.0, 0.0, s.size.0, s.size.1);
    let panes = match &s.tree {
        Some(tree) => split::layout(tree, local, s.gap),
        None => Vec::new(),
    };
    let origin = s.origin;
    let ids: Vec<ItemId> = s.views.keys().copied().collect();
    for id in ids {
        let view = s.views[&id].clone();
        let pane = panes.iter().find(|(pid, _)| *pid == id).map(|(_, r)| *r);
        let show = !s.hidden && pane.is_some() && s.visible.contains(&id);
        view.set_visible(show);
        let ring = s.rings.get(&id).cloned();
        if let Some(r) = pane {
            let (x, y) = ((origin.0 + r.x) as i32, (origin.1 + r.y) as i32);
            let (w, h) = (r.width.max(1.0) as i32, r.height.max(1.0) as i32);
            s.fixed.move_(&view, x, y);
            view.set_size_request(w, h);
            let ring = ring.unwrap_or_else(|| {
                let ring = make_ring(&s.fixed);
                s.rings.insert(id, ring.clone());
                ring
            });
            s.fixed.move_(&ring, x, y);
            ring.set_size_request(w, h);
            ring.shape_combine_region(Some(&corner_region(w, h)));
            ring.set_visible(show);
        } else if let Some(ring) = ring {
            ring.set_visible(false);
        }
    }
}

// Content views cannot be clipped in gtk3; an opaque background-colored
// widget shaped to the four corner slivers fakes the rounding. Alpha over a
// native sibling never composites, a shaped window needs none.
fn make_ring(fixed: &gtk::Fixed) -> gtk::DrawingArea {
    let ring = gtk::DrawingArea::new();
    ring.connect_draw(|_, cr| {
        cr.set_source_rgb(27.0 / 255.0, 27.0 / 255.0, 31.0 / 255.0);
        let _ = cr.paint();
        Propagation::Proceed
    });
    ring.input_shape_combine_region(Some(&cairo::Region::create()));
    fixed.put(&ring, 0, 0);
    ring
}

fn corner_region(w: i32, h: i32) -> cairo::Region {
    let region = cairo::Region::create();
    let r = RADIUS.min(w / 2).min(h / 2).max(1);
    let rf = r as f64;
    // quarter-disc centers in each corner square's local coordinates
    let corners = [
        (0, 0, rf, rf),
        (w - r, 0, 0.0, rf),
        (0, h - r, rf, 0.0),
        (w - r, h - r, 0.0, 0.0),
    ];
    for (x, y, cx, cy) in corners {
        let Ok(surface) = cairo::ImageSurface::create(cairo::Format::A1, r, r) else {
            continue;
        };
        {
            let Ok(cr) = cairo::Context::new(&surface) else {
                continue;
            };
            let _ = cr.paint();
            cr.set_operator(cairo::Operator::Clear);
            cr.arc(cx, cy, rf, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }
        surface.flush();
        let Some(sliver) = gtk::gdk::prelude::GdkSurfaceExt::create_region(&*surface) else {
            continue;
        };
        sliver.translate(x, y);
        let _ = region.union(&sliver);
    }
    region
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
