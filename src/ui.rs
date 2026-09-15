use unicode_width::UnicodeWidthChar;

use crate::editor::{Editor, char_width};
use crate::screen::{Style, Surface};
use crate::syntax::Highlights;

/// Draw a whole frame. Nothing here talks to the terminal; `Screen` works out
/// which of these cells actually need sending.
pub fn draw(editor: &Editor, surface: &mut Surface) {
    let total_lines = editor.doc.len_lines();

    // Highlight only the visible rows. Tree-sitter keeps the whole parse tree,
    // but running the query over the viewport is what keeps big files cheap.
    let first_byte = editor.doc.line_to_byte(editor.scroll_top);
    let last_row = editor.scroll_top + editor.height;
    let last_byte = if last_row >= total_lines {
        editor.doc.len_bytes()
    } else {
        editor.doc.line_to_byte(last_row)
    };
    let highlights = editor.highlights(first_byte..last_byte);

    let (sel_start, sel_end) = editor.sel.range();
    let styling = LineStyling {
        highlights: &highlights,
        selection: editor.theme.style("ui.selection"),
        scroll_left: editor.scroll_left,
    };

    for row in 0..editor.height {
        let line = editor.scroll_top + row;
        if line >= total_lines {
            surface.put(0, row, '~', 1, editor.theme.style("ui.eof"));
            continue;
        }

        let text = editor.doc.line_str(line);
        let line_start = editor.doc.line_to_char(line);
        let line_end = line_start + editor.doc.line_len_chars(line);

        // Selection clipped to this line, as char offsets within it.
        let sel = if !editor.sel.is_empty() && sel_end > line_start && sel_start <= line_end {
            Some((
                sel_start.saturating_sub(line_start),
                sel_end.saturating_sub(line_start),
            ))
        } else {
            None
        };

        draw_line(surface, row, &text, editor.doc.line_to_byte(line), sel, &styling);
    }

    draw_status(editor, surface);
}

/// The parts of drawing a line that are the same for every line in a frame.
struct LineStyling<'a> {
    highlights: &'a Highlights,
    selection: Style,
    scroll_left: usize,
}

fn draw_line(
    surface: &mut Surface,
    row: usize,
    text: &str,
    line_byte: usize,
    sel: Option<(usize, usize)>,
    styling: &LineStyling,
) {
    let (width, _) = surface.size();
    let scroll_left = styling.scroll_left;
    let right = scroll_left + width;
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
        if sel.is_some_and(|(s, e)| char_idx >= s && char_idx < e) {
            style = style.patch(styling.selection);
        }

        let visible_start = start.max(scroll_left);
        let visible_width = end.min(right) - visible_start;
        let x = visible_start - scroll_left;

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

fn draw_status(editor: &Editor, surface: &mut Surface) {
    let (width, height) = surface.size();
    let row = height - 1;

    let (line, col) = editor.cursor_coords();
    let left = if editor.message.is_empty() {
        format!(
            " {}{}",
            editor.doc.display_name(),
            if editor.is_modified() { " [+]" } else { "" }
        )
    } else {
        format!(" {}", editor.message)
    };
    let right = format!("{}:{}  ^S save  ^Z undo  ^Q quit ", line + 1, col + 1);

    let gap = width.saturating_sub(str_width(&left) + str_width(&right));
    let text = format!("{left}{}{right}", " ".repeat(gap));

    let style = editor.theme.style("ui.statusline");
    let mut x = 0;
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if x + w > width {
            break;
        }
        surface.put(x, row, ch, w, style);
        x += w;
    }
    // The bar runs the full width even when the text does not fill it.
    while x < width {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }
}

fn str_width(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}
