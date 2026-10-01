//! Catching up with a scroll.
//!
//! `^d` moves the viewport twenty lines in one frame, and twenty lines is a
//! whole new screen of text with nothing to say where it came from. The eye
//! has to read the top line to find out which way the file went.
//!
//! So the view is drawn behind where it has actually scrolled to, and the
//! difference is closed over the next few frames: the text slides, and which
//! way it slid is the answer. Nothing about the editor's state moves - the
//! cursor is where it is, the file is scrolled where it is scrolled. This is
//! only about which line is drawn at the top of the window while the drawing
//! is catching up.
//!
//! Off unless `:set smoothscroll` says otherwise, and off it costs nothing:
//! `shown` is never consulted.

/// The furthest a scroll is eased over. Beyond this it snaps, because beyond
/// this it is not a scroll: `G` in a ten thousand line file is not a journey
/// through the file, it is arriving somewhere else in it, and sliding ten
/// thousand lines past the window would be a long wait for a blur.
const REACH: usize = 60;

/// How much of what is left to go is covered each frame: a third, so a scroll
/// leaves quickly and lands softly rather than arriving at full speed.
const SHARE: usize = 3;

#[derive(Default)]
pub struct Scrolling {
    /// The line drawn at the top of the window.
    shown: usize,
    /// The line the view has really scrolled to, which `shown` is on its way
    /// to. Equal when there is nothing going on, which is most of the time.
    want: usize,
    /// Whether there has been a frame at all. Without this the first one
    /// would be a scroll from line zero to wherever the file opened at.
    seen: bool,
}

impl Scrolling {
    /// The view's top is here now, as of this frame. A step it cannot ease
    /// over is taken in one, which is also what the first frame does.
    pub fn follows(&mut self, top: usize) {
        if !self.seen || self.shown.abs_diff(top) > REACH {
            (self.shown, self.want, self.seen) = (top, top, true);
            return;
        }
        self.want = top;
    }

    /// Which line to draw at the top of the window, or `None` when there has
    /// been no frame to be behind - the view's own answer stands.
    pub fn shown(&self) -> Option<usize> {
        self.seen.then_some(self.shown)
    }

    /// A tick has passed: the drawn top closes a share of the distance left.
    /// True while there is still distance, which is what tells the run loop
    /// whether to come back.
    pub fn eases(&mut self) -> bool {
        let left = self.want.abs_diff(self.shown);
        if left == 0 {
            return false;
        }
        let step = (left / SHARE).max(1);
        self.shown = match self.want > self.shown {
            true => self.shown + step,
            false => self.shown - step,
        };
        self.shown != self.want
    }

    /// Nothing to catch up with and nothing to wait for. The next `follows`
    /// starts over, so whatever happened while this was off is not a scroll
    /// to be animated afterwards.
    pub fn forget(&mut self) {
        *self = Self::default();
    }

    pub fn running(&self) -> bool {
        self.seen && self.shown != self.want
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_is_not_a_scroll() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(200);
        assert_eq!(scrolling.shown(), Some(200));
        assert!(!scrolling.running(), "a file opening part way down has not scrolled there");
    }

    #[test]
    fn a_frame_that_did_not_scroll_has_nothing_to_catch_up_with() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(10);
        scrolling.follows(10);
        assert!(!scrolling.running());
        assert!(!scrolling.eases());
    }

    /// The whole of it: the drawn top is left behind, closes the gap over
    /// several ticks, and lands exactly on the real one.
    #[test]
    fn the_drawn_top_follows_the_real_one_and_arrives() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(0);
        scrolling.follows(20);
        assert_eq!(scrolling.shown(), Some(0), "the frame it scrolled on is still the old view");
        let mut tops = vec![];
        while scrolling.eases() {
            tops.push(scrolling.shown().unwrap());
            assert!(tops.len() < 40, "{tops:?}");
        }
        assert_eq!(scrolling.shown(), Some(20));
        assert!(!scrolling.running());
        // Fast at first and slow at the end, which is what a share of what is
        // left to go buys: the first step is the biggest.
        let steps: Vec<usize> = std::iter::once(0)
            .chain(tops.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .collect();
        assert_eq!(steps[0], 6, "{steps:?}");
        assert!(steps.windows(2).all(|pair| pair[0] >= pair[1]), "{steps:?}");
    }

    #[test]
    fn it_eases_upwards_the_same_way() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(30);
        scrolling.follows(10);
        assert_eq!(scrolling.shown(), Some(30));
        while scrolling.eases() {}
        assert_eq!(scrolling.shown(), Some(10));
    }

    /// A leap across the file is not eased: there is nothing in between worth
    /// showing, and showing it would take all day.
    #[test]
    fn a_leap_too_far_to_slide_is_taken_in_one() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(0);
        scrolling.follows(REACH + 1);
        assert_eq!(scrolling.shown(), Some(REACH + 1));
        assert!(!scrolling.running());
    }

    /// Scrolling while a scroll is still going on moves the target, and the
    /// drawn top keeps going from where it had got to - leaning on `^d` is
    /// one slide rather than a stutter of restarts.
    #[test]
    fn a_scroll_during_a_scroll_moves_where_it_is_heading() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(0);
        scrolling.follows(20);
        scrolling.eases();
        let part_way = scrolling.shown().unwrap();
        assert!(part_way > 0 && part_way < 20);
        scrolling.follows(40);
        assert_eq!(scrolling.shown(), Some(part_way), "it does not go back to start again");
        while scrolling.eases() {}
        assert_eq!(scrolling.shown(), Some(40));
    }

    #[test]
    fn forgetting_means_the_next_frame_starts_over() {
        let mut scrolling = Scrolling::default();
        scrolling.follows(0);
        scrolling.follows(20);
        assert!(scrolling.running());
        scrolling.forget();
        assert_eq!(scrolling.shown(), None);
        scrolling.follows(20);
        assert!(!scrolling.running(), "what happened while it was off was not a scroll");
    }
}
