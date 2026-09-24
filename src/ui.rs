use std::collections::HashMap;
use std::ops::Range;

use unicode_width::UnicodeWidthChar;

use crate::editor::{Completing, Editor, Prompt};
use crate::view::Diagnostic;
use crate::window::Rect;
use crate::view::cluster_width;
use unicode_segmentation::UnicodeSegmentation;
use crate::keys::Keys;
use crate::picker::{Layout, Picker};
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

    if let Some(pane) = editor.preview_rect() {
        draw_preview(editor, surface, pane);
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
    let highlights = match view.listing {
        // A listing's colours come from what the lines name, not from a
        // grammar - but they arrive in the same shape.
        true => crate::editor::listing::highlights(view, first_byte..last_byte, &editor.theme),
        false => view.highlights(first_byte..last_byte, &editor.theme),
    };

    let gutter = editor.gutter_width_for(view);
    let signs = editor.sign_width();
    let (cursor_line, _) = view.doc.coords(cursor.head);
    // The row the cursor is on, tinted the width of the window. Drawn under
    // everything else: the syntax keeps its colours, and a selection or a
    // search match still wins on the cells it covers.
    let cursorline = (editor.cursorline && focused).then(|| editor.theme.style("ui.cursorline"));
    let number_style = editor.theme.style("ui.linenr");

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
    // A block is not one range but one per line, so it is carried alongside
    // and asked for the line being drawn.
    let block = editor.block().filter(|_| focused);

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

    let wrap = editor.wrap.then(|| rect.width.saturating_sub(gutter).max(1));
    let scroll_left = match wrap {
        Some(_) => 0,
        None => scroll_left,
    };
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
        hint_style: editor.theme.style("ui.inlayhint"),
    };

    // Wrapped, a line takes as many rows as it needs and nothing scrolls
    // sideways; each row is drawn as a line of its own, with the columns,
    // bytes and selection it covers.
    let mut line = scroll_top;
    let mut row = rect.y;
    while row < rect.y + height {
        if line >= total_lines {
            // Past the end there is no line to number, and the `~` keeps the
            // far left column, as it does with no gutter at all.
            for x in rect.x..rect.x + gutter.min(rect.width) {
                surface.put(x, row, ' ', 1, number_style);
            }
            surface.put(rect.x, row, '~', 1, editor.theme.style("ui.eof"));
            row += 1;
            continue;
        }

        let here = cursorline.filter(|_| line == cursor_line);
        let diagnostic = worst.get(&line).copied();
        let text = view.doc.line_str(line);
        let hints = view.hints_on(line);
        let line_start = view.doc.line_to_char(line);
        let line_end = line_start + view.doc.line_len_chars(line);
        let length = text.chars().count();
        let starts = match wrap {
            Some(width) => crate::view::wrap_starts(&text, &hints, width),
            None => vec![0],
        };

        // Selection clipped to this line, as char offsets within it.
        let sel = match &block {
            Some(block) => block.row(line).map(|(start, end)| (start - line_start, end - line_start)),
            None if selection.is_some() && sel_end > line_start && sel_start <= line_end => Some((
                sel_start.saturating_sub(line_start),
                sel_end.saturating_sub(line_start),
            )),
            None => None,
        };

        for (segment, &from) in starts.iter().enumerate() {
            if row >= rect.y + height {
                break;
            }
            if let Some(tint) = here {
                // The whole row first, so the tint reaches past the end of the
                // text and behind the gutter; everything below draws over it.
                for x in rect.x..rect.x + rect.width {
                    surface.put(x, row, ' ', 1, tint);
                }
            }
            let last = segment + 1 == starts.len();
            if segment == 0 {
                draw_gutter(editor, surface, view, GutterRow { row, line, cursor_line, here, diagnostic, x: rect.x, width: rect.width, gutter, signs });
            } else {
                for x in rect.x..rect.x + gutter.min(rect.width) {
                    surface.put(x, row, ' ', 1, under(here, number_style));
                }
            }

            let to = starts.get(segment + 1).copied().unwrap_or(length);
            let (byte_from, byte_to) = (byte_of(&text, from), byte_of(&text, to));
            let piece = &text[byte_from..byte_to];
            let piece_hints: Vec<(usize, &str)> = hints
                .iter()
                .filter(|(at, _)| *at >= from && (*at < to || last))
                .map(|&(at, label)| (at - from, label))
                .collect();
            draw_line(
                surface,
                Row {
                    y: row,
                    text: piece,
                    byte: view.doc.line_to_byte(line) + byte_from,
                    start: line_start + from,
                    selection: sel.map(|(a, b)| (a.saturating_sub(from), b.saturating_sub(from))),
                    cursorline: here,
                    hints: &piece_hints,
                },
                &styling,
            );

            // The worst diagnostic's message, after the text where there is
            // room. Its first line: the rest is for `]d`.
            if let (Some(d), true) = (diagnostic, last) {
                let end = crate::view::hinted_col(piece, piece.chars().count(), &piece_hints);
                let x = styling.left + end.saturating_sub(styling.scroll_left) + 2;
                let right = rect.x + rect.width;
                if x < right {
                    let style = under(here, editor.theme.style(&format!("diagnostic.{}", d.severity.name())));
                    let message = format!("{} {}", glyphs.diagnostic, d.message.lines().next().unwrap_or(""));
                    put_str(surface, x, row, &message, style, right);
                }
            }
            row += 1;
        }
        line += 1;
    }
}

