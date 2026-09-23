//! The mouse, which only the window has.
//!
//! A terminal's mouse belongs to the terminal: dragging in one selects a
//! rectangle of the screen, line numbers and all, and jack never asks for the
//! events. A window has no such fallback, so the pointing is done here - and
//! done in the buffer rather than on the screen, which is why the gutter, the
//! status line and the preview pane cannot end up in what you selected. A
//! drag is visual mode with its head somewhere else; everything that already
//! works on a selection works on this one.

use crate::editor::{Editor, Mode};
use crate::view::{Selection, View, cluster_width, clusters};

/// Where a click landed: the window, the character under it, and whether it
/// was on the gutter rather than the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub window: usize,
    pub line: usize,
    pub column: usize,
    pub gutter: bool,
}

/// A drag in progress: the window it started in, and what kind of selection
/// it is making - a double click drags by character, a click on the gutter by
/// line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drag {
    window: usize,
    mode: Mode,
}


impl Editor {
    /// Where a screen cell is in the text: which window, and which character.
    /// The status lines, the buffer list, the prompt and the preview pane are
    /// not places a cursor can go, so a click on one is `None`.
    pub fn spot_at(&self, x: usize, y: usize) -> Option<Spot> {
        let (rects, _) = self.window_rects();
        let (id, rect) = rects.iter().copied().find(|(_, rect)| {
            (rect.x..rect.x + rect.width).contains(&x) && (rect.y..rect.y + rect.text_height()).contains(&y)
        })?;
        let view = &self.window_state(id).0;
        let gutter = self.gutter_width_for(view);
        let (_, _, top, left) = self.window_state(id);
        // The window's own width, not the focused window's: a click can be in
        // a window that is not the one the viewport numbers describe.
        let wrap = self.wrap.then(|| rect.width.saturating_sub(gutter).max(1));
        let within = x - rect.x;
        let (line, column) = position_at(view, y - rect.y, within.saturating_sub(gutter), wrap, top, left);
        Some(Spot { window: id, line, column, gutter: within < gutter })
    }

    /// The mouse button going down: put the cursor there, in that window.
    /// Twice is the word under it, three times the line - `viw` and `V`, only
    /// pointed at. A click in the gutter is the line it is beside.
    pub fn mouse_press(&mut self, x: usize, y: usize, clicks: u8) {
        // A picker or a prompt owns the keyboard while it is up, and the text
        // underneath is not what is being typed into.
        if self.picker.is_some() || self.prompt.is_some() {
            return;
        }
        let Some(spot) = self.spot_at(x, y) else {
            return;
        };
        self.focus_window(spot.window);
        let insert = self.mode == Mode::Insert;
        // A click is a new selection rather than the end of the old one, so
        // whatever was selected goes first.
        if self.mode.is_visual() {
            self.set_mode(Mode::Normal);
        }
        self.move_to(spot.line, spot.column);
        match (clicks, spot.gutter) {
            // The line, which is what a click on its number should mean.
            (1, true) | (3, _) => {
                self.set_mode(Mode::VisualLine);
                self.drag = Some(Drag { window: spot.window, mode: Mode::VisualLine });
            }
            (2, false) => {
                self.select_word();
                self.drag = Some(Drag { window: spot.window, mode: Mode::Visual });
            }
            _ => {
                // Clicking while typing moves the caret and leaves you typing.
                if !insert {
                    self.set_mode(Mode::Normal);
                }
                self.drag = Some(Drag { window: spot.window, mode: Mode::Visual });
            }
        }
        self.clamp_cursor();
    }

