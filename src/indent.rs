//! What a file's own indentation is made of.
//!
//! Configuration cannot answer this: a Rust file and a Python file want
//! different things, and so do two Python files. The file itself already says
//! which, in every line it has - so it is read rather than guessed, and the
//! configured default is what is left when a file has nothing to say.
//!
//! This is vim-sleuth's job, and roughly vim-sleuth's method: a vote per line
//! whose indent grew on the one before it, because the difference between two
//! indents is the step, whatever the nesting.

use crate::buffer::Document;
use crate::view::{Indent, TAB_WIDTH};

/// How many lines are worth reading. A file indents the same way on line 40 as
/// on line 4000, and the whole of a large file is a cost paid on every open.
const SAMPLE: usize = 512;

/// The widest step worth believing. Eight is deep; anything wider is two
/// levels at once, or a continuation line lined up under a bracket.
const WIDEST: usize = 8;

/// What this document indents with, or `None` when it has no indentation to
/// learn from - a new file, a flat one, a README.
pub fn detect(doc: &Document) -> Option<Indent> {
    let mut tabs = 0usize;
    let mut spaced = 0usize;
    // votes[n] is how many lines stepped in by n spaces.
    let mut votes = [0usize; WIDEST + 1];
    let mut previous = 0usize;

    for line in 0..doc.len_lines().min(SAMPLE) {
        let text = doc.line_str(line);
        let text = text.trim_end_matches('\n');
        if text.trim().is_empty() {
            continue;
        }
        let white = crate::buffer::indent_of(text);
        if white.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let spaces = white.len();
        if spaces > 0 {
            spaced += 1;
        }
        // A step in, not a level: nesting three deep votes for the step, not
        // for twelve. A step out says nothing - the line it returns to has
        // already voted.
        if spaces > previous && spaces - previous <= WIDEST {
            votes[spaces - previous] += 1;
        }
        previous = spaces;
    }

    // Tabs win by being used at all, near enough: a file that indents with
    // tabs anywhere indents with tabs, and the spaces are alignment inside
    // lines that already began with one.
    if tabs > spaced {
        return Some(Indent { width: TAB_WIDTH, tabs: true });
    }
    // The most popular step, and the narrowest of equals: with two votes for
    // four and two for eight, four is the step and eight is two of them.
    let width = votes
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|(width, count)| (**count, std::cmp::Reverse(*width)))
        .filter(|(_, count)| **count > 0)
        .map(|(width, _)| width)?;
    Some(Indent { width, tabs: false })
}

#[cfg(test)]
mod tests {
    use super::detect;
    use crate::buffer::Document;

    fn indent_of(text: &str) -> Option<(usize, bool)> {
        let mut doc = Document::scratch();
        doc.text = ropey::Rope::from_str(text);
        detect(&doc).map(|indent| (indent.width, indent.tabs))
    }

    #[test]
    fn a_file_of_spaces_says_how_many() {
        let python = "def f():\n    if x:\n        return 1\n    return 0\n";
        assert_eq!(indent_of(python), Some((4, false)));

        let two = "function f() {\n  if (x) {\n    return 1;\n  }\n}\n";
        assert_eq!(indent_of(two), Some((2, false)));

        let eight = "if a:\n        b = 1\n";
        assert_eq!(indent_of(eight), Some((8, false)));
    }

    #[test]
    fn a_file_of_tabs_says_so() {
        let text = "fn a() {\n\tlet x = 1;\n\tif x > 0 {\n\t\tcall();\n\t}\n}\n";
        assert_eq!(indent_of(text), Some((crate::view::TAB_WIDTH, true)));
    }

    #[test]
    fn nothing_to_learn_from_leaves_the_default_alone() {
        assert_eq!(indent_of(""), None);
        assert_eq!(indent_of("one\ntwo\nthree\n"), None);
        // Blank lines and whitespace-only lines are not indentation.
        assert_eq!(indent_of("a\n\n   \nb\n"), None);
    }

    #[test]
    fn a_stray_line_does_not_outvote_the_file() {
        // Four spaces throughout, and one line someone left with two.
        let text = "\
def f():
    a = 1
    if a:
        b = 2
        if b:
            c = 3
  stray = 4
";
        assert_eq!(indent_of(text), Some((4, false)));
    }

    #[test]
    fn the_narrower_step_wins_a_tie() {
        // One step of two and one of four: four is two of them, not a step of
        // its own.
        let text = "a:\n  b:\n      c\nd:\n  e\n";
        assert_eq!(indent_of(text), Some((2, false)));
    }

    #[test]
    fn deep_nesting_votes_for_the_step_not_the_column() {
        let text = "a:\n    b:\n        c:\n            d\n";
        assert_eq!(indent_of(text), Some((4, false)));
    }
}

