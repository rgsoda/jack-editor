//! Block selection: `^b`, and what can be done to a rectangle.
//!
//! `v` and `V` select a range - one run of characters, with a start and an
//! end - and every operator in the editor takes a range. A block is not a
//! range: it is one piece of each of several lines, and the pieces are not
//! next to each other in the rope. So it gets its own geometry here, and the
//! commands that understand it are the ones written here.
//!
//! The rectangle is held the way vim holds it, in the two ends of the
//! selection rather than in a field of its own: the corners are the cursor
//! and the anchor, and the sides are the screen columns those two are at.
//! Screen columns, not character offsets, because a rectangle is a thing you
//! see - a tab is one character and eight columns wide, and the block has to
//! line up on screen with what was selected.

use crate::register::{Kind, RegisterValue};
use crate::view::{Selection, char_col_at_display, display_col};

use super::{Editor, Mode};

/// A rectangle over the buffer: the lines it covers, the screen columns it
/// sits between, and the piece of each line that is inside it.
pub struct Block {
    /// The first line, so a line number indexes into `rows`.
    pub first: usize,
    /// The screen columns of the left and right sides. The right side is
    /// inclusive, as visual mode's cursor is on the last character it takes.
    pub left: usize,
    pub right: usize,
    /// `$`: the block runs to the end of every line, however ragged they are,
    /// and the right side is wherever each line happens to stop.
    pub to_eol: bool,
    /// The characters inside the block on each line, as a range in the rope.
    /// A line too short to reach the block has an empty one.
    pub rows: Vec<(usize, usize)>,
}

impl Block {
    /// The piece of `line` that is in the block, if the block reaches it.
    pub fn row(&self, line: usize) -> Option<(usize, usize)> {
        self.rows.get(line.checked_sub(self.first)?).copied()
    }

    pub fn lines(&self) -> std::ops::RangeInclusive<usize> {
        self.first..=self.first + self.rows.len() - 1
    }
}

impl Editor {
    /// The block `^b` has selected, or `None` in any other mode. Everything
    /// that has to draw or edit a rectangle starts here.
    pub fn block(&self) -> Option<Block> {
        if self.mode != Mode::VisualBlock {
            return None;
        }
        let view = self.view();
        let (anchor_line, anchor_col) = view.doc.coords(view.sel.anchor);
        let (head_line, head_col) = view.doc.coords(view.sel.head);
        let anchor_at = display_col(&view.doc.line_str(anchor_line), anchor_col);
        let head_at = display_col(&view.doc.line_str(head_line), head_col);
        let (left, right) = (anchor_at.min(head_at), anchor_at.max(head_at));
        let (first, last) = (anchor_line.min(head_line), anchor_line.max(head_line));
        let to_eol = self.block_to_eol;
        let rows = (first..=last).map(|line| self.row_at(line, left, right, to_eol)).collect();
        Some(Block { first, left, right, to_eol, rows })
    }

    /// The characters of `line` between two screen columns. A line that stops
    /// short of the left side contributes nothing, which is what makes a block
    /// over ragged lines take only what is there.
    fn row_at(&self, line: usize, left: usize, right: usize, to_eol: bool) -> (usize, usize) {
        let view = self.view();
        let text = view.doc.line_str(line);
        let len = view.doc.line_len_chars(line);
        let base = view.doc.line_to_char(line);
        let start = char_col_at_display(&text, left).min(len);
        let end = match to_eol {
            // `$`: every line to wherever it stops, which is the whole point
            // of it - the lines are ragged and the block follows them.
            true => len,
            // One past the right side, so the character the cursor is on is in.
            false => char_col_at_display(&text, right + 1).min(len),
        };
        (base + start, base + end.max(start))
    }

