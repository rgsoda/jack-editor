use anyhow::Result;
use std::ops::Range;
use std::path::Path;
use unicode_segmentation::GraphemeCursor;
use unicode_width::UnicodeWidthChar;

use crate::buffer::Document;
use crate::history::{Change, History, Transaction};
use crate::syntax::{Highlights, Syntax, language_for_path};
use crate::theme::Theme;

pub const TAB_WIDTH: usize = 4;
/// Rows kept between the cursor and the top/bottom edge when scrolling.
const SCROLLOFF: usize = 3;

/// A cursor plus the region it has selected. `head` is where the cursor is
/// drawn; `anchor` is where the selection started. Equal means no selection.
///
/// Every command operates on a `Selection`, so growing this into a
/// `Vec<Selection>` for multi-cursor later is mechanical rather than a rewrite.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn point(pos: usize) -> Self {
        Selection { anchor: pos, head: pos }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The selected char range, normalized so start <= end.
    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    PageUp,
    PageDown,
    FileStart,
    FileEnd,
}

pub struct Editor {
    pub doc: Document,
    pub sel: Selection,
    /// Display column vertical movement tries to return to, so moving down
    /// through a short line and back out keeps the original column.
    goal_col: Option<usize>,
    pub scroll_top: usize,
    pub scroll_left: usize,
    pub width: usize,
    /// Text rows only; the status line is not part of this.
    pub height: usize,
    history: History,
    syntax: Option<Syntax>,
    pub theme: Theme,
    /// Transient status-line text, cleared on the next keypress.
    pub message: String,
}

impl Editor {
    pub fn scratch() -> Self {
        Editor::with_doc(Document::scratch())
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut editor = Editor::with_doc(Document::open(path)?);
        editor.attach_syntax();
        Ok(editor)
    }

    /// Set up highlighting for the document's language, if we know it. A broken
    /// grammar or query is reported in the status line, never fatal - you can
    /// still edit the file without colors.
    fn attach_syntax(&mut self) {
        let Some(config) = language_for_path(self.doc.path.as_deref()) else {
            return;
        };
        match Syntax::new(config, &self.doc.text, &self.theme) {
            Ok(syntax) => self.syntax = Some(syntax),
            Err(err) => self.message = format!("highlighting off: {err:#}"),
        }
    }

    pub fn highlights(&self, range: Range<usize>) -> Highlights {
        match &self.syntax {
            Some(syntax) => syntax.highlights(&self.doc.text, range, &self.theme),
            None => Highlights::none(),
        }
    }

    fn with_doc(doc: Document) -> Self {
        let (theme, warning) = Theme::load_user();
        let mut editor = Editor {
            doc,
            sel: Selection::point(0),
            goal_col: None,
            scroll_top: 0,
            scroll_left: 0,
            width: 80,
            height: 24,
            history: History::new(),
            syntax: None,
            theme,
            message: String::new(),
        };
        if let Some(warning) = warning {
            editor.message = warning;
        }
        editor
    }

    pub fn set_viewport(&mut self, width: usize, height: usize) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    /// (line, char offset in line) of the cursor.
    pub fn cursor_coords(&self) -> (usize, usize) {
        self.doc.coords(self.sel.head)
    }

    /// Where to leave the terminal cursor, in screen coordinates.
    pub fn cursor_screen(&self) -> (u16, u16) {
        let (line, _) = self.cursor_coords();
        (
            self.cursor_display_col().saturating_sub(self.scroll_left) as u16,
            line.saturating_sub(self.scroll_top) as u16,
        )
    }

    /// Screen column of the cursor, with tabs expanded.
    pub fn cursor_display_col(&self) -> usize {
        let (line, col) = self.cursor_coords();
        display_col(&self.doc.line_str(line), col)
    }

    pub fn move_cursor(&mut self, m: Move, extend: bool) {
        let head = match m {
            Move::Left => self.grapheme_left(self.sel.head),
            Move::Right => self.grapheme_right(self.sel.head),
            Move::Up => self.vertical(-1),
            Move::Down => self.vertical(1),
            Move::LineStart => {
                let (line, _) = self.cursor_coords();
                self.doc.line_to_char(line)
            }
            Move::LineEnd => {
                let (line, _) = self.cursor_coords();
                self.doc.line_to_char(line) + self.doc.line_len_chars(line)
            }
            Move::PageUp => self.vertical(-(self.height as isize)),
            Move::PageDown => self.vertical(self.height as isize),
            Move::FileStart => 0,
            Move::FileEnd => self.doc.len_chars(),
        };

        // Vertical moves preserve the goal column; everything else resets it.
        if !matches!(m, Move::Up | Move::Down | Move::PageUp | Move::PageDown) {
            self.goal_col = None;
        }

        self.sel.head = head;
        if !extend {
            self.sel.anchor = head;
        }
    }

