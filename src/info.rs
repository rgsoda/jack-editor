//! The box that floats by the cursor: what a server says about the thing
//! under it, and the signature of the call being typed.
//!
//! Both are the same shape - a few lines of text beside the place they are
//! about - so they are one thing here, and what differs is only how long they
//! stay: an answer to `K` is read once and goes away with the next key, while
//! a signature stands as long as you are still typing arguments into it.
//!
//! Servers write markdown, sometimes elaborately. This renders it down to what
//! a terminal box can honestly show: the code and the prose, without the
//! punctuation that was only ever there to be formatted away.

use std::ops::Range;

/// The widest box worth drawing. Wider than this and it is a second window,
/// not a note beside the cursor.
pub const MAX_WIDTH: usize = 72;
/// The tallest. A docstring can run for pages; the first screenful of it is
/// what anyone reads standing up.
pub const MAX_HEIGHT: usize = 12;

/// One line of the box, and the part of it worth pointing at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// Columns to emphasise: the parameter being typed, in the signature.
    pub active: Option<Range<usize>>,
}

impl Line {
    fn plain(text: impl Into<String>) -> Line {
        Line { text: text.into(), active: None }
    }
}

/// Why the box is up, which is what decides when it comes down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `K`: read once, gone on the next key.
    Hover,
    /// The call being typed: stays until the call, or insert mode, is over.
    Signature,
}

/// A box of text floating by the cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub lines: Vec<Line>,
    pub kind: Kind,
    /// The char offset the box is about, which is where it is drawn.
    pub anchor: usize,
}

impl Info {
    /// What a server said about the thing under the cursor, rendered down from
    /// its markdown. `None` when the answer was all punctuation.
    pub fn hover(markup: &str, anchor: usize) -> Option<Info> {
        let lines: Vec<Line> = plain(markup)
            .into_iter()
            .flat_map(|line| wrap(&line, MAX_WIDTH))
            .map(|(_, text)| Line::plain(text))
            .take(MAX_HEIGHT)
            .collect();
        match lines.is_empty() {
            true => None,
            false => Some(Info { lines, kind: Kind::Hover, anchor }),
        }
    }

    /// The signature of the call being typed, with the parameter the cursor is
    /// in marked. `active` is a range of chars in `label`.
    ///
    /// Wrapping has to carry the mark with it: a long signature breaks across
    /// lines, and the parameter it is pointing at is then somewhere in the
    /// middle of the second one.
    pub fn signature(label: &str, active: Option<Range<usize>>, anchor: usize) -> Option<Info> {
        let mut lines = Vec::new();
        for (start, text) in wrap(label, MAX_WIDTH) {
            let end = start + text.chars().count();
            let here = active.clone().and_then(|active| {
                let from = active.start.max(start);
                let to = active.end.min(end);
                (from < to).then(|| from - start..to - start)
            });
            lines.push(Line { text, active: here });
        }
        lines.truncate(MAX_HEIGHT);
        match lines.is_empty() {
            true => None,
            false => Some(Info { lines, kind: Kind::Signature, anchor }),
        }
    }

    /// The widest line, which is how wide the box is drawn.
    pub fn width(&self) -> usize {
        self.lines.iter().map(|line| line.text.chars().count()).max().unwrap_or(0)
    }
}

