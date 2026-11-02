//! The split tree of one tab: pure geometry, no terminal or window involved.

pub type PaneId = u64;

/// Pixels between split panes; the window's clear colour shows through.
pub const GAP: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Leaf(PaneId),
    Split { side_by_side: bool, ratio: f32, a: Box<Node>, b: Box<Node> },
}

impl Node {
    /// Each pane's rectangle within `area`, in tree order.
    pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut out = vec![];
        self.collect(area, &mut out);
        out
    }

    fn collect(&self, r: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf(id) => out.push((*id, r)),
            Node::Split { side_by_side, ratio, a, b } => {
                if *side_by_side {
                    let first = ((r.w.saturating_sub(GAP)) as f32 * ratio) as u32;
                    a.collect(Rect { w: first, ..r }, out);
                    b.collect(Rect { x: r.x + first + GAP, w: r.w.saturating_sub(first + GAP), ..r }, out);
                } else {
                    let first = ((r.h.saturating_sub(GAP)) as f32 * ratio) as u32;
                    a.collect(Rect { h: first, ..r }, out);
                    b.collect(Rect { y: r.y + first + GAP, h: r.h.saturating_sub(first + GAP), ..r }, out);
                }
            }
        }
    }

    /// Replaces pane `id` with a split of it and `new`. False if `id` is not in this tree.
    pub fn split(&mut self, id: PaneId, new: PaneId, side_by_side: bool) -> bool {
        match self {
            Node::Leaf(leaf) if *leaf == id => {
                *self = Node::Split { side_by_side, ratio: 0.5, a: Box::new(Node::Leaf(id)), b: Box::new(Node::Leaf(new)) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split(id, new, side_by_side) || b.split(id, new, side_by_side),
        }
    }

    /// Removes pane `id`; its sibling takes the space. `None` means the tree is now empty.
    pub fn remove(self, id: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(leaf) if leaf == id => None,
            Node::Leaf(_) => Some(self),
            Node::Split { side_by_side, ratio, a, b } => match (a.remove(id), b.remove(id)) {
                (Some(a), Some(b)) => Some(Node::Split { side_by_side, ratio, a: Box::new(a), b: Box::new(b) }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.rects(Rect { x: 0, y: 0, w: 1000, h: 1000 }).into_iter().map(|(id, _)| id).collect()
    }
}

/// The pane in `dir` from `from`: nearest with overlap on the other axis.
pub fn neighbour(rects: &[(PaneId, Rect)], from: PaneId, dir: Direction) -> Option<PaneId> {
    let (_, f) = rects.iter().find(|(id, _)| *id == from)?;
    let overlap = |a0: u32, a1: u32, b0: u32, b1: u32| a0 < b1 && b0 < a1;
    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, r)| {
            let distance = match dir {
                Direction::Left if r.x + r.w <= f.x && overlap(r.y, r.y + r.h, f.y, f.y + f.h) => f.x - (r.x + r.w),
                Direction::Right if f.x + f.w <= r.x && overlap(r.y, r.y + r.h, f.y, f.y + f.h) => r.x - (f.x + f.w),
                Direction::Up if r.y + r.h <= f.y && overlap(r.x, r.x + r.w, f.x, f.x + f.w) => f.y - (r.y + r.h),
                Direction::Down if f.y + f.h <= r.y && overlap(r.x, r.x + r.w, f.x, f.x + f.w) => r.y - (f.y + f.h),
                _ => return None,
            };
            Some((distance, *id))
        })
        .min()
        .map(|(_, id)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0, y: 0, w: 802, h: 602 };

    #[test]
    fn single_pane_fills_the_area() {
        assert_eq!(Node::Leaf(1).rects(AREA), vec![(1, AREA)]);
    }

    #[test]
    fn splits_divide_with_a_gap() {
        let mut t = Node::Leaf(1);
        assert!(t.split(1, 2, true));
        let r = t.rects(AREA);
        assert_eq!(r[0].1, Rect { x: 0, y: 0, w: 400, h: 602 });
        assert_eq!(r[1].1, Rect { x: 402, y: 0, w: 400, h: 602 });
        assert!(t.split(2, 3, false));
        let r = t.rects(AREA);
        assert_eq!(r[1].1, Rect { x: 402, y: 0, w: 400, h: 300 });
        assert_eq!(r[2].1, Rect { x: 402, y: 302, w: 400, h: 300 });
        assert!(!t.split(9, 10, true), "unknown pane");
    }

    #[test]
    fn removing_gives_space_to_the_sibling() {
        let mut t = Node::Leaf(1);
        t.split(1, 2, true);
        t.split(2, 3, false);
        let t = t.remove(2).unwrap();
        assert_eq!(t.panes(), vec![1, 3]);
        assert_eq!(t.rects(AREA)[1].1, Rect { x: 402, y: 0, w: 400, h: 602 });
        let t = t.remove(1).unwrap();
        assert_eq!(t, Node::Leaf(3));
        assert!(t.remove(3).is_none());
    }

    #[test]
    fn neighbours_follow_geometry() {
        let mut t = Node::Leaf(1);
        t.split(1, 2, true);
        t.split(2, 3, false);
        let r = t.rects(AREA);
        assert_eq!(neighbour(&r, 1, Direction::Right), Some(2));
        assert_eq!(neighbour(&r, 2, Direction::Down), Some(3));
        assert_eq!(neighbour(&r, 3, Direction::Up), Some(2));
        assert_eq!(neighbour(&r, 3, Direction::Left), Some(1));
        assert_eq!(neighbour(&r, 1, Direction::Left), None);
    }

    #[test]
    fn tiny_windows_do_not_underflow() {
        let mut t = Node::Leaf(1);
        t.split(1, 2, true);
        t.split(2, 3, false);
        for (_, r) in t.rects(Rect { x: 0, y: 0, w: 1, h: 1 }) {
            assert!(r.w <= 1 && r.h <= 1);
        }
    }
}
