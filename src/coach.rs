//! What you just did, and the shorter way to have done it.
//!
//! Every key arrives here on its way to being handled, and a run of the same
//! one is the thing worth noticing: ten `j` in a row is a count that was
//! never typed. Off unless `:set coach` says otherwise, because advice
//! nobody asked for is a personality flaw in an editor.
//!
//! Only keys that mean one whole thing on their own are counted. An operator
//! does not: `dd` three times over is six presses of two keys, and a coach
//! that could not tell those apart would offer `6d`, which is not a command.
//! Narrow and right beats clever and wrong, so the list is short and every
//! key on it takes a count.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// How many of the same key in a row is worth mentioning. Fewer than this is
/// quicker to lean on than to count.
const RUN: usize = 5;

/// Keys between one word of advice and the next. A habit is worth mentioning
/// once; mentioning it every time is what makes people turn a thing off.
const PATIENCE: u64 = 60;

/// The keys a run of is worth counting: each is a whole command by itself and
/// each takes a count, so `{n}{key}` is always the same thing said shorter.
/// Operators are deliberately absent - see the module comment.
const COUNTABLE: &str = "hjklwbexWBE~";

/// A key, as the coach counts them. Everything else is not a run, and the
/// arrival of one ends whatever run was going.
#[derive(Clone, Copy, PartialEq)]
enum Counted {
    /// A key that is a whole command and takes a count.
    Key(char),
    /// An arrow, counted the same way and answered differently.
    Arrow,
}

#[derive(Default)]
pub struct Coach {
    /// What has been arriving, and how many times running.
    last: Option<Counted>,
    run: usize,
    /// Whether the run going on right now is one it has spoken about. While
    /// that is true the words go back up on every key of it.
    saying: bool,
    /// The key count when something was last said, so that the next thing is
    /// a while away.
    spoke_at: Option<u64>,
}

impl Coach {
    /// One key, on its way to being handled. `normal` is whether the editor
    /// was in normal mode when it arrived - a `j` while inserting is a `j` -
    /// and `pressed` is how many keys the session has seen, which is the only
    /// clock this needs.
    ///
    /// Hands back what to say, if this is the moment to say anything, and
    /// keeps handing the same thing back for the rest of the run. A message
    /// lasts until the next key, and the next key of a run you are leaning on
    /// wipes it before it has been read, so the words have to be put back up
    /// each time. That way they are there while you lean and still there when
    /// you stop, which is when anybody actually looks.
    pub fn notice(&mut self, key: KeyEvent, normal: bool, pressed: u64) -> Option<String> {
        let counted = match key.code {
            _ if !normal || key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                None
            }
            KeyCode::Char(c) if COUNTABLE.contains(c) => Some(Counted::Key(c)),
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => Some(Counted::Arrow),
            // A count typed in front of a key is already the answer, and an
            // operator is not countable: either way the run is over.
            _ => None,
        };

        match counted {
            // The run goes on, and so does whatever is being said about it.
            Some(_) if counted == self.last => self.run += 1,
            // A different key: the last run is over, and this is the first of
            // the next one. Anything said about the old one stands until the
            // key that ended it has its own say.
            Some(_) => (self.last, self.run, self.saying) = (counted, 1, false),
            None => {
                self.forget();
                return None;
            }
        }

