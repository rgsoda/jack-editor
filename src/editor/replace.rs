//! Replacing a pattern across the whole project: `^r` in the grep picker.
//!
//! The picker is the preview - it lists every line the pattern is on - and
//! the replacement is then made on those lines in every file, as edits to
//! buffers rather than writes to disk. A file that was not open is opened to
//! be changed, each file's changes are one undo step in it, and nothing is
//! saved: `:wa` writes them, `u` in a buffer takes its share back.
//!
//! The search is run again here rather than taken from the picker's rows.
//! Those stop at a limit and cut long lines short, and a file open with
//! unsaved changes is searched as it is on disk. Here every file is read
//! whole, and an open one is read from its buffer, which is what is edited.

use regex::{Regex, RegexBuilder};
use std::path::{Path, PathBuf};

use super::lsp::absolute;
use super::{Editor, PromptKind};
use crate::substitute;

impl Editor {
    /// `^r` in the grep picker: close it, and ask what the pattern it was
    /// searching for should become.
    pub(super) fn start_project_replace(&mut self, pattern: String) {
        if pattern.is_empty() {
            self.message = "search for something to replace first".into();
            return;
        }
        self.picker = None;
        self.retire();
        self.replacing = pattern;
        self.open_prompt(PromptKind::Replace);
    }

    /// Every match of `pattern` under the working directory, replaced with
    /// `replacement` - vim's spelling of it, `&` and `\1`, as `:s` takes.
    pub fn replace_in_project(&mut self, pattern: &str, replacement: &str) {
        match std::env::current_dir() {
            Ok(root) => self.replace_under(&root, pattern, replacement),
            Err(_) => self.message = "no working directory to replace in".into(),
        }
    }

    fn replace_under(&mut self, root: &Path, pattern: &str, replacement: &str) {
        // The same smart case as grep, so what is replaced is what was listed.
        let insensitive = !pattern.chars().any(char::is_uppercase);
        let regex = match RegexBuilder::new(pattern).case_insensitive(insensitive).build() {
            Ok(regex) => regex,
            Err(_) => {
                self.message = format!("not a pattern: {pattern}");
                return;
            }
        };
        let replacement = substitute::replacement(replacement);

        let here = self.view().doc.path.as_deref().map(absolute);
        let was = self.current;
        let (mut total, mut files) = (0, 0);
        for path in project_files(root) {
            let open = self.views.iter().position(|view| view.doc.path.as_deref().map(absolute).as_deref() == Some(path.as_path()));
            let text = match open {
                Some(index) => self.views[index].doc.text.to_string(),
                None => match readable(&path) {
                    Some(text) => text,
                    None => continue,
                },
            };
            let changes = changed_lines(&text, &regex, &replacement);
            if changes.is_empty() {
                continue;
            }
            let index = match open {
                Some(index) => index,
                None => match self.open_file(&path) {
                    Ok(()) => self.current,
                    Err(err) => {
                        self.message = format!("{err:#}");
                        continue;
                    }
                },
            };

            // Bottom up, so each line's position is still where it was found.
            let view = &mut self.views[index];
            // The cursor stays on its line and column, not wherever the last
            // edit above it happened to leave it.
            let (line_at, column) = view.cursor_coords();
            view.begin_undo_group();
            for (line, new) in changes.iter().rev() {
                let start = view.doc.line_to_char(*line);
                let length = view.doc.line_str(*line).chars().count();
                view.edit_at(start, length, new, None);
            }
            view.end_undo_group();
            let line_at = line_at.min(view.last_line());
            let column = column.min(view.doc.line_str(line_at).chars().count());
            view.sel = crate::view::Selection::point(view.doc.line_to_char(line_at) + column);
            total += changes.len();
            files += 1;
        }

        // Back in the buffer it was asked from, found by path: opening files
        // can take the scratch buffer's place and move indexes about.
        let back = here
            .and_then(|here| self.views.iter().position(|view| view.doc.path.as_deref().map(absolute).as_deref() == Some(here.as_path())))
            .unwrap_or(was.min(self.views.len() - 1));
        self.switch_to(back);
        self.clamp_cursor();
        self.message = match (total, files) {
            (0, _) => format!("not found: {pattern}"),
            (1, _) => "replaced 1 line - not written yet, :wa writes it".into(),
            (lines, 1) => format!("replaced {lines} lines in 1 file - not written yet, :wa writes them"),
            (lines, files) => format!("replaced {lines} lines in {files} files - not written yet, :wa writes them"),
        };
    }
}