    /// `d` and `x` over a block: every row goes, and the register remembers
    /// it was a rectangle so `p` can put it back as one.
    pub fn delete_block(&mut self, register: Option<char>) {
        let Some(block) = self.block() else {
            return;
        };
        let text = self.block_text(&block);
        let at = block.rows[0].0;
        // Bottom up: a row's position is only still right while nothing below
        // it - which is to say, before it in the rope - has moved. One undo
        // step over the lot, so `u` takes the whole rectangle back.
        self.begin_undo_group();
        for &(start, end) in block.rows.iter().rev() {
            if end > start {
                self.view_mut().edit_at(start, end - start, "", None);
            }
        }
        self.end_undo_group();
        let registers = &mut self.registers;
        registers.record_delete(register, RegisterValue::block(text));
        self.view_mut().sel = Selection::point(at);
        self.set_mode(Mode::Normal);
        self.clamp_cursor();
    }

    /// `y` over a block. The cursor lands on the top left corner, as it does
    /// on the start of any other yank.
    pub fn yank_block(&mut self, register: Option<char>) {
        let Some(block) = self.block() else {
            return;
        };
        let text = self.block_text(&block);
        let at = block.rows[0].0;
        self.registers.record_yank(register, RegisterValue::block(text));
        self.view_mut().sel = Selection::point(at);
        self.set_mode(Mode::Normal);
        self.clamp_cursor();
    }

    /// The rows of a block as text, one line each - ragged rows stay ragged,
    /// and putting the block back pads them out again.
    fn block_text(&self, block: &Block) -> String {
        let view = self.view();
        block.rows.iter().map(|&(start, end)| view.doc.slice_str(start, end)).collect::<Vec<_>>().join("\n")
    }

    /// `I` and `A` over a block, and `c`, which is `I` after a delete: type
    /// once on the first line, and what was typed goes on all of them.
    ///
    /// `append` is `A`: the right side rather than the left, and a line too
    /// short to reach it is padded out with spaces, since appending to the
    /// end of a ragged block is how a column of trailing text gets added.
    pub fn insert_block(&mut self, append: bool, change: bool) {
        let Some(block) = self.block() else {
            return;
        };
        let lines: Vec<usize> = block.lines().collect();
        // A `$` block has no right side to append at: `A` on one puts the text
        // at the end of each line, wherever that is, which is what makes it
        // the way to add a trailing column to ragged lines.
        if append && block.to_eol {
            return self.append_at_line_ends(&lines, change, &block);
        }
        let column = match append {
            true => block.right + 1,
            false => block.left,
        };

        self.begin_undo_group();
        if change {
            for &(start, end) in block.rows.iter().rev() {
                if end > start {
                    self.view_mut().edit_at(start, end - start, "", None);
                }
            }
        }
        // Where typing starts, on the line the cursor ends up on. The other
        // lines are caught up when insert mode is left.
        let at = self.pad_to(lines[0], column);
        self.view_mut().sel = Selection::point(at);
        self.pending_block = Some(PendingBlock { lines, column, at });
        self.set_mode(Mode::Insert);
    }

    /// `A` on a `$` block: typing goes at the end of each line, and the lines
    /// are where they are rather than being padded out to a column.
    fn append_at_line_ends(&mut self, lines: &[usize], change: bool, block: &Block) {
        self.begin_undo_group();
        if change {
            for &(start, end) in block.rows.iter().rev() {
                if end > start {
                    self.view_mut().edit_at(start, end - start, "", None);
                }
            }
        }
        let at = self.line_end(lines[0]);
        self.view_mut().sel = Selection::point(at);
        self.pending_block = Some(PendingBlock { lines: lines.to_vec(), column: usize::MAX, at });
        self.set_mode(Mode::Insert);
    }

    /// The end of a line's text, before its newline.
    fn line_end(&self, line: usize) -> usize {
        let view = self.view();
        match line > view.last_line() {
            true => view.doc.len_chars(),
            false => view.doc.line_to_char(line) + view.doc.line_len_chars(line),
        }
    }