/// The byte offset of char `index` in `text`, or its length past the end.
fn byte_of(text: &str, index: usize) -> usize {
    text.char_indices().nth(index).map_or(text.len(), |(byte, _)| byte)
}

/// One row's gutter: where it is, and what decides what goes in it.
struct GutterRow<'a> {
    row: usize,
    line: usize,
    cursor_line: usize,
    here: Option<Style>,
    diagnostic: Option<&'a Diagnostic>,
    x: usize,
    width: usize,
    gutter: usize,
    signs: usize,
}

/// The sign column and the line number, for the first row of a line.
fn draw_gutter(editor: &Editor, surface: &mut Surface, view: &crate::view::View, at: GutterRow) {
    let GutterRow { row, line, cursor_line, here, diagnostic, x, width, gutter, signs } = at;
    if signs > 0 {
        let (ch, key) = match view.signs.get(&line) {
            Some(Sign::Added) => ('+', "ui.gutter.added"),
            Some(Sign::Modified) => ('~', "ui.gutter.modified"),
            Some(Sign::Deleted) => ('_', "ui.gutter.deleted"),
            None => (' ', "ui.linenr"),
        };
        // A diagnostic has the column over the git sign: the one needs
        // doing something about, the other is only news.
        let glyphs = status::glyphs(editor);
        let (ch, key) = match diagnostic {
            Some(d) => (glyphs.diagnostic.chars().next().unwrap_or('!'), format!("diagnostic.{}", d.severity.name())),
            None => (ch, key.to_string()),
        };
        let style = editor.theme.style(&key);
        surface.put(x, row, ch, 1, under(here, style));
    }

    if let Some(number) = editor.numbers.label(line, cursor_line) {
        let style = match line == cursor_line {
            true => under(here, editor.theme.style("ui.linenr.selected")),
            false => editor.theme.style("ui.linenr"),
        };
        // Right-aligned, with the space either side the width allows for.
        let text = format!("{number:>width$} ", width = gutter - signs - 1);
        let limit = (x + gutter).min(x + width);
        put_str(surface, x + signs, row, &text, style, limit);
    }
}

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

