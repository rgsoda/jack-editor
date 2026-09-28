//! `.editorconfig`: what the project says its files are indented with.
//!
//! The file is the one thing in a repository that already answers "tabs or
//! spaces, and how many" for everybody, whatever editor they use. jack reads
//! what it can act on - the indentation, the line width, and whether to trim
//! trailing whitespace - and ignores the rest rather than pretending: the
//! keys it does not support are about encodings and line endings, which jack
//! does not yet have anything to say about.
//!
//! Nearest file wins, and `root = true` says where to stop walking up. All of
//! it is per buffer, because that is what the file is: a statement about these
//! files, not about the editor.

use std::path::Path;

use crate::view::Indent;

/// What a `.editorconfig` had to say about one file. Everything is optional:
/// a key that is not there is not an instruction to do anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub indent: Option<Indent>,
    pub textwidth: Option<usize>,
    pub trim: Option<bool>,
}

impl Settings {
    /// Whether it says anything at all.
    pub fn is_empty(&self) -> bool {
        *self == Settings::default()
    }
}

/// One `[glob]` section and the keys under it, lower-cased as the format says.
struct Section {
    glob: String,
    keys: Vec<(String, String)>,
}

/// What the `.editorconfig` files above `path` say about it. `read` is handed
/// in so that the walk is testable without a directory tree.
pub fn find(path: &Path, read: impl Fn(&Path) -> Option<String>) -> Settings {
    // From the file's own directory upwards, stopping at a `root = true`.
    // Nearest last, so that applying them in order lets the nearest win.
    let mut found = Vec::new();
    let mut dir = path.parent();
    while let Some(here) = dir {
        if let Some(text) = read(&here.join(".editorconfig")) {
            let root = is_root(&text);
            found.push((here.to_path_buf(), text));
            if root {
                break;
            }
        }
        dir = here.parent();
    }

    let mut settings = Settings::default();
    for (dir, text) in found.iter().rev() {
        let relative = path.strip_prefix(dir).unwrap_or(path);
        let name = relative.to_string_lossy();
        for section in sections(text) {
            if matches(&section.glob, &name) {
                apply(&mut settings, &section.keys);
            }
        }
    }
    settings
}

/// `root = true` before the first section: the top of the tree.
fn is_root(text: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return false;
        }
        if let Some((key, value)) = split(line)
            && key == "root"
        {
            return value == "true";
        }
    }
    false
}

/// The `[glob]` sections of one file, in the order they are written.
fn sections(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(glob) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            sections.push(Section { glob: glob.to_string(), keys: Vec::new() });
            continue;
        }
        if let (Some(section), Some((key, value))) = (sections.last_mut(), split(line)) {
            section.keys.push((key, value));
        }
    }
    sections
}

/// `key = value`, with the key lower-cased and both sides trimmed. The value
/// keeps its case: a `max_line_length` is a number and `unset` is a word, but
/// nothing here is a name that could be case-sensitive.
fn split(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_lowercase(), value.trim().to_lowercase()))
}

/// The keys jack can act on. `unset` means "say nothing", which is the format's
/// way of undoing a wider section, so it clears rather than sets.
fn apply(settings: &mut Settings, keys: &[(String, String)]) {
    // Both spellings of how wide a step is. `indent_size` wins where both are
    // given, as the specification says, and `tab_width` is what a file using
    // tabs means by how wide one looks.
    let value = |name: &str| keys.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value.as_str());
    let width = value("indent_size").or_else(|| value("tab_width"));

    if let Some(style) = value("indent_style") {
        let tabs = match style {
            "tab" => true,
            "space" => false,
            _ => return,
        };
        let already = settings.indent.map_or(4, |indent| indent.width);
        let width = width.and_then(|width| width.parse().ok()).unwrap_or(already);
        settings.indent = Some(Indent { width, tabs });
    } else if let Some(width) = width.and_then(|width| width.parse().ok()) {
        // A width on its own changes the step and leaves tabs-or-spaces to
        // whatever the file itself says.
        let tabs = settings.indent.is_some_and(|indent| indent.tabs);
        settings.indent = Some(Indent { width, tabs });
    }

    if let Some(length) = value("max_line_length") {
        settings.textwidth = match length {
            "off" | "unset" => None,
            other => other.parse().ok(),
        };
    }
    if let Some(trim) = value("trim_trailing_whitespace") {
        settings.trim = match trim {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
    }
}

/// Whether an editorconfig glob matches a path, which is relative to the
/// directory the `.editorconfig` is in.
///
/// `*` is any run within one path segment, `**` crosses them, `?` is one
/// character, and `{a,b}` is a choice. A glob with no `/` in it is about the
/// file's name wherever it is, which is the format's rule and the reason
/// `[*.rs]` works from the top of a tree.
pub fn matches(glob: &str, path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let subject = match glob.contains('/') {
        true => path,
        false => name,
    };
    let glob = glob.strip_prefix('/').unwrap_or(glob);
    for one in expand(glob) {
        if one_matches(one.as_bytes(), subject.as_bytes()) {
            return true;
        }
    }
    false
}

