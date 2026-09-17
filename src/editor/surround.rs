//! Surround: putting a pair of delimiters around some text (`ys`, and `S` in
//! visual mode), taking one away (`ds`), and swapping one for another (`cs`).
//!
//! The pair to take away is found by the text objects `di(` and `di"` already
//! use, so `ds(` finds exactly what `da(` would delete. The opening bracket of
//! a pair means "with a space inside", as vim-surround has it: `ysiw(` makes
//! `( word )`, `ysiw)` makes `(word)`, and `ds(` takes the spaces too.

use super::Editor;
use crate::object::{self, Object};
use crate::view::Selection;

impl Editor {
    /// Put the pair `key` names around `[start, end)`, and leave the cursor on
    /// the opening delimiter. A range that ends in a line break - `ysip` -
    /// is closed before the break, not on the line after it.
    pub fn surround_range(&mut self, start: usize, end: usize, key: char) {
        let Some((open, close)) = delimiters(key) else {
            self.message = format!("nothing to surround with {key}");
            self.clamp_cursor();
            return;
        };
        let doc = &self.view().doc;
        let mut end = end.min(doc.len_chars());
        if end > start && doc.text.char(end - 1) == '\n' {
            end -= 1;
        }
        self.view_mut().edit_at(end, 0, &close, Some(start));
        self.view_mut().edit_at(start, 0, &open, Some(start));
        self.clamp_cursor();
    }

    /// `yss`: the cursor's line, from its first character that is not
    /// indentation to its last that is not trailing space.
    pub fn line_to_surround(&self) -> (usize, usize) {
        let view = self.view();
        let (line, _) = view.cursor_coords();
        let text = view.doc.line_str(line);
        let content = text.trim_end_matches(['\n', '\r']);
        let base = view.doc.line_to_char(line);
        let indent = content.chars().take_while(|c| c.is_whitespace()).count();
        let end = content.trim_end().chars().count();
        (base + indent, base + end.max(indent))
    }

    /// `ds(`: take away the pair around the cursor.
    pub fn delete_surround(&mut self, key: char) {
        self.replace_surround(key, None);
    }

    /// `cs"'`: the pair around the cursor, swapped for another.
    pub fn change_surround(&mut self, old: char, new: char) {
        match delimiters(new) {
            Some(pair) => self.replace_surround(old, Some(pair)),
            None => self.message = format!("nothing to surround with {new}"),
        }
    }

    /// Both ends of the pair `key` names, replaced by `new`'s - or by nothing.
    /// The closing end goes first, so the opening end's position still holds.
    fn replace_surround(&mut self, key: char, new: Option<(String, String)>) {
        let Some(object) = surrounding(key) else {
            self.message = format!("{key} does not name a pair");
            return;
        };
        let view = self.view();
        let Some((start, end)) = object::resolve(&view.doc, view.sel.head, object, true) else {
            self.message = format!("not inside {key}");
            return;
        };
        // The spaces inside the pair go with it when the pair was named by
        // its opening bracket, and never past the other end.
        let padded = matches!(key, '(' | '[' | '{');
        let space = |c: char| c == ' ' || c == '\t';
        let (mut inside_start, mut inside_end) = (start + 1, end - 1);
        if padded {
            while inside_start < inside_end && space(view.doc.text.char(inside_start)) {
                inside_start += 1;
            }
            while inside_end > inside_start && space(view.doc.text.char(inside_end - 1)) {
                inside_end -= 1;
            }
        }
        let (open, close) = new.unwrap_or_default();
        let view = self.view_mut();
        view.edit_at(inside_end, end - inside_end, &close, Some(start));
        view.edit_at(start, inside_start - start, &open, Some(start));
        view.sel = Selection::point(start);
        self.clamp_cursor();
    }
}

/// What goes either side of the text for `key`. Brackets by either half or by
/// vim's letters for them - `b`, `B`, `r` and `a` - and any other punctuation
/// on both sides as itself: quotes, `*`, `_`, `|`.
pub fn delimiters(key: char) -> Option<(String, String)> {
    let (open, close, padded) = match key {
        '(' => ('(', ')', true),
        ')' | 'b' => ('(', ')', false),
        '[' => ('[', ']', true),
        ']' | 'r' => ('[', ']', false),
        '{' => ('{', '}', true),
        '}' | 'B' => ('{', '}', false),
        '<' | '>' | 'a' => ('<', '>', false),
        c if c.is_ascii_punctuation() => (c, c, false),
        _ => return None,
    };
    Some(match padded {
        true => (format!("{open} "), format!(" {close}")),
        false => (open.to_string(), close.to_string()),
    })
}

/// The text object that finds the pair `key` names.
fn surrounding(key: char) -> Option<Object> {
    match key {
        'r' => Some(Object::Pair('[', ']')),
        'a' => Some(Object::Pair('<', '>')),
        '(' | ')' | 'b' | '{' | '}' | 'B' | '[' | ']' | '<' | '>' => object::from_key(key),
        c if c.is_ascii_punctuation() => Some(Object::Quote(c)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_brackets_pad_and_closing_ones_do_not() {
        assert_eq!(delimiters('('), Some(("( ".into(), " )".into())));
        assert_eq!(delimiters('b'), Some(("(".into(), ")".into())));
        assert_eq!(delimiters('"'), Some(("\"".into(), "\"".into())));
        assert_eq!(delimiters('x'), None);
    }
}