/// Markdown as a terminal can show it: the fences and the emphasis markers are
/// the parts that only mean something to a renderer, so they go, and what they
/// were wrapped around stays.
fn plain(markup: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in markup.lines() {
        let line = line.trim_end();
        // A fence says the next lines are code, which in a box they already
        // look like. The language on the opening one is not worth a line.
        if line.trim_start().starts_with("```") {
            continue;
        }
        // A rule between the signature and the prose under it. The blank line
        // it becomes says the same thing with less ink.
        let ruled = line.trim();
        let rule = ruled.len() >= 3 && ruled.chars().all(|c| matches!(c, '-' | '*' | '_'));
        let text = match rule {
            true => String::new(),
            false => inline(line.trim_start_matches('#').trim_start()),
        };
        // One blank line between paragraphs, never two, and none at the top.
        if text.trim().is_empty() {
            if !out.last().is_none_or(|last: &String| last.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        out.push(text);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// The markers inside a line: backticks, and the stars around bold and italic.
/// Dropped rather than turned into styling, because a box of code with the
/// quotes still in it reads worse than one without.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => {}
            '*' | '_' if chars.peek() == Some(&c) => {
                chars.next();
            }
            // An escaped character in markdown is the character itself.
            '\\' if chars.peek().is_some_and(|c| c.is_ascii_punctuation()) => {
                out.push(chars.next().expect("peeked"));
            }
            other => out.push(other),
        }
    }
    out.trim_end().to_string()
}

/// One line broken to `width`, at spaces where there is one to break at and
/// anywhere when there is not. Each piece comes with where it started in the
/// original, so a mark on the text can be carried across the break.
fn wrap(text: &str, width: usize) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return vec![(0, text.to_string())];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        if end == chars.len() {
            out.push((start, chars[start..].iter().collect()));
            break;
        }
        // The last space that fits, so words stay whole - unless there is
        // none, which is one long word and has to be cut. One past the end is
        // looked at too: a word ending exactly at the edge is a word that fit.
        let scan = (end + 1).min(chars.len());
        let cut = chars[start..scan].iter().rposition(|c| *c == ' ').map(|i| start + i);
        let (piece, next) = match cut {
            Some(at) if at > start => (at, at + 1),
            _ => (end, end),
        };
        out.push((start, chars[start..piece].iter().collect()));
        start = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fenced_signature_comes_out_as_the_signature() {
        let markup = "```python\n(method) def count(self) -> int\n```\n---\nHow many there are.";
        let info = Info::hover(markup, 0).expect("something to show");
        let lines: Vec<&str> = info.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lines, ["(method) def count(self) -> int", "", "How many there are."]);
    }

    #[test]
    fn the_markers_go_and_what_they_marked_stays() {
        assert_eq!(inline("a `code` word"), "a code word");
        assert_eq!(inline("**bold** and __under__"), "bold and under");
        assert_eq!(inline(r"a \* star"), "a * star");
    }

    #[test]
    fn an_answer_of_nothing_but_punctuation_is_no_answer() {
        assert_eq!(Info::hover("", 0), None);
        assert_eq!(Info::hover("```rust\n```\n---\n", 0), None);
    }

    #[test]
    fn blank_lines_are_never_stacked() {
        let info = Info::hover("one\n\n\n\ntwo\n\n", 0).expect("something to show");
        let lines: Vec<&str> = info.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lines, ["one", "", "two"]);
    }

    #[test]
    fn a_line_too_wide_breaks_at_a_space() {
        assert_eq!(wrap("one two three", 7), [(0, "one two".into()), (8, "three".into())]);
        // No space to break at: cut where it must be.
        assert_eq!(wrap("abcdefghij", 4), [(0, "abcd".into()), (4, "efgh".into()), (8, "ij".into())]);
    }

    #[test]
    fn the_active_parameter_is_carried_across_the_break() {
        let label = "def competition_count(self, tournament: Tournament, played: bool, year: int) -> int";
        // `year: int` is the parameter being typed, and it is past the width.
        let at = label.find("year").expect("in the label");
        let info = Info::signature(label, Some(at..at + 9), 0).expect("a signature");
        assert!(info.lines.len() > 1, "wide enough to wrap");
        // The mark can be broken by the wrap the same as the text is; what
        // matters is that every piece of it is still marked, and nothing else.
        let marked: Vec<&str> = info
            .lines
            .iter()
            .filter_map(|line| line.active.clone().map(|active| &line.text[active]))
            .collect();
        assert_eq!(marked.join(" "), "year: int");
    }

    #[test]
    fn a_signature_that_fits_keeps_its_mark_where_it_was() {
        let info = Info::signature("f(a, b)", Some(2..3), 0).expect("a signature");
        assert_eq!(info.lines, [Line { text: "f(a, b)".into(), active: Some(2..3) }]);
        assert_eq!(info.width(), 7);
    }
}
