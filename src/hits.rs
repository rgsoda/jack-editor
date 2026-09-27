//! `:hits` - every place in the quickfix list, gathered into one buffer you
//! can edit.
//!
//! The quickfix list walks the places one at a time and a project-wide replace
//! changes them all without showing you any of them. This is the middle: the
//! hits with a little of their context, grouped by file, as text. Editing is
//! ordinary editing - `cw`, `.`, `u`, visual mode - and `:w` works out what
//! that meant to the files, the way a directory listing works out that a
//! changed line was a rename.
//!
//! This file is the buffer and the arithmetic: what the text is, which line of
//! which file each row came from, and what a changed row means. No editor, no
//! filesystem, no windows - `read` is handed in, so the awkward part is
//! testable with a `HashMap`.

/// What one row of the buffer is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A file's name, over its group.
    Heading,
    /// The blank line between groups.
    Blank,
    /// A line of a file: which file in `files`, and which line of it,
    /// counting from zero.
    Line { file: usize, line: usize },
}

/// The gathered hits, beside the buffer holding them.
///
/// `rows` is one entry per line of the buffer, so the buffer and this stay in
/// step by index - which is the only thing that makes a row's line number, and
/// a write, knowable at all.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Hits {
    /// The files, in the order their groups appear.
    pub files: Vec<String>,
    /// One per line of the buffer as it was made.
    pub rows: Vec<Row>,
    /// The buffer as it was made, line by line, for `:w` to diff against.
    pub was: Vec<String>,
    /// How many places were gathered, which is not how many lines there are.
    pub places: usize,
}

/// One change a write has to make to one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// This line of this file now reads like this.
    Edit { file: usize, line: usize, text: String },
    /// This line of this file is to go.
    Delete { file: usize, line: usize },
    /// These lines are to go in after this line of this file.
    Insert { file: usize, after: usize, text: Vec<String> },
}

impl Change {
    pub fn file(&self) -> usize {
        match self {
            Change::Edit { file, .. } | Change::Delete { file, .. } | Change::Insert { file, .. } => *file,
        }
    }

    /// The line it is about, for sorting a file's changes.
    pub fn line(&self) -> usize {
        match self {
            Change::Edit { line, .. } | Change::Delete { line, .. } => *line,
            Change::Insert { after, .. } => *after,
        }
    }
}

/// Every hit, with `context` lines either side, grouped by file in the order
/// the list has them.
///
/// `read` gives a file's lines, or `None` when it cannot be read - a hit in a
/// file that has since gone is left out rather than made up. Windows that
/// overlap are merged, so two hits three lines apart are one run of text
/// rather than the same lines twice.
pub fn gather(
    places: &[(String, usize)],
    context: usize,
    read: impl Fn(&str) -> Option<Vec<String>>,
) -> Hits {
    let mut hits = Hits::default();
    // Grouped, but in the order the list arrived in: the first file to appear
    // is the first group, and so on.
    let mut order: Vec<String> = Vec::new();
    let mut wanted: Vec<Vec<usize>> = Vec::new();
    for (path, line) in places {
        match order.iter().position(|open| open == path) {
            Some(at) => wanted[at].push(*line),
            None => {
                order.push(path.clone());
                wanted.push(vec![*line]);
            }
        }
    }

    for (path, lines) in order.iter().zip(wanted) {
        let Some(text) = read(path) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let mut lines: Vec<usize> = lines.into_iter().filter(|line| *line < text.len()).collect();
        lines.sort_unstable();
        lines.dedup();
        if lines.is_empty() {
            continue;
        }
        // The runs of lines to show: each hit's window, merged where they
        // touch or overlap.
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for line in &lines {
            let start = line.saturating_sub(context);
            let end = (line + context).min(text.len() - 1);
            match runs.last_mut() {
                // Touching counts as overlapping: a one-line gap between two
                // runs is worth filling rather than writing a separator over.
                Some((_, last)) if start <= *last + 1 => *last = (*last).max(end),
                _ => runs.push((start, end)),
            }
        }

        let file = hits.files.len();
        hits.files.push(path.clone());
        hits.places += lines.len();
        if !hits.rows.is_empty() {
            hits.rows.push(Row::Blank);
            hits.was.push(String::new());
        }
        hits.rows.push(Row::Heading);
        hits.was.push(path.clone());
        for (start, end) in runs {
            for (line, read) in text.iter().enumerate().take(end + 1).skip(start) {
                hits.rows.push(Row::Line { file, line });
                hits.was.push(read.clone());
            }
        }
    }
    hits
}

