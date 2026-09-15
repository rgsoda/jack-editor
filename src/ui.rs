use std::ops::Range;

use unicode_width::UnicodeWidthChar;

use crate::editor::{Completing, Editor};
use crate::view::char_width;
use crate::keys::Keys;
use crate::picker::Picker;
use crate::complete::{Candidate, Completion};
use crate::screen::{Style, Surface};
use crate::status::{self, Glyphs, Segment};
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
    // The row the cursor is on, tinted the whole width of the screen. Drawn
    // under everything else: the syntax keeps its colours, and a selection or
    // a search match still wins on the cells it covers.
    let cursorline = editor.cursorline.then(|| editor.theme.style("ui.cursorline"));
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

    let top = editor.top();
    for row in top..top + editor.height {
        let line = editor.view().scroll_top + row - top;
        if line >= total_lines {
            // Past the end there is no line to number, and the `~` keeps the
            // far left column, as it does with no gutter at all.
            for x in 0..gutter {
                surface.put(x, row, ' ', 1, number_style);
            }
            surface.put(0, row, '~', 1, editor.theme.style("ui.eof"));
            continue;
        }

        let here = cursorline.filter(|_| line == cursor_line);
        if let Some(tint) = here {
            // The whole row first, so the tint reaches past the end of the
            // text and behind the gutter; everything below draws over it.
            let (width, _) = surface.size();
            for x in 0..width {
                surface.put(x, row, ' ', 1, tint);
            }
        }

        if signs > 0 {
            let (ch, key) = match editor.view().signs.get(&line) {
                Some(Sign::Added) => ('+', "ui.gutter.added"),
                Some(Sign::Modified) => ('~', "ui.gutter.modified"),
                Some(Sign::Deleted) => ('_', "ui.gutter.deleted"),
                None => (' ', "ui.linenr"),
            };
            let style = editor.theme.style(key);
            surface.put(0, row, ch, 1, under(here, style));
        }

        if let Some(number) = editor.numbers.label(line, cursor_line) {
            let style = match line == cursor_line {
                true => under(here, current_style),
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
            here,
            &styling,
        );
    }

    if editor.show_tabline() {
        draw_tabline(editor, surface);
    }

    if let Some(completion) = editor.completion.as_ref() {
        draw_completion(editor, completion, surface);
    }

    if let Some(picker) = editor.picker.as_ref() {
        draw_picker(editor, picker, surface);
    }

    draw_status(editor, keys, surface);
}

/// The picker panel, drawn over the bottom rows of the text area. It is opaque:
/// every cell in the panel is written, so nothing of the text shows through.
/// The open buffers along the top. Scrolled from the left when there are more
/// than fit, so the one you are in is always on it.
fn draw_tabline(editor: &Editor, surface: &mut Surface) {
    let (width, _) = surface.size();
    let bar = editor.theme.style("ui.tabline");
    let glyphs = status::glyphs(editor);
    let tabs = status::tabs(editor);

    let mut first = 0;
    let mut pieces = run(&tabs[first..], bar, glyphs, true);
    while first < editor.current_index() && run_width(&pieces) > width {
        first += 1;
        pieces = run(&tabs[first..], bar, glyphs, true);
    }

    let mut x = 0;
    // A mark where the ones that did not fit went.
    if first > 0 {
        x = put_str(surface, x, 0, glyphs.truncated, bar, width);
    }
    for (text, style) in &pieces {
        x = put_str(surface, x, 0, text, *style, width);
    }
    while x < width {
        surface.put(x, 0, ' ', 1, bar);
        x += 1;
    }
}

