//! `:hits` - the quickfix list as one buffer you can edit, and `:w` for it.
//!
//! The arithmetic is `crate::hits`: what the buffer says, which line of which
//! file each row came from, and what a changed row means. This is the part with
//! an editor and a filesystem in it - gathering the text, putting it in a
//! buffer, going to a place from a row, and making a write happen.

use std::collections::BTreeMap;
use std::path::Path;

use super::Editor;
use crate::buffer::Document;
use crate::hits::{Change, Row};
use crate::view::{Selection, View};

impl Editor {
    /// `:hits`: every place in the quickfix list, with its context, in one
    /// buffer.
    ///
    /// The lines come from the buffer where a file is already open and from
    /// disk where it is not, so what you see is what you would see if you went
    /// there - unsaved changes and all.
    pub fn open_hits(&mut self) {
        if self.quickfix.is_empty() {
            self.message = "the quickfix list is empty".into();
            return;
        }
        let Some(view) = self.gather_hits() else {
            self.message = "none of those places could be read".into();
            return;
        };
        let index = match self.views.len() == 1 && self.views[0].is_empty_scratch() {
            true => {
                self.views[0] = view;
                0
            }
            false => {
                // The same hits again go back in the buffer they are already
                // in, rather than opening a second of them every time.
                match self.views.iter().position(|view| view.hits.is_some()) {
                    Some(at) => {
                        self.views[at] = view;
                        at
                    }
                    None => {
                        self.views.push(view);
                        self.views.len() - 1
                    }
                }
            }
        };
        self.switch_to(index);
        self.scroll_to_cursor();
        self.refresh_hits_display();
        let name = self.views[index].name();
        self.message = format!("{name} - edit them and :w, <enter> to go to one");
    }

    /// A buffer of the quickfix list's places, or `None` when not one of them
    /// could be read.
    fn gather_hits(&self) -> Option<View> {
        let places: Vec<(String, usize)> =
            self.quickfix.entries().iter().map(|entry| (entry.path.clone(), entry.line)).collect();
        let gathered = crate::hits::gather(&places, self.hitcontext, |path| self.lines_of_file(path));
        if gathered.files.is_empty() {
            return None;
        }
        let mut document = Document::scratch();
        document.text = ropey::Rope::from_str(&gathered.text());
        let mut view = View::new(document);
        view.hits = Some(Box::new(gathered));
        view.sel = Selection::point(0);
        // Its text was never typed, so there is nothing to undo back to and
        // nothing in it that counts as unsaved work.
        view.forget_history();
        Some(view)
    }

    /// A file's lines, from the buffer it is open in if it is open, else from
    /// disk. The trailing newline is dropped, because a file's last line is a
    /// line and not an empty one after it.
    fn lines_of_file(&self, path: &str) -> Option<Vec<String>> {
        let text = match self.buffer_with(path) {
            Some(index) => self.views[index].doc.text.to_string(),
            None => std::fs::read_to_string(path).ok()?,
        };
        Some(text.strip_suffix('\n').unwrap_or(&text).split('\n').map(str::to_string).collect())
    }

    /// The buffer a path is open in, if it is open in one.
    fn buffer_with(&self, path: &str) -> Option<usize> {
        let absolute = super::lsp::absolute(Path::new(path));
        self.views.iter().position(|view| {
            view.hits.is_none()
                && view.doc.path.as_deref().map(super::lsp::absolute) == Some(absolute.clone())
        })
    }

    pub fn in_hits(&self) -> bool {
        self.view().hits.is_some()
    }

    /// `<enter>` on a row: the file it came from, at that line.
    pub fn hits_enter(&mut self) {
        let row = self.cursor_coords().0;
        let Some((path, line)) = self.view().hits.as_ref().and_then(|hits| hits.place_at(row)) else {
            self.message = "nothing to go to on this line".into();
            return;
        };
        let origin = self.here();
        if let Err(err) = self.open_file(&path) {
            self.message = format!("{err:#}");
            return;
        }
        self.jumps.push(origin);
        // Lines count from zero inside jack, and a row holds the file's own
        // number as the gutter shows it.
        self.goto_line(line);
        self.scroll_to_cursor();
    }

