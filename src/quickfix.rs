//! `]q` and `[q` - one list of places, and the walk through it.
//!
//! The list itself and nothing else: no document, no windows, no opening of
//! files. What a walk lands on is `Editor::quickfix_step`, which is where a
//! file exists to be opened; this is the part with the arithmetic in it, and
//! the part worth testing on its own.

/// One place worth coming back to.
///
/// A path and a line rather than anything that points into a document: the
/// files in a list are mostly not open, and the ones that are go on being
/// edited while the list sits there. A line that has drifted puts you near
/// where you meant; a stale char index puts you anywhere at all. The jump
/// list is a line and a column for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// As a picker's `target` has it: relative to the working directory, and
    /// openable as it stands.
    pub path: String,
    /// Zero-based, like every line number inside jack. Grep's are not, and
    /// converting on the way in is how that stays a detail of grep's.
    pub line: usize,
    /// The line as it read when the list was made, for the picker to show.
    pub text: String,
}

/// The list, and where in it you are.
///
/// One list, replaced whole. Vim keeps ten and has `:colder` to walk between
/// them, which is an answer to a question nobody asks twice.
#[derive(Default)]
pub struct Quickfix {
    list: Vec<Entry>,
    index: usize,
    /// Whether the walk has started. A fresh list is *before* its first entry,
    /// so `]q` goes to the first rather than the second - and `[q` to the
    /// last, which is the same rule read the other way.
    started: bool,
}

/// What a step did, beside moving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// Which entry, counting from one, and how many there are: `(3, 27)`.
    pub at: (usize, usize),
    /// Whether the walk came round the end to get here. Search says so when
    /// it wraps rather than refusing, and this is the same walk.
    pub wrapped: bool,
}

impl Quickfix {
    /// Replace the list. The walk starts again from before the first entry:
    /// a new list is a new question, and carrying an index over from the old
    /// one would land you in the middle of it for no reason.
    pub fn fill(&mut self, entries: Vec<Entry>) {
        self.list = entries;
        self.index = 0;
        self.started = false;
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.list
    }

    /// Where the walk is, for anything that wants to say so.
    pub fn current(&self) -> Option<&Entry> {
        self.started.then(|| self.list.get(self.index)).flatten()
    }

    /// Move `count` entries and hand back where that landed. `None` only when
    /// the list is empty: everything else wraps.
    pub fn step(&mut self, forward: bool, count: usize) -> Option<(Entry, Step)> {
        let len = self.list.len();
        if len == 0 {
            return None;
        }
        let mut wrapped = false;
        for _ in 0..count.max(1) {
            if !self.started {
                // Before the first entry, so one step forward is the first
                // and one step back is the last.
                self.started = true;
                self.index = match forward {
                    true => 0,
                    false => len - 1,
                };
                continue;
            }
            match forward {
                true => {
                    wrapped |= self.index + 1 == len;
                    self.index = (self.index + 1) % len;
                }
                false => {
                    wrapped |= self.index == 0;
                    self.index = (self.index + len - 1) % len;
                }
            }
        }
        let step = Step { at: (self.index + 1, len), wrapped };
        Some((self.list[self.index].clone(), step))
    }

    /// Go to a particular entry, which is what choosing one in the picker
    /// does: the walk carries on from there afterwards.
    pub fn go_to(&mut self, index: usize) -> Option<Entry> {
        let entry = self.list.get(index)?.clone();
        self.index = index;
        self.started = true;
        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(paths: &[&str]) -> Quickfix {
        let mut quickfix = Quickfix::default();
        quickfix.fill(
            paths
                .iter()
                .enumerate()
                .map(|(line, path)| Entry {
                    path: path.to_string(),
                    line,
                    text: format!("hit in {path}"),
                })
                .collect(),
        );
        quickfix
    }

    #[test]
    fn the_first_step_is_the_first_entry() {
        // A fresh list is before its first entry, not on it, so `]q` does not
        // skip the hit you were looking at the list to find.
        let mut q = list(&["a", "b", "c"]);
        assert!(q.current().is_none(), "nowhere yet");
        let (entry, step) = q.step(true, 1).expect("an entry");
        assert_eq!(entry.path, "a");
        assert_eq!(step.at, (1, 3));
        assert!(!step.wrapped);

        // And backwards from a fresh list is the last, which is the same rule
        // read the other way.
        let mut q = list(&["a", "b", "c"]);
        assert_eq!(q.step(false, 1).expect("an entry").0.path, "c");
    }

    #[test]
    fn the_walk_wraps_and_says_so() {
        // Search wraps rather than refusing, and this is the same walk.
        let mut q = list(&["a", "b"]);
        q.step(true, 1);
        let (entry, step) = q.step(true, 1).expect("an entry");
        assert_eq!(entry.path, "b");
        assert!(!step.wrapped, "the last entry is not yet a wrap");

        let (entry, step) = q.step(true, 1).expect("an entry");
        assert_eq!(entry.path, "a");
        assert!(step.wrapped, "round the end");

        let (entry, step) = q.step(false, 1).expect("an entry");
        assert_eq!(entry.path, "b");
        assert!(step.wrapped, "and back round it");
    }

    #[test]
    fn a_count_is_one_move_not_several_messages() {
        let mut q = list(&["a", "b", "c", "d"]);
        let (entry, step) = q.step(true, 3).expect("an entry");
        assert_eq!(entry.path, "c");
        assert_eq!(step.at, (3, 4));
    }

    #[test]
    fn an_empty_list_has_nowhere_to_go() {
        let mut q = Quickfix::default();
        assert!(q.step(true, 1).is_none());
        assert!(q.is_empty());
    }

    #[test]
    fn choosing_one_is_where_the_walk_carries_on_from() {
        let mut q = list(&["a", "b", "c"]);
        assert_eq!(q.go_to(1).expect("an entry").path, "b");
        assert_eq!(q.step(true, 1).expect("an entry").0.path, "c");
    }

    #[test]
    fn filling_again_starts_the_walk_over() {
        let mut q = list(&["a", "b", "c"]);
        q.step(true, 2);
        assert_eq!(q.current().expect("an entry").path, "b");

        q.fill(vec![Entry { path: "x".into(), line: 0, text: "x".into() }]);
        assert!(q.current().is_none(), "a new list is a new question");
        assert_eq!(q.step(true, 1).expect("an entry").0.path, "x");
    }
}
