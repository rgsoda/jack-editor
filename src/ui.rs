use std::collections::HashMap;
use std::ops::Range;

use unicode_width::UnicodeWidthChar;

use crate::editor::{Completing, Editor, Prompt};
use crate::view::Diagnostic;
use crate::window::Rect;
use crate::view::char_width;
use crate::keys::Keys;
use crate::picker::Picker;
use crate::complete::{Candidate, Completion};
use crate::info::Info;
use crate::screen::{Style, Surface};
use crate::status::{self, Glyphs, Segment};
use crate::stream::Sign;
use crate::syntax::Highlights;

/// Draw a whole frame. Nothing here talks to the terminal; `Screen` works out
/// which of these cells actually need sending.
pub fn draw(editor: &Editor, keys: &Keys, surface: &mut Surface) {
    let (rects, lines) = editor.window_rects();
    for &(id, rect) in &rects {
        draw_window(editor, surface, id, rect);
        match id == editor.focus() {
            true => draw_status(editor, keys, surface, rect),
            false => draw_inactive_status(editor, surface, id, rect),
        }
    }
    // The line between side-by-side windows, down through their status lines
    // too, so a row of status lines still reads as separate windows.
    let separator = editor.theme.style("ui.window.separator");
    for line in lines {
        for row in line.y..line.y + line.height {
            surface.put(line.x, row, '\u{2502}', 1, separator);
        }
    }

    if editor.show_tabline() {
        draw_tabline(editor, surface);
    }

    // The box and the popup want the same few rows beside the cursor. The
    // popup is the one being typed into, so it wins them.
    match (editor.completion.as_ref(), editor.info.as_ref()) {
        (Some(completion), _) => draw_completion(editor, completion, surface),
        (None, Some(info)) => draw_info(editor, info, surface),
        (None, None) => {}
    }

    if let Some(picker) = editor.picker.as_ref() {
        draw_picker(editor, picker, surface);
    }

    // A prompt takes the bottom line of the screen, whichever window's status
    // line is there, the way a command line does.
    if let Some(prompt) = editor.prompt.as_ref() {
        draw_prompt(editor, prompt, surface);
    }
}