    /// The mouse moving with the button down: drag the far end of the
    /// selection to it, which is what visual mode already is. Dragging above
    /// or below the window scrolls it, so a selection can be longer than the
    /// screen.
    pub fn mouse_drag(&mut self, x: usize, y: usize) {
        let Some(drag) = self.drag else {
            return;
        };
        let Some((_, rect)) = self.window_rects().0.into_iter().find(|(id, _)| *id == drag.window) else {
            return;
        };
        // Outside the window, the drag is to the nearest cell inside it: a
        // selection follows the pointer even when the pointer has left.
        let bottom = rect.y + rect.text_height().saturating_sub(1);
        let away = match y {
            y if y < rect.y => Some(false),
            y if y > bottom => Some(true),
            _ => None,
        };
        let x = x.clamp(rect.x, rect.x + rect.width.saturating_sub(1));
        let y = y.clamp(rect.y, bottom);
        let Some(spot) = self.spot_at(x, y) else {
            return;
        };
        if spot.window != drag.window {
            return;
        }
        if self.mode != drag.mode {
            self.set_mode(drag.mode);
        }
        // Held past an edge, the selection grows a line at a time from where
        // it has got to, and the view follows the cursor as it always does.
        // Measuring from the edge instead would step by however much room the
        // scrolloff keeps below the cursor - a line's worth of holding would
        // select five.
        let line = match away {
            Some(down) => {
                let (line, _) = self.view().cursor_coords();
                match down {
                    true => (line + 1).min(self.view().doc.len_lines().saturating_sub(1)),
                    false => line.saturating_sub(1),
                }
            }
            None => spot.line,
        };
        self.extend_to(line, spot.column);
    }

    /// Whether the drag is pointing above or below its window, which is the
    /// kind that scrolls. A pointer held still sends nothing, so the window
    /// asks again on a timer for as long as this is true - otherwise a
    /// selection would only grow while the mouse jiggled.
    pub fn dragging_away(&self, y: usize) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };
        let Some((_, rect)) = self.window_rects().0.into_iter().find(|(id, _)| *id == drag.window) else {
            return false;
        };
        y < rect.y || y >= rect.y + rect.text_height()
    }

    /// The button coming up. A click that never moved selected nothing, and
    /// an empty selection is not worth being in visual mode for.
    pub fn mouse_release(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.mode == Mode::Visual && self.mode == Mode::Visual && self.view().sel.is_empty() {
            self.set_mode(Mode::Normal);
        }
    }

    /// Put the cursor on a line and column of the focused view, dropping any
    /// selection: where a click landed.
    fn move_to(&mut self, line: usize, column: usize) {
        let at = self.view().doc.line_to_char(line) + column;
        let at = at.min(self.view().doc.len_chars());
        self.view_mut().sel = Selection::point(at);
    }

    /// Drag the head of the selection to a line and column, leaving the
    /// anchor where the drag started.
    fn extend_to(&mut self, line: usize, column: usize) {
        let at = self.view().doc.line_to_char(line) + column;
        let at = at.min(self.view().doc.len_chars());
        self.view_mut().sel.head = at;
        self.clamp_cursor();
    }

    /// The word under the cursor, selected: a double click, and what `viw`
    /// does. On whitespace or an empty line there is no word, and the cursor
    /// simply stays put.
    fn select_word(&mut self) {
        let at = self.view().sel.head;
        let Some((start, end)) = crate::object::resolve(&self.view().doc, at, crate::object::Object::Word, false) else {
            return;
        };
        self.set_mode(Mode::Visual);
        let sel = &mut self.view_mut().sel;
        sel.anchor = start;
        sel.head = end.saturating_sub(1).max(start);
        self.clamp_cursor();
    }
}

