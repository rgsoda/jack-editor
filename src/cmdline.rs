//! The `:` and `/` line: editing it, and what was typed on it before.
//!
//! A command line is a one-line editor, and it was one only in the sense that
//! characters went on the end and backspace took them off again. This is the
//! rest of it: a cursor that can be in the middle, the chords that move it,
//! and a history that Up walks.
//!
//! Text and cursor together, because the two disagreeing is the whole of what
//! goes wrong with a line editor. Nothing here knows about the editor, so the
//! awkward parts - where a word ends, what Up does when you have typed half a
//! command already - are testable on their own.

/// How many lines of each kind are kept. Past this the oldest go; a command
/// from a thousand commands ago is not one Up is going to find.
pub const LIMIT: usize = 200;

/// What is typed, and where the cursor is in it. The cursor is a character
/// offset, never a byte one, so that a line with anything but ASCII on it
/// behaves.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Line {
    text: String,
    at: usize,
}

impl Line {
    pub fn new(text: impl Into<String>) -> Line {
        let text = text.into();
        let at = text.chars().count();
        Line { text, at }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Put the whole line back, with the cursor at its end: what history does.
    pub fn set(&mut self, text: impl Into<String>) {
        *self = Line::new(text);
    }

    pub fn insert(&mut self, c: char) {
        let at = self.byte(self.at);
        self.text.insert(at, c);
        self.at += 1;
    }

    pub fn insert_str(&mut self, text: &str) {
        let at = self.byte(self.at);
        self.text.insert_str(at, text);
        self.at += text.chars().count();
    }

    /// Backspace. `false` when there was nothing to delete, which is what
    /// closes an empty prompt.
    pub fn delete_back(&mut self) -> bool {
        if self.at == 0 {
            return false;
        }
        let (from, to) = (self.byte(self.at - 1), self.byte(self.at));
        self.text.replace_range(from..to, "");
        self.at -= 1;
        true
    }

    /// Delete: the character the cursor is on, which at the end is none.
    pub fn delete_forward(&mut self) {
        let count = self.text.chars().count();
        if self.at >= count {
            return;
        }
        let (from, to) = (self.byte(self.at), self.byte(self.at + 1));
        self.text.replace_range(from..to, "");
    }

    /// `^w`: the word before the cursor, and the run of anything-but-word
    /// characters in front of it, the way a shell does it.
    pub fn delete_word_back(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        let mut start = self.at;
        while start > 0 && !chars[start - 1].is_alphanumeric() {
            start -= 1;
        }
        while start > 0 && chars[start - 1].is_alphanumeric() {
            start -= 1;
        }
        let (from, to) = (self.byte(start), self.byte(self.at));
        self.text.replace_range(from..to, "");
        self.at = start;
    }

    /// `^u`: everything before the cursor, as a shell's `^u` does rather than
    /// as vim's, which clears the lot.
    pub fn delete_to_start(&mut self) {
        let to = self.byte(self.at);
        self.text.replace_range(..to, "");
        self.at = 0;
    }

    /// `^k`: everything from the cursor on.
    pub fn delete_to_end(&mut self) {
        let from = self.byte(self.at);
        self.text.truncate(from);
    }

    pub fn left(&mut self) {
        self.at = self.at.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.at = (self.at + 1).min(self.text.chars().count());
    }

    pub fn word_left(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        while self.at > 0 && !chars[self.at - 1].is_alphanumeric() {
            self.at -= 1;
        }
        while self.at > 0 && chars[self.at - 1].is_alphanumeric() {
            self.at -= 1;
        }
    }

    pub fn word_right(&mut self) {
        let chars: Vec<char> = self.text.chars().collect();
        while self.at < chars.len() && !chars[self.at].is_alphanumeric() {
            self.at += 1;
        }
        while self.at < chars.len() && chars[self.at].is_alphanumeric() {
            self.at += 1;
        }
    }

    pub fn home(&mut self) {
        self.at = 0;
    }

    pub fn end(&mut self) {
        self.at = self.text.chars().count();
    }

    /// The text up to the cursor, for measuring where to draw it.
    pub fn before_cursor(&self) -> &str {
        &self.text[..self.byte(self.at)]
    }

    /// Replace the characters from `start` to the cursor, which is what a
    /// `tab` completion does to the word being completed.
    pub fn replace_from(&mut self, start: usize, text: &str) {
        let (from, to) = (self.byte(start), self.byte(self.at));
        self.text.replace_range(from..to, text);
        self.at = start + text.chars().count();
    }

    fn byte(&self, at: usize) -> usize {
        self.text
            .char_indices()
            .nth(at)
            .map_or(self.text.len(), |(index, _)| index)
    }
}

/// The lines typed before, oldest first, and where Up has walked to.
///
/// Up only offers lines that start with what was typed before it was first
/// pressed, which is vim's rule and the one worth having: `:se` then Up is a
/// list of the `:set` commands, not a list of everything.
#[derive(Default, Clone, Debug)]
pub struct History {
    lines: Vec<String>,
    /// Where in `lines` the walk is, counted from the end, and the line it
    /// started from. `None` when Up has not been pressed.
    walking: Option<Walk>,
}

#[derive(Clone, Debug)]
struct Walk {
    /// What was typed before the walk began, and so what it filters by.
    stem: String,
    /// The index in `lines` of what is showing, or `lines.len()` for the stem
    /// itself - what Down comes back to.
    at: usize,
}

impl History {
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Remember a line that was run. The same line twice running is one line,
    /// and an empty one is not a line at all.
    pub fn push(&mut self, line: &str) {
        self.walking = None;
        if line.trim().is_empty() {
            return;
        }
        if self.lines.last().is_some_and(|last| last == line) {
            return;
        }
        self.lines.push(line.to_string());
        if self.lines.len() > LIMIT {
            let over = self.lines.len() - LIMIT;
            self.lines.drain(..over);
        }
    }