/// `{a,b}` written out: one glob per choice, and nested braces expanded by
/// the next pass round. A glob with no braces is one glob.
fn expand(glob: &str) -> Vec<String> {
    let Some(open) = glob.find('{') else {
        return vec![glob.to_string()];
    };
    // The `}` that closes this `{`, counting the ones in between.
    let mut depth = 0;
    let mut close = None;
    for (index, c) in glob[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + index);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return vec![glob.to_string()];
    };

    let (before, after) = (&glob[..open], &glob[close + 1..]);
    let mut out = Vec::new();
    for choice in split_choices(&glob[open + 1..close]) {
        for rest in expand(&format!("{before}{choice}{after}")) {
            out.push(rest);
        }
    }
    out
}

/// The commas of one `{}`, ignoring the ones inside a nested `{}`.
fn split_choices(inside: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for c in inside.chars() {
        match c {
            '{' => {
                depth += 1;
                current.push(c);
            }
            '}' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    out.push(current);
    out
}

/// One glob with no braces left in it, against one path.
///
/// Written out rather than stepped through with a backtracking pointer: two
/// stars in one glob - `src/**/*.rs` - need to give ground independently, and
/// a pattern this short can afford to ask.
fn one_matches(glob: &[u8], path: &[u8]) -> bool {
    match glob.first() {
        None => path.is_empty(),
        // `**` crosses separators, and `**/` also stands for no segments at
        // all, so `src/**/*.rs` covers `src/main.rs`.
        Some(b'*') if glob.get(1) == Some(&b'*') => {
            let rest = &glob[2..];
            if let Some(after) = rest.strip_prefix(b"/")
                && one_matches(after, path)
            {
                return true;
            }
            (0..=path.len()).any(|at| one_matches(rest, &path[at..]))
        }
        // One star stays inside a path segment.
        Some(b'*') => {
            let rest = &glob[1..];
            let limit = path.iter().position(|&c| c == b'/').unwrap_or(path.len());
            (0..=limit).any(|at| one_matches(rest, &path[at..]))
        }
        Some(b'?') => match path.first() {
            Some(&c) if c != b'/' => one_matches(&glob[1..], &path[1..]),
            _ => false,
        },
        Some(&c) => match path.first() {
            Some(&next) if next == c => one_matches(&glob[1..], &path[1..]),
            _ => false,
        },
    }
}

/// Read the files off the disk, which is what the editor wants.
pub fn for_path(path: &Path) -> Settings {
    find(path, |file| std::fs::read_to_string(file).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn tree(files: &[(&str, &str)]) -> impl Fn(&Path) -> Option<String> {
        let files: HashMap<PathBuf, String> = files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), text.to_string()))
            .collect();
        move |path: &Path| files.get(path).cloned()
    }

    #[test]
    fn the_globs_are_the_ones_the_format_uses() {
        assert!(matches("*.rs", "src/main.rs"), "a name-only glob is about the name");
        assert!(!matches("*.rs", "src/main.py"));
        assert!(matches("*", "anything"));
        assert!(matches("src/*.rs", "src/main.rs"));
        assert!(!matches("src/*.rs", "src/deep/main.rs"), "one star stays in a segment");
        assert!(matches("src/**/*.rs", "src/deep/down/main.rs"));
        assert!(matches("{Makefile,*.mk}", "Makefile"));
        assert!(matches("{Makefile,*.mk}", "rules.mk"));
        assert!(!matches("{Makefile,*.mk}", "main.rs"));
        assert!(matches("*.{c,h}", "thing.h"));
        assert!(matches("file?.txt", "file1.txt"));
        assert!(!matches("file?.txt", "file12.txt"));
    }

    #[test]
    fn the_nearest_file_wins_and_root_stops_the_walk() {
        let read = tree(&[
            (
                "/home/p/.editorconfig",
                "root = true\n[*]\nindent_style = space\nindent_size = 2\nmax_line_length = 80\n",
            ),
            ("/home/p/src/.editorconfig", "[*.rs]\nindent_size = 4\n"),
            // Above the root, and so never read.
            ("/home/.editorconfig", "[*]\nindent_style = tab\n"),
        ]);

        let found = find(Path::new("/home/p/src/main.rs"), &read);
        assert_eq!(found.indent, Some(Indent { width: 4, tabs: false }), "the nearer size");
        assert_eq!(found.textwidth, Some(80), "and the further width");

        // A file the nearer one says nothing about keeps the wider answer.
        let found = find(Path::new("/home/p/src/notes.txt"), &read);
        assert_eq!(found.indent, Some(Indent { width: 2, tabs: false }));

        // Nothing anywhere is nothing to do.
        assert!(find(Path::new("/elsewhere/main.rs"), &read).is_empty());
    }

    #[test]
    fn tabs_and_the_keys_that_turn_things_off() {
        let read = tree(&[(
            "/p/.editorconfig",
            "root=true\n\
             [*]\n\
             indent_style = tab\n\
             tab_width = 8\n\
             trim_trailing_whitespace = true\n\
             charset = utf-8\n\
             [*.md]\n\
             max_line_length = off\n\
             trim_trailing_whitespace = false\n",
        )]);

        let code = find(Path::new("/p/main.go"), &read);
        assert_eq!(code.indent, Some(Indent { width: 8, tabs: true }));
        assert_eq!(code.trim, Some(true));

        // Markdown's two spaces at the end of a line are a line break, which
        // is exactly what `trim_trailing_whitespace = false` is for.
        let prose = find(Path::new("/p/README.md"), &read);
        assert_eq!(prose.trim, Some(false));
        assert_eq!(prose.textwidth, None);
    }
}