/// One window's text: gutter, highlighting, and for the focused one the
/// selection, the cursor line and the bracket pair - the things that are about
/// where you are rather than what is in the file.
fn draw_window(editor: &Editor, surface: &mut Surface, id: usize, rect: Rect) {
    let (view, cursor, scroll_top, scroll_left) = editor.window_state(id);
    let focused = id == editor.focus();
    let height = rect.text_height();
    let total_lines = view.doc.len_lines();

    // Highlight only the visible rows. Tree-sitter keeps the whole parse tree,
    // but running the query over the viewport is what keeps big files cheap.
    let first_byte = view.doc.line_to_byte(scroll_top.min(total_lines.saturating_sub(1)));
    let last_row = scroll_top + height;
    let last_byte = if last_row >= total_lines {
        view.doc.len_bytes()
    } else {
        view.doc.line_to_byte(last_row)
    };
    let highlights = view.highlights(first_byte..last_byte, &editor.theme);

    let gutter = editor.gutter_width_for(view);
    let signs = editor.sign_width();
    let (cursor_line, _) = view.doc.coords(cursor.head);
    // The row the cursor is on, tinted the width of the window. Drawn under
    // everything else: the syntax keeps its colours, and a selection or a
    // search match still wins on the cells it covers.
    let cursorline = (editor.cursorline && focused).then(|| editor.theme.style("ui.cursorline"));
    let number_style = editor.theme.style("ui.linenr");
    let current_style = editor.theme.style("ui.linenr.selected");

    // Matches are painted only while a search is live, and only for the rows
    // on screen - the pattern is run over the viewport, not the buffer.
    let matches = match editor.search.highlight {
        true => editor.search.matches_in_lines(&view.doc, scroll_top, last_row),
        false => Vec::new(),
    };

    // The bracket under the cursor and its mate, so a pair can be seen at a
    // glance rather than counted.
    let brackets = editor.bracket_pair().filter(|_| focused);

    let selection = editor.selection_range().filter(|_| focused);
    let (sel_start, sel_end) = selection.unwrap_or((0, 0));

    // The diagnostics on screen: their ranges to underline, and for each line
    // the worst of them, for the gutter and the message after the text.
    let screen_start = view.doc.line_to_char(scroll_top.min(total_lines.saturating_sub(1)));
    let screen_end = match last_row >= total_lines {
        true => view.doc.len_chars(),
        false => view.doc.line_to_char(last_row),
    };
    let mut underlines = Vec::new();
    let mut worst: HashMap<usize, &Diagnostic> = HashMap::new();
    for diagnostic in &view.diagnostics {
        if diagnostic.start > screen_end || diagnostic.end.max(diagnostic.start + 1) <= screen_start {
            continue;
        }
        let key = format!("diagnostic.underline.{}", diagnostic.severity.name());
        // A diagnostic with no width still marks a character.
        underlines.push((diagnostic.start, diagnostic.end.max(diagnostic.start + 1), editor.theme.style(&key)));
        let line = view.doc.char_to_line(diagnostic.start);
        let entry = worst.entry(line).or_insert(diagnostic);
        if diagnostic.severity < entry.severity {
            *entry = diagnostic;
        }
    }
    let glyphs = status::glyphs(editor);

    let styling = LineStyling {
        highlights: &highlights,
        selection: editor.theme.style("ui.selection"),
        scroll_left,
        left: rect.x + gutter,
        right: rect.x + rect.width,
        matches: &matches,
        match_style: editor.theme.style("ui.search.match"),
        brackets,
        bracket_style: editor.theme.style("ui.bracket.match"),
        diagnostics: &underlines,
    };

    for row in rect.y..rect.y + height {
        let line = scroll_top + row - rect.y;
        if line >= total_lines {
            // Past the end there is no line to number, and the `~` keeps the
            // far left column, as it does with no gutter at all.
            for x in rect.x..rect.x + gutter.min(rect.width) {
                surface.put(x, row, ' ', 1, number_style);
            }
            surface.put(rect.x, row, '~', 1, editor.theme.style("ui.eof"));
            continue;
        }

        let here = cursorline.filter(|_| line == cursor_line);
        if let Some(tint) = here {
            // The whole row first, so the tint reaches past the end of the
            // text and behind the gutter; everything below draws over it.
            for x in rect.x..rect.x + rect.width {
                surface.put(x, row, ' ', 1, tint);
            }
        }

        let diagnostic = worst.get(&line).copied();
        if signs > 0 {
            let (ch, key) = match view.signs.get(&line) {
                Some(Sign::Added) => ('+', "ui.gutter.added"),
                Some(Sign::Modified) => ('~', "ui.gutter.modified"),
                Some(Sign::Deleted) => ('_', "ui.gutter.deleted"),
                None => (' ', "ui.linenr"),
            };
            // A diagnostic has the column over the git sign: the one needs
            // doing something about, the other is only news.
            let (ch, key) = match diagnostic {
                Some(d) => (glyphs.diagnostic.chars().next().unwrap_or('!'), format!("diagnostic.{}", d.severity.name())),
                None => (ch, key.to_string()),
            };
            let style = editor.theme.style(&key);
            surface.put(rect.x, row, ch, 1, under(here, style));
        }

        if let Some(number) = editor.numbers.label(line, cursor_line) {
            let style = match line == cursor_line {
                true => under(here, current_style),
                false => number_style,
            };
            // Right-aligned, with the space either side the width allows for.
            let text = format!("{number:>width$} ", width = gutter - signs - 1);
            let limit = (rect.x + gutter).min(rect.x + rect.width);
            put_str(surface, rect.x + signs, row, &text, style, limit);
        }

        let text = view.doc.line_str(line);
        let line_start = view.doc.line_to_char(line);
        let line_end = line_start + view.doc.line_len_chars(line);

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
            view.doc.line_to_byte(line),
            line_start,
            sel,
            here,
            &styling,
        );

        // The worst diagnostic's message, after the text where there is room.
        // Its first line: the rest is for `]d`.
        if let Some(d) = diagnostic {
            let end = char_col_width(&text);
            let x = styling.left + end.saturating_sub(scroll_left) + 2;
            let right = rect.x + rect.width;
            if x < right {
                let style = under(here, editor.theme.style(&format!("diagnostic.{}", d.severity.name())));
                let message = format!("{} {}", glyphs.diagnostic, d.message.lines().next().unwrap_or(""));
                put_str(surface, x, row, &message, style, right);
            }
        }
    }
}