/// The files grep looks in: everything under `root` that git would not
/// ignore, hidden files left out, in a stable order.
fn project_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = ignore::WalkBuilder::new(root)
        .require_git(false)
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .map(|entry| absolute(entry.path()))
        .collect();
    files.sort();
    files
}

/// A file's text, if it is text: not binary, and UTF-8.
fn readable(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    (!text.contains('\0')).then_some(text)
}

/// The lines of `text` the pattern is on, each with what it becomes. Every
/// match on a line is replaced, as grep lists the line for any of them.
fn changed_lines(text: &str, regex: &Regex, replacement: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter_map(|(number, line)| {
            let body = line.trim_end_matches('\r');
            match regex.replace_all(body, replacement) {
                std::borrow::Cow::Owned(new) if new != body => Some((number, new)),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_that_change_are_listed_with_what_they_become() {
        let regex = Regex::new("count").unwrap();
        let lines = changed_lines("let count = 1;\nnone here\ncount + count\r\n", &regex, "total");
        assert_eq!(lines, [(0, "let total = 1;".to_string()), (2, "total + total".to_string())]);
        // A match replaced with itself is not a change.
        assert!(changed_lines("same\n", &Regex::new("same").unwrap(), "same").is_empty());
    }

    #[test]
    fn a_replace_reaches_every_file_and_writes_none_of_them() {
        let dir = std::env::temp_dir().join(format!("jack_replace_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.rs"), "fn old_name() {}\nold_name();\n").unwrap();
        std::fs::write(dir.join("sub/b.rs"), "use old_name;\nother\n").unwrap();
        std::fs::write(dir.join("c.rs"), "nothing\n").unwrap();

        let mut editor = Editor::open(&[dir.join("a.rs")]).unwrap();
        // Typed in the buffer and not saved: the buffer is what gets replaced.
        editor.view_mut().edit_at(0, 0, "// old_name here\n", Some(0));
        editor.replace_under(&dir, r"old_(\w+)", r"new_\1");

        assert_eq!(editor.message, "replaced 4 lines in 2 files - not written yet, :wa writes them");
        assert_eq!(editor.view().doc.path.as_deref(), Some(dir.join("a.rs").as_path()), "back where it started");
        assert_eq!(editor.view().doc.text.to_string(), "// new_name here\nfn new_name() {}\nnew_name();\n");
        assert_eq!(editor.cursor_coords(), (0, 0), "the cursor where it was");
        let b = editor.views().iter().find(|view| view.doc.path.as_deref().is_some_and(|p| p.ends_with("sub/b.rs"))).expect("opened");
        assert_eq!(b.doc.text.to_string(), "use new_name;\nother\n");
        assert_eq!(std::fs::read_to_string(dir.join("sub/b.rs")).unwrap(), "use old_name;\nother\n");
        assert_eq!(editor.views().len(), 2, "a file with nothing to replace is not opened");

        // One undo takes a file's share back.
        editor.undo();
        assert_eq!(editor.view().doc.text.to_string(), "// old_name here\nfn old_name() {}\nold_name();\n");

        editor.write_all(false);
        assert_eq!(editor.message, "wrote 2 buffers");
        assert_eq!(std::fs::read_to_string(dir.join("sub/b.rs")).unwrap(), "use new_name;\nother\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
