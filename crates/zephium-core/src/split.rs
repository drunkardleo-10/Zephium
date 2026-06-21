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
