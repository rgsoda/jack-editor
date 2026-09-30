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
    /// Hands back what to say, if this is the moment to say anything. That
    /// moment is the end of a run rather than the middle of one: a message
    /// lasts until the next key, and the next key of a run you are leaning on
    /// arrives too soon to read a word during. So the word comes on the key
    /// that breaks the run, about the run that just broke.
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

        if counted.is_some() && counted == self.last {
            self.run += 1;
            return None;
        }
        let said = self.ended(pressed);
        (self.last, self.run) = (counted, counted.is_some() as usize);
        said
    }

    /// A run has just finished: whether it was worth a word, with the count
    /// cleared either way.
    fn ended(&mut self, pressed: u64) -> Option<String> {
        let (last, run) = (self.last.take(), std::mem::take(&mut self.run));
        let last = last.filter(|_| run >= RUN)?;

        // Said something recently: the run still ends, but quietly. A habit
        // is worth mentioning once, and every time is what makes people turn
        // a thing off.
        if self.spoke_at.is_some_and(|at| pressed.saturating_sub(at) < PATIENCE) {
            return None;
        }
        self.spoke_at = Some(pressed);

        Some(match last {
            Counted::Key(key) => format!("{run} {key} in a row - {run}{key} is two keys"),
            Counted::Arrow => "the arrows work - h j k l are nearer, and take a count".to_string(),
        })
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

    /// `count` of the same key, and then something that is not it.
    fn run_of(key: KeyEvent, count: usize) -> Vec<KeyEvent> {
        let mut keys: Vec<_> = std::iter::repeat_n(key, count).collect();
        keys.push(stop());
        keys
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

    /// The word comes on the key that breaks the run, not in the middle of
    /// it: a message lasts until the next key, and during a run the next key
    /// is already on its way.
    #[test]
    fn nothing_is_said_while_the_run_is_still_going() {
        let mut coach = Coach::default();
        let leaning: Vec<_> = (0..RUN * 2)
            .filter_map(|index| coach.notice(press('j'), true, index as u64))
            .collect();
        assert!(leaning.is_empty(), "{leaning:?} would be wiped by the next j");
        let said = coach.notice(stop(), true, RUN as u64 * 2);
        assert_eq!(said, Some(format!("{0} j in a row - {0}j is two keys", RUN * 2)));
    }

    #[test]
    fn a_different_key_starts_the_count_again() {
        let mut keys: Vec<_> = std::iter::repeat_n(press('j'), RUN - 1).collect();
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
        keys.extend(run_of(press('x'), RUN));
        keys.extend(run_of(press('x'), RUN));
        assert_eq!(said(&keys).len(), 1, "once is advice, twice is nagging");
    }

    #[test]
    fn far_enough_apart_is_worth_saying_again() {
        let mut coach = Coach::default();
        let mut spoke = 0;
        for round in 0..2 {
            for (index, key) in run_of(press('x'), RUN).into_iter().enumerate() {
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
        let mut keys: Vec<_> = std::iter::repeat_n(press('j'), RUN - 1).collect();
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
