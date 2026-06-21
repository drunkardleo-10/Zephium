//! Split tree: the content region subdivides into panes, each bound to a tab.
//! Pure geometry; the engine shows one content webview per resulting rect.

use crate::geometry::Rect;
use crate::tab::TabId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Col,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pane {
    Leaf(TabId),
    Branch {
        axis: Axis,
        ratio: f64,
        a: Box<Pane>,
        b: Box<Pane>,
    },
}

impl Pane {
    pub fn leaf(id: TabId) -> Self {
        Pane::Leaf(id)
    }

    pub fn tabs(&self) -> Vec<TabId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    pub fn contains(&self, tab: TabId) -> bool {
        match self {
            Pane::Leaf(id) => *id == tab,
            Pane::Branch { a, b, .. } => a.contains(tab) || b.contains(tab),
        }
    }

    pub fn split(&mut self, target: TabId, new: TabId, axis: Axis, before: bool) -> bool {
        match self {
            Pane::Leaf(id) if *id == target => {
                let kept = Pane::Leaf(*id);
                let added = Pane::Leaf(new);
                let (a, b) = if before { (added, kept) } else { (kept, added) };
                *self = Pane::Branch {
                    axis,
                    ratio: 0.5,
                    a: Box::new(a),
                    b: Box::new(b),
                };
                true
            }
            Pane::Leaf(_) => false,
            Pane::Branch { a, b, .. } => {
                a.split(target, new, axis, before) || b.split(target, new, axis, before)
            }
        }
    }

    pub fn set_ratio(&mut self, path: &[usize], ratio: f64) {
        let mut node = self;
        for &step in path {
            match node {
                Pane::Branch { a, b, .. } => node = if step == 0 { a } else { b },
                Pane::Leaf(_) => return,
            }
        }
        if let Pane::Branch { ratio: r, .. } = node {
            *r = ratio.clamp(0.05, 0.95);
        }
    }