/// The picker: a box floating in the middle of the screen, as Telescope draws
/// it, or on a small terminal the panel across the bottom it used to be. It is
/// opaque either way - every cell inside it is written, so nothing of the text
/// shows through.
fn draw_picker(editor: &Editor, picker: &Picker, surface: &mut Surface) {
    let layout = editor.picker_layout();
    let top = editor.top();
    let (left, right) = (layout.left(), layout.right());

    let base = editor.theme.style("ui.picker");
    let selected = editor.theme.style("ui.picker.selected");
    let matched = editor.theme.style("ui.picker.match");
    let detail = editor.theme.style("ui.picker.detail");

    if layout.framed {
        draw_frame(editor, surface, layout, top, base);
    }

    // Prompt row: the source's name, the query, and the match count.
    let prompt = picker.prompt_text();
    // A trailing `+` while a walk is still feeding the list, so a count that
    // is climbing does not look like the whole answer.
    let count = format!(
        "{}/{}{} ",
        picker.matches().len(),
        picker.item_count(),
        if picker.is_complete() { "" } else { "+" }
    );
    let style = editor.theme.style("ui.picker.prompt");
    let row = top + layout.prompt_row();
    let mut x = put_str(surface, left, row, &prompt, style, right);
    let gap = right.saturating_sub(x + str_width(&count));
    x = put_str(surface, x, row, &" ".repeat(gap), style, right);
    x = put_str(surface, x, row, &count, style, right);
    while x < right {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }

    for row in 0..layout.list_rows() {
        let y = top + layout.list_top() + row;
        let index = picker.scroll() + row;
        // A row with nothing on it is not the selected row, whatever the
        // cursor says: an empty list still has a cursor at 0, and highlighting
        // that row paints a lit-up bar with a lone `>` on it - which is what a
        // picker looks like for the moment between opening and the walk
        // arriving, and for as long as a query matches nothing.
        let found = picker.matches().get(index);
        let is_cursor = found.is_some() && index == picker.cursor();
        let row_style = if is_cursor { selected } else { base };

        let mut x =
            put_str(surface, left, y, if is_cursor { " > " } else { "   " }, row_style, right);
        // Nothing matched, and nothing else is coming: say so, rather than
        // leaving an empty box to be read as a broken one.
        if row == 0 && picker.matches().is_empty() && !picker.query.is_empty() && picker.is_complete()
        {
            x = put_str(surface, x, y, "no matches", base.patch(detail), right);
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
            let room = right.saturating_sub(tail);

            // Matched characters are tinted; the selected row keeps its own
            // background, so the tint patches over it rather than replacing it.
            // A glyph is lit if the query landed on any character in it - the
            // matcher counts characters, and half a glyph cannot be tinted.
            for (at, cluster) in crate::view::clusters(&item.text) {
                let w = cluster_width(cluster, x).max(1);
                if x + w > room {
                    break;
                }
                let lit = (at..at + cluster.chars().count()).any(|i| m.positions.contains(&i));
                let style = match lit {
                    true => row_style.patch(matched),
                    false => row_style,
                };
                surface.put_cluster(x, y, cluster, w, style);
                x += w;
            }
            if tail > 0 {
                while x < right.saturating_sub(tail - 1) {
                    surface.put(x, y, ' ', 1, row_style);
                    x += 1;
                }
                x = put_str(surface, x, y, &item.detail, row_style.patch(detail), right);
            }
        }
        while x < right {
            surface.put(x, y, ' ', 1, row_style);
            x += 1;
        }
    }
}

