//! What a build said, read back as places to go.
//!
//! `:make` runs a command and gets a few hundred lines of text. Somewhere in
//! them are the file and the line each complaint is about, and that is all the
//! quickfix list wants - the rest is for reading in the terminal, which is
//! what `:!` is still for.
//!
//! Four shapes cover nearly everything, because compilers copied each other:
//!
//! ```text
//! src/main.rs:12:5: error: no method named `foo`     gcc, clang, go, eslint
//! src/main.rs:12: undefined: foo                     older tools, git grep
//!   --> src/main.rs:12:5                             rustc, after its message
//!   File "app.py", line 12, in <module>              python
//! ```
//!
//! A line is only a place if the file it names is really there. Text is full
//! of things shaped like `path:line` - a URL with a port, a timestamp, a
//! duration - and the disk is the one cheap way to tell them apart.

use std::path::Path;

use crate::quickfix::Entry;

/// Past this the list is a wall rather than a walk: a build that produced a
/// thousand complaints is one to read at the top, not to step through.
const LIMIT: usize = 1_000;

/// What builds a project, guessed from what is lying in its root. A build
/// file is an explicit answer and wins over a language's usual one; `cargo
/// check` rather than `cargo build`, because the question `:make` asks is
/// "what is wrong with this", and checking answers it in a fraction of the
/// time.
///
/// `:set makeprg=...` says it yourself, and is the answer whenever this
/// guesses wrong or guesses nothing.
pub fn builder(root: &Path) -> Option<&'static str> {
    const BY: &[(&str, &str)] = &[
        ("Makefile", "make"),
        ("makefile", "make"),
        ("Cargo.toml", "cargo check"),
        ("go.mod", "go build ./..."),
        ("build.zig", "zig build"),
        ("pyproject.toml", "python -m compileall ."),
    ];
    BY.iter().find(|(marker, _)| root.join(marker).exists()).map(|(_, command)| *command)
}

/// Every place named in `output`, in the order it was named. `root` is what
/// relative paths are relative to, which is the directory the build ran in.
pub fn places(output: &str, root: &Path) -> Vec<Entry> {
    let mut places: Vec<Entry> = Vec::new();
    // rustc says what is wrong on one line and where on the next, so the
    // message is carried forward until a place is found to put it on.
    let mut carried: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if is_complaint(trimmed) {
            carried = Some(trimmed.to_string());
        }
        let Some((path, number, message)) = named(trimmed) else {
            continue;
        };
        let Some(path) = real(&path, root) else {
            continue;
        };
        let text = match message.is_empty() {
            false => message,
            // A bare `--> file:line:col` is rustc pointing at the message
            // above it, and that message is the only readable part.
            true => carried.clone().unwrap_or_else(|| trimmed.to_string()),
        };
        let entry = Entry { path, line: number.saturating_sub(1), text };
        // The same place twice running is one place: a rustc note repeats the
        // line it is about, and so does a test that failed once.
        if places.last() == Some(&entry) {
            continue;
        }
        places.push(entry);
        if places.len() == LIMIT {
            break;
        }
    }
    places
}

/// Whether a line is a compiler saying something is wrong, which is the text
/// worth carrying to the place named below it.
fn is_complaint(line: &str) -> bool {
    let word = line.split(['[', ':']).next().unwrap_or("");
    matches!(word, "error" | "warning" | "note" | "help") && line.contains(':')
}

/// The file a candidate path really is, if it is one. What comes before the
/// line number is not always only a path - `thread 'main' panicked at
/// src/main.rs:4:5:` has a sentence in front of it - so the last word is
/// tried as well as the whole of it.
fn real(path: &str, root: &Path) -> Option<String> {
    let last = path.rsplit(char::is_whitespace).next().unwrap_or(path);
    [path, last].into_iter().find(|candidate| root.join(candidate).is_file()).map(str::to_string)
}