    pub fn remove(self, tab: TabId) -> Option<Pane> {
        match self {
            Pane::Leaf(id) if id == tab => None,
            leaf @ Pane::Leaf(_) => Some(leaf),
            Pane::Branch { axis, ratio, a, b } => match (a.remove(tab), b.remove(tab)) {
                (Some(a), Some(b)) => Some(Pane::Branch {
                    axis,
                    ratio,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    fn collect(&self, out: &mut Vec<TabId>) {
        match self {
            Pane::Leaf(id) => out.push(*id),
            Pane::Branch { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }
}

pub fn layout(root: &Pane, region: Rect, gap: f64) -> Vec<(TabId, Rect)> {
    let mut out = Vec::new();
    place(root, region, gap, &mut out);
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct Divider {
    pub path: Vec<usize>,
    pub axis: Axis,
    pub rect: Rect,
}

pub fn divider_at(tree: &Pane, region: Rect, gap: f64, px: f64, py: f64) -> Option<Divider> {
    fn walk(pane: &Pane, rect: Rect, gap: f64, p: (f64, f64), path: &mut Vec<usize>) -> Option<Divider> {
        let Pane::Branch { axis, ratio, a, b } = pane else {
            return None;
        };
        let (ra, rb) = divide(rect, *axis, *ratio, gap);
        let (px, py) = p;
        let on_divider = match axis {
            Axis::Row => {
                px >= ra.x + ra.width && px <= rb.x && py >= rect.y && py <= rect.y + rect.height
            }
            Axis::Col => {
                py >= ra.y + ra.height && py <= rb.y && px >= rect.x && px <= rect.x + rect.width
            }
        };
        if on_divider {
            return Some(Divider {
                path: path.clone(),
                axis: *axis,
                rect,
            });
        }
        path.push(0);
        if let Some(d) = walk(a, ra, gap, p, path) {
            return Some(d);
        }
        path.pop();
        path.push(1);
        let d = walk(b, rb, gap, p, path);
        path.pop();
        d
    }
    walk(tree, region, gap, (px, py), &mut Vec::new())
}

pub fn ratio_for(axis: Axis, rect: Rect, gap: f64, px: f64, py: f64) -> f64 {
    let raw = match axis {
        Axis::Row => (px - rect.x) / (rect.width - gap),
        Axis::Col => (py - rect.y) / (rect.height - gap),
    };
    raw.clamp(0.05, 0.95)
}

fn place(pane: &Pane, rect: Rect, gap: f64, out: &mut Vec<(TabId, Rect)>) {
    match pane {
        Pane::Leaf(id) => out.push((*id, rect)),
        Pane::Branch { axis, ratio, a, b } => {
            let (ra, rb) = divide(rect, *axis, *ratio, gap);
            place(a, ra, gap, out);
            place(b, rb, gap, out);
        }
    }
}

fn divide(r: Rect, axis: Axis, ratio: f64, gap: f64) -> (Rect, Rect) {
    let ratio = ratio.clamp(0.0, 1.0);
    match axis {
        Axis::Row => {
            let avail = (r.width - gap).max(0.0);
            let aw = avail * ratio;
            (
                Rect::new(r.x, r.y, aw, r.height),
                Rect::new(r.x + aw + gap, r.y, avail - aw, r.height),
            )
        }
        Axis::Col => {
            let avail = (r.height - gap).max(0.0);
            let ah = avail * ratio;
            (
                Rect::new(r.x, r.y, r.width, ah),
                Rect::new(r.x, r.y + ah + gap, r.width, avail - ah),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGION: Rect = Rect {
        x: 100.0,
        y: 0.0,
        width: 1000.0,
        height: 800.0,
    };

    #[test]
    fn single_leaf_fills_region() {
        let panes = layout(&Pane::leaf(7), REGION, 8.0);
        assert_eq!(panes, vec![(7, REGION)]);
    }

    #[test]
    fn row_split_halves_width_with_gap() {
        let tree = Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(Pane::leaf(1)),
            b: Box::new(Pane::leaf(2)),
        };
        let panes = layout(&tree, REGION, 8.0);
        // (1000 - 8) / 2 = 496 each, second starts at 100 + 496 + 8.
        assert_eq!(panes[0], (1, Rect::new(100.0, 0.0, 496.0, 800.0)));
        assert_eq!(panes[1], (2, Rect::new(604.0, 0.0, 496.0, 800.0)));
        // no overlap: a.right + gap == b.left
        assert_eq!(panes[0].1.x + panes[0].1.width + 8.0, panes[1].1.x);
    }

    #[test]
    fn col_split_divides_height() {
        let tree = Pane::Branch {
            axis: Axis::Col,
            ratio: 0.25,
            a: Box::new(Pane::leaf(1)),
            b: Box::new(Pane::leaf(2)),
        };
        let panes = layout(&tree, REGION, 8.0);
        // (800 - 8) * 0.25 = 198
        assert_eq!(panes[0].1.height, 198.0);
        assert_eq!(panes[1].1.y, 206.0);
        assert_eq!(panes[1].1.height, 594.0);
    }

    #[test]
    fn nested_split_collects_all_tabs_without_overlap() {
        let tree = Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(Pane::leaf(1)),
            b: Box::new(Pane::Branch {
                axis: Axis::Col,
                ratio: 0.5,
                a: Box::new(Pane::leaf(2)),
                b: Box::new(Pane::leaf(3)),
            }),
        };
        assert_eq!(tree.tabs(), vec![1, 2, 3]);
        let panes = layout(&tree, REGION, 8.0);
        assert_eq!(panes.len(), 3);
        let right = &panes[1].1;
        let bottom = &panes[2].1;
        // right column's two panes share x/width, stacked with the gap.
        assert_eq!(right.x, bottom.x);
        assert_eq!(right.width, bottom.width);
        assert_eq!(right.y + right.height + 8.0, bottom.y);
    }

    #[test]
    fn split_replaces_target_leaf_with_branch() {
        let mut tree = Pane::leaf(1);
        assert!(tree.split(1, 2, Axis::Row, false));
        assert_eq!(tree.tabs(), vec![1, 2]);

        // split pane 2 vertically, new tab above it
        assert!(tree.split(2, 3, Axis::Col, true));
        assert_eq!(tree.tabs(), vec![1, 3, 2]);

        // unknown target leaves the tree untouched
        let before = tree.clone();
        assert!(!tree.split(99, 4, Axis::Row, false));
        assert_eq!(tree, before);
    }

    #[test]
    fn remove_collapses_branch_into_sibling() {
        let mut tree = Pane::leaf(1);
        tree.split(1, 2, Axis::Row, false);
        tree.split(2, 3, Axis::Col, false);
        assert_eq!(tree.tabs(), vec![1, 2, 3]);

        let tree = tree.remove(2).unwrap();
        assert_eq!(tree.tabs(), vec![1, 3]);
        let tree = tree.remove(1).unwrap();
        assert_eq!(tree, Pane::leaf(3));
        assert!(tree.remove(3).is_none());
    }

    fn nested() -> Pane {
        Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(Pane::leaf(1)),
            b: Box::new(Pane::Branch {
                axis: Axis::Col,
                ratio: 0.5,
                a: Box::new(Pane::leaf(2)),
                b: Box::new(Pane::leaf(3)),
            }),
        }
    }

    #[test]
    fn divider_at_finds_row_split_and_descends() {
        let region = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let outer = divider_at(&nested(), region, 8.0, 500.0, 400.0).unwrap();
        assert_eq!(outer.path, Vec::<usize>::new());
        assert_eq!(outer.axis, Axis::Row);

        let inner = divider_at(&nested(), region, 8.0, 700.0, 400.0).unwrap();
        assert_eq!(inner.path, vec![1]);
        assert_eq!(inner.axis, Axis::Col);

        assert!(divider_at(&nested(), region, 8.0, 100.0, 400.0).is_none());
    }

    #[test]
    fn ratio_for_maps_position_and_clamps() {
        let rect = Rect::new(0.0, 0.0, 1000.0, 800.0);
        assert!((ratio_for(Axis::Row, rect, 8.0, 300.0, 0.0) - 300.0 / 992.0).abs() < 1e-9);
        assert_eq!(ratio_for(Axis::Row, rect, 8.0, -50.0, 0.0), 0.05);
        assert_eq!(ratio_for(Axis::Row, rect, 8.0, 9999.0, 0.0), 0.95);
    }

    #[test]
    fn set_ratio_updates_branch_at_path() {
        let mut tree = nested();
        tree.set_ratio(&[1], 0.3);
        let Pane::Branch { b, .. } = &tree else {
            panic!()
        };
        let Pane::Branch { ratio, .. } = b.as_ref() else {
            panic!()
        };
        assert!((*ratio - 0.3).abs() < 1e-9);
    }

    #[test]
    fn degenerate_region_stays_nonnegative() {
        let tree = Pane::Branch {
            axis: Axis::Row,
            ratio: 0.5,
            a: Box::new(Pane::leaf(1)),
            b: Box::new(Pane::leaf(2)),
        };
        let tiny = Rect::new(0.0, 0.0, 4.0, 4.0);
        for (_, r) in layout(&tree, tiny, 8.0) {
            assert!(r.width >= 0.0 && r.height >= 0.0);
        }
    }
}