    /// `:w` in a hits buffer: what was done to its lines, done to the files.
    ///
    /// Every change is checked against what its row said when it was gathered,
    /// and a file that has moved on since is refused whole rather than edited
    /// in the wrong places. A file that is open in a buffer is changed through
    /// the buffer, as one undo step, and then written, so that the buffer and
    /// the file never disagree about what just happened.
    pub(crate) fn write_hits(&mut self, force: bool) {
        let after = self.hits_lines();
        let Some(hits) = self.view().hits.as_ref() else {
            return;
        };
        let changes = match hits.plan(&after) {
            Ok(changes) => changes,
            Err(complaint) => {
                self.message = complaint;
                return;
            }
        };
        if changes.is_empty() {
            self.message = "the hits are as they were".into();
            return;
        }
        let deletions =
            changes.iter().filter(|change| matches!(change, Change::Delete { .. })).count();
        if deletions > 0 && !force {
            let s = if deletions == 1 { "" } else { "s" };
            self.message = format!("{deletions} line{s} would go - :w! to go ahead");
            return;
        }

        // What each row said when it was gathered, by the line it names: the
        // check that the file has not moved on under us.
        let mut said: BTreeMap<(usize, usize), String> = BTreeMap::new();
        for (row, line) in hits.rows.iter().zip(&hits.was) {
            if let Row::Line { file, line: number } = row {
                said.insert((*file, *number), line.clone());
            }
        }
        let files = hits.files.clone();

        let (mut written, mut changed, mut refused) = (0, 0, Vec::new());
        for (file, path) in files.iter().enumerate() {
            let mine = crate::hits::for_file(&changes, file);
            if mine.is_empty() {
                continue;
            }
            let Some(mut lines) = self.lines_of_file(path) else {
                refused.push(format!("{path}: gone"));
                continue;
            };
            match crate::hits::apply(&mut lines, &mine, |change| {
                said.get(&(file, change.line())).cloned()
            }) {
                Ok(count) => {
                    let mut text = lines.join("\n");
                    text.push('\n');
                    match self.put_file(path, &text) {
                        Ok(()) => {
                            written += 1;
                            changed += count;
                            // The places below what just moved are not where
                            // they were, and the quickfix list is what the
                            // next gathering reads.
                            let deleted: Vec<usize> = mine
                                .iter()
                                .filter_map(|change| match change {
                                    Change::Delete { line, .. } => Some(*line),
                                    _ => None,
                                })
                                .collect();
                            let inserted: Vec<(usize, usize)> = mine
                                .iter()
                                .filter_map(|change| match change {
                                    Change::Insert { after, text, .. } => {
                                        Some((*after, text.len()))
                                    }
                                    _ => None,
                                })
                                .collect();
                            if !deleted.is_empty() || !inserted.is_empty() {
                                self.quickfix.adjust(path, &deleted, &inserted);
                            }
                        }
                        Err(err) => refused.push(format!("{path}: {err:#}")),
                    }
                }
                Err(complaint) => refused.push(format!("{path}: {complaint}")),
            }
        }

        let s = if changed == 1 { "" } else { "s" };
        let of = if written == 1 { "file" } else { "files" };
        self.message = match refused.is_empty() {
            true => format!("{changed} change{s} in {written} {of}"),
            false => format!("{changed} change{s} in {written} {of}; {}", refused.join(", ")),
        };
        // The files have moved on, so the rows have to. Gathering again is the
        // honest way to say what they now are: the line numbers after a
        // deletion are not the ones the rows were holding.
        if written > 0 {
            let row = self.cursor_coords().0;
            if let Some(view) = self.gather_hits() {
                let index = self.current;
                self.views[index] = view;
                let line = row.min(self.views[index].doc.len_lines().saturating_sub(1));
                self.goto_line(line);
                self.scroll_to_cursor();
                self.refresh_hits_display();
            }
        }
    }

