//! The editor's side of git hunks: going from one to the next, seeing what
//! one changed, putting it back, and staging it.
//!
//! The hunks come with the signs - the same diff, run on the same thread - so
//! everything here reads what the gutter is already showing. What it will not
//! do is act on a diff that is older than the text: a hunk is a range of
//! lines, and after an edit that range is about different lines.

use super::{Editor, Mode};
use crate::info::Info;
use crate::stream::{self, Hunk};

impl Editor {
    /// `]c` and `[c`: the next hunk below the cursor, or the one above, round
    /// the ends of the buffer, and which of how many it is.
    pub fn goto_hunk(&mut self, forward: bool) {
        let line = self.view().cursor_coords().0;
        let hunks = &self.view().hunks;
        let found = match forward {
            true => hunks.iter().position(|h| h.lines.start > line).or((!hunks.is_empty()).then_some(0)),
            false => hunks.iter().rposition(|h| h.lines.start < line).or(hunks.len().checked_sub(1)),
        };
        let Some(index) = found else {
            self.message = "no changes".into();
            return;
        };
        let (start, count) = (hunks[index].lines.start, hunks.len());
        self.push_jump();
        self.goto_line(start.min(self.view().last_line()));
        self.clamp_cursor();
        self.message = format!("hunk {} of {count}", index + 1);
    }

    /// The hunk the cursor is on, as long as the diff it came from is about
    /// the text as it is now. A deletion has no lines of its own, so it
    /// belongs to the line its mark is drawn on - the one after it.
    fn hunk_here(&mut self) -> Option<Hunk> {
        if self.mode == Mode::Insert {
            return None;
        }
        let view = self.view();
        if view.hunks_revision != Some(view.revision()) {
            self.message = "the diff is catching up - try again in a moment".into();
            return None;
        }
        let line = view.cursor_coords().0;
        let last = view.last_line();
        let hunk = view.hunks.iter().find(|hunk| match hunk.lines.is_empty() {
            true => hunk.lines.start.min(last) == line,
            false => hunk.lines.contains(&line),
        });
        match hunk {
            Some(hunk) => Some(hunk.clone()),
            None => {
                self.message = "no change on this line".into();
                None
            }
        }
    }

    /// `<space>h`: what the hunk under the cursor changed, in a box - git's
    /// lines marked `-`, and the buffer's `+`.
    pub fn preview_hunk(&mut self) {
        let Some(hunk) = self.hunk_here() else {
            return;
        };
        let new = self.hunk_lines(&hunk);
        let lines: Vec<String> = hunk
            .old
            .iter()
            .map(|line| format!("- {}", line.trim_end_matches(['\n', '\r'])))
            .chain(new.iter().map(|line| format!("+ {}", line.trim_end_matches(['\n', '\r']))))
            .collect();
        let anchor = self.view().sel.head;
        self.info = Info::text(&lines, anchor);
    }

    /// `:revert`: the hunk under the cursor made what git has again. One edit,
    /// so `u` brings your version back.
    pub fn revert_hunk(&mut self) {
        let Some(hunk) = self.hunk_here() else {
            return;
        };
        let doc = &self.view().doc;
        let start = doc.line_to_char(hunk.lines.start.min(doc.len_lines()));
        let end = doc.line_to_char(hunk.lines.end.min(doc.len_lines()));
        let mut old = hunk.old.concat();
        // The last line of the file, replaced by lines that ended in a break
        // where the buffer's did not: keep the buffer's ending, or reverting a
        // change to the last line would add a line.
        if end == doc.len_chars() && !doc.text.to_string().ends_with('\n') && old.ends_with('\n') {
            old.pop();
        }
        let cursor = start;
        self.view_mut().edit_at(start, end - start, &old, Some(cursor));
        self.clamp_cursor();
        self.message = match hunk.old.len() {
            0 => "removed what git does not have".into(),
            1 => "reverted 1 line".into(),
            n => format!("reverted {n} lines"),
        };
    }

