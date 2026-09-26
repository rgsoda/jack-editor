//! `:diff` - what is different between two buffers, written into a third.
//!
//! The unified format, because it is the one every other tool in the terminal
//! writes and the one every pair of eyes already reads. It goes into a
//! scratch buffer rather than a pane of its own: a diff you can search, yank
//! from and leave open is worth more than one that owns the screen.

use similar::{ChangeTag, TextDiff};

/// How many unchanged lines go either side of a change. Three, as `diff -u`
/// has it.
const CONTEXT: usize = 3;

/// The two buffers as a unified diff, or an empty vector when they are the
/// same - which the caller says out loud rather than opening a buffer of
/// nothing.
pub fn unified(left: &str, left_text: &str, right: &str, right_text: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(left_text, right_text);
    let mut out = Vec::new();
    for group in diff.grouped_ops(CONTEXT).iter() {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        // The `@@` line counts lines from one, and counts zero lines as
        // starting at the line before, which is what `diff -u` prints for a
        // pure insertion at the top of a file.
        let (old, new) = (first.old_range().start, first.new_range().start);
        let (old_end, new_end) = (last.old_range().end, last.new_range().end);
        let (old_len, new_len) = (old_end - old, new_end - new);
        out.push(format!(
            "@@ -{},{} +{},{} @@",
            match old_len {
                0 => old,
                _ => old + 1,
            },
            old_len,
            match new_len {
                0 => new,
                _ => new + 1,
            },
            new_len,
        ));
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                out.push(format!("{sign}{}", change.value().trim_end_matches('\n')));
            }
        }
    }
    if out.is_empty() {
        return out;
    }
    let mut header = vec![format!("--- {left}"), format!("+++ {right}")];
    header.append(&mut out);
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_buffers_the_same_are_no_diff_at_all() {
        assert!(unified("a", "one\ntwo\n", "b", "one\ntwo\n").is_empty());
        assert!(unified("a", "", "b", "").is_empty());
    }

    #[test]
    fn a_changed_line_is_one_taken_away_and_one_put_back() {
        let got = unified("old.txt", "one\ntwo\nthree\n", "new.txt", "one\nTWO\nthree\n");
        assert_eq!(got[0], "--- old.txt");
        assert_eq!(got[1], "+++ new.txt");
        assert_eq!(got[2], "@@ -1,3 +1,3 @@");
        assert_eq!(&got[3..], [" one", "-two", "+TWO", " three"]);
    }

    #[test]
    fn an_added_line_says_where_it_went_in() {
        let got = unified("a", "one\n", "b", "one\ntwo\n");
        assert_eq!(got[2], "@@ -1,1 +1,2 @@");
        assert_eq!(&got[3..], [" one", "+two"]);
    }

    #[test]
    fn a_file_against_nothing_is_every_line_taken_away() {
        let got = unified("a", "one\ntwo\n", "b", "");
        assert_eq!(got[2], "@@ -1,2 +0,0 @@");
        assert_eq!(&got[3..], ["-one", "-two"]);
    }
}
