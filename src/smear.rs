//! The cursor's wake.
//!
//! A terminal cursor is one cell and it teleports. `G` puts it forty lines
//! away with nothing in between, and the eye has to go and find it. The wake
//! is the cells it would have crossed if it had travelled: drawn for a moment
//! and then let go of one at a time, so where it came from is still on screen
//! while you are looking for where it went.
//!
//! Off unless `:set smear` says otherwise, and it costs nothing at all when
//! it is off - the cursor's position is not even asked for.

/// The most cells of wake there are. Long enough to streak across a screen,
/// short enough that it is gone before you have finished reading it.
const LENGTH: usize = 14;

#[derive(Default)]
pub struct Smear {
    /// Cells the cursor has lately crossed, newest first, so that letting go
    /// of the oldest is taking one off the end.
    trail: Vec<(u16, u16)>,
    /// Where it was when last asked. Without this there is no way to tell a
    /// move from a redraw.
    was: Option<(u16, u16)>,
}

impl Smear {
    /// The cursor is here now. Everything between where it was and where it
    /// is joins the wake, which is what makes a jump a streak rather than two
    /// dots: `G` crosses every row between, even though the cursor did not.
    pub fn follows(&mut self, at: (u16, u16)) {
        let Some(was) = self.was.replace(at) else {
            return;
        };
        if was == at {
            return;
        }
        let mut fresh = between(was, at);
        // Newest first: the cells nearest where the cursor now is were
        // crossed last, and are the last to be let go of.
        fresh.reverse();
        fresh.append(&mut self.trail);
        self.trail = fresh;
        self.trail.truncate(LENGTH);
    }

    /// A moment has passed with nothing typed: the far end of the wake goes.
    /// True while there is still something to draw, which is what tells the
    /// run loop whether to come back.
    pub fn fades(&mut self) -> bool {
        self.trail.pop();
        !self.trail.is_empty()
    }

    /// Nothing to draw and nothing to wait for.
    pub fn forget(&mut self) {
        self.trail.clear();
        self.was = None;
    }

    pub fn trail(&self) -> &[(u16, u16)] {
        &self.trail
    }

    pub fn running(&self) -> bool {
        !self.trail.is_empty()
    }
}

/// The cells from `from` towards `to`, `from` included and `to` left out -
/// the cursor is drawn on `to` itself, and a wake under the cursor is not a
/// wake. A straight line by Bresenham, because the cursor's path between two
/// places is not a real thing and a straight one is the one that reads.
fn between(from: (u16, u16), to: (u16, u16)) -> Vec<(u16, u16)> {
    let (mut x, mut y) = (from.0 as i32, from.1 as i32);
    let (x1, y1) = (to.0 as i32, to.1 as i32);
    let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
    let (sx, sy) = (if x < x1 { 1 } else { -1 }, if y < y1 { 1 } else { -1 });
    let mut error = dx + dy;

    let mut cells = Vec::new();
    loop {
        if (x, y) == (x1, y1) {
            return cells;
        }
        cells.push((x as u16, y as u16));
        // A long jump is cut to what will be kept anyway: the whole of a
        // forty-row leap is thirteen cells of wake and the rest is thrown
        // away, so there is no reason to walk it.
        if cells.len() > LENGTH * 2 {
            return cells;
        }
        let doubled = 2 * error;
        if doubled >= dy {
            error += dy;
            x += sx;
        }
        if doubled <= dx {
            error += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_look_is_not_a_move() {
        let mut smear = Smear::default();
        smear.follows((10, 5));
        assert!(!smear.running(), "arriving somewhere is not coming from somewhere");
    }

    #[test]
    fn a_redraw_that_did_not_move_leaves_the_wake_alone() {
        let mut smear = Smear::default();
        smear.follows((10, 5));
        smear.follows((14, 5));
        let after = smear.trail().to_vec();
        smear.follows((14, 5));
        assert_eq!(smear.trail(), after);
    }

    /// Four cells to the right leaves the four it came over, and not the one
    /// it is on - the cursor is drawn there itself.
    #[test]
    fn a_step_sideways_leaves_what_it_crossed() {
        let mut smear = Smear::default();
        smear.follows((10, 5));
        smear.follows((14, 5));
        assert_eq!(smear.trail(), [(13, 5), (12, 5), (11, 5), (10, 5)]);
    }

    /// The point of drawing the path rather than the places: a jump the
    /// cursor made in one step still reads as having come from somewhere.
    #[test]
    fn a_jump_down_the_file_is_a_streak_rather_than_two_dots() {
        let mut smear = Smear::default();
        smear.follows((4, 2));
        smear.follows((4, 30));
        assert_eq!(smear.trail().len(), LENGTH);
        let rows: Vec<u16> = smear.trail().iter().map(|&(_, y)| y).collect();
        assert_eq!(rows[0], 29, "it starts under the cursor");
        assert!(rows.windows(2).all(|pair| pair[0] == pair[1] + 1), "{rows:?}");
    }

    #[test]
    fn the_wake_lets_go_of_its_far_end_first() {
        let mut smear = Smear::default();
        smear.follows((10, 5));
        smear.follows((13, 5));
        assert_eq!(smear.trail(), [(12, 5), (11, 5), (10, 5)]);
        assert!(smear.fades());
        assert_eq!(smear.trail(), [(12, 5), (11, 5)]);
        assert!(smear.fades());
        assert!(!smear.fades(), "and then there is nothing to come back for");
    }

    #[test]
    fn a_wake_is_never_longer_than_it_is_allowed_to_be() {
        let mut smear = Smear::default();
        smear.follows((0, 0));
        for step in 1..40u16 {
            smear.follows((step, step));
        }
        assert_eq!(smear.trail().len(), LENGTH);
    }
}