    /// The position of screen column `column` on `line`, padding the line out
    /// with spaces when it stops short of it. A line that is not there at all
    /// - past the end of the buffer - is left alone and reports its own end.
    fn pad_to(&mut self, line: usize, column: usize) -> usize {
        let view = self.view();
        if line > view.last_line() {
            return view.doc.len_chars();
        }
        let text = view.doc.line_str(line);
        let base = view.doc.line_to_char(line);
        let len = view.doc.line_len_chars(line);
        let width = display_col(&text, len);
        if width >= column {
            return base + char_col_at_display(&text, column).min(len);
        }
        let padding = " ".repeat(column - width);
        self.view_mut().edit_at(base + len, 0, &padding, None);
        base + len + padding.chars().count()
    }

    /// Insert mode is over after a block `I`, `A` or `c`: whatever was typed
    /// on the first line goes on the rest of them.
    ///
    /// What was typed is the text between where insert started and where the
    /// cursor is now. A multi-line insert is not repeated - there is no column
    /// left to repeat it at - and neither is one that was backspaced away.
    pub fn finish_block_insert(&mut self) {
        let Some(pending) = self.pending_block.take() else {
            return;
        };
        let view = self.view();
        let head = view.sel.head;
        if head <= pending.at || view.doc.char_to_line(head) != view.doc.char_to_line(pending.at) {
            self.end_undo_group();
            return;
        }
        let typed = view.doc.slice_str(pending.at, head);
        for &line in pending.lines.iter().skip(1).rev() {
            // `usize::MAX` is the end of the line, which is where a `$` block
            // appends: there is no column to pad out to.
            let at = match pending.column {
                usize::MAX => self.line_end(line),
                column => self.pad_to(line, column),
            };
            self.view_mut().edit_at(at, 0, &typed, None);
        }
        // The edits above are all below the cursor in the rope on their own
        // lines, but padding one of them can still have moved it.
        self.view_mut().sel = Selection::point(head.min(self.view().doc.len_chars()));
        self.end_undo_group();
        self.clamp_cursor();
    }

    /// `p` and `P` with a rectangle in the register: each line of it goes at
    /// the cursor's column on a line of the buffer, one after another, and a
    /// buffer that runs out of lines grows the ones it needs.
    pub fn put_block(&mut self, value: &RegisterValue, after: bool) {
        debug_assert_eq!(value.kind, Kind::Block);
        let view = self.view();
        let (line, char_column) = view.cursor_coords();
        let mut column = display_col(&view.doc.line_str(line), char_column);
        if after && view.doc.line_len_chars(line) > 0 {
            column += 1;
        }

        self.begin_undo_group();
        let rows: Vec<&str> = value.text.split('\n').collect();
        // Bottom up again, and the lines that have to be made come first:
        // appending to the buffer changes nothing above it.
        for (offset, row) in rows.iter().enumerate().rev() {
            let target = line + offset;
            if target > self.view().last_line() {
                let end = self.view().doc.len_chars();
                self.view_mut().edit_at(end, 0, "\n", None);
            }
            let at = self.pad_to(target, column);
            self.view_mut().edit_at(at, 0, row, None);
        }
        self.end_undo_group();
        let at = self.pad_to(line, column);
        self.view_mut().sel = Selection::point(at);
        self.clamp_cursor();
    }
}

/// A block `I`, `A` or `c` that is waiting for insert mode to end, so what
/// was typed on one line can be put on the others.
pub struct PendingBlock {
    /// Every line of the block, the one being typed on first.
    pub lines: Vec<usize>,
    /// The screen column the text goes at on each of them, or `usize::MAX`
    /// for the end of each line, which is where a `$` block appends.
    pub column: usize,
    /// Where typing started, so what was typed can be read back off.
    pub at: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_editor(text: &str) -> Editor {
        let mut editor = Editor::scratch();
        editor.view_mut().doc.text = ropey::Rope::from_str(text);
        editor
    }

    /// Put the cursor at (line, column) and the anchor at another corner.
    fn corners(editor: &mut Editor, from: (usize, usize), to: (usize, usize)) {
        // The mode first: entering visual collapses the selection onto the
        // cursor, which would throw the anchor away.
        editor.set_mode(Mode::VisualBlock);
        let view = editor.view_mut();
        let anchor = view.doc.line_to_char(from.0) + from.1;
        let head = view.doc.line_to_char(to.0) + to.1;
        view.sel = Selection { anchor, head };
    }

