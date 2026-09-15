use unicode_width::UnicodeWidthChar;

use crate::editor::{Editor, Mode};
use crate::view::char_width;
use crate::keys::Keys;
use crate::picker::Picker;
use crate::screen::{Style, Surface};
use crate::stream::Sign;
use crate::syntax::Highlights;

/// Draw a whole frame. Nothing here talks to the terminal; `Screen` works out
/// which of these cells actually need sending.
pub fn draw(editor: &Editor, keys: &Keys, surface: &mut Surface) {
    let total_lines = editor.view().doc.len_lines();

    // Highlight only the visible rows. Tree-sitter keeps the whole parse tree,
    // but running the query over the viewport is what keeps big files cheap.
    let first_byte = editor.view().doc.line_to_byte(editor.view().scroll_top);
    let last_row = editor.view().scroll_top + editor.height;
    let last_byte = if last_row >= total_lines {
        editor.view().doc.len_bytes()
    } else {
        editor.view().doc.line_to_byte(last_row)
    };
    let highlights = editor.highlights(first_byte..last_byte);

    let gutter = editor.gutter_width();
    let signs = editor.sign_width();
    let (cursor_line, _) = editor.cursor_coords();
    let number_style = editor.theme.style("ui.linenr");
    let current_style = editor.theme.style("ui.linenr.selected");

    // Matches are painted only while a search is live, and only for the rows
    // on screen - the pattern is run over the viewport, not the buffer.
    let matches = match editor.search.highlight {
        true => editor.search.matches_in_lines(
            &editor.view().doc,
            editor.view().scroll_top,
            last_row,
        ),
        false => Vec::new(),
    };

    // The bracket under the cursor and its mate, so a pair can be seen at a
    // glance rather than counted.
    let brackets = editor.bracket_pair();

    let selection = editor.selection_range();
    let (sel_start, sel_end) = selection.unwrap_or((0, 0));
    let styling = LineStyling {
        highlights: &highlights,
        selection: editor.theme.style("ui.selection"),
        scroll_left: editor.view().scroll_left,
        left: gutter,
        matches: &matches,
        match_style: editor.theme.style("ui.search.match"),
        brackets,
        bracket_style: editor.theme.style("ui.bracket.match"),
    };

    for row in 0..editor.height {
        let line = editor.view().scroll_top + row;
        if line >= total_lines {
            // Past the end there is no line to number, and the `~` keeps the
            // far left column, as it does with no gutter at all.
            for x in 0..gutter {
                surface.put(x, row, ' ', 1, number_style);
            }
            surface.put(0, row, '~', 1, editor.theme.style("ui.eof"));
            continue;
        }

        if signs > 0 {
            let (ch, key) = match editor.view().signs.get(&line) {
                Some(Sign::Added) => ('+', "ui.gutter.added"),
                Some(Sign::Modified) => ('~', "ui.gutter.modified"),
                Some(Sign::Deleted) => ('_', "ui.gutter.deleted"),
                None => (' ', "ui.linenr"),
            };
            surface.put(0, row, ch, 1, editor.theme.style(key));
        }

        if let Some(number) = editor.numbers.label(line, cursor_line) {
            let style = match line == cursor_line {
                true => current_style,
                false => number_style,
            };
            // Right-aligned, with the space either side the width allows for.
            let text = format!("{number:>width$} ", width = gutter - signs - 1);
            put_str(surface, signs, row, &text, style, gutter);
        }

        let text = editor.view().doc.line_str(line);
        let line_start = editor.view().doc.line_to_char(line);
        let line_end = line_start + editor.view().doc.line_len_chars(line);

        // Selection clipped to this line, as char offsets within it.
        let sel = if selection.is_some() && sel_end > line_start && sel_start <= line_end {
            Some((
                sel_start.saturating_sub(line_start),
                sel_end.saturating_sub(line_start),
            ))
        } else {
            None
        };

        draw_line(
            surface,
            row,
            &text,
            editor.view().doc.line_to_byte(line),
            line_start,
            sel,
            &styling,
        );
    }

    if let Some(picker) = editor.picker.as_ref() {
        draw_picker(editor, picker, surface);
    }

    draw_status(editor, keys, surface);
}

