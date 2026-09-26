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
    Command { name: "wall", argument: Argument::None },
    Command { name: "wqall", argument: Argument::None },
    Command { name: "edit", argument: Argument::Path },
    Command { name: "split", argument: Argument::Path },
    Command { name: "vsplit", argument: Argument::Path },
    Command { name: "close", argument: Argument::None },
    Command { name: "only", argument: Argument::None },
    Command { name: "bdelete", argument: Argument::None },
    Command { name: "lsp", argument: Argument::None },
    Command { name: "delete", argument: Argument::None },
    Command { name: "global", argument: Argument::None },
    Command { name: "format", argument: Argument::None },
    Command { name: "hunk", argument: Argument::None },
    Command { name: "blame", argument: Argument::None },
    Command { name: "stage", argument: Argument::None },
    Command { name: "revert", argument: Argument::None },
    Command { name: "shell", argument: Argument::None },
    Command { name: "suspend", argument: Argument::None },
    Command { name: "map", argument: Argument::None },
    Command { name: "unmap", argument: Argument::None },
    Command { name: "set", argument: Argument::Option },
    Command { name: "setw", argument: Argument::Option },
    Command { name: "help", argument: Argument::None },
    Command { name: "make", argument: Argument::None },
    Command { name: "cd", argument: Argument::Path },
    Command { name: "pwd", argument: Argument::None },
    Command { name: "dog", argument: Argument::None },
    Command { name: "ai", argument: Argument::None },
    Command { name: "config", argument: Argument::None },
    Command { name: "preview", argument: Argument::None },
    Command { name: "guifonts", argument: Argument::None },
    Command { name: "nohlsearch", argument: Argument::None },
];

/// Which setting an argument to `:set` is about, so a config file can be
/// asked whether it already sets that one. `number`, `nonumber` and
/// `relativenumber` are three ways to write one setting, and `shiftwidth=2`
/// is that setting whatever the number is. A short form, or a name no table
/// knows, is taken at its word: it is still the same setting as itself.
pub fn setting_name(option: &str) -> &str {
    let option = option.trim();
    let word = option.split('=').next().unwrap_or(option).trim();
    for setting in SETTINGS {
        let hit = match setting.kind {
            Kind::Flag(_) => word == setting.name || word.strip_prefix("no") == Some(setting.name),
            Kind::Word(words) => words.contains(&word),
            Kind::Value(..) => word == setting.name,
        };
        if hit {
            return setting.name;
        }
    }
    word.strip_prefix("no").unwrap_or(word)
}

/// What one `:set` option takes.
pub enum Kind {
    /// On or off: `set trim`, `set notrim`. The bool is the default.
    Flag(bool),
    /// One word out of several, each spelled out in full: `set number`,
    /// `set hybrid`. The first is the default.
    Word(&'static [&'static str]),
    /// `name=value`. The list is what `tab` offers; the string is the default,
    /// which need not be one of them.
    Value(&'static [&'static str], &'static str),
}

pub struct Setting {
    pub name: &'static str,
    pub kind: Kind,
    /// One line, written to the config file above the setting. What it does,
    /// not how to spell it - the spelling is on the line below it.
    pub about: &'static str,
}