/// The frame around a floating picker, with the rule under its prompt row.
///
/// Box-drawing characters unless `:set noglyphs` says the terminal has no font
/// for them, in which case the ASCII a frame was drawn with before anyone had
/// one. The corners are the only part that has to be said twice.
fn draw_frame(editor: &Editor, surface: &mut Surface, layout: Layout, top: usize, base: Style) {
    let style = editor.theme.style("ui.picker.border");
    let [tl, tr, bl, br, h, v, tee_left, tee_right] = match editor.glyphs {
        true => ['╭', '╮', '╰', '╯', '─', '│', '├', '┤'],
        false => ['+', '+', '+', '+', '-', '|', '+', '+'],
    };

    let (x0, x1) = (layout.x, layout.x + layout.width - 1);
    let (y0, y1) = (top + layout.y, top + layout.y + layout.height - 1);
    // The rule under the prompt, which is what makes the query a field of its
    // own rather than the first line of the list.
    let rule = top + layout.prompt_row() + 1;

    for x in x0..=x1 {
        surface.put(x, y0, h, 1, style);
        surface.put(x, y1, h, 1, style);
        surface.put(x, rule, h, 1, style);
    }
    for y in y0 + 1..y1 {
        surface.put(x0, y, v, 1, style);
        surface.put(x1, y, v, 1, style);
        // The inside is painted here so that every cell of the box is written
        // whatever the rows below do with it: a short list must not leave the
        // text showing through the bottom of the frame.
        if y != rule {
            for x in x0 + 1..x1 {
                surface.put(x, y, ' ', 1, base);
            }
        }
    }
    surface.put(x0, y0, tl, 1, style);
    surface.put(x1, y0, tr, 1, style);
    surface.put(x0, y1, bl, 1, style);
    surface.put(x1, y1, br, 1, style);
    surface.put(x0, rule, tee_left, 1, style);
    surface.put(x1, rule, tee_right, 1, style);
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
    hint_style: Style,
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

/// The one line being drawn: which row of the screen it is on, its text, and
/// where that text sits in the buffer - in bytes for the highlighter, in chars
/// for everything else.
struct Row<'a> {
    y: usize,
    text: &'a str,
    byte: usize,
    start: usize,
    /// The selection clipped to this line, as char offsets within it.
    selection: Option<(usize, usize)>,
    /// The cursor line's tint, when this is the cursor's line.
    cursorline: Option<Style>,
    /// Inlay hints, as columns in the line and their text.
    hints: &'a [(usize, &'a str)],
}

fn draw_line(surface: &mut Surface, line: Row, styling: &LineStyling) {
    let Row { y: row, text, byte: line_byte, start: line_start, selection: sel, cursorline, hints } = line;
    let scroll_left = styling.scroll_left;
    // Columns here are the text's own, with the gutter added only when a cell
    // is actually written.
    let right = scroll_left + styling.right.saturating_sub(styling.left);
    let mut col = 0usize;
    let hint_style = under(cursorline, styling.hint_style);
    // A hint's characters, drawn from `col` on and pushing the text after them
    // along. Clipped at either edge a character at a time, like the text.
    let draw_hints = |surface: &mut Surface, col: &mut usize, at: usize| {
        for (_, label) in hints.iter().filter(|(column, _)| *column == at) {
            for ch in label.chars() {
                let width = UnicodeWidthChar::width(ch).unwrap_or(0);
                let start = *col;
                *col += width;
                if width == 0 || start < scroll_left || *col > right {
                    continue;
                }
                surface.put(start - scroll_left + styling.left, row, ch, width, hint_style);
            }
        }
    };

    // A glyph at a time, not a character at a time: a cluster is one thing on
    // screen and lives in one cell, however many characters went into it.
    let mut column = 0usize;
    for (byte_in_line, cluster) in text.grapheme_indices(true) {
        // The char column this glyph starts at, which is what the highlights,
        // the matches and the selection are all counted in.
        let char_idx = column;
        column += cluster.chars().count();
        let ch = cluster.chars().next().unwrap_or(' ');
        if !hints.is_empty() {
            draw_hints(surface, &mut col, char_idx);
        }
        let start = col;
        let end = start + cluster_width(cluster, start);
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
            // Tabs, and wide glyphs straddling an edge, become blanks.
            for offset in 0..visible_width {
                surface.put(x + offset, row, ' ', 1, style);
            }
        } else {
            surface.put_cluster(x, row, cluster, visible_width, style);
        }
    }
    // The hints after the last character: a type at the end of a line.
    if !hints.is_empty() && col < right {
        draw_hints(surface, &mut col, text.chars().count());
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
    let text = prompt.line();
    let mut x = put_str(surface, 0, row, &text, style, width);
    while x < width {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }
}