    /// Four lines, one of them too short to reach across the block.
    const RAGGED: &str = "abcdef\nghijkl\nmn\nopqrst\n";

    fn row_text(editor: &Editor, block: &Block) -> Vec<String> {
        block.rows.iter().map(|&(s, e)| editor.view().doc.slice_str(s, e)).collect()
    }

    #[test]
    fn a_block_is_the_same_columns_of_every_line_it_covers() {
        let mut editor = block_editor(RAGGED);
        corners(&mut editor, (0, 1), (3, 3));
        let block = editor.block().expect("a block");
        assert_eq!((block.first, block.left, block.right), (0, 1, 3));
        // The third line stops inside the block, so it gives what it has.
        assert_eq!(row_text(&editor, &block), ["bcd", "hij", "n", "pqr"]);
    }

    #[test]
    fn a_tab_is_as_wide_as_it_looks() {
        // A tab is one character and four columns, so a block reaching column
        // five is over the whole of it and the character after it.
        let mut editor = block_editor("\tx\nabcdefghij\n");
        corners(&mut editor, (0, 0), (1, 5));
        let block = editor.block().expect("a block");
        assert_eq!((block.left, block.right), (0, 5));
        assert_eq!(row_text(&editor, &block), ["\tx", "abcdef"]);
    }

    #[test]
    fn deleting_a_block_takes_a_column_out_of_every_line() {
        let mut editor = block_editor(RAGGED);
        corners(&mut editor, (0, 1), (3, 3));
        editor.delete_block(None);
        assert_eq!(editor.view().doc.text.to_string(), "aef\ngkl\nm\nost\n");
        assert_eq!(editor.mode, Mode::Normal);
        assert_eq!(editor.cursor_coords(), (0, 1), "the top left corner");
        assert_eq!(editor.registers.get(None).kind, Kind::Block);
        assert_eq!(editor.registers.get(None).text, "bcd\nhij\nn\npqr");
        // One undo takes the whole rectangle back.
        editor.undo();
        assert_eq!(editor.view().doc.text.to_string(), RAGGED);
    }

    #[test]
    fn a_block_goes_back_as_a_block() {
        let mut editor = block_editor("abcdef\nghijkl\nxy\n");
        corners(&mut editor, (0, 1), (1, 3));
        editor.yank_block(None);
        assert_eq!(editor.cursor_coords(), (0, 1), "the top left corner");
        assert_eq!(editor.view().doc.text.to_string(), "abcdef\nghijkl\nxy\n", "a yank changes nothing");

        // Put it after the first character of the short last line: the row
        // under it has no line yet, so one is made, and it is padded out to
        // reach the column.
        editor.goto_line(2);
        editor.put(None, 1, true);
        assert_eq!(editor.view().doc.text.to_string(), "abcdef\nghijkl\nxbcdy\n hij\n");
    }

    #[test]
    fn typing_on_one_line_of_a_block_types_on_all_of_them() {
        let mut editor = block_editor("one\ntwo\nthree\n");
        corners(&mut editor, (0, 0), (2, 0));
        editor.insert_block(false, false);
        assert_eq!(editor.mode, Mode::Insert);
        editor.insert("// ");
        editor.finish_block_insert();
        assert_eq!(editor.view().doc.text.to_string(), "// one\n// two\n// three\n");
        // And one undo takes the whole column back.
        editor.undo();
        assert_eq!(editor.view().doc.text.to_string(), "one\ntwo\nthree\n");
    }

    #[test]
    fn appending_to_a_ragged_block_pads_the_short_lines() {
        let mut editor = block_editor("long line\nx\nanother\n");
        corners(&mut editor, (0, 0), (2, 3));
        editor.insert_block(true, false);
        editor.insert("|");
        editor.finish_block_insert();
        assert_eq!(editor.view().doc.text.to_string(), "long| line\nx   |\nanot|her\n");
    }
}