/// The completion popup: a small box anchored under the word being completed,
/// or over it when there is no room below.
fn draw_completion(editor: &Editor, completion: &Completion, surface: &mut Surface) {
    const MAX_ROWS: usize = 8;
    const MAX_WIDTH: usize = 40;

    let (screen_width, _) = surface.size();
    let rows = completion.len().min(MAX_ROWS);
    if rows == 0 || editor.height == 0 {
        return;
    }

    let base = editor.theme.style("ui.completion");
    let selected = editor.theme.style("ui.completion.selected");
    let kind_style = editor.theme.style("ui.completion.kind");

    // Scrolled so the selection is always one of the rows drawn, and wide
    // enough for the rows that are. An untouched popup shows the top of the
    // list, because there is no selection to keep in view.
    let first = completion.selected().unwrap_or(0).saturating_sub(rows - 1);
    let shown: Vec<&Candidate> = completion.items().skip(first).take(rows).collect();
    let width = shown
        .iter()
        .map(|c| str_width(&c.text) + c.kind.map_or(0, |k| str_width(k) + 1) + 2)
        .max()
        .unwrap_or(0)
        .clamp(1, MAX_WIDTH.min(screen_width));

    // Under the first character of the word, so the list lines up with what it
    // would replace, and pushed left off the right edge if it has to be.
    let (cursor_x, cursor_y) = editor.cursor_screen();
    let anchor = (cursor_x as usize).saturating_sub(completion.prefix().chars().count());
    let left = anchor.min(screen_width.saturating_sub(width));

    // Below the cursor unless the box would not fit, in which case above it.
    let bottom = editor.top() + editor.height;
    let below = cursor_y as usize + 1;
    let top = match below + rows <= bottom {
        true => below,
        false => (cursor_y as usize).saturating_sub(rows),
    };

    for (row, candidate) in shown.into_iter().enumerate() {
        let y = top + row;
        if y >= bottom {
            break;
        }
        let style = match Some(first + row) == completion.selected() {
            true => selected,
            false => base,
        };
        let right = left + width;
        let mut x = put_str(surface, left, y, " ", style, right);
        x = put_str(surface, x, y, &candidate.text, style, right);
        // The kind is right-aligned, and the name gives way to it rather than
        // the other way round.
        if let Some(kind) = candidate.kind {
            let kind_at = right.saturating_sub(str_width(kind) + 1);
            while x < kind_at {
                surface.put(x, y, ' ', 1, style);
                x += 1;
            }
            x = put_str(surface, x.max(kind_at), y, kind, style.patch(kind_style), right);
        }
        while x < right {
            surface.put(x, y, ' ', 1, style);
            x += 1;
        }
    }
}

