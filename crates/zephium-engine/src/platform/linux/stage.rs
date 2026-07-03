//! Gtk mirror of the macOS ContentStage. Content webviews live in the
//! composition root's gtk::Fixed with the chrome webview beneath them, so
//! pane gaps show the chrome background. Divider drags run on invisible
//! input-only EventBoxes over the gaps; the drop indicator is a DrawingArea
//! painted with cairo to match the macOS one.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{cairo, gdk, glib::Propagation};
use wry::WebViewExtUnix;

use zephium_core::geometry::Rect;
use zephium_core::ids::{ItemId, WindowId};
use zephium_core::ports::engine::EngineEvent;
use zephium_core::split::{self, Axis, Divider, Pane};

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
    strips: Vec<gtk::EventBox>,
    dividers: Vec<Divider>,
    drag: Option<Divider>,
    indicator: Option<gtk::DrawingArea>,
    on_ratio: Box<dyn Fn(Pane)>,
}

#[derive(Clone)]
pub struct Stage {
    state: Rc<RefCell<State>>,
}

impl Stage {
    pub fn new(
        fixed: gtk::Fixed,
        gap: f64,
        window: WindowId,
        sink: Arc<dyn Fn(EngineEvent) + Send + Sync>,
    ) -> Self {
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
                strips: Vec::new(),
                dividers: Vec::new(),
                drag: None,
                indicator: None,
                on_ratio: Box::new(move |tree| {
                    sink(EngineEvent::SplitChanged { window, tree });
                }),
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
        self.state.borrow_mut().views.remove(&id);
    }

    pub fn set_visible(&self, visible: &[ItemId]) {
        self.state.borrow_mut().visible = visible.to_vec();
        sync(&self.state);
    }

    pub fn set_drop_indicator(&self, zone: Option<Rect>) {
        let mut s = self.state.borrow_mut();
        match zone {
            None => {
                if let Some(area) = s.indicator.take() {
                    s.fixed.remove(&area);
                }
            }
            Some(zone) => {
                let area = match &s.indicator {
                    Some(area) => area.clone(),
                    None => {
                        let area = make_indicator(&s.fixed);
                        s.indicator = Some(area.clone());
                        area
                    }
                };
                let x = (s.origin.0 + zone.x) as i32;
                let y = (s.origin.1 + zone.y) as i32;
                s.fixed.move_(&area, x, y);
                area.set_size_request(zone.width as i32, zone.height as i32);
                area.show();
                if let Some(win) = area.window() {
                    win.raise();
                }
                area.queue_draw();
            }
        }
    }
}

fn sync(state: &Rc<RefCell<State>>) {
    let mut s = state.borrow_mut();
    let local = Rect::new(0.0, 0.0, s.size.0, s.size.1);
    let (panes, dividers) = match &s.tree {
        Some(tree) => (
            split::layout(tree, local, s.gap),
            split::dividers(tree, local, s.gap),
        ),
        None => (Vec::new(), Vec::new()),
    };

    while s.strips.len() < dividers.len() {
        let strip = make_strip(state, s.strips.len(), &s.fixed);
        s.strips.push(strip);
    }
    while s.strips.len() > dividers.len() {
        if let Some(strip) = s.strips.pop() {
            s.fixed.remove(&strip);
        }
    }
    s.dividers = dividers;

    let origin = s.origin;
    for (id, view) in &s.views {
        let pane = panes.iter().find(|(pid, _)| pid == id).map(|(_, r)| r);
        let show = !s.hidden && pane.is_some() && s.visible.contains(id);
        if let Some(r) = pane {
            s.fixed
                .move_(view, (origin.0 + r.x) as i32, (origin.1 + r.y) as i32);
            view.set_size_request(r.width as i32, r.height as i32);
        }
        view.set_visible(show);
    }
    for (strip, divider) in s.strips.iter().zip(s.dividers.iter()) {
        let r = &divider.strip;
        s.fixed
            .move_(strip, (origin.0 + r.x) as i32, (origin.1 + r.y) as i32);
        strip.set_size_request(r.width as i32, r.height as i32);
        strip.set_visible(!s.hidden);
    }
}

fn resize_cursor(fixed: &gtk::Fixed, axis: Option<Axis>) {
    let Some(win) = fixed.window() else {
        return;
    };
    let cursor = axis.and_then(|axis| {
        let name = match axis {
            Axis::Row => "ew-resize",
            Axis::Col => "ns-resize",
        };
        gdk::Cursor::from_name(&win.display(), name)
    });
    win.set_cursor(cursor.as_ref());
}

fn make_strip(state: &Rc<RefCell<State>>, index: usize, fixed: &gtk::Fixed) -> gtk::EventBox {
    let strip = gtk::EventBox::new();
    strip.set_visible_window(false);
    strip.add_events(
        gdk::EventMask::BUTTON_PRESS_MASK
            | gdk::EventMask::BUTTON_RELEASE_MASK
            | gdk::EventMask::POINTER_MOTION_MASK
            | gdk::EventMask::ENTER_NOTIFY_MASK
            | gdk::EventMask::LEAVE_NOTIFY_MASK,
    );

    let st = state.clone();
    strip.connect_enter_notify_event(move |_, _| {
        let s = st.borrow();
        resize_cursor(&s.fixed, s.dividers.get(index).map(|d| d.axis));
        Propagation::Stop
    });
    let st = state.clone();
    strip.connect_leave_notify_event(move |_, _| {
        let s = st.borrow();
        if s.drag.is_none() {
            resize_cursor(&s.fixed, None);
        }
        Propagation::Stop
    });
    let st = state.clone();
    strip.connect_button_press_event(move |_, event| {
        if event.button() != 1 {
            return Propagation::Proceed;
        }
        let mut s = st.borrow_mut();
        s.drag = s.dividers.get(index).cloned();
        Propagation::Stop
    });
    let st = state.clone();
    strip.connect_motion_notify_event(move |_, event| {
        let (drag, strip_rect, gap) = {
            let s = st.borrow();
            let Some(drag) = s.drag.clone() else {
                return Propagation::Proceed;
            };
            let Some(current) = s.dividers.get(index) else {
                return Propagation::Proceed;
            };
            (drag, current.strip, s.gap)
        };
        // The event window follows the strip, so its position plus the local
        // event offset is a stage-local point even while both are moving.
        let (ex, ey) = event.position();
        let ratio = split::ratio_for(
            drag.axis,
            drag.rect,
            gap,
            strip_rect.x + ex,
            strip_rect.y + ey,
        );
        {
            let mut s = st.borrow_mut();
            match s.tree.as_mut() {
                Some(tree) => tree.set_ratio(&drag.path, ratio),
                None => return Propagation::Proceed,
            }
        }
        sync(&st);
        Propagation::Stop
    });
    let st = state.clone();
    strip.connect_button_release_event(move |_, _| {
        let tree = {
            let mut s = st.borrow_mut();
            if s.drag.take().is_none() {
                return Propagation::Proceed;
            }
            s.tree.clone()
        };
        if let Some(tree) = tree {
            (st.borrow().on_ratio)(tree);
        }
        Propagation::Stop
    });

    fixed.put(&strip, 0, 0);
    strip
}

fn make_indicator(fixed: &gtk::Fixed) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_app_paintable(true);
    if let Some(screen) = WidgetExt::screen(fixed) {
        area.set_visual(screen.rgba_visual().as_ref());
    }
    area.connect_draw(|area, cr| {
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
    fixed.put(&area, 0, 0);
    area
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