/// The `:preview` pane: a line down its left edge like the one between
/// windows, the rendered markdown, and a status line saying what it is.
///
/// It scrolls itself. While you are in the buffer it shows, the line rendered
/// from the one the cursor is on sits level with the cursor, so what you are
/// reading is beside what you are typing; anywhere else it stays put.
fn draw_preview(editor: &Editor, surface: &mut Surface, pane: Rect) {
    let Some(preview) = editor.preview.as_ref() else {
        return;
    };
    let Some(view) = editor.views().get(preview.view) else {
        return;
    };
    let separator = editor.theme.style("ui.window.separator");
    let left = pane.x.saturating_sub(1);
    for row in pane.y..pane.y + pane.height {
        surface.put(left, row, '\u{2502}', 1, separator);
    }
    let rows = pane.height.saturating_sub(1);
    // A column of margin either side: text against a line reads as cramped.
    let (x0, end) = (pane.x + 1, pane.x + pane.width.saturating_sub(1));
    let width = end.saturating_sub(x0);
    let lines = preview.lines(&view.doc.text, view.edits(), width, editor.glyphs);

    if preview.view == editor.current_index() {
        let (line, _) = view.cursor_coords();
        let (_, row) = view.cursor_screen(editor.wrap_width());
        let target = crate::preview::Preview::row_for(&lines, line);
        preview.top.set(target.saturating_sub(row as usize));
    }
    let top = preview.top.get().min(lines.len().saturating_sub(1));

    let base = Style::default();
    for row in 0..rows {
        let y = pane.y + row;
        for x in pane.x..pane.x + pane.width {
            surface.put(x, y, ' ', 1, base);
        }
        let Some(line) = lines.get(top + row) else {
            continue;
        };
        let mut x = x0;
        for span in &line.spans {
            x = put_str(surface, x, y, &span.text, preview_style(editor, span.look), end);
        }
    }

    let style = editor.theme.style("ui.statusline.inactive");
    let row = pane.y + rows;
    let label = format!(" preview  {}", view.doc.display_name());
    let mut x = put_str(surface, pane.x, row, &label, style, pane.x + pane.width);
    while x < pane.x + pane.width {
        surface.put(x, row, ' ', 1, style);
        x += 1;
    }
}

/// The theme's colours for what the preview says a span is. The same keys
/// the markdown highlighting uses, so the rendered page and the source you
/// type it in agree on what a heading or a piece of code looks like.
fn preview_style(editor: &Editor, look: crate::preview::Look) -> Style {
    use crate::preview::Kind;
    let base = match look.kind {
        Kind::Text => Style::default(),
        Kind::Heading => editor.theme.style("text.title"),
        Kind::Code => editor.theme.style("text.literal"),
        Kind::Link => editor.theme.style("text.uri"),
        Kind::Marker => editor.theme.style("punctuation.special"),
    };
    base.patch(Style {
        bold: look.bold,
        italic: look.italic,
        underline: look.underline,
        dim: look.dim,
        ..Style::default()
    })
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
    for (_, cluster) in crate::view::clusters(text) {
        let w = cluster_width(cluster, 0);
        if x + w > width {
            break;
        }
        surface.put_cluster(x, row, cluster, w, style);
        x += w;
    }
    x
}