/// The file, the line and whatever was said about it, out of one line of
/// output. Nothing here touches the disk - the caller decides whether the
/// file is real.
fn named(line: &str) -> Option<(String, usize, String)> {
    // `File "app.py", line 12, in <module>`: python's, and the only one that
    // spells it out in words.
    if let Some(rest) = line.strip_prefix("File \"")
        && let Some((path, rest)) = rest.split_once('"')
        && let Some(rest) = rest.trim_start_matches(',').trim_start().strip_prefix("line ")
    {
        let number: usize = rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
        return Some((path.to_string(), number, line.to_string()));
    }

    // `--> src/main.rs:12:5`, which is rustc pointing rather than talking.
    let line = line.strip_prefix("-->").map(str::trim).unwrap_or(line);

    // `path:line:col: message`, `path:line: message`, `path:line:col` - the
    // path is everything before the first colon that a number follows, so a
    // windows drive letter or a `../` in front of it costs nothing.
    let (path, rest) = split_path(line)?;
    let (number, rest) = split_number(rest)?;
    // A column, when there is one: taken off the front so it cannot be read
    // as part of the message, and then dropped. The list is lines, as the
    // jump list is - a column that has drifted is worse than no column.
    let rest = match rest.strip_prefix(':').and_then(split_number) {
        Some((_, after)) => after,
        None => rest,
    };
    let message = rest.trim_start_matches(':').trim().to_string();
    Some((path.to_string(), number, message))
}

/// Up to the colon that a line number follows.
fn split_path(line: &str) -> Option<(&str, &str)> {
    let (path, rest) = line.split_once(':')?;
    let numbered = rest.starts_with(|c: char| c.is_ascii_digit());
    (!path.is_empty() && numbered).then_some((path, rest))
}

/// A run of digits, and what follows it.
fn split_number(rest: &str) -> Option<(usize, &str)> {
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (digits, after) = rest.split_at(end);
    Some((digits.parse().ok()?, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory with the files the output talks about, since a place is
    /// only a place when the file is really there.
    fn project(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("jack_compile_{name}_{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).expect("a project");
        for file in ["src/main.rs", "src/app.py", "src/main.go"] {
            std::fs::write(root.join(file), "").expect("a file");
        }
        root.canonicalize().expect("a real path")
    }

    #[test]
    fn what_builds_a_project_is_what_is_lying_in_its_root() {
        let root = project("builder");
        assert_eq!(builder(&root), None, "nothing to go on");
        std::fs::write(root.join("Cargo.toml"), "").expect("a manifest");
        assert_eq!(builder(&root), Some("cargo check"));
        // A build file is somebody saying it outright, and wins.
        std::fs::write(root.join("Makefile"), "").expect("a makefile");
        assert_eq!(builder(&root), Some("make"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_place_is_a_file_a_line_and_what_was_said_about_it() {
        let root = project("said");
        let output = "src/main.rs:12:5: error: no method named `foo`\n";
        let places = places(output, &root);
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].path, "src/main.rs");
        assert_eq!(places[0].line, 11, "zero-based, as every line in jack is");
        assert_eq!(places[0].text, "error: no method named `foo`");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rustc_says_what_is_wrong_above_where_it_is() {
        let root = project("rustc");
        let output = "\
error[E0599]: no method named `frobnicate` found
  --> src/main.rs:12:5
   |
12 |     thing.frobnicate();
   |           ^^^^^^^^^^ method not found
warning: unused variable: `x`
  --> src/main.rs:3:9
";
        let places = places(output, &root);
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].line, 11);
        assert_eq!(places[0].text, "error[E0599]: no method named `frobnicate` found");
        assert_eq!(places[1].line, 2);
        assert_eq!(places[1].text, "warning: unused variable: `x`");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_panic_names_the_file_after_a_sentence() {
        let root = project("panic");
        let output = "thread 'main' panicked at src/main.rs:4:5:\nassertion failed\n";
        let places = places(output, &root);
        assert_eq!(places.len(), 1, "the last word of it is a file");
        assert_eq!(places[0].line, 3);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn python_spells_it_out_in_words() {
        let root = project("python");
        let output = "  File \"src/app.py\", line 12, in <module>\n    import nope\n";
        let places = places(output, &root);
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].path, "src/app.py");
        assert_eq!(places[0].line, 11);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_line_with_no_column_is_a_place_too() {
        let root = project("nocolumn");
        let output = "src/main.go:7: undefined: foo\n";
        let places = places(output, &root);
        assert_eq!(places.len(), 1);
        assert_eq!(places[0].line, 6);
        assert_eq!(places[0].text, "undefined: foo");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn what_is_shaped_like_a_place_but_is_not_one_is_left_alone() {
        let root = project("notplaces");
        let output = "\
    Finished dev profile in 1.13s
listening on http://localhost:8080/health
12:04:07 INFO ready
src/nowhere.rs:3:1: error: a file that is not there
warning: 2 warnings emitted
";
        assert!(places(output, &root).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_same_place_twice_running_is_one_place() {
        let root = project("twice");
        let output = "src/main.rs:12: oops\nsrc/main.rs:12: oops\nsrc/main.rs:13: oops\n";
        assert_eq!(places(output, &root).len(), 2);
        std::fs::remove_dir_all(&root).ok();
    }
}
