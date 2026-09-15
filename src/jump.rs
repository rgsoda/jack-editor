/// Where the cursor was before a jump, so `^o` can go back to it.
///
/// A line and a column rather than a char index: the text under a jump goes on
/// changing while you are away from it, and a line number that has drifted puts
/// you near where you meant, where a stale char index puts you anywhere at all.
/// Vim's jumps drift the same way, and for the same reason.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Jump {
    /// Which open buffer, by index. Buffers are only ever added, so an index
    /// stays pointing at the same file for as long as the editor runs.
    pub view: usize,
    pub line: usize,
    /// Chars into the line, not display columns: this is a position to restore,
    /// not one to draw.
    pub column: usize,
}

/// How many jumps are worth remembering. Vim's number, and for the same
/// reason: past a hundred you are not going back, you are searching.
const MAX: usize = 100;

/// The jump list: the positions you jumped away from, and where you are in
/// that history.
///
/// `index` is which entry `^o` would take you to next, and `index == len` means
/// you are at the present rather than somewhere inside the history - the
/// distinction that lets `^i` find its way back to where you started.
#[derive(Default)]
pub struct Jumps {
    list: Vec<Jump>,
    index: usize,
}

impl Jumps {
    /// Record a position being jumped away from.
    pub fn push(&mut self, from: Jump) {
        // Jumping from inside the history throws away what was ahead of it:
        // you took a different turning, and the old one is not coming back.
        self.list.truncate(self.index);
        // Two jumps from the same line leave one entry, as vim does - a line
        // repeated down the list is one `^o` press wasted each time.
        if self.list.last().is_some_and(|last| last.view == from.view && last.line == from.line) {
            self.list.pop();
        }
        self.list.push(from);
        if self.list.len() > MAX {
            self.list.remove(0);
        }
        self.index = self.list.len();
    }

    /// `^o`. `now` is where the cursor is, which is remembered the first time
    /// you step back so that `^i` has somewhere to return to.
    pub fn back(&mut self, now: Jump) -> Option<Jump> {
        if self.index == 0 {
            return None;
        }
        if self.index == self.list.len() {
            self.list.push(now);
        }
        self.index -= 1;
        Some(self.list[self.index])
    }

    /// `^i`: forward again, as far as the position `^o` was first pressed from.
    pub fn forward(&mut self) -> Option<Jump> {
        if self.index + 1 >= self.list.len() {
            return None;
        }
        self.index += 1;
        Some(self.list[self.index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize) -> Jump {
        Jump { view: 0, line, column: 0 }
    }

    #[test]
    fn stepping_back_walks_the_jumps_in_reverse() {
        let mut jumps = Jumps::default();
        jumps.push(at(1));
        jumps.push(at(2));
        jumps.push(at(3));

        assert_eq!(jumps.back(at(9)), Some(at(3)));
        assert_eq!(jumps.back(at(3)), Some(at(2)));
        assert_eq!(jumps.back(at(2)), Some(at(1)));
        // The oldest jump is as far back as it goes.
        assert_eq!(jumps.back(at(1)), None);
    }

    #[test]
    fn stepping_forward_returns_to_where_the_first_step_back_started() {
        let mut jumps = Jumps::default();
        jumps.push(at(1));
        jumps.push(at(2));

        jumps.back(at(9));
        jumps.back(at(2));
        assert_eq!(jumps.forward(), Some(at(2)));
        // Line 9 was never jumped from, only jumped back out of - and it is
        // still where `^i` ends up.
        assert_eq!(jumps.forward(), Some(at(9)));
        assert_eq!(jumps.forward(), None);
    }

    #[test]
    fn a_new_jump_from_inside_the_history_forgets_what_was_ahead() {
        let mut jumps = Jumps::default();
        jumps.push(at(1));
        jumps.push(at(2));
        jumps.push(at(3));

        jumps.back(at(9));
        jumps.back(at(3));
        // Back at 2, and off somewhere else: 3 and 9 are not on the way to
        // anywhere any more.
        jumps.push(at(2));
        assert_eq!(jumps.forward(), None);
        assert_eq!(jumps.back(at(7)), Some(at(2)));
    }

    #[test]
    fn the_same_line_twice_leaves_one_entry() {
        let mut jumps = Jumps::default();
        jumps.push(at(4));
        jumps.push(Jump { view: 0, line: 4, column: 8 });

        assert_eq!(jumps.back(at(9)), Some(Jump { view: 0, line: 4, column: 8 }));
        assert_eq!(jumps.back(at(4)), None);
    }

    #[test]
    fn the_same_line_in_another_buffer_is_a_different_place() {
        let mut jumps = Jumps::default();
        jumps.push(at(4));
        jumps.push(Jump { view: 1, line: 4, column: 0 });

        assert_eq!(jumps.back(at(9)), Some(Jump { view: 1, line: 4, column: 0 }));
        assert_eq!(jumps.back(at(4)), Some(at(4)));
    }

    #[test]
    fn the_list_stops_growing_and_drops_the_oldest() {
        let mut jumps = Jumps::default();
        for line in 0..MAX + 20 {
            jumps.push(at(line));
        }
        assert_eq!(jumps.list.len(), MAX);
        assert_eq!(jumps.list[0], at(20));
    }
}
