//! `gc` - commenting lines out and back in.
//!
//! Which marker a file uses, and what toggling a run of lines means. No
//! document here, only lines of text and the edits to make to them: the
//! deciding is the part worth testing, and `View::toggle_comments` is what
//! turns the edits into one transaction.

use std::path::Path;

/// How a language comments out a line: a marker in front, and for the
/// languages that only have block comments, one behind as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub open: &'static str,
    pub close: &'static str,
}

const SLASHES: Marker = Marker { open: "//", close: "" };
const HASH: Marker = Marker { open: "#", close: "" };
const DASHES: Marker = Marker { open: "--", close: "" };
const MARKUP: Marker = Marker { open: "<!--", close: "-->" };
const STARS: Marker = Marker { open: "/*", close: "*/" };

/// The marker for a file, by its name. Wider than the languages jack has a
/// grammar for: a shell script or a YAML file has no highlighting here, but
/// commenting out a line of one is still an everyday thing to want.
pub fn marker_for(path: Option<&Path>) -> Option<Marker> {
    let path = path?;
    let name = path.file_name()?.to_str()?;
    // The files that are known by their whole name rather than an extension.
    match name {
        "Makefile" | "makefile" | "GNUmakefile" | "Dockerfile" | "Containerfile"
        | ".gitignore" | ".dockerignore" | ".env" | "Justfile" | "justfile" => {
            return Some(HASH);
        }
        _ => {}
    }
    let marker = match path.extension()?.to_str()? {
        "rs" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "go" | "java" | "c" | "h"
        | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" | "cs" | "swift" | "kt" | "kts"
        | "scala" | "dart" | "zig" | "proto" | "jsonc" => SLASHES,
        "py" | "pyi" | "toml" | "sh" | "bash" | "zsh" | "fish" | "yaml" | "yml" | "rb"
        | "pl" | "r" | "conf" | "cfg" | "ini" | "nix" | "tf" | "cmake" | "mk" | "ex"
        | "exs" | "jl" | "nim" | "ps1" => HASH,
        "lua" | "sql" | "hs" | "elm" => DASHES,
        "html" | "htm" | "xml" | "svg" | "md" | "markdown" | "vue" | "svelte" => MARKUP,
        "css" | "scss" | "less" => STARS,
        _ => return None,
    };
    Some(marker)
}

/// One edit to one line: at `column` (in chars), take out `removed` and put
/// `inserted` there. In line order, and left to right within a line, which is
/// the order a transaction wants its changes in.
#[derive(Debug, PartialEq, Eq)]
pub struct Edit {
    pub line: usize,
    pub column: usize,
    pub removed: String,
    pub inserted: String,
}

/// What toggling did, for the message line.
#[derive(Debug, PartialEq, Eq)]
pub enum Toggled {
    Commented(usize),
    Uncommented(usize),
    /// Nothing but blank lines, which are left alone either way.
    Nothing,
}

/// Toggle `lines`, the first of which is line `first` of the file.
///
/// The whole run moves together, as vim-commentary has it: if every line with
/// something on it is already a comment, they all come back; otherwise they
/// are all commented out, including the ones that already were, so that the
/// same `gc` undoes it. The markers go in at the smallest indent of the run,
/// which keeps a commented block lined up as a block, and blank lines get
/// none - a paragraph gap should not become a line of `//`.
pub fn toggle(lines: &[String], first: usize, marker: Marker) -> (Vec<Edit>, Toggled) {
    let filled: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(index, text)| (first + index, text.as_str()))
        .collect();
    if filled.is_empty() {
        return (Vec::new(), Toggled::Nothing);
    }

    if filled.iter().all(|(_, text)| is_commented(text, marker)) {
        let edits = filled.iter().flat_map(|&(line, text)| uncomment(line, text, marker)).collect();
        return (edits, Toggled::Uncommented(filled.len()));
    }

    let column = filled.iter().map(|(_, text)| leading(text)).min().unwrap_or(0);
    let mut edits = Vec::new();
    for &(line, text) in &filled {
        edits.push(Edit {
            line,
            column,
            removed: String::new(),
            inserted: format!("{} ", marker.open),
        });
        if !marker.close.is_empty() {
            edits.push(Edit {
                line,
                column: text.trim_end().chars().count(),
                removed: String::new(),
                inserted: format!(" {}", marker.close),
            });
        }
    }
    (edits, Toggled::Commented(filled.len()))
}

fn leading(text: &str) -> usize {
    crate::buffer::indent_of(text).chars().count()
}