impl Hits {
    /// The buffer's text, as it is first made.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for line in &self.was {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    /// The place a row points at, for `<enter>` to go to it. A heading points
    /// at the top of its file, because that is the only line it names.
    pub fn place_at(&self, row: usize) -> Option<(String, usize)> {
        match self.rows.get(row)? {
            Row::Line { file, line } => Some((self.files[*file].clone(), *line)),
            Row::Heading => {
                let after = self.rows.get(row + 1)?;
                match after {
                    Row::Line { file, line } => Some((self.files[*file].clone(), *line)),
                    _ => None,
                }
            }
            Row::Blank => None,
        }
    }

    /// What `after` - the buffer as it now reads - means to the files.
    ///
    /// Anything that cannot be meant exactly is refused rather than guessed
    /// at: an edited heading, a line added where there is no line above it to
    /// belong to. A write that half understood what you typed would be worse
    /// than one that would not do it.
    pub fn plan(&self, after: &[String]) -> Result<Vec<Change>, String> {
        let before: Vec<&str> = self.was.iter().map(String::as_str).collect();
        let after_refs: Vec<&str> = after.iter().map(String::as_str).collect();
        let diff = similar::TextDiff::from_slices(&before, &after_refs);
        let mut changes = Vec::new();
        for op in diff.ops() {
            let (old, new) = (op.old_range(), op.new_range());
            match op.tag() {
                similar::DiffTag::Equal => {}
                similar::DiffTag::Delete => {
                    for row in old {
                        changes.push(self.deletion(row)?);
                    }
                }
                similar::DiffTag::Insert => {
                    let text: Vec<String> = after[new].to_vec();
                    changes.push(self.insertion(old.start, text)?);
                }
                similar::DiffTag::Replace => {
                    // Same shape: line for line, which is the ordinary case -
                    // you changed some text and left the rest alone.
                    if old.len() == new.len() {
                        for (row, line) in old.zip(new) {
                            changes.push(self.replacement(row, after[line].clone())?);
                        }
                        continue;
                    }
                    // Otherwise it is lines gone and lines arrived, and both
                    // halves have to be sayable on their own.
                    let text: Vec<String> = after[new].to_vec();
                    changes.push(self.insertion(old.end, text)?);
                    for row in old {
                        changes.push(self.deletion(row)?);
                    }
                }
            }
        }
        Ok(changes)
    }

    fn replacement(&self, row: usize, text: String) -> Result<Change, String> {
        match self.rows.get(row) {
            Some(Row::Line { file, line }) => Change::Edit { file: *file, line: *line, text }.into_ok(),
            Some(Row::Heading) => Err(format!("{}: a file's name is not text to edit", self.was[row])),
            _ => Err("that blank line is not part of a file".into()),
        }
    }

    fn deletion(&self, row: usize) -> Result<Change, String> {
        match self.rows.get(row) {
            Some(Row::Line { file, line }) => Change::Delete { file: *file, line: *line }.into_ok(),
            Some(Row::Heading) => Err(format!(
                "{}: deleting a file's name would not delete the file - use the listing for that",
                self.was[row]
            )),
            _ => Err("that blank line is not part of a file".into()),
        }
    }

    /// Lines typed in: they belong after the nearest line above them that is
    /// part of a file. A line typed above the first heading, or under a
    /// heading with nothing between, has nothing to belong to.
    fn insertion(&self, at: usize, text: Vec<String>) -> Result<Change, String> {
        let above = self.rows[..at.min(self.rows.len())]
            .iter()
            .rev()
            .find_map(|row| match row {
                Row::Line { file, line } => Some((*file, *line)),
                _ => None,
            });
        match above {
            Some((file, line)) => Change::Insert { file, after: line, text }.into_ok(),
            None => Err("a new line needs a line of a file above it to belong to".into()),
        }
    }
}