/// Every `:set` option, spelled the long way, with what it does and what it
/// does by default. The short forms (`nu`, `sw`, `ai`) still work; they are
/// not offered, because a completion you have to decode is not help.
///
/// One table, three readers: `tab` completion, the generated config file, and
/// the tests that walk it through `:set` to check nothing here has drifted
/// from what the editor actually accepts.
pub static SETTINGS: &[Setting] = &[
    Setting {
        name: "number",
        kind: Kind::Word(&["number", "nonumber", "relativenumber", "hybrid"]),
        about: "Line numbers: absolute, off, relative, or hybrid (relative but \
                the cursor's own line absolute).",
    },
    Setting {
        name: "cursorline",
        kind: Kind::Flag(true),
        about: "Tint the row the cursor is on, the whole width of the screen.",
    },
    Setting {
        name: "signs",
        kind: Kind::Flag(true),
        about: "Git signs in the gutter: + ~ _ for added, changed and deleted.",
    },
    Setting {
        name: "glyphs",
        kind: Kind::Flag(true),
        about: "Draw the status line with Nerd Font glyphs. Off is plain ASCII, \
                for a terminal whose font has not been patched.",
    },
    Setting {
        name: "dog",
        kind: Kind::Flag(true),
        about: "A dog in the status line. It runs while the cursor moves and \
                sits down where it stopped. Needs glyphs.",
    },
    Setting {
        name: "tabline",
        kind: Kind::Value(&["off", "auto", "always"], "auto"),
        about: "The buffer list along the top: auto shows it once a second file \
                is open.",
    },
    Setting {
        name: "shiftwidth",
        kind: Kind::Value(&["2", "4", "8"], "4"),
        about: "How wide one step of indentation is, 1 to 16. A file that is \
                already indented is followed instead; this is what to do with \
                one that is not.",
    },
    Setting {
        name: "expandtab",
        kind: Kind::Flag(false),
        about: "Indent with spaces rather than tabs. A file that already \
                indents one way or the other is followed instead, so this is \
                the answer for a new file. Typing `:set expandtab` while \
                editing overrules what was read.",
    },
    Setting {
        name: "autoindent",
        kind: Kind::Flag(true),
        about: "Keep the indent on enter, and take it from the grammar where \
                there is one.",
    },
    Setting {
        name: "autopairs",
        kind: Kind::Flag(true),
        about: "Close brackets and quotes as they are opened, step over the \
                closer when it is typed, and take both back with backspace.",
    },
    Setting {
        name: "wrap",
        kind: Kind::Flag(false),
        about: "Show a line too long for the window on as many rows as it \
                takes, broken at a space where there is one, instead of \
                scrolling sideways. j and k still move by lines of the file.",
    },
    Setting {
        name: "inlayhints",
        kind: Kind::Flag(true),
        about: "Show the types and parameter names a language server would \
                write into the code, dimmed, between the characters they are \
                about. They are drawn, not typed: the cursor steps over them.",
    },
    Setting {
        name: "undofile",
        kind: Kind::Flag(true),
        about: "Keep each file's undo history when it is written, in the state \
                directory, so undo reaches back past a restart. Used only while \
                the file is still the text the history ends at.",
    },
    Setting {
        name: "autocomplete",
        kind: Kind::Value(&["0", "2", "3"], "2"),
        about: "How many characters of a word bring the completion popup up on \
                their own. 0 turns it off; ^n still works.",
    },
    Setting {
        name: "trim",
        kind: Kind::Flag(true),
        about: "Strip trailing whitespace from changed lines when saving.",
    },
    Setting {
        name: "emacs",
        kind: Kind::Flag(false),
        about: "Emacs chords in insert mode: ^a ^e ^k ^y and the rest. They \
                win over the insert-mode keys they share.",
    },
    Setting {
        name: "lsp",
        kind: Kind::Flag(true),
        about: "Start a language server for files that have one installed: diagnostics, and gd across files.",
    },
    Setting {
        name: "aiprg",
        kind: Kind::Value(&["claude -p", "llm", "ollama run llama3"], ""),
        about: "What `:ai` runs: a program that reads a prompt on stdin and \
                writes an answer on stdout. Empty means `:ai` does nothing, \
                which is how it stays until you name one.",
    },
    Setting {
        name: "dogname",
        kind: Kind::Value(&["Rex", "Bluey", "Laika"], ""),
        about: "What the dog in the status line answers to. It turns up when \
                it barks and when `:dog` says how far it has run.",
    },
    Setting {
        name: "makeprg",
        kind: Kind::Value(&["make", "cargo check", "cargo test"], ""),
        about: "What `:make` runs when it is not told what to run. Empty is \
                whatever builds the project you are in - a Makefile, a \
                Cargo.toml, a go.mod.",
    },
    Setting {
        name: "guifont",
        kind: Kind::Value(&["monospace"], "monospace"),
        about: "The window's font, as fontconfig names it - `monospace`, or \
                `JetBrainsMono Nerd Font`. Only `jack --gui` reads it.",
    },
    Setting {
        name: "guifontsize",
        kind: Kind::Value(&["12", "14", "16", "18"], "15"),
        about: "How big the window's text is, in pixels. `ctrl` with `+`, `-` \
                or the wheel changes it while running; this is where it starts.",
    },
    Setting {
        name: "semicolon",
        kind: Kind::Value(&["find", "command"], "find"),
        about: "What ; does: repeat the last f/t, or open the command line the \
                way : does.",
    },
];