    fn grapheme_left(&self, pos: usize) -> usize {
        let (line, col) = self.doc.coords(pos);
        if col == 0 {
            if line == 0 {
                return 0;
            }
            let prev = line - 1;
            return self.doc.line_to_char(prev) + self.doc.line_len_chars(prev);
        }
        let text = self.doc.line_str(line);
        let byte = byte_of_char(&text, col);
        let prev_byte = prev_boundary(&text, byte);
        pos - (char_of_byte(&text, byte) - char_of_byte(&text, prev_byte))
    }

    fn grapheme_right(&self, pos: usize) -> usize {
        let (line, col) = self.doc.coords(pos);
        let line_len = self.doc.line_len_chars(line);
        if col >= line_len {
            if line + 1 >= self.doc.len_lines() {
                return pos;
            }
            return self.doc.line_to_char(line + 1);
        }
        let text = self.doc.line_str(line);
        let byte = byte_of_char(&text, col);
        let next_byte = next_boundary(&text, byte);
        pos + (char_of_byte(&text, next_byte) - char_of_byte(&text, byte))
    }

    /// Move `delta` lines, landing as close as possible to the goal column.
    fn vertical(&mut self, delta: isize) -> usize {
        let (line, col) = self.cursor_coords();
        let goal = *self
            .goal_col
            .get_or_insert_with(|| display_col(&self.doc.line_str(line), col));

        let last = self.doc.len_lines().saturating_sub(1);
        let target = (line as isize + delta).clamp(0, last as isize) as usize;

        let text = self.doc.line_str(target);
        self.doc.line_to_char(target) + char_col_at_display(&text, goal)
    }

    /// Scroll the viewport so the cursor is visible, keeping SCROLLOFF rows of
    /// context where the buffer allows it.
    pub fn scroll_to_cursor(&mut self) {
        let (line, _) = self.cursor_coords();
        let col = self.cursor_display_col();

        let pad = SCROLLOFF.min(self.height.saturating_sub(1) / 2);
        if line < self.scroll_top + pad {
            self.scroll_top = line.saturating_sub(pad);
        }
        let bottom = self.scroll_top + self.height;
        if line + pad >= bottom {
            self.scroll_top = (line + pad + 1).saturating_sub(self.height);
        }
        // Never scroll past the last line.
        let max_top = self.doc.len_lines().saturating_sub(1);
        self.scroll_top = self.scroll_top.min(max_top);

        if col < self.scroll_left {
            self.scroll_left = col;
        } else if col >= self.scroll_left + self.width {
            self.scroll_left = col + 1 - self.width;
        }
    }

    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// The single funnel for every edit: builds a transaction, applies it,
    /// moves the cursor, and records it for undo.
    fn edit(&mut self, pos: usize, remove_chars: usize, insert: &str) {
        let removed = self.doc.slice_str(pos, pos + remove_chars);
        if removed.is_empty() && insert.is_empty() {
            return;
        }

        let sel_before = self.sel;
        let sel_after = Selection::point(pos + insert.chars().count());
        let tx = Transaction::new(
            vec![Change { pos, removed, inserted: insert.to_string() }],
            sel_before,
            sel_after,
        );

        let edits = tx.apply(&mut self.doc);
        if let Some(syntax) = self.syntax.as_mut() {
            syntax.edit(&edits, &self.doc.text);
        }
        self.sel = sel_after;
        self.goal_col = None;
        self.history.push(tx);
    }

    /// Insert text, replacing the selection if there is one.
    pub fn insert(&mut self, text: &str) {
        let (start, end) = self.sel.range();
        self.edit(start, end - start, text);
    }

    /// Enter, carrying the current line's leading whitespace onto the new line.
    pub fn insert_newline(&mut self) {
        let (start, end) = self.sel.range();
        let (line, col) = self.doc.coords(start);
        let text = self.doc.line_str(line);
        let indent: String = text
            .chars()
            .take(col)
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        self.edit(start, end - start, &format!("\n{indent}"));
    }