/// The picker panel, drawn over the bottom rows of the text area. It is opaque:
/// every cell in the panel is written, so nothing of the text shows through.
fn draw_picker(editor: &Editor, picker: &Picker, surface: &mut Surface) {
    let (width, _) = surface.size();
    let rows = editor.height;
    let panel = Picker::panel_height(rows);
    let top = rows.saturating_sub(panel);

    let base = editor.theme.style("ui.picker");
    let selected = editor.theme.style("ui.picker.selected");
    let matched = editor.theme.style("ui.picker.match");
    let detail = editor.theme.style("ui.picker.detail");

    // Prompt row: the source's name, the query, and the match count.
    let prompt = format!(" {}> {}", picker.source.prompt(), picker.query);
    // A trailing `+` while a walk is still feeding the list, so a count that
    // is climbing does not look like the whole answer.
    let count = format!(
        "{}/{}{} ",
        picker.matches().len(),
        picker.item_count(),
        if picker.is_complete() { "" } else { "+" }
    );
    let style = editor.theme.style("ui.picker.prompt");
    let mut x = put_str(surface, 0, top, &prompt, style, width);
    let gap = width.saturating_sub(x + str_width(&count));
    x = put_str(surface, x, top, &" ".repeat(gap), style, width);
    x = put_str(surface, x, top, &count, style, width);
    while x < width {
        surface.put(x, top, ' ', 1, style);
        x += 1;
    }

    let list_rows = Picker::list_rows(rows);
    for row in 0..list_rows {
        let y = top + 1 + row;
        // On a very short terminal the panel would reach the status line.
        if y >= rows {
            break;
        }
        let index = picker.scroll() + row;
        let is_cursor = index == picker.cursor();
        let row_style = if is_cursor { selected } else { base };

        let mut x = put_str(surface, 0, y, if is_cursor { " > " } else { "   " }, row_style, width);
        if let Some(m) = picker.matches().get(index) {
            let item = picker.item(m);
            // The detail is right-aligned and the text truncated to fit before
            // it, so a long grep hit cannot push the file name it came from
            // off the row.
            let tail = match item.detail.is_empty() {
                true => 0,
                false => str_width(&item.detail) + 2,
            };
            let room = width.saturating_sub(tail);

            // Matched characters are tinted; the selected row keeps its own
            // background, so the tint patches over it rather than replacing it.
            for (i, ch) in item.text.chars().enumerate() {
                let w = char_width(ch, x).max(1);
                if x + w > room {
                    break;
                }
                let style = match m.positions.contains(&i) {
                    true => row_style.patch(matched),
                    false => row_style,
                };
                surface.put(x, y, ch, w, style);
                x += w;
            }
            if tail > 0 {
                while x < width.saturating_sub(tail - 1) {
                    surface.put(x, y, ' ', 1, row_style);
                    x += 1;
                }
                x = put_str(surface, x, y, &item.detail, row_style.patch(detail), width);
            }
        }
        while x < width {
            surface.put(x, y, ' ', 1, row_style);
            x += 1;
        }
    }
}

/// The parts of drawing a line that are the same for every line in a frame.
struct LineStyling<'a> {
    highlights: &'a Highlights,
    selection: Style,
    scroll_left: usize,
    /// First column the text may use: the gutter's width.
    left: usize,
    /// Search matches on screen, as absolute character ranges.
    matches: &'a [(usize, usize)],
    match_style: Style,
    brackets: Option<(usize, usize)>,
    bracket_style: Style,
}

fn draw_line(
    surface: &mut Surface,
    row: usize,
    text: &str,
    line_byte: usize,
    line_start: usize,
    sel: Option<(usize, usize)>,
    styling: &LineStyling,
) {
    let (width, _) = surface.size();
    let scroll_left = styling.scroll_left;
    // Columns here are the text's own, with the gutter added only when a cell
    // is actually written.
    let right = scroll_left + width.saturating_sub(styling.left);
    let mut col = 0usize;

    for (char_idx, (byte_in_line, ch)) in text.char_indices().enumerate() {
        let start = col;
        let end = start + char_width(ch, start);
        col = end;

        if start >= right {
            break;
        }
        if end <= scroll_left {
            continue;
        }

        // Syntax first, then the selection tints it rather than replacing it.
        let mut style = styling
            .highlights
            .style_at(line_byte + byte_in_line)
            .unwrap_or_default();
        let at = line_start + char_idx;
        if styling.matches.iter().any(|&(s, e)| at >= s && at < e) {
            style = style.patch(styling.match_style);
        }
        if styling.brackets.is_some_and(|(a, b)| at == a || at == b) {
            style = style.patch(styling.bracket_style);
        }
        if sel.is_some_and(|(s, e)| char_idx >= s && char_idx < e) {
            style = style.patch(styling.selection);
        }

        let visible_start = start.max(scroll_left);
        let visible_width = end.min(right) - visible_start;
        let x = visible_start - scroll_left + styling.left;

        if ch == '\t' || start < scroll_left || end > right {
            // Tabs, and wide characters straddling an edge, become blanks.
            for offset in 0..visible_width {
                surface.put(x + offset, row, ' ', 1, style);
            }
        } else {
            surface.put(x, row, ch, visible_width, style);
        }
    }
}