/// How many cells a string takes, measured a glyph at a time. There are no
/// tab stops to keep here - these are labels and names, not lines of a file -
/// so every cluster is measured from column zero.
pub fn str_width(s: &str) -> usize {
    crate::view::clusters(s).map(|(_, cluster)| crate::view::cluster_width(cluster, 0)).sum()
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

    /// A frame of the whole screen, `width` by `height`, as lines of text.
    fn screen_text(editor: &Editor, width: usize, height: usize) -> Vec<String> {
        let mut screen = Screen::new();
        let surface = screen.begin(width, height);
        draw(editor, &Keys::default(), surface);
        (0..height).map(|y| (0..width).map(|x| surface.get(x, y).ch).collect()).collect()
    }

    fn previewing(text: &str) -> Editor {
        let mut editor = Editor::scratch();
        editor.view_mut().doc.text = ropey::Rope::from_str(text);
        editor.view_mut().doc.path = Some("notes.md".into());
        editor.numbers = Numbers::Off;
        editor.signs_enabled = false;
        editor.set_viewport(80, 20);
        editor.run_command("preview");
        assert!(editor.preview.is_some(), "{}", editor.message);
        editor
    }

    #[test]
    fn the_preview_is_drawn_beside_the_text_and_level_with_the_cursor() {
        let mut editor = previewing("# Title\n\n## Section\n\nwords\n");
        let rows = screen_text(&editor, 80, 21);
        let pane = |row: &String| row.chars().skip(41).collect::<String>().trim_end().to_string();
        assert_eq!(pane(&rows[0]), "Title", "{rows:#?}");
        assert!(rows[0].chars().nth(39) == Some('\u{2502}'), "a line between: {:?}", rows[0]);
        assert!(pane(&rows[20]).contains("preview"), "{:?}", rows[20]);

        // On the section heading: its rendering comes level with it.
        editor.goto_line(2);
        let rows = screen_text(&editor, 80, 21);
        assert_eq!(pane(&rows[2]), "Section", "{rows:#?}");
        editor.goto_line(4);
        let rows = screen_text(&editor, 80, 21);
        assert_eq!(pane(&rows[4]), "words", "{rows:#?}");
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
        let layout = editor.picker_layout();
        let first = editor.top() + layout.list_top();

        // Inside the frame: the text of the file goes on either side of a box
        // that floats, and it is the box's own row that has to be empty.
        let row: String =
            row_text(&editor, &keys, first).chars().take(layout.right()).skip(layout.left()).collect();
        assert!(row.trim().is_empty(), "no lone marker on an empty list: {row:?}");
        let styles = row_backgrounds(&editor, &keys, first);
        let selected = editor.theme.style("ui.picker.selected").bg;
        assert!(styles.iter().all(|bg| *bg != selected), "and nothing lit up either");
    }

    #[test]
    fn the_picker_floats_in_a_frame_with_the_text_still_around_it() {
        let mut editor = editor_with_lines(40);
        editor.picker = Some(crate::picker::Picker::streaming(crate::picker::Source::Files));
        let keys = Keys::default();
        let layout = editor.picker_layout();
        assert!(layout.framed, "80x24 has room to float");

        // The frame's top row: a corner, a rule, a corner - and the file
        // showing on both sides of it.
        let row = row_text(&editor, &keys, editor.top() + layout.y);
        let chars: Vec<char> = row.chars().collect();
        assert_eq!(chars[layout.x], '\u{256d}');
        assert_eq!(chars[layout.x + layout.width - 1], '\u{256e}');
        assert_eq!(chars[layout.x + 1], '\u{2500}');
        assert!(row[..layout.x].trim().is_empty() || row.trim_start().starts_with(|c: char| c.is_ascii_digit()));
        assert!(chars[..layout.x].iter().any(|c| !c.is_whitespace()), "text to the left");

        // And the rule under the prompt, which makes the query its own field.
        let rule = row_text(&editor, &keys, editor.top() + layout.prompt_row() + 1);
        let chars: Vec<char> = rule.chars().collect();
        assert_eq!(chars[layout.x], '\u{251c}');
        assert_eq!(chars[layout.x + layout.width - 1], '\u{2524}');
    }

    #[test]
    fn a_narrow_terminal_gets_the_panel_instead_of_a_box() {
        // A frame costs two columns and four rows, which on a small terminal
        // is most of the list. Below that it is the bottom panel it was.
        let layout = crate::picker::Layout::new(40, 20);
        assert!(!layout.framed);
        assert_eq!(layout.x, 0);
        assert_eq!(layout.width, 40);
        assert_eq!(layout.y + layout.height, 20, "against the status line");
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
        let first = editor.top() + editor.picker_layout().list_top();
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

    /// Which cells of a drawn row are in the selection, by the reverse-video
    /// the default theme marks it with.
    fn row_selected(editor: &Editor, keys: &Keys, y: usize) -> Vec<bool> {
        let mut screen = Screen::new();
        let (width, height) = (editor.width, editor.top() + editor.height + 1);
        let surface = screen.begin(width, height);
        draw(editor, keys, surface);
        (0..width).map(|x| surface.get(x, y).style.reverse).collect()
    }

    #[test]
    fn a_block_selection_is_drawn_as_a_rectangle() {
        // Lines 1 to 3, over columns 5 to 7 of each of them.
        let mut editor = editor_with_lines(10);
        editor.goto_line(1);
        // The corner the block is anchored at, before it is a block at all.
        for _ in 0..5 {
            editor.move_cursor(Move::Right, false);
        }
        editor.set_mode(crate::editor::Mode::VisualBlock);
        for _ in 0..2 {
            editor.move_cursor(Move::Down, true);
        }
        for _ in 0..2 {
            editor.move_cursor(Move::Right, true);
        }
        let keys = Keys::default();
        let gutter = editor.gutter_width();

        // The same three columns on all three rows, and nothing on the row
        // above: a rectangle, rather than a run of text crossing lines.
        for y in 1..=3 {
            let row = row_selected(&editor, &keys, y);
            let lit: Vec<usize> = (0..12).filter(|x| row[gutter + x]).collect();
            assert_eq!(lit, [5, 6, 7], "row {y}");
        }
        assert!(!row_selected(&editor, &keys, 0).iter().any(|&on| on), "the line above is not in it");
        assert!(!row_selected(&editor, &keys, 4).iter().any(|&on| on), "nor the line below");
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
            raw: serde_json::Value::Null,
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

    #[test]
    fn hints_are_drawn_into_the_line_and_the_cursor_steps_past_them() {
        let mut editor = Editor::scratch();
        editor.view_mut().doc.text = ropey::Rope::from_str("let x = f(2);\n");
        editor.set_viewport(40, 5);
        editor.numbers = Numbers::Off;
        editor.signs_enabled = false;
        editor.view_mut().hints = vec![
            crate::view::Hint { at: 5, label: ": i32".into() },
            crate::view::Hint { at: 10, label: "n: ".into() },
        ];
        let keys = Keys::default();
        assert_eq!(row_text(&editor, &keys, 0).trim_end(), "let x: i32 = f(n: 2);");
        editor.view_mut().sel = crate::view::Selection::point(10);
        assert_eq!(editor.cursor_screen(), (18, 0), "on the 2, past the hint before it");
    }

    #[test]
    fn a_wrapped_line_takes_the_rows_it_needs_and_the_cursor_follows() {
        let mut editor = Editor::scratch();
        editor.view_mut().doc.text = ropey::Rope::from_str("one two three four five six\nnext\n");
        editor.numbers = Numbers::Absolute;
        editor.signs_enabled = false;
        // The gutter takes four columns, which leaves ten for the text.
        editor.set_viewport(14, 6);
        editor.wrap = true;
        let keys = Keys::default();
        let rows: Vec<String> = (0..5).map(|y| row_text(&editor, &keys, y).trim_end().to_string()).collect();
        assert_eq!(rows, ["  1 one two", "    three four", "    five six", "  2 next", "  3"]);

        // On "five": the third row, at the start of it.
        editor.view_mut().sel = crate::view::Selection::point(19);
        assert_eq!(editor.cursor_screen(), (4, 2));

        // Off, it is one row again, scrolled sideways to where the cursor is.
        editor.wrap = false;
        editor.scroll_to_cursor();
        assert!(row_text(&editor, &keys, 1).starts_with("  2 "));
        assert_eq!(editor.cursor_screen(), (13, 0));
    }

    #[test]
    fn scrolling_to_a_wrapped_cursor_counts_rows_not_lines() {
        let mut editor = Editor::scratch();
        let long = "word ".repeat(10);
        let text: String = (0..10).map(|_| format!("{long}\n")).collect();
        editor.view_mut().doc.text = ropey::Rope::from_str(&text);
        editor.numbers = Numbers::Off;
        editor.signs_enabled = false;
        editor.set_viewport(10, 8);
        editor.wrap = true;
        // Each line is five rows at ten wide; line 3 is rows 15 to 19.
        editor.goto_line(3);
        editor.scroll_to_cursor();
        let (_, y) = editor.cursor_screen();
        assert!((y as usize) < 8, "on screen: {y}");
        assert!(editor.view().scroll_top >= 2, "{}", editor.view().scroll_top);

        editor.reveal(crate::view::Reveal::Top);
        assert_eq!(editor.view().scroll_top, 0, "three lines kept above it, as without wrap");
    }
}
