//! Closed folds: which lines a fold hides, and where the lines went after an
//! edit. Lines only - what is foldable comes from the grammar, and drawing is
//! the screen's business.
//!
//! A fold is two line numbers: the head, which stays on screen with a count
//! after it, and the last line it hides. `head + 1 ..= end` is what is not
//! drawn, so a fold always leaves something to put the cursor on.

/// One closed fold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fold {
    pub head: usize,
    pub end: usize,
}

impl Fold {
    /// How many lines it hides, which is what the marker says.
    pub fn hidden(&self) -> usize {
        self.end - self.head
    }
}

/// Every closed fold in a buffer, innermost last where they nest.
#[derive(Clone, Default, Debug)]
pub struct Folds {
    closed: Vec<Fold>,
}

impl Folds {
    pub fn is_empty(&self) -> bool {
        self.closed.is_empty()
    }

    pub fn clear(&mut self) {
        self.closed.clear();
    }

    /// Close one. A fold that hides nothing is not a fold, and the same one
    /// twice is the same one; the list stays sorted by head so that the
    /// drawing walks it in the order it draws.
    pub fn close(&mut self, head: usize, end: usize) -> bool {
        if end <= head || self.closed.iter().any(|fold| fold.head == head) {
            return false;
        }
        self.closed.push(Fold { head, end });
        self.closed.sort_by_key(|fold| (fold.head, fold.end));
        true
    }

    /// The fold whose head is this line: the one with a marker on it.
    pub fn at(&self, line: usize) -> Option<Fold> {
        self.closed.iter().copied().find(|fold| fold.head == line)
    }

    /// The fold hiding this line, if one does. The head of a fold is not
    /// hidden by it, which is what keeps a closed fold on screen.
    pub fn hiding(&self, line: usize) -> Option<Fold> {
        self.closed
            .iter()
            .copied()
            .find(|fold| line > fold.head && line <= fold.end)
    }

    /// Open the innermost fold covering this line - the one `zo` opens, and
    /// the one `za` opens when there is one to open.
    pub fn open(&mut self, line: usize) -> bool {
        let found = self
            .closed
            .iter()
            .enumerate()
            .filter(|(_, fold)| line >= fold.head && line <= fold.end)
            .max_by_key(|(_, fold)| fold.head);
        match found {
            Some((index, _)) => {
                self.closed.remove(index);
                true
            }
            None => false,
        }
    }

    /// The first line at or before `line` that is drawn.
    pub fn prev_visible(&self, line: usize) -> usize {
        match self.hiding(line) {
            Some(fold) => fold.head,
            None => line,
        }
    }

    /// The line `count` drawn lines below `line`, and how far it actually
    /// got: at the end of the buffer there is nowhere further to go.
    pub fn down(&self, line: usize, count: usize, last: usize) -> usize {
        let mut at = line;
        for _ in 0..count {
            let next = match self.at(at) {
                Some(fold) => fold.end + 1,
                None => at + 1,
            };
            if next > last {
                break;
            }
            at = next;
        }
        at
    }

    /// The line `count` drawn lines above `line`.
    pub fn up(&self, line: usize, count: usize) -> usize {
        let mut at = line;
        for _ in 0..count {
            if at == 0 {
                break;
            }
            at = self.prev_visible(at - 1);
        }
        at
    }

    /// How many drawn rows there are from `top` up to but not including
    /// `line`: where the cursor sits on the screen.
    pub fn rows_between(&self, top: usize, line: usize) -> usize {
        let mut rows = 0;
        let mut at = top;
        while at < line {
            at = match self.at(at) {
                Some(fold) => fold.end + 1,
                None => at + 1,
            };
            rows += 1;
        }
        rows
    }

    /// Where the lines went after an edit at `line` that took `removed` line
    /// breaks away and put `inserted` back.
    ///
    /// A fold whose lines the edit cut across is opened rather than guessed
    /// at: the grammar no longer says what it said, and a fold hiding lines
    /// that are not the ones it was made of is worse than no fold.
    pub fn carry(&mut self, line: usize, removed: usize, inserted: usize) {
        let delta = inserted as isize - removed as isize;
        self.closed.retain_mut(|fold| {
            if removed > 0 && line + removed >= fold.head && line <= fold.end {
                return false;
            }
            match line < fold.head {
                true => {
                    fold.head = (fold.head as isize + delta) as usize;
                    fold.end = (fold.end as isize + delta) as usize;
                }
                // Inside it, or on its head: the lines went in where they are
                // hidden, and the fold grew by them.
                false if line <= fold.end => fold.end = (fold.end as isize + delta) as usize,
                false => {}
            }
            fold.end > fold.head
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fold_hides_the_lines_after_its_head() {
        let mut folds = Folds::default();
        assert!(folds.close(2, 6));
        // Nothing to hide is not a fold, and the same head twice is one fold.
        assert!(!folds.close(9, 9));
        assert!(!folds.close(2, 8));

        assert_eq!(folds.at(2).map(|fold| fold.hidden()), Some(4));
        assert!(folds.at(3).is_none());
        assert!(folds.hiding(2).is_none(), "the head is drawn");
        assert_eq!(folds.hiding(6), folds.at(2));
        assert!(folds.hiding(7).is_none());
    }

    #[test]
    fn moving_by_a_row_steps_over_a_closed_fold() {
        let mut folds = Folds::default();
        folds.close(2, 6);

        assert_eq!(folds.down(1, 1, 20), 2);
        assert_eq!(folds.down(2, 1, 20), 7, "one row down from the head");
        assert_eq!(folds.up(7, 1), 2);
        // Never past the end of the buffer.
        assert_eq!(folds.down(19, 4, 20), 20);
        assert_eq!(folds.up(0, 3), 0);

        // The rows the screen spends on the same lines.
        assert_eq!(folds.rows_between(0, 8), 4, "0, 1, the fold, 7");
        assert_eq!(folds.rows_between(0, 2), 2);
    }

    #[test]
    fn opening_takes_the_innermost_fold() {
        let mut folds = Folds::default();
        folds.close(1, 20);
        folds.close(4, 8);

        assert!(folds.open(6), "the nested one, from inside it");
        assert!(folds.at(4).is_none());
        assert!(folds.at(1).is_some());
        assert!(folds.open(6));
        assert!(folds.is_empty());
        assert!(!folds.open(6));
    }

    #[test]
    fn an_edit_carries_the_folds_and_opens_the_ones_it_cuts_across() {
        let mut folds = Folds::default();
        folds.close(10, 20);

        // Above it: the whole fold moves down by the lines that went in.
        folds.carry(3, 0, 2);
        assert_eq!(folds.at(12).map(|fold| fold.end), Some(22));

        // Inside it: it grew by them, and the head stayed where it was.
        folds.carry(15, 0, 1);
        assert_eq!(folds.at(12).map(|fold| fold.end), Some(23));

        // Below it: nothing.
        folds.carry(40, 1, 0);
        assert_eq!(folds.at(12).map(|fold| fold.end), Some(23));

        // Lines taken away across it: opened rather than guessed at.
        folds.carry(11, 4, 0);
        assert!(folds.is_empty());
    }
}
