//! What the `:` line knows about itself: the commands it takes, the options
//! `:set` takes, and what `tab` should offer for a half-typed one.
//!
//! The tables here are what completion reads; the commands themselves are run
//! by `Editor::run_command`. Two lists that have to agree, so a test walks this
//! one through that one and fails if a name here is not a command there.

use std::path::PathBuf;

/// What follows a command's name, and so what `tab` offers after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Argument {
    /// `:q`, `:noh` - nothing to complete.
    None,
    /// `:e`, `:w` - a file.
    Path,
    /// `:set` - an option name.
    Option,
}

pub struct Command {
    pub name: &'static str,
    pub argument: Argument,
}

/// The commands `tab` offers, in the order it offers them. Aliases are left
/// out: `:w` still works, but completing to `write` from nothing typed is
/// noise, and completing `w` to `w` is not a completion.
pub static COMMANDS: &[Command] = &[
    Command { name: "write", argument: Argument::Path },
    Command { name: "quit", argument: Argument::None },
    Command { name: "wq", argument: Argument::None },
    Command { name: "edit", argument: Argument::Path },
    Command { name: "set", argument: Argument::Option },
    Command { name: "nohlsearch", argument: Argument::None },
];

/// Every `:set` option, spelled the long way. The short forms (`nu`, `sw`,
/// `ai`) still work; they are not offered, because a completion you have to
/// decode is not help.
pub static OPTIONS: &[&str] = &[
    "autocomplete=",
    "noautocomplete",
    "autoindent",
    "noautoindent",
    "cursorline",
    "nocursorline",
    "emacs",
    "noemacs",
    "expandtab",
    "noexpandtab",
    "glyphs",
    "noglyphs",
    "hybrid",
    "number",
    "nonumber",
    "relativenumber",
    "semicolon=",
    "shiftwidth=",
    "signs",
    "nosigns",
    "tabline=",
    "notabline",
    "trim",
    "notrim",
];

/// The values the options that take one will accept, so `tab` can finish
/// `:set tabline=` as well as reach it.
static VALUES: &[(&str, &[&str])] = &[
    ("tabline", &["off", "auto", "always"]),
    ("semicolon", &["find", "command"]),
    ("shiftwidth", &["2", "4", "8"]),
    ("autocomplete", &["0", "2", "3"]),
];

/// What `tab` should offer for `input`: where in it the replacement starts, and
/// what could go there. Empty when there is nothing to offer.
///
/// The prompt has no cursor to move, so the word being completed is always the
/// last one - which is what makes this a function of the text and nothing else.
pub fn complete(input: &str) -> (usize, Vec<String>) {
    let start = input.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let word = &input[start..];
    let before = input[..start].trim();

    // The first word is the command itself. A leading `!` or a line number is
    // not a command and has nothing to offer.
    if before.is_empty() {
        let matches = COMMANDS
            .iter()
            .filter(|command| command.name.starts_with(word))
            .map(|command| command.name.to_string())
            .collect();
        return (start, matches);
    }

    let name = before.split_whitespace().next().unwrap_or("");
    match argument_for(name) {
        Argument::Option => (start, options(word)),
        // A path replacement covers the last segment only, leaving the
        // directories already typed where they are.
        Argument::Path => {
            let (offset, found) = paths(word);
            (start + offset, found)
        }
        Argument::None => (start, Vec::new()),
    }
}

/// What a command's name says its argument is, aliases included - those have
/// to be understood even though they are never offered.
fn argument_for(name: &str) -> Argument {
    let full = match name.trim_end_matches('!') {
        "w" => "write",
        "e" => "edit",
        "q" => "quit",
        "x" => "wq",
        "noh" => "nohlsearch",
        other => other,
    };
    COMMANDS
        .iter()
        .find(|command| command.name == full)
        .map_or(Argument::None, |command| command.argument)
}

/// `:set ` completions: the option names, or the values of one that has been
/// named and given its `=`.
fn options(word: &str) -> Vec<String> {
    if let Some((name, typed)) = word.split_once('=') {
        let Some((_, values)) = VALUES.iter().find(|(option, _)| *option == name) else {
            return Vec::new();
        };
        return values
            .iter()
            .filter(|value| value.starts_with(typed))
            .map(|value| format!("{name}={value}"))
            .collect();
    }
    OPTIONS
        .iter()
        .filter(|option| option.starts_with(word))
        .map(|option| option.to_string())
        .collect()
}

/// File name completion, one directory deep: what is beside the partial path,
/// with a `/` on the directories so another `tab` can go on into them.
///
/// `read_dir` of one directory rather than the picker's walk: this is a path
/// being typed, not a file being looked for, and the answer is always the
/// names in one place.
fn paths(word: &str) -> (usize, Vec<String>) {
    let (directory, partial) = match word.rfind('/') {
        Some(i) => (&word[..=i], &word[i + 1..]),
        None => ("", word),
    };
    let root = match directory.is_empty() {
        true => PathBuf::from("."),
        false => PathBuf::from(directory),
    };

    let Ok(entries) = std::fs::read_dir(&root) else {
        return (0, Vec::new());
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            // A leading dot has to be asked for, as it is in every shell.
            if !name.starts_with(partial) || (name.starts_with('.') && !partial.starts_with('.')) {
                return None;
            }
            let directory = entry.path().is_dir();
            Some(match directory {
                true => format!("{name}/"),
                false => name,
            })
        })
        .collect();
    found.sort();
    (word.len() - partial.len(), found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(input: &str) -> Vec<String> {
        complete(input).1
    }

    #[test]
    fn a_half_typed_command_offers_the_commands() {
        assert_eq!(names("w"), ["write", "wq"]);
        assert_eq!(names("q"), ["quit"]);
        assert!(names("").len() == COMMANDS.len());
        assert!(names("zz").is_empty());
    }

    #[test]
    fn set_offers_options_and_then_their_values() {
        assert_eq!(names("set auto"), ["autocomplete=", "autoindent"]);
        assert_eq!(names("set tabline="), ["tabline=off", "tabline=auto", "tabline=always"]);
        assert_eq!(names("set semicolon=c"), ["semicolon=command"]);
        // An option that takes no value has none to offer.
        assert!(names("set trim=").is_empty());
    }

    #[test]
    fn a_command_that_takes_nothing_offers_nothing() {
        assert!(names("quit ").is_empty());
        assert!(names("nohlsearch any").is_empty());
    }

    #[test]
    fn the_replacement_starts_at_the_word_being_typed() {
        assert_eq!(complete("set auto").0, 4);
        assert_eq!(complete("w").0, 0);
    }
}
