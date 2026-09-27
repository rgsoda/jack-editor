//! `:hook` - a command to run when something happens.
//!
//! Vim calls these autocommands and has a hundred events; the two worth having
//! are *a file was opened* and *a file was written*, because those are the two
//! moments where what you want done depends on which file it is. `:hook save
//! *.rs !cargo fmt` and `:hook open *.md set tw=72` are the whole of it.
//!
//! This file is the rule and the glob, and nothing else: no editor, no running
//! of commands, no order of events. That keeps the matching - which is the part
//! with the awkward cases in it - testable on its own.

/// When a hook runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A file has been read into a buffer, and that buffer is the one in front
    /// of you.
    Open,
    /// A file has been written, successfully. Not a write that failed, and not
    /// a listing written back to its directory.
    Save,
}

impl Event {
    pub fn named(name: &str) -> Option<Event> {
        match name {
            "open" => Some(Event::Open),
            "save" | "write" => Some(Event::Save),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Event::Open => "open",
            Event::Save => "save",
        }
    }
}

/// One rule: on this event, for files like this, run this.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    pub event: Event,
    /// A glob, or empty for every file.
    pub glob: String,
    /// A command line, as it would be typed after `:`.
    pub command: String,
}

impl Hook {
    /// The line that would make it, for `:hook` to read them back.
    pub fn line(&self) -> String {
        match self.glob.is_empty() {
            true => format!("{} {}", self.event.name(), self.command),
            false => format!("{} {} {}", self.event.name(), self.glob, self.command),
        }
    }

    /// Whether this rule is about `path`, which is as the buffer has it -
    /// relative to the working directory, most of the time.
    pub fn about(&self, event: Event, path: &str) -> bool {
        if self.event != event {
            return false;
        }
        if self.glob.is_empty() {
            return true;
        }
        // A glob with no separator in it is about the name, so `*.rs` matches
        // however deep the file is; one with a separator is about the path, so
        // `src/*.rs` means what it looks like.
        let subject = match self.glob.contains('/') {
            true => path,
            false => path.rsplit('/').next().unwrap_or(path),
        };
        matches(&self.glob, subject)
    }
}

/// `*` against a name, and nothing else. `?` is not worth the line it would
/// cost: nobody has ever wanted it and every `?` in a file name is a surprise.
///
/// Written as a walk rather than a regex so that a glob is never a pattern by
/// accident - `a.b` is a file with a dot in it, not any character at all.
pub fn matches(glob: &str, name: &str) -> bool {
    let (glob, name) = (glob.as_bytes(), name.as_bytes());
    // Where in `glob` and `name` we are, and where to come back to if the run
    // we are in turns out to be the wrong one: the last `*` and how much it
    // had swallowed. One pass, and no recursion to blow up on `****`.
    let (mut g, mut n) = (0, 0);
    let (mut star, mut after) = (None, 0);
    while n < name.len() {
        match glob.get(g) {
            Some(b'*') => {
                star = Some(g);
                g += 1;
                after = n;
            }
            Some(&byte) if byte == name[n] => {
                g += 1;
                n += 1;
            }
            // Backtrack: let the last `*` swallow one more.
            _ => match star {
                Some(at) => {
                    g = at + 1;
                    after += 1;
                    n = after;
                }
                None => return false,
            },
        }
    }
    // What is left of the glob can only be stars.
    glob[g..].iter().all(|&byte| byte == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_is_stars_and_nothing_else() {
        assert!(matches("*.rs", "main.rs"));
        assert!(!matches("*.rs", "main.rst"));
        assert!(matches("*", "anything"));
        assert!(matches("main.rs", "main.rs"));
        assert!(!matches("main.rs", "main_rs"), "a dot is a dot, not any character");
        assert!(matches("*test*", "my_test_file.py"));
        assert!(matches("src/*.rs", "src/main.rs"));
        assert!(!matches("*.rs", ""), "nothing is not a rust file");
        assert!(matches("*", ""), "but a star matches nothing at all");

        // The backtracking cases, which are why this is not a one-liner.
        assert!(matches("*a*b", "xaybzb"));
        assert!(!matches("*a*b", "xaybz"));
        assert!(matches("**.rs", "a.rs"));
        assert!(matches("a*", "a"));
    }

    #[test]
    fn a_name_glob_is_about_the_name_and_a_path_glob_about_the_path() {
        let name = Hook { event: Event::Save, glob: "*.rs".into(), command: "format".into() };
        assert!(name.about(Event::Save, "src/editor/lsp.rs"), "however deep it is");
        assert!(!name.about(Event::Open, "src/editor/lsp.rs"), "the wrong event");
        assert!(!name.about(Event::Save, "README.md"));

        let path = Hook { event: Event::Save, glob: "src/*.rs".into(), command: "format".into() };
        assert!(path.about(Event::Save, "src/main.rs"));
        assert!(!path.about(Event::Save, "tests/main.rs"), "a glob with a slash means the path");

        // No glob at all is every file.
        let all = Hook { event: Event::Open, glob: String::new(), command: "set number".into() };
        assert!(all.about(Event::Open, "anything.txt"));
        assert_eq!(all.line(), "open set number");
        assert_eq!(name.line(), "save *.rs format");
    }

    #[test]
    fn write_is_another_name_for_save_and_the_rest_are_not_events() {
        assert_eq!(Event::named("save"), Some(Event::Save));
        assert_eq!(Event::named("write"), Some(Event::Save));
        assert_eq!(Event::named("open"), Some(Event::Open));
        assert_eq!(Event::named("Save"), None, "lowercase, like every other command");
        assert_eq!(Event::named("bufwritepost"), None);
    }
}