/// The option names `tab` offers: a flag both ways round, a word for each of
/// its spellings, and a `=` on the ones that take a value so another `tab`
/// reaches the values.
///
/// Sorted, unlike the table itself. The file reads in the order a person would
/// want to read it; a completion list reads in the order a person would look
/// something up in.
pub fn option_names() -> Vec<String> {
    let mut names = Vec::new();
    for setting in SETTINGS {
        match setting.kind {
            Kind::Flag(_) => {
                names.push(setting.name.to_string());
                names.push(format!("no{}", setting.name));
            }
            Kind::Word(words) => names.extend(words.iter().map(|word| word.to_string())),
            Kind::Value(..) => names.push(format!("{}=", setting.name)),
        }
    }
    names.sort();
    names
}

/// The `:set` line that puts a setting at its default - which is what the
/// generated config file is made of.
pub fn default_line(setting: &Setting) -> String {
    match setting.kind {
        Kind::Flag(true) => format!("set {}", setting.name),
        Kind::Flag(false) => format!("set no{}", setting.name),
        Kind::Word(words) => format!("set {}", words[0]),
        Kind::Value(_, default) => format!("set {}={default}", setting.name),
    }
}

/// The config file as it ships: every setting at its default, with a line
/// above it saying what it does and what else it takes. Written out rather
/// than commented out, so changing one is editing a word rather than
/// remembering a spelling.
pub fn default_config() -> String {
    let mut out = String::new();
    for line in [
        "# jack's config: one command per line, written as it would be typed",
        "# after `:`. Blank lines and lines starting with # are ignored, and a",
        "# line that is not understood stops the file there and says so.",
        "#",
        "# Everything below is a default, so a fresh file changes nothing.",
    ] {
        out.push_str(line);
        out.push('\n');
    }
    for line in [
        "",
        "# `<space>` is yours: `map {key} {command}` says what one of its keys",
        "# does, written as it would be typed after `:`. `:map` on its own",
        "# lists them, and `:unmap {key}` takes one back. The few leader keys",
        "# jack already uses - b f s d n x ? - are not yours to take.",
        "#",
        "# map g !lazygit",
        "# map t !cargo test",
    ] {
        out.push_str(line);
        out.push('\n');
    }

    for setting in SETTINGS {
        out.push('\n');
        for line in wrap(setting.about, 72) {
            out.push_str(&format!("# {line}\n"));
        }
        if let Some(alternatives) = alternatives(setting) {
            out.push_str(&format!("# {alternatives}\n"));
        }
        out.push_str(&default_line(setting));
        out.push('\n');
    }
    out
}

/// The other spellings of a setting, for the comment above it.
fn alternatives(setting: &Setting) -> Option<String> {
    match setting.kind {
        Kind::Flag(_) => Some(format!("set {0} | set no{0}", setting.name)),
        Kind::Word(words) => Some(format!("set {}", words.join(" | set "))),
        Kind::Value(values, _) => {
            Some(format!("set {}={}", setting.name, values.join(" | ")))
        }
    }
}