/// The buffer position drawn at screen `row` and `col` of a window
/// showing `view` from `top` and `left`, which is what a click is:
/// the inverse of `cursor_screen`. Row and column are the text's own,
/// with the gutter already taken off. Past the last line is the last
/// line, and past the end of a line is its end, so a click below or
/// beyond the text lands somewhere real rather than nowhere.
fn position_at(view: &View, row: usize, col: usize, wrap: Option<usize>, top: usize, left: usize) -> (usize, usize) {
    let last = view.doc.len_lines().saturating_sub(1);
    let Some(width) = wrap else {
        let line = (top + row).min(last);
        let text = view.doc.line_str(line);
        return (line, char_col_at(&text, col + left, &view.hints_on(line)));
    };
    // Wrapped, a screen row is a row of some line rather than a line, so
    // the rows are counted off line by line from the top of the window.
    let mut left_to_go = row;
    for line in top..=last {
        let starts = view.line_rows(line, width);
        if left_to_go < starts.len() {
            let start = starts[left_to_go];
            let text = view.doc.line_str(line);
            let hints: Vec<(usize, &str)> = view
                .hints_on(line)
                .into_iter()
                .filter(|(at, _)| *at >= start)
                .map(|(at, label)| (at - start, label))
                .collect();
            let segment: String = text.chars().skip(start).collect();
            return (line, start + char_col_at(&segment, col, &hints));
        }
        left_to_go -= starts.len();
    }
    (last, view.doc.line_str(last).chars().count())
}