    /// Load lines read back from disk, oldest first.
    pub fn fill(&mut self, lines: Vec<String>) {
        self.lines = lines;
        if self.lines.len() > LIMIT {
            let over = self.lines.len() - LIMIT;
            self.lines.drain(..over);
        }
    }

    /// A keystroke that is not Up or Down ends the walk: what is on the line
    /// is now yours rather than something being offered.
    pub fn stop(&mut self) {
        self.walking = None;
    }

    /// Up: the previous line that starts with what was typed, or `None` when
    /// there is no earlier one.
    pub fn back(&mut self, typed: &str) -> Option<String> {
        let walk = self.walking.get_or_insert_with(|| Walk {
            stem: typed.to_string(),
            at: self.lines.len(),
        });
        let found = self.lines[..walk.at]
            .iter()
            .rposition(|line| line.starts_with(&walk.stem))?;
        walk.at = found;
        Some(self.lines[found].clone())
    }

    /// Down: back towards what was typed, and then back to it.
    pub fn forward(&mut self) -> Option<String> {
        let walk = self.walking.as_mut()?;
        let next = self.lines[walk.at + 1..]
            .iter()
            .position(|line| line.starts_with(&walk.stem))
            .map(|index| walk.at + 1 + index);
        match next {
            Some(index) => {
                walk.at = index;
                Some(self.lines[index].clone())
            }
            // Past the newest: what was typed before the walk started.
            None => {
                let stem = walk.stem.clone();
                self.walking = None;
                Some(stem)
            }
        }
    }
}

/// `$XDG_STATE_HOME/jack/history`, beside the cursor positions. State rather
/// than config: jack writes it, and losing it loses nothing you made.
pub fn store_path() -> Option<std::path::PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
        _ => std::path::PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(state.join("jack").join("history"))
}

/// The two lists as they were left, oldest first: commands, then searches.
///
/// One file, a line each, and the first character says which list it belongs
/// to - a `:` line and a `/` line can both contain anything, but neither can
/// contain a newline, so the line itself is the record.
pub fn load(store: Option<&std::path::Path>) -> (Vec<String>, Vec<String>) {
    let text = store.and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
    let mut commands = Vec::new();
    let mut searches = Vec::new();
    for line in text.lines() {
        match line.split_at_checked(1) {
            Some((":", rest)) => commands.push(rest.to_string()),
            Some(("/", rest)) => searches.push(rest.to_string()),
            _ => {}
        }
    }
    (commands, searches)
}