    pub fn delete_backward(&mut self) {
        if !self.sel.is_empty() {
            let (start, end) = self.sel.range();
            self.edit(start, end - start, "");
            return;
        }
        let head = self.sel.head;
        let prev = self.grapheme_left(head);
        if prev < head {
            self.edit(prev, head - prev, "");
        }
    }

    pub fn delete_forward(&mut self) {
        if !self.sel.is_empty() {
            let (start, end) = self.sel.range();
            self.edit(start, end - start, "");
            return;
        }
        let head = self.sel.head;
        let next = self.grapheme_right(head);
        if next > head {
            self.edit(head, next - head, "");
        }
    }

    pub fn undo(&mut self) {
        match self.history.undo() {
            Some(inverse) => self.apply_history(inverse),
            None => self.message = "nothing to undo".into(),
        }
    }

    pub fn redo(&mut self) {
        match self.history.redo() {
            Some(tx) => self.apply_history(tx),
            None => self.message = "nothing to redo".into(),
        }
    }

    fn apply_history(&mut self, tx: Transaction) {
        let edits = tx.apply(&mut self.doc);
        if let Some(syntax) = self.syntax.as_mut() {
            syntax.edit(&edits, &self.doc.text);
        }
        self.sel = tx.sel_after;
        self.goal_col = None;
    }

    pub fn save(&mut self) {
        match self.doc.save() {
            Ok(()) => {
                self.history.mark_saved();
                self.message = format!("wrote {}", self.doc.display_name());
            }
            Err(err) => self.message = format!("{err:#}"),
        }
    }
}