impl Change {
    fn into_ok(self) -> Result<Change, String> {
        Ok(self)
    }
}

/// One file's changes, bottom up, so that applying one never moves the line
/// another one is about.
pub fn for_file(changes: &[Change], file: usize) -> Vec<Change> {
    let mut mine: Vec<Change> =
        changes.iter().filter(|change| change.file() == file).cloned().collect();
    // Bottom up, and for the same line a delete before an insert: the insert
    // is *after* that line, so doing it first would put the new text where the
    // delete is looking.
    mine.sort_by_key(|change| {
        let second = match change {
            Change::Insert { .. } => 0,
            _ => 1,
        };
        (std::cmp::Reverse(change.line()), second)
    });
    mine
}

/// A file's lines with one file's changes made to them, bottom up.
///
/// `lines` is the file as it is now, and every change's line is checked
/// against what the buffer said it was before it is touched. A file that has
/// moved on since the hits were gathered is refused whole, naming the line,
/// rather than edited in the wrong places.
pub fn apply(lines: &mut Vec<String>, changes: &[Change], was: impl Fn(&Change) -> Option<String>) -> Result<usize, String> {
    let mut done = 0;
    for change in changes {
        let line = change.line();
        if line >= lines.len() {
            return Err(format!("line {} is no longer there", line + 1));
        }
        if let Some(expected) = was(change) && lines[line] != expected {
            return Err(format!("line {} has changed since", line + 1));
        }
        match change {
            Change::Edit { text, .. } => lines[line] = text.clone(),
            Change::Delete { .. } => {
                lines.remove(line);
            }
            Change::Insert { text, .. } => {
                for (offset, new) in text.iter().enumerate() {
                    lines.insert(line + 1 + offset, new.clone());
                }
            }
        }
        done += 1;
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn files() -> HashMap<String, Vec<String>> {
        let mut files = HashMap::new();
        files.insert(
            "a.rs".to_string(),
            (1..=20).map(|n| format!("a line {n}")).collect::<Vec<String>>(),
        );
        files.insert(
            "b.rs".to_string(),
            (1..=5).map(|n| format!("b line {n}")).collect::<Vec<String>>(),
        );
        files
    }

    fn gathered(places: &[(&str, usize)], context: usize) -> Hits {
        let files = files();
        let places: Vec<(String, usize)> =
            places.iter().map(|(path, line)| (path.to_string(), *line)).collect();
        gather(&places, context, |path| files.get(path).cloned())
    }

    #[test]
    fn hits_are_grouped_by_file_in_the_order_they_arrived() {
        let hits = gathered(&[("b.rs", 1), ("a.rs", 4), ("b.rs", 3)], 1);
        assert_eq!(hits.files, ["b.rs", "a.rs"], "b was first, so b's group is first");
        assert_eq!(hits.places, 3);
        assert_eq!(
            hits.text(),
            "b.rs\n\
             b line 1\nb line 2\nb line 3\nb line 4\nb line 5\n\
             \n\
             a.rs\n\
             a line 4\na line 5\na line 6\n"
        );
        // Two hits two lines apart in b.rs are one run, not two windows with
        // the same line in both.
        assert_eq!(hits.rows.iter().filter(|row| **row == Row::Heading).count(), 2);
        assert_eq!(hits.was.iter().filter(|line| *line == "b line 2").count(), 1);
    }

    #[test]
    fn a_row_knows_which_line_of_which_file_it_is() {
        let hits = gathered(&[("a.rs", 9)], 2);
        assert_eq!(hits.place_at(0), Some(("a.rs".into(), 7)), "the heading points at its first line");
        assert_eq!(hits.place_at(1), Some(("a.rs".into(), 7)));
        assert_eq!(hits.place_at(3), Some(("a.rs".into(), 9)), "the hit itself");
        assert_eq!(hits.place_at(99), None);

        // The window is clamped to the file rather than running off it.
        let hits = gathered(&[("b.rs", 4)], 3);
        assert_eq!(hits.was.len(), 5, "a heading and the last four lines: {:?}", hits.was);
    }

    #[test]
    fn an_edited_line_is_an_edit_to_the_file_it_came_from() {
        let hits = gathered(&[("a.rs", 4), ("b.rs", 1)], 0);
        let mut after: Vec<String> = hits.was.clone();
        after[1] = "a line 5, fixed".into();
        after[4] = "b line 2, fixed".into();
        let changes = hits.plan(&after).expect("two edits");
        assert_eq!(changes, [
            Change::Edit { file: 0, line: 4, text: "a line 5, fixed".into() },
            Change::Edit { file: 1, line: 1, text: "b line 2, fixed".into() },
        ]);
        // Nothing typed is no change at all.
        assert_eq!(hits.plan(&hits.was).unwrap(), []);
    }

    #[test]
    fn a_deleted_line_is_a_deletion_and_a_typed_one_belongs_above_it() {
        let hits = gathered(&[("a.rs", 4)], 1);
        // The buffer is: heading, line 4, line 5, line 6.
        let mut after: Vec<String> = hits.was.clone();
        after.remove(2);
        assert_eq!(hits.plan(&after).unwrap(), [Change::Delete { file: 0, line: 4 }]);

        let mut after: Vec<String> = hits.was.clone();
        after.insert(3, "a new line".into());
        assert_eq!(hits.plan(&after).unwrap(), [Change::Insert {
            file: 0,
            after: 4,
            text: vec!["a new line".into()]
        }]);
    }

    #[test]
    fn what_cannot_be_meant_exactly_is_refused() {
        let hits = gathered(&[("a.rs", 4), ("b.rs", 1)], 0);
        // The heading.
        let mut after: Vec<String> = hits.was.clone();
        after[0] = "c.rs".into();
        assert!(hits.plan(&after).unwrap_err().contains("not text to edit"), "{:?}", hits.plan(&after));

        let mut after: Vec<String> = hits.was.clone();
        after.remove(0);
        assert!(hits.plan(&after).unwrap_err().contains("would not delete the file"));

        // A line typed in above everything has nothing to belong to.
        let mut after: Vec<String> = hits.was.clone();
        after.insert(0, "stray".into());
        assert!(hits.plan(&after).unwrap_err().contains("above it to belong to"));
    }

    #[test]
    fn a_files_changes_are_made_bottom_up_and_checked_on_the_way() {
        let mut lines: Vec<String> = files()["a.rs"].clone();
        let changes = vec![
            Change::Edit { file: 0, line: 2, text: "third".into() },
            Change::Delete { file: 0, line: 5 },
            Change::Insert { file: 0, after: 8, text: vec!["ninth and a half".into()] },
        ];
        let ordered = for_file(&changes, 0);
        assert_eq!(ordered.iter().map(Change::line).collect::<Vec<usize>>(), [8, 5, 2]);

        let done = apply(&mut lines, &ordered, |_| None).unwrap();
        assert_eq!(done, 3);
        assert_eq!(lines[2], "third");
        assert_eq!(lines[5], "a line 7", "line 6 went");
        assert_eq!(lines[8], "ninth and a half", "after line 9, one up because line 6 went");

        // A file that has moved on is refused, not edited in the wrong place.
        let mut lines: Vec<String> = files()["a.rs"].clone();
        lines[2] = "someone else was here".into();
        let refused = apply(&mut lines, &ordered, |change| match change {
            Change::Edit { line, .. } => Some(format!("a line {}", line + 1)),
            _ => None,
        });
        assert_eq!(refused, Err("line 3 has changed since".into()));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_out_rather_than_made_up() {
        let hits = gathered(&[("gone.rs", 1), ("b.rs", 1)], 0);
        assert_eq!(hits.files, ["b.rs"]);
        assert_eq!(hits.places, 1);
        // And a hit past the end of a file it is in is dropped the same way.
        let hits = gathered(&[("b.rs", 99)], 0);
        assert!(hits.files.is_empty(), "{:?}", hits.files);
    }
}
