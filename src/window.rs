//! Windows: how the screen is divided between them.
//!
//! A window is a place a buffer is shown - its own cursor and its own scroll
//! over a `View` that any number of windows can share. This file is only the
//! arithmetic: a tree of splits with window ids at the leaves, the rectangles
//! it works out to, and which window is next to which. What a window shows,
//! and moving state in and out of the buffer it shows, is the editor's.

use crate::view::Selection;

/// Where a window's text was left when focus went somewhere else. The focused
/// window keeps its state in the `View` itself, which is what lets everything
/// that edits and moves go on addressing `editor.view()`; this is the copy the
/// other windows hold until they are focused again.
#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// Which buffer, as an index into the editor's views.
    pub view: usize,
    pub sel: Selection,
    /// The first char of the top line on screen, rather than the line number:
    /// a char position can be carried through an edit made in another window.
    pub top_char: usize,
    pub scroll_left: usize,
    /// How far through the buffer's edit log this snapshot has been brought.
    pub mark: usize,
}

/// A rectangle of screen cells. `height` includes the window's status line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    /// Rows for text, once the status line has had its row.
    pub fn text_height(&self) -> usize {
        self.height.saturating_sub(1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

/// The splits. `vertical` is vim's: a vertical split puts windows side by side
/// with a line between them, a horizontal one stacks them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    Window(usize),
    Split { vertical: bool, children: Vec<Layout> },
}

impl Default for Layout {
    fn default() -> Self {
        Layout::Window(0)
    }
}

impl Layout {
    /// Put window `new` beside window `at`: right of it for a vertical split,
    /// below it for a horizontal one. Joins the enclosing split when that runs
    /// the same way, so three `:vsplit`s are three equal columns rather than
    /// halves of halves.
    pub fn split(&mut self, at: usize, new: usize, vertical: bool) {
        match self {
            Layout::Window(id) if *id == at => {
                *self = Layout::Split {
                    vertical,
                    children: vec![Layout::Window(at), Layout::Window(new)],
                };
            }
            Layout::Window(_) => {}
            Layout::Split { vertical: runs, children } => {
                let here = children.iter().position(|child| *child == Layout::Window(at));
                match here {
                    Some(index) if *runs == vertical => {
                        children.insert(index + 1, Layout::Window(new));
                    }
                    _ => {
                        for child in children {
                            child.split(at, new, vertical);
                        }
                    }
                }
            }
        }
    }

    /// Take window `id` out, and renumber the ones after it so the ids stay
    /// indexes into the editor's list. A split left with one child becomes
    /// that child, and a child that runs the same way as its new parent is
    /// folded into it.
    pub fn remove(&mut self, id: usize) {
        self.cut(id);
        self.renumber(id);
        self.tidy();
    }

    fn cut(&mut self, id: usize) {
        if let Layout::Split { children, .. } = self {
            children.retain(|child| *child != Layout::Window(id));
            for child in children.iter_mut() {
                child.cut(id);
            }
        }
    }

    fn renumber(&mut self, removed: usize) {
        match self {
            Layout::Window(id) if *id > removed => *id -= 1,
            Layout::Window(_) => {}
            Layout::Split { children, .. } => {
                for child in children {
                    child.renumber(removed);
                }
            }
        }
    }

    fn tidy(&mut self) {
        let Layout::Split { vertical, children } = self else {
            return;
        };
        let vertical = *vertical;
        let mut flat = Vec::new();
        for mut child in std::mem::take(children) {
            child.tidy();
            match child {
                Layout::Split { vertical: runs, children: inner } if runs == vertical => {
                    flat.extend(inner);
                }
                other => flat.push(other),
            }
        }
        *self = match flat.len() {
            1 => flat.pop().expect("one child"),
            _ => Layout::Split { vertical, children: flat },
        };
    }

    /// Every window's rectangle, in the order `^w w` visits them, and the
    /// columns the lines between side-by-side windows are drawn in.
    pub fn rects(&self, area: Rect) -> (Vec<(usize, Rect)>, Vec<Rect>) {
        let mut rects = Vec::new();
        let mut lines = Vec::new();
        self.place(area, &mut rects, &mut lines);
        (rects, lines)
    }

    fn place(&self, area: Rect, rects: &mut Vec<(usize, Rect)>, lines: &mut Vec<Rect>) {
        let children = match self {
            Layout::Window(id) => {
                rects.push((*id, area));
                return;
            }
            Layout::Split { children, .. } => children,
        };
        let vertical = matches!(self, Layout::Split { vertical: true, .. });
        let count = children.len();
        // Side by side, one column goes to each line between two windows.
        let total = match vertical {
            true => area.width.saturating_sub(count - 1),
            false => area.height,
        };
        let each = total / count;
        let mut offset = 0;
        for (index, child) in children.iter().enumerate() {
            // The last one takes what division left over.
            let size = match index + 1 == count {
                true => total - each * (count - 1),
                false => each,
            };
            let rect = match vertical {
                true => Rect { x: area.x + offset, width: size, ..area },
                false => Rect { y: area.y + offset, height: size, ..area },
            };
            child.place(rect, rects, lines);
            offset += size;
            if vertical && index + 1 < count {
                lines.push(Rect { x: area.x + offset, width: 1, ..area });
                offset += 1;
            }
        }
    }

}

/// The window next to `from` in `direction`, preferring the one level with
/// `at` - the screen cell the cursor is on - so that moving right and back
/// left returns to the same window rather than the top one.
pub fn neighbour(
    rects: &[(usize, Rect)],
    from: usize,
    direction: Direction,
    at: (usize, usize),
) -> Option<usize> {
    let (_, here) = *rects.iter().find(|(id, _)| *id == from)?;
    let (x, y) = at;
    let touching = rects.iter().filter(|(id, rect)| {
        *id != from
            && match direction {
                // A line between side-by-side windows is one column wide.
                Direction::Right => rect.x == here.x + here.width + 1 && overlaps_rows(rect, &here),
                Direction::Left => rect.x + rect.width + 1 == here.x && overlaps_rows(rect, &here),
                Direction::Down => rect.y == here.y + here.height && overlaps_columns(rect, &here),
                Direction::Up => rect.y + rect.height == here.y && overlaps_columns(rect, &here),
            }
    });
    let level = |rect: &Rect| match direction {
        Direction::Left | Direction::Right => y.clamp(rect.y, rect.y + rect.height - 1).abs_diff(y),
        Direction::Up | Direction::Down => x.clamp(rect.x, rect.x + rect.width - 1).abs_diff(x),
    };
    touching.min_by_key(|(_, rect)| level(rect)).map(|(id, _)| *id)
}

fn overlaps_rows(a: &Rect, b: &Rect) -> bool {
    a.y < b.y + b.height && b.y < a.y + a.height
}

fn overlaps_columns(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0, y: 0, width: 81, height: 24 };

    fn ids(layout: &Layout) -> Vec<usize> {
        layout.rects(SCREEN).0.into_iter().map(|(id, _)| id).collect()
    }

    #[test]
    fn one_window_has_the_whole_screen() {
        let (rects, lines) = Layout::default().rects(SCREEN);
        assert_eq!(rects, [(0, SCREEN)]);
        assert!(lines.is_empty());
    }

    #[test]
    fn a_vertical_split_shares_the_width_around_a_line() {
        let mut layout = Layout::default();
        layout.split(0, 1, true);
        let (rects, lines) = layout.rects(SCREEN);
        assert_eq!(rects[0].1, Rect { x: 0, y: 0, width: 40, height: 24 });
        assert_eq!(rects[1].1, Rect { x: 41, y: 0, width: 40, height: 24 });
        assert_eq!(lines, [Rect { x: 40, y: 0, width: 1, height: 24 }]);
    }

    #[test]
    fn a_horizontal_split_stacks_and_the_last_takes_the_odd_row() {
        let mut layout = Layout::default();
        layout.split(0, 1, false);
        layout.split(1, 2, false);
        let (rects, _) = layout.rects(Rect { height: 25, ..SCREEN });
        let heights: Vec<usize> = rects.iter().map(|(_, r)| r.height).collect();
        assert_eq!(heights, [8, 8, 9]);
        assert_eq!(rects[2].1.y, 16);
        // Joined into one split, not nested halves.
        assert!(matches!(&layout, Layout::Split { children, .. } if children.len() == 3));
    }

    #[test]
    fn splits_nest_when_they_run_the_other_way() {
        let mut layout = Layout::default();
        layout.split(0, 1, true);
        layout.split(1, 2, false);
        assert_eq!(ids(&layout), [0, 1, 2]);
        let (rects, _) = layout.rects(SCREEN);
        assert_eq!(rects[1].1, Rect { x: 41, y: 0, width: 40, height: 12 });
        assert_eq!(rects[2].1, Rect { x: 41, y: 12, width: 40, height: 12 });
    }

    #[test]
    fn closing_a_window_renumbers_and_collapses() {
        let mut layout = Layout::default();
        layout.split(0, 1, true);
        layout.split(1, 2, false);
        layout.remove(1);
        // Window 2 became 1, and the column it was alone in is gone.
        assert_eq!(
            layout,
            Layout::Split { vertical: true, children: vec![Layout::Window(0), Layout::Window(1)] }
        );
        layout.remove(0);
        assert_eq!(layout, Layout::Window(0));
    }

    #[test]
    fn a_collapsed_split_folds_into_a_parent_running_the_same_way() {
        let mut layout = Layout::default();
        layout.split(0, 1, true);
        layout.split(1, 2, false);
        layout.split(2, 3, true);
        layout.remove(1);
        assert_eq!(ids(&layout), [0, 1, 2]);
        assert!(matches!(&layout, Layout::Split { vertical: true, children } if children.len() == 3));
    }

    #[test]
    fn neighbours_are_found_in_every_direction() {
        // 0 | 1
        //   | -
        //   | 2
        let mut layout = Layout::default();
        layout.split(0, 1, true);
        layout.split(1, 2, false);
        let (rects, _) = layout.rects(SCREEN);

        assert_eq!(neighbour(&rects, 0, Direction::Right, (5, 2)), Some(1));
        assert_eq!(neighbour(&rects, 0, Direction::Right, (5, 20)), Some(2), "level with the cursor");
        assert_eq!(neighbour(&rects, 1, Direction::Down, (50, 5)), Some(2));
        assert_eq!(neighbour(&rects, 2, Direction::Up, (50, 15)), Some(1));
        assert_eq!(neighbour(&rects, 2, Direction::Left, (50, 15)), Some(0));
        assert_eq!(neighbour(&rects, 0, Direction::Left, (5, 5)), None);
        assert_eq!(neighbour(&rects, 1, Direction::Up, (50, 5)), None);
    }
}