/// Display width of one char at screen column `at` (tabs depend on position).
pub fn char_width(ch: char, at: usize) -> usize {
    if ch == '\t' {
        TAB_WIDTH - (at % TAB_WIDTH)
    } else {
        // TODO: width should be measured per grapheme cluster, not per char;
        // this is wrong for emoji ZWJ sequences and combining marks.
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

/// Screen column of char offset `char_col` within `line`.
pub fn display_col(line: &str, char_col: usize) -> usize {
    let mut w = 0;
    for ch in line.chars().take(char_col) {
        w += char_width(ch, w);
    }
    w
}

/// Char offset in `line` whose screen column is at or just past `target`.
fn char_col_at_display(line: &str, target: usize) -> usize {
    let mut w = 0;
    let mut col = 0;
    for ch in line.chars() {
        if w >= target {
            break;
        }
        w += char_width(ch, w);
        col += 1;
    }
    col
}

fn byte_of_char(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

fn char_of_byte(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx].chars().count()
}

fn next_boundary(s: &str, byte: usize) -> usize {
    let mut cursor = GraphemeCursor::new(byte, s.len(), true);
    cursor.next_boundary(s, 0).ok().flatten().unwrap_or(s.len())
}

fn prev_boundary(s: &str, byte: usize) -> usize {
    let mut cursor = GraphemeCursor::new(byte, s.len(), true);
    cursor.prev_boundary(s, 0).ok().flatten().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn editor(text: &str) -> Editor {
        let mut e = Editor::scratch();
        e.doc.text = Rope::from_str(text);
        e
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        assert_eq!(display_col("\tx", 1), 4);
        assert_eq!(display_col("ab\tx", 3), 4);
        assert_eq!(display_col("abcd\tx", 5), 8);
    }

    #[test]
    fn wide_chars_count_two_columns() {
        assert_eq!(display_col("日本", 2), 4);
        assert_eq!(char_col_at_display("日本", 2), 1);
    }

    #[test]
    fn horizontal_movement_steps_over_whole_graphemes() {
        // "e" + combining acute is one grapheme, two chars.
        let mut e = editor("e\u{301}x");
        e.move_cursor(Move::Right, false);
        assert_eq!(e.sel.head, 2);
        e.move_cursor(Move::Left, false);
        assert_eq!(e.sel.head, 0);
    }

    #[test]
    fn horizontal_movement_crosses_line_boundaries() {
        let mut e = editor("ab\ncd\n");
        e.sel = Selection::point(2); // end of line 1
        e.move_cursor(Move::Right, false);
        assert_eq!(e.cursor_coords(), (1, 0));
        e.move_cursor(Move::Left, false);
        assert_eq!(e.cursor_coords(), (0, 2));
    }

    #[test]
    fn vertical_movement_keeps_the_goal_column() {
        let mut e = editor("long line\n\nlong line\n");
        e.sel = Selection::point(7); // column 7 on line 0
        e.move_cursor(Move::Down, false); // empty line, snaps to column 0
        assert_eq!(e.cursor_coords(), (1, 0));
        e.move_cursor(Move::Down, false); // back out to the goal column
        assert_eq!(e.cursor_coords(), (2, 7));
    }

    #[test]
    fn movement_at_the_buffer_edges_is_clamped() {
        let mut e = editor("ab\n");
        e.move_cursor(Move::Left, false);
        assert_eq!(e.sel.head, 0);
        e.move_cursor(Move::FileEnd, false);
        let end = e.sel.head;
        e.move_cursor(Move::Right, false);
        assert_eq!(e.sel.head, end);
    }

    #[test]
    fn shift_movement_extends_from_the_anchor() {
        let mut e = editor("abcdef\n");
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, true);
        e.move_cursor(Move::Right, true);
        assert_eq!(e.sel.range(), (1, 3));
        e.move_cursor(Move::Right, false);
        assert!(e.sel.is_empty());
    }

    fn type_str(e: &mut Editor, text: &str) {
        for ch in text.chars() {
            e.insert(&ch.to_string());
        }
    }

    #[test]
    fn typing_then_undo_restores_text_and_cursor() {
        let mut e = editor("hello\n");
        e.move_cursor(Move::LineEnd, false);
        let before = e.sel.head;
        type_str(&mut e, " there");
        assert_eq!(e.doc.text.to_string(), "hello there\n");

        e.undo();
        assert_eq!(e.doc.text.to_string(), "hello\n");
        assert_eq!(e.sel.head, before);

        e.redo();
        assert_eq!(e.doc.text.to_string(), "hello there\n");
    }

    #[test]
    fn a_run_of_typing_is_one_undo_step() {
        let mut e = editor("");
        type_str(&mut e, "abc");
        e.undo();
        assert_eq!(e.doc.text.to_string(), "");
    }

    #[test]
    fn a_newline_ends_the_coalescing_run() {
        let mut e = editor("");
        type_str(&mut e, "ab");
        e.insert_newline();
        type_str(&mut e, "cd");

        e.undo();
        assert_eq!(e.doc.text.to_string(), "ab\n");
        e.undo();
        assert_eq!(e.doc.text.to_string(), "ab");
        e.undo();
        assert_eq!(e.doc.text.to_string(), "");
    }

    #[test]
    fn a_run_of_backspaces_is_one_undo_step() {
        let mut e = editor("abcdef");
        e.move_cursor(Move::FileEnd, false);
        e.delete_backward();
        e.delete_backward();
        e.delete_backward();
        assert_eq!(e.doc.text.to_string(), "abc");
        e.undo();
        assert_eq!(e.doc.text.to_string(), "abcdef");
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut e = editor("abcdef");
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, true);
        e.move_cursor(Move::Right, true);
        e.insert("X");
        assert_eq!(e.doc.text.to_string(), "aXdef");
        e.undo();
        assert_eq!(e.doc.text.to_string(), "abcdef");
        assert_eq!(e.sel.range(), (1, 3));
    }

    #[test]
    fn backspace_deletes_a_whole_grapheme() {
        let mut e = editor("xe\u{301}");
        e.move_cursor(Move::FileEnd, false);
        e.delete_backward();
        assert_eq!(e.doc.text.to_string(), "x");
    }

    #[test]
    fn enter_carries_the_leading_indent() {
        let mut e = editor("    foo\n");
        e.move_cursor(Move::LineEnd, false);
        e.insert_newline();
        assert_eq!(e.doc.text.to_string(), "    foo\n    \n");

        // Splitting inside the leading whitespace: the indent measured is only
        // what is before the cursor, and it is prepended to the text that moves
        // down, so the tail keeps its original indentation.
        let mut e = editor("    foo\n");
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, false);
        e.insert_newline();
        assert_eq!(e.doc.text.to_string(), "  \n    foo\n");
    }

    #[test]
    fn editing_after_undo_drops_the_redo_stack() {
        let mut e = editor("");
        type_str(&mut e, "abc");
        e.undo();
        type_str(&mut e, "xyz");
        e.redo();
        assert_eq!(e.doc.text.to_string(), "xyz");
    }

    #[test]
    fn deleting_backward_at_the_start_of_the_buffer_is_a_no_op() {
        let mut e = editor("abc");
        e.delete_backward();
        assert_eq!(e.doc.text.to_string(), "abc");
        assert!(!e.is_modified());
    }
}