/// Write them back on the way out, newest last, the same way round.
pub fn save(
    store: Option<&std::path::Path>,
    commands: &[String],
    searches: &[String],
) -> std::io::Result<()> {
    let Some(store) = store else {
        return Ok(());
    };
    if let Some(dir) = store.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = String::new();
    for line in commands {
        text.push(':');
        text.push_str(line);
        text.push('\n');
    }
    for line in searches {
        text.push('/');
        text.push_str(line);
        text.push('\n');
    }
    // Through a temporary, as the positions are: two jacks quitting at once
    // should leave one of the two lists, not half of each.
    let temporary = store.with_extension(format!("{}", std::process::id()));
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_and_the_text_agree_wherever_it_is() {
        let mut line = Line::new("set numb");
        line.left();
        line.left();
        line.insert('e');
        assert_eq!(line.text(), "set nuemb");
        assert_eq!(line.before_cursor(), "set nue");

        line.delete_back();
        line.delete_forward();
        assert_eq!(line.text(), "set nub");

        // A word at a time, both ways, and the ends.
        line.home();
        line.word_right();
        assert_eq!(line.before_cursor(), "set");
        line.end();
        line.delete_word_back();
        assert_eq!(line.text(), "set ");
        assert!(line.delete_back());
        line.delete_to_start();
        assert!(line.is_empty());
        assert!(!line.delete_back(), "nothing left to take off");
    }

    #[test]
    fn the_kills_take_the_side_of_the_cursor_they_name() {
        let mut line = Line::new("one two three");
        line.word_left();
        line.delete_to_end();
        assert_eq!(line.text(), "one two ");

        let mut line = Line::new("one two three");
        line.word_left();
        line.delete_to_start();
        assert_eq!(line.text(), "three");
    }

    #[test]
    fn characters_wider_than_a_byte_are_still_one_character() {
        let mut line = Line::new("łódź");
        line.left();
        line.insert('x');
        assert_eq!(line.text(), "łódxź");
        line.delete_back();
        assert_eq!(line.text(), "łódź");
    }

    #[test]
    fn a_history_file_keeps_the_two_lists_apart() {
        let dir = std::env::temp_dir().join(format!("jack-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a directory to write in");
        let store = dir.join("history");
        save(Some(&store), &["set list".into(), "w".into()], &["fn main".into()])
            .expect("writing the history");

        let (commands, searches) = load(Some(&store));
        assert_eq!(commands, ["set list", "w"]);
        assert_eq!(searches, ["fn main"]);

        // Nowhere to keep it is not an error, and neither is a file that is
        // not there yet.
        assert!(save(None, &[], &[]).is_ok());
        assert_eq!(load(Some(&dir.join("nothing"))), (Vec::new(), Vec::new()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn up_offers_the_lines_that_start_with_what_is_typed() {
        let mut history = History::default();
        for line in ["set number", "w", "set list", "q"] {
            history.push(line);
        }
        // The same line twice running is one line, and a blank one is none.
        history.push("q");
        history.push("   ");
        assert_eq!(history.lines().len(), 4);

        // Nothing typed: everything, newest first.
        assert_eq!(history.back("").as_deref(), Some("q"));
        assert_eq!(history.back("").as_deref(), Some("set list"));
        assert_eq!(history.forward().as_deref(), Some("q"));
        assert_eq!(history.forward().as_deref(), Some(""), "back to what was typed");

        // Typed something: only the lines that start with it.
        history.stop();
        assert_eq!(history.back("set").as_deref(), Some("set list"));
        assert_eq!(history.back("set").as_deref(), Some("set number"));
        assert_eq!(history.back("set"), None, "no earlier one to offer");
        assert_eq!(history.forward().as_deref(), Some("set list"));
        assert_eq!(history.forward().as_deref(), Some("set"));
    }
}