/// How wide a line's text is on screen, tabs expanded.
fn char_col_width(text: &str) -> usize {
    crate::view::display_col(text, text.chars().count())
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
    let rect = editor.window_rect(editor.focus());
    let bottom = rect.y + rect.text_height();
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

/// The box beside the cursor: what `K` asked the server, or the signature of
/// the call being typed.
///
/// Above the cursor by preference, unlike the completion popup: what it is
/// about is the line the cursor is on, and a box under that line covers what
/// you are about to type into.
fn draw_info(editor: &Editor, info: &Info, surface: &mut Surface) {
    let (screen_width, _) = surface.size();
    if info.lines.is_empty() || editor.height == 0 {
        return;
    }

    let base = editor.theme.style("ui.info");
    let active = editor.theme.style("ui.info.active");

    // A space of padding each side, and never wider than the screen.
    let width = (info.width() + 2).min(screen_width);
    let (cursor_x, cursor_y) = editor.cursor_screen();
    let left = (cursor_x as usize).min(screen_width.saturating_sub(width));

    let rect = editor.window_rect(editor.focus());
    let bottom = rect.y + rect.text_height();
    let rows = info.lines.len().min(bottom.saturating_sub(rect.y));
    let above = (cursor_y as usize).saturating_sub(rows);
    // Above unless there is no room up there, in which case under the cursor,
    // and pushed up off the bottom if it has to be.
    let top = match cursor_y as usize >= rect.y + rows {
        true => above,
        false => (cursor_y as usize + 1).min(bottom.saturating_sub(rows)),
    };

    for (row, line) in info.lines.iter().take(rows).enumerate() {
        let y = top + row;
        if y >= bottom {
            break;
        }
        let right = left + width;
        let mut x = put_str(surface, left, y, " ", base, right);
        for (column, c) in line.text.chars().enumerate() {
            let style = match line.active.as_ref().is_some_and(|range| range.contains(&column)) {
                true => base.patch(active),
                false => base,
            };
            x = put_str(surface, x, y, &c.to_string(), style, right);
        }
        while x < right {
            surface.put(x, y, ' ', 1, base);
            x += 1;
        }
    }
}

fn draw_picker(editor: &Editor, picker: &Picker, surface: &mut Surface) {
    let (width, _) = surface.size();
    let rows = editor.area_rows();
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
        // A row with nothing on it is not the selected row, whatever the
        // cursor says: an empty list still has a cursor at 0, and highlighting
        // that row paints a lit-up bar with a lone `>` on it - which is what a
        // picker looks like for the moment between opening and the walk
        // arriving, and for as long as a query matches nothing.
        let found = picker.matches().get(index);
        let is_cursor = found.is_some() && index == picker.cursor();
        let row_style = if is_cursor { selected } else { base };

        let mut x = put_str(surface, 0, y, if is_cursor { " > " } else { "   " }, row_style, width);
        // Nothing matched, and nothing else is coming: say so, rather than
        // leaving an empty box to be read as a broken one.
        if row == 0 && picker.matches().is_empty() && !picker.query.is_empty() && picker.is_complete()
        {
            x = put_str(surface, x, y, "no matches", base.patch(detail), width);
        }
        if let Some(m) = found {
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
    /// First column the text may use: past the window's gutter.
    left: usize,
    /// The column after the last one the text may use: the window's edge.
    right: usize,
    /// Search matches on screen, as absolute character ranges.
    matches: &'a [(usize, usize)],
    match_style: Style,
    brackets: Option<(usize, usize)>,
    bracket_style: Style,
    /// Ranges a diagnostic covers, with the style to underline them in.
    diagnostics: &'a [(usize, usize, Style)],
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
    let scroll_left = styling.scroll_left;
    // Columns here are the text's own, with the gutter added only when a cell
    // is actually written.
    let right = scroll_left + styling.right.saturating_sub(styling.left);
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
        if let Some(&(_, _, underline)) = styling.diagnostics.iter().find(|&&(s, e, _)| at >= s && at < e) {
            style = style.patch(underline);
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
    let lane = gap.end.saturating_sub(gap.start);
    if lane < status::DOG_ROOM {
        return;
    }

    let dog = match editor.dog.running {
        true => status::DOG_RUNNING,
        false => status::DOG_SITTING,
    };
    // Where it has got to, running or resting: a dog that stops running stops
    // where it is, and starts again from there, which is the only way the two
    // glyphs read as one animal rather than two. Before the first key that is
    // the start of the lane, which is where it comes in from.
    let x = gap.start + editor.dog.steps % lane;
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

fn draw_prompt(editor: &Editor, prompt: &Prompt, surface: &mut Surface) {
    let (width, height) = surface.size();
    let row = height - 1;
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
}

/// The status line of a window that is not the one being typed in: which
/// file, and where in it, dimmed - enough to tell the windows apart.
fn draw_inactive_status(editor: &Editor, surface: &mut Surface, id: usize, rect: Rect) {
    let Some(row) = (rect.y + rect.height).checked_sub(1) else {
        return;
    };
    let (view, cursor, _, _) = editor.window_state(id);
    let style = editor.theme.style("ui.statusline.inactive");
    let end = rect.x + rect.width;

    let mut name = format!(" {}", view.doc.display_name());
    if view.is_modified() {
        name.push(' ');
        name.push_str(status::glyphs(editor).modified);
    }
    let (line, column) = view.doc.coords(cursor.head);
    let position = format!("{}:{} ", line + 1, column + 1);
    let start = end.saturating_sub(str_width(&position)).max(rect.x);

    let mut x = put_str(surface, rect.x, row, &name, style, start);
    while x < start {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }
    x = put_str(surface, x, row, &position, style, end);
    while x < end {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }
}

fn draw_status(editor: &Editor, keys: &Keys, surface: &mut Surface, rect: Rect) {
    let Some(row) = (rect.y + rect.height).checked_sub(1) else {
        return;
    };
    let (origin, width) = (rect.x, rect.width);
    let end = origin + width;

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
    let start = end.saturating_sub(run_width(&right)).max(origin);

    let mut x = origin;
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
        x = put_str(surface, x, row, text, *style, end);
    }
    while x < end {
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
    fn the_dog_starts_at_the_left_and_runs_when_you_type() {
        let mut editor = editor_with_lines(10);
        let keys = Keys::default();
        let middle = editor.width / 2;

        // It comes in from the left end of its lane rather than the middle.
        let sitting: Vec<char> = status_row(&editor, &keys).chars().collect();
        let start = sitting.iter().position(|c| *c == status::DOG_SITTING);
        let start = start.expect("a sitting dog");
        assert!(start < middle, "the left of the lane: {start}");

        // A key, and it is off - a different dog, one cell on.
        editor.dog_runs();
        let running: Vec<char> = status_row(&editor, &keys).chars().collect();
        assert!(!running.contains(&status::DOG_SITTING));
        let at = running.iter().position(|c| *c == status::DOG_RUNNING);
        assert_eq!(at, Some(start + 1));

        // It keeps going while the keys keep coming, and comes back around
        // rather than running off the end of its lane. The lane is the whole
        // gap the status line leaves, so a lap reaches both sides of it.
        let mut seen = vec![at.unwrap()];
        for _ in 0..120 {
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

        // And it sits down where it stopped, not back in the middle: the
        // sitting dog takes the running one's place exactly.
        let stopped = *seen.last().unwrap();
        editor.dog_rests();
        let resting: Vec<char> = status_row(&editor, &keys).chars().collect();
        assert_eq!(resting[stopped], status::DOG_SITTING);
        assert!(!resting.contains(&status::DOG_RUNNING));

        // The next key carries on from there rather than starting a new lap.
        editor.dog_runs();
        let moved: Vec<char> = status_row(&editor, &keys).chars().collect();
        let now = moved.iter().position(|c| *c == status::DOG_RUNNING).unwrap();
        assert_eq!(now, stopped + 1);
    }

    #[test]
    fn an_empty_picker_has_no_selected_row_to_light_up() {
        let mut editor = editor_with_lines(20);
        // What `<space>f` looks like for the moment before the walk arrives:
        // open, and with nothing in it yet.
        editor.picker = Some(crate::picker::Picker::streaming(crate::picker::Source::Files));
        let keys = Keys::default();
        let rows = editor.area_rows();
        let panel = crate::picker::Picker::panel_height(rows);
        let first = editor.top() + rows - panel + 1;

        let row = row_text(&editor, &keys, first);
        assert!(row.trim().is_empty(), "no lone marker on an empty list: {row:?}");
        let styles = row_backgrounds(&editor, &keys, first);
        let selected = editor.theme.style("ui.picker.selected").bg;
        assert!(styles.iter().all(|bg| *bg != selected), "and nothing lit up either");
    }

    #[test]
    fn a_query_that_matches_nothing_says_so() {
        let mut editor = editor_with_lines(20);
        let items = vec![crate::picker::Item {
            text: "src/main.rs".into(),
            detail: String::new(),
            target: String::new(),
            id: 0,
        }];
        editor.picker = Some(crate::picker::Picker::new(crate::picker::Source::Files, items));
        for c in "zzz".chars() {
            editor.picker_input(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ));
        }

        let keys = Keys::default();
        let rows = editor.area_rows();
        let first = editor.top() + rows - crate::picker::Picker::panel_height(rows) + 1;
        assert!(row_text(&editor, &keys, first).contains("no matches"));
    }

    #[test]
    fn the_info_box_sits_above_the_line_it_is_about() {
        let mut editor = editor_with_lines(20);
        editor.goto_line(5);
        editor.info = crate::info::Info::hover("what this is\n\nand why", 0);
        let keys = Keys::default();
        // The cursor is on screen row 5; the box takes the three rows above.
        assert!(row_text(&editor, &keys, 2).contains(" what this is "));
        // The blank line between the paragraphs is a row of the box, not a
        // row of the file showing through it.
        assert!(!row_text(&editor, &keys, 3).contains("line 4"));
        assert!(row_text(&editor, &keys, 4).contains(" and why "));
        assert!(row_text(&editor, &keys, 5).contains("line 6 of text"), "the cursor line is clear");
    }

    #[test]
    fn a_box_with_no_room_above_it_goes_below() {
        let mut editor = editor_with_lines(20);
        editor.goto_line(0);
        editor.info = crate::info::Info::hover("one\ntwo", 0);
        let keys = Keys::default();
        assert!(row_text(&editor, &keys, 0).contains("line 1 of text"));
        assert!(row_text(&editor, &keys, 1).contains(" one "));
        assert!(row_text(&editor, &keys, 2).contains(" two "));
    }

    #[test]
    fn the_popup_takes_the_space_from_the_box() {
        let mut editor = editor_with_lines(20);
        editor.goto_line(5);
        editor.info = crate::info::Info::hover("what this is", 0);
        editor.set_mode(crate::editor::Mode::Insert);
        editor.open_completion(false);
        assert!(editor.completion.is_some(), "a popup of the buffer's own words");
        let keys = Keys::default();
        for row in 0..6 {
            let text = row_text(&editor, &keys, row);
            assert!(!text.contains("what this is"), "row {row}: {text}");
        }
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

    #[test]
    fn a_diagnostic_is_marked_underlined_and_said_after_its_line() {
        let mut editor = editor_with_lines(5);
        editor.set_viewport(60, 6);
        let start = editor.view().doc.line_to_char(1) + 5;
        editor.view_mut().diagnostics = vec![crate::view::Diagnostic {
            start,
            end: start + 1,
            severity: crate::lsp::Severity::Warning,
            message: "two is suspicious\nsecond line".into(),
        }];
        let keys = Keys::default();

        let row = row_text(&editor, &keys, 1);
        assert_eq!(row.chars().next(), Some('\u{25cf}'), "the gutter mark: {row}");
        assert!(row.contains("line 2 of text  \u{25cf} two is suspicious"), "{row}");
        assert!(!row.contains("second line"));

        let mut screen = Screen::new();
        let surface = screen.begin(60, 7);
        draw(&editor, &keys, surface);
        let x = editor.gutter_width() + 5;
        assert!(surface.get(x, 1).style.underline, "the character it is about");
        assert!(!surface.get(x + 1, 1).style.underline);

        // And counted in the status line.
        assert!(status_row(&editor, &keys).contains('1'));
    }

    #[test]
    fn side_by_side_windows_are_drawn_with_a_line_between_them() {
        let mut editor = editor_with_lines(30);
        editor.set_viewport(81, 10);
        editor.split_window(true, None);
        editor.goto_line(4);
        editor.scroll_to_cursor();
        let keys = Keys::default();

        let mut screen = Screen::new();
        let surface = screen.begin(81, 11);
        draw(&editor, &keys, surface);
        let row = |y: usize| -> String { (0..81).map(|x| surface.get(x, y).ch).collect() };

        for y in 0..11 {
            assert_eq!(surface.get(40, y).ch, '\u{2502}', "row {y}");
        }
        // Both halves show the file, each with its own cursor line numbered.
        assert!(row(0)[..40].contains("line 1 of text"), "{}", row(0));
        assert!(row(0).chars().skip(41).collect::<String>().contains("line 1 of text"));

        // The focused window - the new one, on the right - has the full status
        // line; the other has its name and its own position, 1:1.
        let status = row(10);
        let left: String = status.chars().take(40).collect();
        let right: String = status.chars().skip(41).collect();
        assert!(right.contains("ABNORMAL"), "{right}");
        assert!(right.contains("5:1"), "{right}");
        assert!(!left.contains("ABNORMAL"), "{left}");
        assert!(left.contains("[scratch]") && left.trim_end().ends_with("1:1"), "{left}");

        // And the terminal cursor is in the right-hand window.
        let (x, y) = editor.cursor_screen();
        assert_eq!((x as usize, y as usize), (41 + editor.gutter_width(), 4));
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