    /// `<space>B`: who last changed the cursor's line, when, and why - the
    /// commit's first line - in a box by the cursor.
    pub fn blame_line(&mut self) {
        let Some(path) = self.view().doc.path.clone() else {
            self.message = "no file to blame".into();
            return;
        };
        let line = self.view().cursor_coords().0;
        let contents = self.view().doc.text.to_string();
        let blame = match stream::git_blame(&path, line, &contents) {
            Ok(blame) => blame,
            Err(err) => {
                self.message = err;
                return;
            }
        };
        let lines = match blame.committed() {
            false => vec!["not committed yet".to_string()],
            true => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs());
                let short: String = blame.commit.chars().take(8).collect();
                vec![format!("{short} {}, {}", blame.author, stream::ago(blame.time, now)), blame.summary]
            }
        };
        let anchor = self.view().sel.head;
        self.info = Info::text(&lines, anchor);
    }

    /// `:stage`: the hunk under the cursor, into the index - the buffer's
    /// version of it, saved or not, since that is what is on screen.
    pub fn stage_hunk(&mut self) {
        let Some(hunk) = self.hunk_here() else {
            return;
        };
        let Some(path) = self.view().doc.path.clone() else {
            return;
        };
        let new = self.hunk_lines(&hunk);
        match stream::git_stage(&path, &hunk, &new) {
            Ok(()) => {
                self.message = "staged".into();
                // The index moved, so the signs are out of date even though
                // the text is not.
                self.view_mut().signs_revision = None;
                self.refresh_signs();
            }
            Err(err) => self.message = format!("git: {err}"),
        }
    }

    /// The buffer's lines for a hunk, each with its line break.
    fn hunk_lines(&self, hunk: &Hunk) -> Vec<String> {
        let doc = &self.view().doc;
        (hunk.lines.start..hunk.lines.end.min(doc.len_lines()))
            .map(|line| doc.text.line(line).to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::diff_hunks;

    /// An editor on `text`, whose diff against `staged` has arrived.
    fn editor(staged: &str, text: &str) -> Editor {
        let mut e = Editor::scratch();
        e.view_mut().doc.text = ropey::Rope::from_str(text);
        let revision = e.view().revision();
        let view = e.view_mut();
        view.hunks = diff_hunks(staged, text);
        view.hunks_revision = Some(revision);
        e
    }

    #[test]
    fn bracket_c_walks_the_hunks_round_the_buffer() {
        let mut e = editor("a\nb\nc\nd\ne\n", "a\nB\nc\nd\nE\n");
        e.goto_hunk(true);
        assert_eq!(e.cursor_coords().0, 1);
        assert_eq!(e.message, "hunk 1 of 2");
        e.goto_hunk(true);
        assert_eq!(e.cursor_coords().0, 4);
        e.goto_hunk(true);
        assert_eq!(e.cursor_coords().0, 1, "round the end");
        e.goto_hunk(false);
        assert_eq!(e.cursor_coords().0, 4, "and back round the start");
    }

    #[test]
    fn revert_puts_back_what_git_has_as_one_undo() {
        let mut e = editor("a\nb\nc\n", "a\nB\nextra\nc\n");
        e.goto_line(1);
        e.revert_hunk();
        assert_eq!(e.view().doc.text.to_string(), "a\nb\nc\n");
        assert_eq!(e.message, "reverted 1 line");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "a\nB\nextra\nc\n");
    }

    #[test]
    fn a_deleted_line_is_reverted_from_the_line_after_it() {
        let mut e = editor("a\nb\nc\n", "a\nc\n");
        // The mark for the missing `b` is on `c`.
        e.goto_line(1);
        e.revert_hunk();
        assert_eq!(e.view().doc.text.to_string(), "a\nb\nc\n");
    }

    #[test]
    fn a_diff_older_than_the_text_is_not_acted_on() {
        let mut e = editor("a\nb\n", "a\nB\n");
        e.view_mut().edit_at(0, 0, "new\n", None);
        e.goto_line(1);
        e.revert_hunk();
        assert!(e.message.contains("catching up"), "{}", e.message);
        assert_eq!(e.view().doc.text.to_string(), "new\na\nB\n", "untouched");
    }

    #[test]
    fn the_preview_shows_both_sides() {
        let mut e = editor("a\nold\nc\n", "a\nnew\nc\n");
        e.goto_line(1);
        e.preview_hunk();
        let info = e.info.as_ref().expect("a box");
        let lines: Vec<&str> = info.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(lines, ["- old", "+ new"]);
    }

    #[test]
    fn staging_a_hunk_stages_only_that_hunk() {
        let dir = std::env::temp_dir().join(format!("jack_stage_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(&dir)
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .args(args)
                .output()
                .unwrap();
            assert!(status.status.success(), "{args:?}: {}", String::from_utf8_lossy(&status.stderr));
            String::from_utf8(status.stdout).unwrap()
        };
        let file = dir.join("sub/f.txt");
        std::fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "first"]);

        let text = "a\nB\nc\nd\nE\n";
        let mut e = Editor::open(std::slice::from_ref(&file)).unwrap();
        let all = e.view().doc.len_chars();
        e.view_mut().edit_at(0, all, text, Some(0));
        let revision = e.view().revision();
        e.view_mut().hunks = diff_hunks("a\nb\nc\nd\ne\n", text);
        e.view_mut().hunks_revision = Some(revision);

        e.goto_line(4);
        e.stage_hunk();
        assert_eq!(e.message, "staged");
        // Only the last hunk is in the index; the first is not, and the file
        // on disk was never written.
        assert_eq!(git(&["show", ":sub/f.txt"]), "a\nb\nc\nd\nE\n");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a\nb\nc\nd\ne\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blame_names_the_commit_or_says_nobody_committed_it() {
        let dir = std::env::temp_dir().join(format!("jack_blame_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(&dir)
                .args(["-c", "user.email=t@t", "-c", "user.name=Ann Author"])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        };
        let file = dir.join("f.txt");
        std::fs::write(&file, "one\ntwo\n").unwrap();
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "The first lines"]);

        let mut e = Editor::open(std::slice::from_ref(&file)).unwrap();
        e.blame_line();
        let lines = |e: &Editor| -> Vec<String> {
            e.info.as_ref().expect("a box").lines.iter().map(|line| line.text.clone()).collect()
        };
        let shown = lines(&e);
        assert!(shown[0].ends_with(" Ann Author, just now"), "{shown:?}");
        assert_eq!(shown[1], "The first lines");

        // An edit nobody has saved, let alone committed.
        e.goto_line(1);
        let start = e.view().doc.line_to_char(1);
        e.view_mut().edit_at(start, 0, "new\n", Some(start));
        e.blame_line();
        assert_eq!(lines(&e), ["not committed yet"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