/// The char column in `line` that the screen column `want` is drawn at: the
/// inverse of `hinted_col`, and what the mouse is pointing at. A column
/// inside a tab, a wide glyph or an inlay hint is the column that thing
/// starts at, and a column past the end of the line is the end of the line.
pub fn char_col_at(line: &str, want: usize, hints: &[(usize, &str)]) -> usize {
    let mut w = 0;
    let mut end = 0;
    for (at, cluster) in clusters(line) {
        for (_, label) in hints.iter().filter(|(column, _)| *column == at) {
            w += crate::ui::str_width(label);
            // A hint is not text: pointing at one points at the character it
            // was written in front of.
            if w > want {
                return at;
            }
        }
        w += cluster_width(cluster, w);
        if w > want {
            return at;
        }
        end = at + cluster.chars().count();
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Mode;

    /// An editor with a screen to click on: eighty columns and twelve rows of
    /// text, which is what `set_viewport` means by twelve.
    fn editor(text: &str) -> Editor {
        let mut editor = Editor::scratch();
        editor.view_mut().doc.text = ropey::Rope::from_str(text);
        editor.set_viewport(80, 12);
        editor
    }

    /// The text starts after the line numbers and the sign column, and where
    /// that is is the editor's business rather than a number to guess at.
    fn gutter(editor: &Editor) -> usize {
        editor.gutter_width()
    }

    #[test]
    fn a_click_is_the_character_under_it() {
        let mut editor = editor("hello world\nsecond line\nthird\n");
        // Row 1, five characters into the text: the `d` of `second`.
        editor.mouse_press(gutter(&editor) + 5, 1, 1);
        assert_eq!(editor.cursor_coords(), (1, 5));
        assert_eq!(editor.mode, Mode::Normal, "a click is not a selection");

        // Past the end of a line is the end of the line, not the line below.
        editor.mouse_press(gutter(&editor) + 40, 2, 1);
        assert_eq!(editor.cursor_coords(), (2, 4));

        // Below the text is the last line - the last one a cursor can go on,
        // which is not the empty one a trailing newline leaves.
        editor.mouse_press(gutter(&editor) + 1, 9, 1);
        assert_eq!(editor.cursor_coords(), (2, 0));
    }

    #[test]
    fn a_tab_is_one_place_however_wide_it_is_drawn() {
        let mut editor = editor("\tindented\n");
        // Anywhere in the tab is the tab: there is nowhere between its
        // columns for a cursor to be.
        for column in 0..4 {
            editor.mouse_press(gutter(&editor) + column, 0, 1);
            assert_eq!(editor.cursor_coords(), (0, 0), "column {column}");
        }
        editor.mouse_press(gutter(&editor) + 4, 0, 1);
        assert_eq!(editor.cursor_coords(), (0, 1), "the `i` after it");
    }

    #[test]
    fn dragging_selects_what_it_was_dragged_over() {
        let mut editor = editor("hello world\nsecond line\n");
        editor.mouse_press(gutter(&editor), 0, 1);
        editor.mouse_drag(gutter(&editor) + 4, 1);
        assert_eq!(editor.mode, Mode::Visual);
        let sel = editor.view().sel;
        assert_eq!(editor.view().doc.coords(sel.anchor), (0, 0));
        assert_eq!(editor.view().doc.coords(sel.head), (1, 4));

        // Dragging back the other way is the same selection the other way
        // round, not an empty one.
        editor.mouse_drag(gutter(&editor) + 2, 0);
        assert_eq!(editor.view().doc.coords(editor.view().sel.head), (0, 2));

        // The button coming up leaves it selected, to yank or operate on.
        editor.mouse_release();
        assert_eq!(editor.mode, Mode::Visual);
    }

    #[test]
    fn a_click_that_does_not_move_selects_nothing() {
        let mut editor = editor("hello world\n");
        editor.mouse_press(gutter(&editor) + 3, 0, 1);
        editor.mouse_release();
        assert_eq!(editor.mode, Mode::Normal, "a click is a cursor, not a selection");
        assert!(editor.view().sel.is_empty());
    }

    #[test]
    fn twice_is_the_word_and_three_times_is_the_line() {
        let mut editor = editor("hello world\nsecond line\n");
        editor.mouse_press(gutter(&editor) + 7, 0, 2);
        assert_eq!(editor.mode, Mode::Visual);
        let (start, end) = editor.view().sel.range();
        assert_eq!(editor.view().doc.slice_str(start, end + 1), "world");

        editor.mouse_press(gutter(&editor) + 3, 1, 3);
        assert_eq!(editor.mode, Mode::VisualLine);
        assert_eq!(editor.cursor_coords().0, 1);
    }

    #[test]
    fn the_gutter_is_the_line_it_is_beside() {
        let mut editor = editor("hello world\nsecond line\nthird\n");
        // A click on a line number selects that line, which is what a line
        // number is for.
        editor.mouse_press(1, 1, 1);
        assert_eq!(editor.mode, Mode::VisualLine);
        assert_eq!(editor.cursor_coords().0, 1);

        // And dragging down the gutter takes the lines with it.
        editor.mouse_drag(1, 2);
        let (start, end) = editor.view().sel.range();
        assert_eq!(editor.view().doc.coords(start).0, 1);
        assert_eq!(editor.view().doc.coords(end).0, 2);
    }

    #[test]
    fn the_status_line_is_not_a_place_to_put_a_cursor() {
        let editor = editor("hello world\n");
        // Twelve rows of text, so row twelve is the status line and row
        // thirteen is off the bottom of the screen.
        assert!(editor.spot_at(gutter(&editor) + 1, 11).is_some(), "the last text row");
        assert!(editor.spot_at(gutter(&editor) + 1, 12).is_none(), "the status line");
        assert!(editor.spot_at(gutter(&editor) + 1, 20).is_none(), "off the screen");
    }

    #[test]
    fn a_column_is_where_the_glyph_is_drawn_not_how_many_chars_are_before_it() {
        // The inverse of `hinted_col`, which is what a click has to undo.
        assert_eq!(char_col_at("abc", 1, &[]), 1);
        assert_eq!(char_col_at("abc", 9, &[]), 3, "past the end is the end");
        // A tab is one character drawn a tab stop wide.
        assert_eq!(char_col_at("\tx", 3, &[]), 0);
        assert_eq!(char_col_at("\tx", 4, &[]), 1);
        // A wide glyph is one character drawn in two columns.
        assert_eq!(char_col_at("\u{6f22}b", 1, &[]), 0);
        assert_eq!(char_col_at("\u{6f22}b", 2, &[]), 1);
        // A hint is not text: pointing at one points at the character it was
        // written in front of.
        assert_eq!(char_col_at("ab", 2, &[(1, ": usize")]), 1);
    }
}