fn draw_status(editor: &Editor, keys: &Keys, surface: &mut Surface) {
    let (width, height) = surface.size();
    let row = height - 1;

    // A prompt takes the whole status line, the way a command line does.
    if let Some(prompt) = editor.prompt.as_ref() {
        let style = editor.theme.style("ui.statusline");
        let text = format!("{}{}", prompt.sigil(), prompt.input);
        let mut x = put_str(surface, 0, row, &text, style, width);
        while x < width {
            surface.put(x, row, ' ', 1, style);
            x += 1;
        }
        return;
    }

    let mode = format!(" {} ", editor.mode.name());
    let mode_style = editor.theme.style(match editor.mode {
        Mode::Normal => "ui.mode.normal",
        Mode::Insert => "ui.mode.insert",
        Mode::Visual | Mode::VisualLine => "ui.mode.visual",
    });

    let (line, col) = editor.cursor_coords();
    // The buffer position is only worth the columns when there is more than one.
    let position = match editor.views().len() {
        1 => String::new(),
        total => format!(" [{}/{}]", editor.current_index() + 1, total),
    };
    let left = if editor.message.is_empty() {
        format!(
            " {}{}{}",
            editor.view().doc.display_name(),
            if editor.is_modified() { " [+]" } else { "" },
            position
        )
    } else {
        format!(" {}", editor.message)
    };
    // A half-typed command shows on the right, as vim does.
    let right = format!("{}  {}:{} ", keys.pending_text(), line + 1, col + 1);

    let gap = width
        .saturating_sub(str_width(&mode) + str_width(&left) + str_width(&right));
    let text = format!("{left}{}{right}", " ".repeat(gap));

    let style = editor.theme.style("ui.statusline");
    let mut x = put_str(surface, 0, row, &mode, mode_style, width);
    x = put_str(surface, x, row, &text, style, width);
    // The bar runs the full width even when the text does not fill it.
    while x < width {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }
}

fn put_str(surface: &mut Surface, mut x: usize, row: usize, text: &str, style: Style, width: usize) -> usize {
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if x + w > width {
            break;
        }
        surface.put(x, row, ch, w, style);
        x += w;
    }
    x
}

fn str_width(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Numbers;
    use crate::screen::Screen;
    use crate::view::Move;

    /// Bytes the terminal is sent for one frame, which is what damage tracking
    /// exists to keep small.
    fn frame(editor: &Editor, screen: &mut Screen, keys: &Keys) -> usize {
        let (width, height) = (editor.width, editor.height + 1);
        draw(editor, keys, screen.begin(width, height));
        let mut out: Vec<u8> = Vec::new();
        screen.present(&mut out, editor.cursor_screen()).unwrap();
        out.len()
    }

    fn editor_with_lines(count: usize) -> Editor {
        let mut editor = Editor::scratch();
        let text: String = (1..=count).map(|n| format!("line {n} of text\n")).collect();
        editor.view_mut().doc.text = ropey::Rope::from_str(&text);
        editor.set_viewport(80, 40);
        editor
    }

    fn cost_of_a_cursor_move(numbers: Numbers) -> usize {
        let mut editor = editor_with_lines(500);
        editor.numbers = numbers;
        editor.signs_enabled = false;
        let keys = Keys::default();
        let mut screen = Screen::new();

        frame(&editor, &mut screen, &keys); // the full first paint
        editor.move_cursor(Move::Down, false);
        frame(&editor, &mut screen, &keys)
    }

    #[test]
    fn relative_numbering_costs_a_whole_gutter_on_every_cursor_move() {
        let off = cost_of_a_cursor_move(Numbers::Off);
        let absolute = cost_of_a_cursor_move(Numbers::Absolute);
        let hybrid = cost_of_a_cursor_move(Numbers::Hybrid);
        println!("one line down, 40 rows: off {off}b, absolute {absolute}b, hybrid {hybrid}b");

        // Absolute numbering does not repaint numbers that did not change; the
        // cursor's line changing style is the only difference from off.
        assert!(absolute < off * 3, "absolute {absolute} vs off {off}");
        // Hybrid repaints every number, which is the cost it is worth knowing.
        assert!(hybrid > absolute * 3, "hybrid {hybrid} vs absolute {absolute}");
    }

    #[test]
    fn the_gutter_is_sized_for_the_whole_buffer_not_the_screen() {
        let mut editor = editor_with_lines(1200);
        editor.numbers = Numbers::Absolute;
        editor.signs_enabled = false;
        // Four digits plus a space either side, whatever is on screen.
        assert_eq!(editor.gutter_width(), 6);
        assert_eq!(editor.text_width(), 74);

        editor.goto_line(1100);
        assert_eq!(editor.gutter_width(), 6);
    }

    #[test]
    fn turning_numbers_off_gives_the_columns_back() {
        let mut editor = editor_with_lines(10);
        editor.numbers = Numbers::Off;
        // The sign column is still there, and is one wide.
        assert_eq!(editor.gutter_width(), 1);
        assert_eq!(editor.cursor_screen(), (1, 0));

        editor.signs_enabled = false;
        assert_eq!(editor.gutter_width(), 0);
        assert_eq!(editor.text_width(), 80);
        assert_eq!(editor.cursor_screen(), (0, 0));
    }

    #[test]
    fn the_cursor_sits_past_the_gutter() {
        let mut editor = editor_with_lines(10);
        editor.numbers = Numbers::Absolute;
        editor.signs_enabled = false;
        // Two-digit buffer: a two-wide number with a space either side.
        assert_eq!(editor.gutter_width(), 4);
        assert_eq!(editor.cursor_screen(), (4, 0));
        editor.move_cursor(Move::Right, false);
        assert_eq!(editor.cursor_screen(), (5, 0));

        // With signs, everything shifts one further right.
        editor.signs_enabled = true;
        assert_eq!(editor.cursor_screen(), (6, 0));
    }
}