        if self.run < RUN {
            return None;
        }
        // Already talking about this run: say it again, because the key that
        // just arrived wiped it. The count goes up as the run does.
        if self.saying {
            return Some(self.words());
        }
        // Said something recently, so this run passes without comment. A
        // habit is worth mentioning once, and every time is what makes people
        // turn a thing off.
        if self.spoke_at.is_some_and(|at| pressed.saturating_sub(at) < PATIENCE) {
            return None;
        }
        (self.spoke_at, self.saying) = (Some(pressed), true);
        Some(self.words())
    }

    /// The run as it stands, and the two keys it should have been.
    fn words(&self) -> String {
        let run = self.run;
        match self.last {
            Some(Counted::Key(key)) => format!("{run} {key} in a row - {run}{key} is two keys"),
            _ => "the arrows work - h j k l are nearer, and take a count".to_string(),
        }
    }

    fn forget(&mut self) {
        (self.last, self.run, self.saying) = (None, 0, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// A key that is no part of a run, and so ends one.
    fn stop() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn run_of(key: KeyEvent, count: usize) -> Vec<KeyEvent> {
        std::iter::repeat_n(key, count).collect()
    }

    /// Run `keys` through a fresh coach and collect everything it said.
    fn said(keys: &[KeyEvent]) -> Vec<String> {
        let mut coach = Coach::default();
        (keys.iter().enumerate())
            .filter_map(|(index, &key)| coach.notice(key, true, index as u64))
            .collect()
    }

    #[test]
    fn a_short_run_is_quicker_to_lean_on_than_to_count() {
        let keys = run_of(press('j'), RUN - 1);
        assert!(said(&keys).is_empty(), "{RUN} is the point, not fewer");
    }

    #[test]
    fn a_long_run_is_a_count_that_was_never_typed() {
        let keys = run_of(press('j'), RUN);
        assert_eq!(said(&keys), [format!("{RUN} j in a row - {RUN}j is two keys")]);
    }

    /// The thing that makes it visible at all: every key of a run wipes the
    /// message the last one put up, so the words go back up each time and
    /// are still there when the leaning stops.
    #[test]
    fn the_words_go_back_up_for_the_rest_of_the_run() {
        let said = said(&run_of(press('j'), RUN + 3));
        assert_eq!(said.len(), 4, "one for each key from the {RUN}th on");
        assert_eq!(said.last().unwrap(), &format!("{0} j in a row - {0}j is two keys", RUN + 3));
    }

    /// And the key that ends the run does not get them again, because it has
    /// its own message to write.
    #[test]
    fn the_key_that_ends_the_run_says_nothing() {
        let mut keys = run_of(press('j'), RUN);
        keys.push(stop());
        assert_eq!(said(&keys).len(), 1, "the esc is not part of the run");
    }

    #[test]
    fn a_different_key_starts_the_count_again() {
        let mut keys = run_of(press('j'), RUN - 1);
        keys.push(press('k'));
        keys.extend(run_of(press('j'), RUN - 1));
        assert!(said(&keys).is_empty(), "two short runs are not one long one");
    }

    /// The reason operators are not on the list: `dd dd dd` is six keys and
    /// three commands, and `6d` is not a thing anybody can type.
    #[test]
    fn an_operator_is_never_counted() {
        let keys = run_of(press('d'), RUN * 2);
        assert!(said(&keys).is_empty(), "6d is not a command");
    }

    #[test]
    fn the_same_habit_is_mentioned_once_and_then_left_alone() {
        let mut keys = run_of(press('x'), RUN);
        for _ in 0..2 {
            keys.push(stop());
            keys.extend(run_of(press('x'), RUN));
        }
        assert_eq!(said(&keys).len(), 1, "once is advice, twice is nagging");
    }

    #[test]
    fn far_enough_apart_is_worth_saying_again() {
        let mut coach = Coach::default();
        let mut spoke = 0;
        for round in 0..2 {
            let mut keys = vec![stop()];
            keys.extend(run_of(press('x'), RUN));
            for (index, key) in keys.into_iter().enumerate() {
                let at = index as u64 + round * PATIENCE;
                spoke += coach.notice(key, true, at).is_some() as usize;
            }
        }
        assert_eq!(spoke, 2, "a habit picked up again is worth a word");
    }

    #[test]
    fn insert_mode_is_typing_rather_than_moving() {
        let mut coach = Coach::default();
        let spoke = (0..RUN * 2)
            .filter_map(|index| coach.notice(press('j'), false, index as u64))
            .count();
        assert_eq!(spoke, 0, "a j while inserting is a j");
    }

    #[test]
    fn the_arrows_get_told_about_hjkl() {
        let keys = run_of(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), RUN);
        assert_eq!(said(&keys).len(), 1);
        assert!(said(&keys)[0].contains("h j k l"), "{:?}", said(&keys));
    }

    /// Arrows and `j` are not the same habit, so they are not one run.
    #[test]
    fn an_arrow_breaks_a_run_of_keys() {
        let arrow = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        let mut keys = run_of(press('j'), RUN - 1);
        keys.extend(run_of(arrow, RUN - 1));
        assert!(said(&keys).is_empty(), "neither one of them got to {RUN}");
    }

    #[test]
    fn a_chord_is_not_a_run() {
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        let keys = run_of(ctrl_d, RUN * 2);
        assert!(said(&keys).is_empty(), "^d is scrolling, and has no count to type");
    }
}