    /// What the gutter says about each line of a hits buffer, worked out again
    /// when the buffer has changed.
    ///
    /// The gathered rows are positions in the buffer *as it was gathered*, so a
    /// line typed in or taken out leaves every number below it one out. A write
    /// is diffed against the text and so is never fooled by that, but the
    /// gutter is read by eye and would be. The same diff says which line each
    /// row is now on, and a line that was typed in is on no line of any file
    /// yet, so it gets no number.
    pub(crate) fn refresh_hits_display(&mut self) {
        for index in 0..self.views.len() {
            if self.views[index].hits.is_none() {
                continue;
            }
            let revision = self.views[index].revision();
            if self.views[index].hits_shown_at == Some(revision) {
                continue;
            }
            let after: Vec<String> = (0..self.views[index].doc.len_lines())
                .map(|line| self.views[index].doc.line_str(line).trim_end_matches('\n').to_string())
                .collect();
            let hits = self.views[index].hits.as_ref().expect("a hits buffer");
            let before: Vec<&str> = hits.was.iter().map(String::as_str).collect();
            let now: Vec<&str> = after.iter().map(String::as_str).collect();
            let diff = similar::TextDiff::from_slices(&before, &now);
            let mut shown: Vec<Row> = Vec::with_capacity(after.len());
            for op in diff.ops() {
                let (old, new) = (op.old_range(), op.new_range());
                match op.tag() {
                    similar::DiffTag::Delete => {}
                    similar::DiffTag::Insert => shown.extend(new.map(|_| Row::Blank)),
                    _ => {
                        for (offset, _) in new.enumerate() {
                            shown.push(match hits.rows.get(old.start + offset) {
                                Some(row) if offset < old.len() => row.clone(),
                                _ => Row::Blank,
                            });
                        }
                    }
                }
            }
            self.views[index].hits_shown = shown;
            self.views[index].hits_shown_at = Some(revision);
        }
    }

    /// The hits buffer's lines as the gathering counts them: the trailing empty
    /// line every buffer ends with is not one of them.
    fn hits_lines(&self) -> Vec<String> {
        let view = self.view();
        let mut lines: Vec<String> = (0..view.doc.len_lines())
            .map(|line| view.doc.line_str(line).trim_end_matches('\n').to_string())
            .collect();
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }

    /// A file's new text, written through the buffer it is open in when it is
    /// open in one, and straight to disk when it is not.
    fn put_file(&mut self, path: &str, text: &str) -> anyhow::Result<()> {
        let Some(index) = self.buffer_with(path) else {
            std::fs::write(path, text)?;
            return Ok(());
        };
        let view = &mut self.views[index];
        let whole = view.doc.len_chars();
        let cursor = view.sel.head.min(text.chars().count());
        view.begin_undo_group();
        view.edit_at(0, whole, text, Some(cursor));
        view.end_undo_group();
        self.views[index].save()?;
        self.files_written += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quickfix::Entry;
    use std::path::PathBuf;

    /// Two files in a directory of their own, and an editor with the places of
    /// a grep for `needle` in its quickfix list.
    fn three_hits(name: &str) -> (PathBuf, PathBuf, Editor) {
        let dir = std::env::temp_dir().join(format!("jack_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "one\ntwo needle\nthree\nfour\nfive\nsix needle\nseven\n").unwrap();
        std::fs::write(&b, "alpha\nbeta needle\ngamma\n").unwrap();
        let mut editor = Editor::scratch();
        editor.quickfix.fill(vec![
            Entry { path: a.display().to_string(), line: 1, text: "two needle".into() },
            Entry { path: a.display().to_string(), line: 5, text: "six needle".into() },
            Entry { path: b.display().to_string(), line: 1, text: "beta needle".into() },
        ]);
        (a, b, editor)
    }

    /// The buffer's lines, for comparing against what was gathered.
    fn lines(editor: &Editor) -> Vec<String> {
        let view = editor.view();
        (0..view.doc.len_lines())
            .map(|line| view.doc.line_str(line).trim_end_matches('\n').to_string())
            .collect()
    }

    #[test]
    fn hits_gathers_the_places_with_their_context_grouped_by_file() {
        let (a, b, mut editor) = three_hits("hits_gather");
        editor.hitcontext = 1;
        editor.open_hits();
        assert!(editor.in_hits());
        assert_eq!(editor.view().name(), "[hits: 3 places in 2 files]");
        assert!(editor.message.starts_with("[hits: 3 places in 2 files] - edit them"), "{}", editor.message);

        assert_eq!(lines(&editor), [
            a.display().to_string(),
            "one".to_string(),
            "two needle".to_string(),
            "three".to_string(),
            "five".to_string(),
            "six needle".to_string(),
            "seven".to_string(),
            String::new(),
            b.display().to_string(),
            "alpha".to_string(),
            "beta needle".to_string(),
            "gamma".to_string(),
            String::new(),
        ]);

        // Asking again does not open a second one.
        editor.open_hits();
        assert_eq!(editor.views.iter().filter(|view| view.hits.is_some()).count(), 1);
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn editing_the_rows_and_writing_changes_the_files() {
        let (a, b, mut editor) = three_hits("hits_write");
        editor.hitcontext = 0;
        editor.open_hits();

        // Three hits, three rows, two headings and the gap: change two of them.
        editor.goto_line(1);
        editor.run_command("s/needle/needle fixed/");
        editor.goto_line(5);
        editor.run_command("s/needle/needle too/");

        editor.run_command("w");
        assert_eq!(editor.message, "2 changes in 2 files");
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "one\ntwo needle fixed\nthree\nfour\nfive\nsix needle\nseven\n"
        );
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "alpha\nbeta needle too\ngamma\n");

        // And again has nothing to do: the rows now say what the files say.
        editor.run_command("w");
        assert_eq!(editor.message, "the hits are as they were");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn a_file_that_has_moved_on_is_refused_rather_than_edited_in_the_wrong_place() {
        let (a, b, mut editor) = three_hits("hits_stale");
        editor.hitcontext = 0;
        editor.open_hits();

        // Somebody else edits a.txt behind us, and the line the buffer is
        // holding is not what is there any more.
        std::fs::write(&a, "ONE\nSOMETHING ELSE\nthree\nfour\nfive\nsix needle\nseven\n").unwrap();
        editor.goto_line(1);
        editor.run_command("s/needle/needle fixed/");
        editor.goto_line(5);
        editor.run_command("s/needle/needle too/");

        editor.run_command("w");
        // b.txt was fine and was written; a.txt was refused, and said why.
        assert!(editor.message.starts_with("1 change in 1 file;"), "{}", editor.message);
        assert!(editor.message.contains("has changed since"), "{}", editor.message);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "alpha\nbeta needle too\ngamma\n");
        assert!(std::fs::read_to_string(&a).unwrap().contains("SOMETHING ELSE"), "left alone");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn a_deleted_row_wants_saying_twice_and_then_deletes_the_line() {
        let (a, _b, mut editor) = three_hits("hits_delete");
        editor.hitcontext = 0;
        editor.open_hits();
        editor.goto_line(1);
        editor.delete_lines(None, 1);

        editor.run_command("w");
        assert_eq!(editor.message, "1 line would go - :w! to go ahead");
        assert!(std::fs::read_to_string(&a).unwrap().contains("two needle"), "not yet");

        editor.run_command("w!");
        assert_eq!(editor.message, "1 change in 1 file");
        let after = std::fs::read_to_string(&a).unwrap();
        assert_eq!(after, "one\nthree\nfour\nfive\nsix needle\nseven\n");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn the_places_below_a_deleted_line_are_still_the_places_afterwards() {
        let (a, _b, mut editor) = three_hits("hits_adjust");
        editor.hitcontext = 0;
        editor.open_hits();
        // Rows: heading, "two needle" (line 2), "six needle" (line 6), gap,
        // heading, "beta needle". Delete the first of a.txt's two.
        editor.goto_line(1);
        editor.delete_lines(None, 1);
        editor.run_command("w!");
        assert_eq!(editor.message, "1 change in 1 file");

        // The other hit in that file is one line up, and still gathered - the
        // whole point of adjusting the list rather than gathering it stale.
        assert!(editor.in_hits(), "still the hits buffer");
        assert_eq!(editor.view().name(), "[hits: 2 places in 2 files]");
        assert!(lines(&editor).contains(&"six needle".to_string()), "{:?}", lines(&editor));
        let places: Vec<usize> = editor
            .quickfix
            .entries()
            .iter()
            .filter(|entry| entry.path == a.display().to_string())
            .map(|entry| entry.line)
            .collect();
        assert_eq!(places, [4], "line 6 became line 5, and the deleted place went");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn the_numbers_in_the_gutter_follow_the_lines_as_they_are_typed() {
        let (a, _b, mut editor) = three_hits("hits_gutter");
        editor.hitcontext = 0;
        editor.open_hits();
        // Rows: heading, a's line 2, a's line 6, gap, heading, b's line 2.
        let shown = |editor: &Editor| -> Vec<Option<usize>> {
            editor
                .view()
                .hits_shown
                .iter()
                .map(|row| match row {
                    Row::Line { line, .. } => Some(line + 1),
                    _ => None,
                })
                .collect()
        };
        // With a last entry for the empty line every buffer ends with.
        assert_eq!(shown(&editor), [None, Some(2), Some(6), None, None, Some(2), None]);

        // A line typed in is on no line of any file yet, so it has no number,
        // and the rows below it keep theirs.
        editor.goto_line(1);
        editor.view_mut().put_lines("brand new\n", true);
        editor.refresh_hits_display();
        assert_eq!(shown(&editor), [None, Some(2), None, Some(6), None, None, Some(2), None]);

        // An edited line keeps its number: it is still that line of the file.
        editor.open_hits();
        editor.goto_line(1);
        editor.run_command("s/needle/pin/");
        editor.refresh_hits_display();
        assert_eq!(shown(&editor), [None, Some(2), Some(6), None, None, Some(2), None]);

        // A line taken out: everything below it is still numbered as its own
        // file numbers it, rather than as the row it has slid into.
        editor.open_hits();
        editor.goto_line(1);
        editor.delete_lines(None, 1);
        editor.refresh_hits_display();
        assert_eq!(shown(&editor), [None, Some(6), None, None, Some(2), None]);

        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn a_heading_is_not_text_to_edit() {
        let (a, _b, mut editor) = three_hits("hits_heading");
        editor.hitcontext = 0;
        editor.open_hits();
        editor.goto_line(0);
        editor.run_command("s/^/x/");

        editor.run_command("w");
        assert!(editor.message.contains("not text to edit"), "{}", editor.message);
        assert!(std::fs::read_to_string(&a).unwrap().starts_with("one\n"), "nothing written");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn enter_on_a_row_goes_to_the_line_it_came_from() {
        let (a, _b, mut editor) = three_hits("hits_enter");
        editor.hitcontext = 0;
        editor.open_hits();
        // The third row is a.txt's second hit - line 6 of the file.
        editor.goto_line(2);
        editor.hits_enter();
        assert_eq!(editor.view().doc.path.as_deref(), Some(a.as_path()));
        assert_eq!(editor.cursor_coords().0, 5, "line 6, counting from zero");

        // And the way back is the jump list, as it is from a quickfix step.
        editor.jump_back();
        assert!(editor.in_hits(), "back in the hits buffer");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }

    #[test]
    fn a_file_open_in_a_buffer_is_changed_through_the_buffer() {
        let (a, _b, mut editor) = three_hits("hits_open_buffer");
        editor.hitcontext = 0;
        editor.open_file(&a).unwrap();
        // An unsaved change in the buffer is what the gathering sees.
        editor.goto_line(0);
        editor.run_command("s/^/first /");

        editor.open_hits();
        assert!(lines(&editor).contains(&"two needle".to_string()));

        editor.goto_line(1);
        editor.run_command("s/needle/needle fixed/");
        editor.run_command("w");
        assert_eq!(editor.message, "1 change in 1 file");

        // The buffer has it, and it went to disk with the unsaved change that
        // was already in the buffer - one write, one file, no disagreement.
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(text.starts_with("first one\n"), "{text}");
        assert!(text.contains("two needle fixed"), "{text}");
        let buffer = editor.views.iter().find(|view| view.doc.path.as_deref() == Some(a.as_path()));
        assert!(!buffer.expect("the file's buffer").is_modified(), "written, so not modified");
        std::fs::remove_dir_all(a.parent().unwrap()).ok();
    }
}