fn draw_picker(editor: &Editor, picker: &Picker, surface: &mut Surface) {
    let (width, _) = surface.size();
    let rows = editor.height;
    let panel = Picker::panel_height(rows);
    // Relative to the text area, which may start a row down.
    let top = editor.top() + rows.saturating_sub(panel);
    let bottom = editor.top() + rows;

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
        if y >= bottom {
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

/// `style` over the cursor line's tint, when this row has one. The tint is a
/// background and what goes on it keeps its own colours, so it has to go
/// underneath rather than on top.
fn under(tint: Option<Style>, style: Style) -> Style {
    match tint {
        Some(tint) => tint.patch(style),
        None => style,
    }
}

fn draw_line(
    surface: &mut Surface,
    row: usize,
    text: &str,
    line_byte: usize,
    line_start: usize,
    sel: Option<(usize, usize)>,
    cursorline: Option<Style>,
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
        let mut style = under(
            cursorline,
            styling
                .highlights
                .style_at(line_byte + byte_in_line)
                .unwrap_or_default(),
        );
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

/// The dog, in the gap the status line leaves in the middle: running while you
/// type and sitting when you stop.
///
/// The lane is the whole gap - everything between what the left side has
/// written and where the right side starts - so the dog has the run of the
/// line rather than a few cells of it. A long file name or a message shortens
/// the lane from the left rather than being drawn over, and a gap too small to
/// run in gets no dog at all.
fn draw_dog(editor: &Editor, surface: &mut Surface, row: usize, gap: Range<usize>, bar: Style) {
    let (width, _) = surface.size();
    let lane = gap.end.saturating_sub(gap.start);
    if lane < status::DOG_ROOM {
        return;
    }

    let (dog, x) = match editor.dog.running {
        true => (status::DOG_RUNNING, gap.start + editor.dog.steps % lane),
        // Sitting, it wants the middle of the screen rather than the middle of
        // the gap - that is where it was asked to sit, and where the eye goes
        // looking for it. The gap's own middle when the line is too lopsided
        // for the screen's to be in it.
        false => match gap.contains(&(width / 2)) {
            true => (status::DOG_SITTING, width / 2),
            false => (status::DOG_SITTING, gap.start + lane / 2),
        },
    };
    surface.put(x, row, dog, 1, bar);
}

/// What `tab` is offering on the `:` line, in the row above it, with the one
/// it has put there highlighted. Vim calls this the wildmenu.
fn draw_wildmenu(editor: &Editor, completing: &Completing, surface: &mut Surface, row: usize) {
    let (width, _) = surface.size();
    let base = editor.theme.style("ui.completion");
    let selected = editor.theme.style("ui.completion.selected");
    let glyphs = status::glyphs(editor);

    // Scrolled from the left until the one showing fits on the row.
    let item = |text: &String| str_width(text) + 2;
    let mut first = 0;
    while first < completing.selected
        && completing.matches[first..=completing.selected].iter().map(item).sum::<usize>() > width
    {
        first += 1;
    }

    let mut x = 0;
    if first > 0 {
        x = put_str(surface, x, row, glyphs.truncated, base, width);
    }
    for (i, text) in completing.matches.iter().enumerate().skip(first) {
        let style = match i == completing.selected {
            true => selected,
            false => base,
        };
        if x + item(text) > width {
            break;
        }
        x = put_str(surface, x, row, &format!(" {text} "), style, width);
    }
    while x < width {
        surface.put(x, row, ' ', 1, base);
        x += 1;
    }
}

fn draw_status(editor: &Editor, keys: &Keys, surface: &mut Surface) {
    let (width, height) = surface.size();
    let row = height - 1;

    // A prompt takes the whole status line, the way a command line does.
    if let Some(prompt) = editor.prompt.as_ref() {
        if let Some(completing) = prompt.completion.as_ref()
            && row > 0
        {
            draw_wildmenu(editor, completing, surface, row - 1);
        }
        let style = editor.theme.style("ui.statusline");
        let text = format!("{}{}", prompt.sigil(), prompt.input);
        let mut x = put_str(surface, 0, row, &text, style, width);
        while x < width {
            surface.put(x, row, ' ', 1, style);
            x += 1;
        }
        return;
    }

    let bar = editor.theme.style("ui.statusline");
    let glyphs = status::glyphs(editor);
    let mut status = status::build(editor, keys);

    // The two sides are laid out as runs of (text, style): each segment padded,
    // and between two of them a separator drawn in the left one's background
    // over the right one's. A segment whose colours the theme leaves to the
    // terminal cannot be blended into its neighbour, so those get the hairline
    // separator instead.
    let left = run(&status.left, bar, glyphs, true);
    let left_width = run_width(&left);

    // In a narrow terminal the right side gives up its segments from the left,
    // because the cursor position is the one worth keeping, and the file name
    // is then clipped rather than pushing the position off the end.
    while status.right.len() > 1 {
        let right = run(&status.right, bar, glyphs, false);
        if left_width + run_width(&right) <= width {
            break;
        }
        status.right.remove(0);
    }
    let right = run(&status.right, bar, glyphs, false);
    let start = width.saturating_sub(run_width(&right));

    let mut x = 0;
    for (text, style) in &left {
        x = put_str(surface, x, row, text, *style, start);
    }
    // A message sits in the gap between the two sides, clipped where the right
    // side begins rather than pushing it along.
    if !status.message.is_empty() {
        let message = format!(" {}", status.message);
        x = put_str(surface, x, row, &message, bar, start);
    }
    let text_end = x;
    while x < start {
        surface.put(x, row, ' ', 1, bar);
        x += 1;
    }

    if editor.show_dog && glyphs.nerd {
        draw_dog(editor, surface, row, text_end..start, bar);
    }
    for (text, style) in &right {
        x = put_str(surface, x, row, text, *style, width);
    }
    while x < width {
        surface.put(x, row, ' ', 1, bar);
        x += 1;
    }
}

/// Expand segments into the pieces to paint, separators included. `forward` is
/// the left side, which points its wedges the way the text reads.
fn run(
    segments: &[Segment],
    bar: Style,
    glyphs: &Glyphs,
    forward: bool,
) -> Vec<(String, Style)> {
    let mut pieces: Vec<(String, Style)> = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        // Outside the bar at the far end, the neighbour is the next segment.
        let outer = match forward {
            true => segments.get(i + 1).map_or(bar, |s| s.style),
            false => segments.get(i.wrapping_sub(1)).map_or(bar, |s| s.style),
        };
        let separator = separator(segment.style, outer, glyphs, forward);
        if !forward && let Some(separator) = separator.clone() {
            pieces.push(separator);
        }
        pieces.push((format!(" {} ", segment.text), segment.style));
        if forward && let Some(separator) = separator {
            pieces.push(separator);
        }
    }
    pieces
}

/// The wedge between a segment and what follows it, or `None` when the two
/// share a background and there is nothing to divide.
fn separator(
    inner: Style,
    outer: Style,
    glyphs: &Glyphs,
    forward: bool,
) -> Option<(String, Style)> {
    let wedge = match forward {
        true => glyphs.section_right,
        false => glyphs.section_left,
    };
    // Nothing to blend - the two share a background, or the glyph set has no
    // wedge to draw - so a hairline in the segment's own colours divides them.
    if inner.bg == outer.bg || wedge.is_empty() {
        let thin = match forward {
            true => glyphs.thin_right,
            false => glyphs.thin_left,
        };
        return match thin.is_empty() {
            true => None,
            false => Some((thin.to_string(), inner)),
        };
    }
    // The wedge is the inner colour, drawn on the outer one - which is what
    // makes one block look like it flows into the next.
    let style = Style { fg: inner.bg, bg: outer.bg, ..Style::default() };
    Some((wedge.to_string(), style))
}

fn run_width(run: &[(String, Style)]) -> usize {
    run.iter().map(|(text, _)| str_width(text)).sum()
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

        // `off` is not free: the status line repaints its position segment on
        // every move whatever the numbering is, so what each mode actually
        // costs is the difference from it.
        let (absolute, hybrid) = (absolute - off, hybrid - off);
        // Absolute numbering does not repaint numbers that did not change; the
        // cursor's line changing style is the only difference from off.
        assert!(absolute < off, "absolute {absolute} vs off {off}");
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

    /// One row of a drawn frame as text, for asserting on what it says.
    fn row_text(editor: &Editor, keys: &Keys, y: usize) -> String {
        let mut screen = Screen::new();
        let (width, height) = (editor.width, editor.top() + editor.height + 1);
        let surface = screen.begin(width, height);
        draw(editor, keys, surface);
        (0..width).map(|x| surface.get(x, y).ch).collect()
    }

    /// The background of every cell on a row, so a tint can be told from what
    /// is drawn on it.
    fn row_backgrounds(editor: &Editor, keys: &Keys, y: usize) -> Vec<Option<crossterm::style::Color>> {
        let mut screen = Screen::new();
        let (width, height) = (editor.width, editor.top() + editor.height + 1);
        let surface = screen.begin(width, height);
        draw(editor, keys, surface);
        (0..width).map(|x| surface.get(x, y).style.bg).collect()
    }

    #[test]
    fn the_dog_sits_in_the_middle_and_runs_when_you_type() {
        let mut editor = editor_with_lines(10);
        let keys = Keys::default();
        let row = editor.top() + editor.height;
        let middle = editor.width / 2;

        let sitting: Vec<char> = status_row(&editor, &keys).chars().collect();
        assert_eq!(sitting[middle], status::DOG_SITTING);

        // A key, and it is off - a different dog, and no longer in the middle.
        editor.dog_runs();
        let running: Vec<char> = status_row(&editor, &keys).chars().collect();
        assert_eq!(running[middle], ' ');
        let at = running.iter().position(|c| *c == status::DOG_RUNNING);
        assert!(at.is_some(), "the dog is somewhere on the line");

        // It keeps going while the keys keep coming, and comes back around
        // rather than running off the end of its lane. The lane is the whole
        // gap the status line leaves, so a lap reaches both sides of it.
        let mut seen = vec![at.unwrap()];
        for _ in 0..80 {
            editor.dog_runs();
            let row: Vec<char> = status_row(&editor, &keys).chars().collect();
            seen.push(row.iter().position(|c| *c == status::DOG_RUNNING).unwrap());
        }
        let (first, last) = (*seen.iter().min().unwrap(), *seen.iter().max().unwrap());
        assert!(first < middle && last > middle, "the whole gap: {first}..{last}");
        // And the lap is the gap itself: every cell of it, and no cell twice
        // in a row.
        let lap = last - first + 1;
        assert_eq!(seen.len().min(lap * 2), lap * 2, "at least two laps");
        assert!(seen.windows(2).all(|w| w[0] != w[1]));

        // And it sits back down where it started.
        editor.dog_rests();
        let resting: Vec<char> = status_row(&editor, &keys).chars().collect();
        assert_eq!(resting[middle], status::DOG_SITTING);
    }

    #[test]
    fn a_message_in_the_middle_keeps_the_dog_out_of_it() {
        let mut editor = editor_with_lines(10);
        let keys = Keys::default();
        editor.message = "x".repeat(editor.width);

        let row = status_row(&editor, &keys);
        assert!(!row.contains(status::DOG_SITTING), "the message has the room");
    }

    #[test]
    fn no_dog_without_a_patched_font_or_with_the_option_off() {
        let mut editor = editor_with_lines(10);
        let keys = Keys::default();
        editor.show_dog = false;
        assert!(!status_row(&editor, &keys).contains(status::DOG_SITTING));

        editor.show_dog = true;
        editor.glyphs = false;
        assert!(!status_row(&editor, &keys).contains(status::DOG_SITTING));
    }

    #[test]
    fn the_cursor_line_is_tinted_the_whole_width() {
        let mut editor = editor_with_lines(10);
        editor.goto_line(3);
        let keys = Keys::default();
        let tint = editor.theme.style("ui.cursorline").bg;
        assert!(tint.is_some(), "the theme has a cursorline colour");

        let cursor_row = row_backgrounds(&editor, &keys, 3);
        // Every cell, gutter and past the end of the text alike - a tint that
        // stops at the last character is a smear, not a line.
        assert!(cursor_row.iter().all(|bg| *bg == tint), "{cursor_row:?}");

        // And no other row has it.
        assert!(row_backgrounds(&editor, &keys, 2).iter().all(|bg| *bg != tint));
    }

    #[test]
    fn the_tint_does_not_take_over_a_selection() {
        let mut editor = editor_with_lines(10);
        editor.goto_line(3);
        editor.set_mode(crate::editor::Mode::Visual);
        editor.move_cursor(Move::Right, true);
        let keys = Keys::default();
        let tint = editor.theme.style("ui.cursorline").bg;
        let selection = editor.theme.style("ui.selection");

        let row = row_backgrounds(&editor, &keys, 3);
        let text = editor.gutter_width();
        // The selection covers the first two characters, and keeps its own
        // look there; the rest of the row is still tinted.
        match selection.bg {
            Some(_) => assert_ne!(row[text], tint),
            // The default theme reverses video rather than setting a colour,
            // in which case what matters is that the tint is still under it.
            None => assert_eq!(row[text], tint),
        }
        assert_eq!(row[text + 8], tint);
    }

    #[test]
    fn turning_the_cursor_line_off_costs_nothing_to_draw() {
        let cost = |on: bool| {
            let mut editor = editor_with_lines(500);
            editor.cursorline = on;
            editor.signs_enabled = false;
            let keys = Keys::default();
            let mut screen = Screen::new();
            frame(&editor, &mut screen, &keys);
            editor.move_cursor(Move::Down, false);
            frame(&editor, &mut screen, &keys)
        };
        let (off, on) = (cost(false), cost(true));
        println!("one line down, 40 rows: cursorline off {off}b, on {on}b");
        // Two rows repaint rather than none, which is the whole cost of it.
        assert!(on > off, "on {on} vs off {off}");
        assert!(on < off + 2000, "a cursor move costs {} bytes more", on - off);
    }

    fn status_row(editor: &Editor, keys: &Keys) -> String {
        row_text(editor, keys, editor.top() + editor.height)
    }

    #[test]
    fn the_status_line_shows_the_mode_the_file_and_the_position() {
        let mut editor = editor_with_lines(10);
        editor.set_viewport(80, 20);
        let row = status_row(&editor, &Keys::default());
        assert!(row.contains("ABNORMAL"), "{row}");
        assert!(row.contains("[scratch]"), "{row}");
        assert!(row.trim_end().ends_with('1'), "{row}");
    }

    #[test]
    fn a_message_survives_the_gap_it_is_drawn_into() {
        // The gap between the two sides is filled with spaces after the message
        // is drawn, which once erased it.
        let mut editor = editor_with_lines(10);
        editor.set_viewport(80, 20);
        editor.message = "wrote demo.rs".into();
        let row = status_row(&editor, &Keys::default());
        assert!(row.contains("wrote demo.rs"), "{row}");
    }

    #[test]
    fn a_narrow_terminal_keeps_the_cursor_position() {
        let mut editor = editor_with_lines(10);
        editor.set_viewport(24, 20);
        let row = status_row(&editor, &Keys::default());
        // The file name is what gives way, not the position.
        assert!(row.trim_end().ends_with('1'), "{row}");
        assert!(!row.contains("[scratch]"), "{row}");
    }

    #[test]
    fn plain_glyphs_keep_the_status_line_ascii() {
        let mut editor = editor_with_lines(10);
        editor.set_viewport(80, 20);
        editor.glyphs = false;
        let row = status_row(&editor, &Keys::default());
        assert!(row.is_ascii(), "{row}");
    }


    #[test]
    fn the_buffer_list_appears_when_there_is_more_than_one() {
        let mut editor = editor_with_lines(10);
        editor.set_viewport(80, 20);
        // One buffer: the row would be a waste, and the text starts at the top.
        assert!(!editor.show_tabline());
        assert_eq!(editor.top(), 0);
        assert_eq!(editor.cursor_screen().1, 0);

        editor.open_file("demo.rs").unwrap();
        assert!(editor.show_tabline());
        let tabs = row_text(&editor, &Keys::default(), 0);
        assert!(tabs.contains("[scratch]"), "{tabs}");
        assert!(tabs.contains("demo.rs"), "{tabs}");
        // And the text has moved down to make room.
        assert_eq!(editor.top(), 1);
        assert_eq!(editor.cursor_screen().1, 1);
    }

    #[test]
    fn the_buffer_list_can_be_asked_for_or_turned_off() {
        let mut editor = editor_with_lines(10);
        editor.set_viewport(80, 20);
        editor.run_command("set tabline");
        assert!(editor.show_tabline(), "one buffer, but asked for");

        editor.open_file("demo.rs").unwrap();
        editor.run_command("set notabline");
        assert!(!editor.show_tabline(), "two buffers, but turned off");
        let row = row_text(&editor, &Keys::default(), 0);
        assert!(!row.contains("demo.rs"), "{row}");
    }

    #[test]
    fn the_buffer_you_are_in_is_always_on_the_line() {
        let mut editor = editor_with_lines(10);
        editor.set_viewport(40, 20);
        for name in ["one_long_name.rs", "two_long_name.rs", "three_long.rs", "four_long.rs"] {
            editor.open_file(name).unwrap();
        }
        let tabs = row_text(&editor, &Keys::default(), 0);
        assert!(tabs.contains("four_long.rs"), "{tabs}");
        // The ones that did not fit are marked rather than silently missing.
        assert!(tabs.starts_with('<') || tabs.starts_with('\u{e0b3}'), "{tabs}");
    }

}