/// Wrap on spaces, for the comment lines. Long enough words are left long:
/// breaking a path or an option name would be worse than a ragged edge.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let line = lines.last_mut().expect("never empty");
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}

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
        "sp" => "split",
        "vs" => "vsplit",
        "noh" => "nohlsearch",
        "fmt" => "format",
        "sh" => "shell",
        "sus" | "stop" => "suspend",
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
        let values = SETTINGS.iter().find_map(|setting| match setting.kind {
            Kind::Value(values, _) if setting.name == name => Some(values),
            _ => None,
        });
        let Some(values) = values else {
            return Vec::new();
        };
        return values
            .iter()
            .filter(|value| value.starts_with(typed))
            .map(|value| format!("{name}={value}"))
            .collect();
    }
    option_names()
        .into_iter()
        .filter(|option| option.starts_with(word))
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
    fn the_config_file_is_every_setting_at_its_default() {
        let config = default_config();
        for setting in SETTINGS {
            let line = default_line(setting);
            assert!(config.contains(&format!("\n{line}\n")), "{line} is in the file");
            assert!(config.contains(setting.about.split(' ').next().unwrap()));
        }
        // Every line is a comment or a command; nothing else is legal in it.
        for line in config.lines().filter(|line| !line.trim().is_empty()) {
            assert!(line.starts_with('#') || line.starts_with("set "), "{line:?}");
        }
    }

    #[test]
    fn every_setting_offers_itself_for_completion() {
        let names = option_names();
        for setting in SETTINGS {
            let spelled = match setting.kind {
                Kind::Flag(_) => setting.name.to_string(),
                Kind::Word(words) => words[0].to_string(),
                Kind::Value(..) => format!("{}=", setting.name),
            };
            assert!(names.contains(&spelled), "{spelled} is offered");
        }
    }

    #[test]
    fn a_half_typed_command_offers_the_commands() {
        assert_eq!(names("w"), ["write", "wq", "wall", "wqall"]);
        assert_eq!(names("q"), ["quit"]);
        assert!(names("").len() == COMMANDS.len());
        assert!(names("zz").is_empty());
    }

    #[test]
    fn what_a_setting_says_about_itself_has_no_gaps_in_it() {
        // A string broken across lines without its `\\` keeps the next
        // line's indentation, and that run of spaces ends up in the config
        // file written for you and in the help.
        for setting in SETTINGS {
            assert!(!setting.about.contains("  "), "{}: {:?}", setting.name, setting.about);
        }
    }

    #[test]
    fn set_offers_options_and_then_their_values() {
        assert_eq!(names("set auto"), ["autocomplete=", "autoindent", "autopairs"]);
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
    fn every_way_of_writing_a_setting_names_the_same_setting() {
        // What `:setw` asks the config file: is there already a line about
        // this? The three spellings of the line numbers are one setting.
        assert_eq!(setting_name("number"), "number");
        assert_eq!(setting_name("nonumber"), "number");
        assert_eq!(setting_name("relativenumber"), "number");
        assert_eq!(setting_name("hybrid"), "number");
        assert_eq!(setting_name("shiftwidth=2"), "shiftwidth");
        assert_eq!(setting_name("notabline"), "tabline");
        // Settings whose names start the same way are not each other.
        assert_ne!(setting_name("guifontsize=15"), setting_name("guifont=Iosevka"));
        // A short form, or a name no table knows, is taken at its word.
        assert_eq!(setting_name("sw=2"), "sw");
        assert_eq!(setting_name("nosuchthing"), "suchthing");
    }

    #[test]
    fn setw_takes_the_same_options_as_set() {
        assert_eq!(names("setw auto"), names("set auto"));
        assert_eq!(names("setw tabline="), names("set tabline="));
    }

    #[test]
    fn the_replacement_starts_at_the_word_being_typed() {
        assert_eq!(complete("set auto").0, 4);
        assert_eq!(complete("w").0, 0);
    }
}