fn is_commented(text: &str, marker: Marker) -> bool {
    let text = text.trim();
    text.starts_with(marker.open)
        && text.ends_with(marker.close)
        && text.len() >= marker.open.len() + marker.close.len()
}

/// The edits that take a line's markers out, and the one space each of them
/// was put in with, when it is there: `//foo` comes back as `foo` as well.
fn uncomment(line: usize, text: &str, marker: Marker) -> Vec<Edit> {
    let mut edits = Vec::new();
    let column = leading(text);
    let after = &text.trim_start()[marker.open.len()..];
    let mut removed = marker.open.to_string();
    if after.starts_with(' ') {
        removed.push(' ');
    }
    let taken = removed.chars().count();
    edits.push(Edit { line, column, removed, inserted: String::new() });

    if !marker.close.is_empty() {
        let body = text.trim_end();
        let before = &body[..body.len() - marker.close.len()];
        let mut removed = marker.close.to_string();
        // The space in front of the closing marker, unless it is the same
        // space the opening one already took: `<!-- -->` is one space.
        let end = before.chars().count();
        let mut start = end;
        if before.ends_with(' ') && end > column + taken {
            removed.insert(0, ' ');
            start -= 1;
        }
        edits.push(Edit { line, column: start, removed, inserted: String::new() });
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|s| s.to_string()).collect()
    }

    /// The lines after the edits, applied right to left so the columns hold.
    fn apply(text: &[&str], first: usize, marker: Marker) -> Vec<String> {
        let mut out = lines(text);
        let (edits, _) = toggle(&out, first, marker);
        for edit in edits.iter().rev() {
            let line = &mut out[edit.line - first];
            let mut chars: Vec<char> = line.chars().collect();
            let removed: Vec<char> = edit.removed.chars().collect();
            assert_eq!(chars[edit.column..edit.column + removed.len()], removed[..], "{edit:?}");
            chars.splice(edit.column..edit.column + removed.len(), edit.inserted.chars());
            *line = chars.into_iter().collect();
        }
        out
    }

    #[test]
    fn a_run_is_commented_at_its_smallest_indent() {
        let out = apply(&["fn main() {", "    one();", "", "}"], 0, SLASHES);
        assert_eq!(out, ["// fn main() {", "//     one();", "", "// }"]);

        let out = apply(&["    if x:", "        y()"], 4, HASH);
        assert_eq!(out, ["    # if x:", "    #     y()"]);
    }

    #[test]
    fn a_commented_run_comes_back() {
        let out = apply(&["    // one();", "    //two();", "", "    //"], 0, SLASHES);
        assert_eq!(out, ["    one();", "    two();", "", "    "]);
    }

    #[test]
    fn a_mixed_run_is_commented_again_so_the_same_keys_undo_it() {
        let text = ["// already", "not yet"];
        let once = apply(&text, 0, SLASHES);
        assert_eq!(once, ["// // already", "// not yet"]);
        let back: Vec<&str> = once.iter().map(String::as_str).collect();
        assert_eq!(apply(&back, 0, SLASHES), text);
    }

    #[test]
    fn block_markers_go_on_both_ends_and_come_off_both() {
        let out = apply(&["  <p>hi</p>", "<br>"], 0, MARKUP);
        assert_eq!(out, ["<!--   <p>hi</p> -->", "<!-- <br> -->"]);
        let back: Vec<&str> = out.iter().map(String::as_str).collect();
        assert_eq!(apply(&back, 0, MARKUP), ["  <p>hi</p>", "<br>"]);

        // Tight markers and an empty comment both come apart cleanly.
        assert_eq!(apply(&["/*a*/", "/* */"], 0, STARS), ["a", ""]);
    }

    #[test]
    fn blank_lines_alone_are_left_alone() {
        let (edits, toggled) = toggle(&lines(&["", "   "]), 0, HASH);
        assert!(edits.is_empty());
        assert_eq!(toggled, Toggled::Nothing);
    }

    #[test]
    fn the_marker_follows_the_file() {
        let marker = |name: &str| marker_for(Some(Path::new(name)));
        assert_eq!(marker("src/main.rs"), Some(SLASHES));
        assert_eq!(marker("setup.py"), Some(HASH));
        assert_eq!(marker("Cargo.toml"), Some(HASH));
        assert_eq!(marker("index.html"), Some(MARKUP));
        assert_eq!(marker("init.lua"), Some(DASHES));
        assert_eq!(marker("Makefile"), Some(HASH));
        assert_eq!(marker("notes.txt"), None);
        assert_eq!(marker_for(None), None);
    }
}
