//! `:preview` - a markdown buffer as it reads rather than as it is typed.
//!
//! A renderer and nothing else: markdown text and a width in, lines of styled
//! words out, each line knowing which line of the source it came from. What a
//! style *looks* like is the theme's, decided in `ui.rs`; this file says only
//! that a span is a heading or a link or emphasised, which is what makes it
//! testable without a terminal.
//!
//! The source line on every rendered line is what lets the pane follow the
//! cursor: the editor looks up the line the cursor is on and scrolls the
//! preview to the first line rendered from it.

use std::cell::RefCell;
use std::rc::Rc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::ui::str_width;

/// What a span is, which the theme turns into a colour. Kept to the few kinds
/// the theme already has names for, so the preview reads in the same colours
/// as the highlighting you edit it in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Text,
    Heading,
    Code,
    Link,
    /// Bullets, quote bars, rules, table lines: the scaffolding, not the words.
    Marker,
}

/// A span's kind and the attributes on top of it. Emphasis inside a link
/// inside a heading is all of those at once, which is why this is flags and
/// not one more variant of `Kind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub kind: Kind,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub look: Look,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    pub spans: Vec<Span>,
    /// The line of the source this was rendered from, zero-based. Every line
    /// of a wrapped paragraph carries the line the paragraph starts on.
    pub source: usize,
}

impl Line {
    /// The text without its styles, which is what the tests read.
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// The characters the scaffolding is drawn with, and their ASCII fallback for
/// `:set noglyphs` - the same setting the status line and the picker's frame
/// answer to. None of these need a patched font, but they do need a font with
/// box drawing in it, and that is the question the setting asks.
struct Marks {
    bullets: [&'static str; 3],
    unchecked: &'static str,
    checked: &'static str,
    quote: &'static str,
    rule: &'static str,
    double: &'static str,
    column: &'static str,
    cross: &'static str,
}

const FANCY: Marks = Marks {
    bullets: ["•", "◦", "▪"],
    unchecked: "☐",
    checked: "☑",
    quote: "│",
    rule: "─",
    double: "═",
    column: "│",
    cross: "┼",
};

const PLAIN: Marks = Marks {
    bullets: ["-", "*", "+"],
    unchecked: "[ ]",
    checked: "[x]",
    quote: "|",
    rule: "-",
    double: "=",
    column: "|",
    cross: "+",
};

/// Render `text` to lines no wider than `width`, except where there is no
/// way to be: a code block or a table is cut at the edge by whatever draws
/// it, rather than wrapped into something that is no longer code.
pub fn render(text: &str, width: usize, glyphs: bool) -> Vec<Line> {
    let mut renderer = Renderer::new(text, width.max(10), glyphs);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        renderer.event(event, range.start);
    }
    renderer.flush();
    let mut lines = renderer.out;
    // Trailing blank lines are the spacing after the last block, not content.
    while lines.last().is_some_and(|line| line.text().trim().is_empty()) {
        lines.pop();
    }
    lines
}

/// Something lines are drawn inside: a quote puts a bar down the left of all
/// of them, a list item a bullet on its first and an indent on the rest.
struct Container {
    first: Vec<Span>,
    rest: Vec<Span>,
    /// Whether a line has been drawn in it yet, which is what spends `first`.
    started: bool,
}

/// A table being collected. It cannot be drawn until the widest cell of every
/// column is known, which is the end of the table.
struct Table {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<Vec<Span>>>,
    source: usize,
}

struct Renderer<'a> {
    marks: &'static Marks,
    width: usize,
    /// Byte offsets of the source's line starts, for turning an event's offset
    /// into the line it is on.
    starts: Vec<usize>,
    out: Vec<Line>,
    containers: Vec<Container>,
    /// The words of the block being built: a paragraph, a heading, a cell.
    inline: Vec<Span>,
    inline_source: usize,
    /// The attributes the text arriving now should have, as a stack of what
    /// has been opened: `**` pushes bold, its end pops it.
    looks: Vec<Look>,
    heading: Option<HeadingLevel>,
    code: Option<(String, usize)>,
    table: Option<Table>,
    /// How deep in lists, for which bullet to draw.
    list_depth: usize,
    /// The number of the next item in each open list, `None` for bullets.
    numbers: Vec<Option<u64>>,
    /// A block ended and the next one should have a blank line before it.
    gap: bool,
    source_text: &'a str,
}

impl<'a> Renderer<'a> {
    fn new(text: &'a str, width: usize, glyphs: bool) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(at, _)| at + 1));
        Renderer {
            marks: if glyphs { &FANCY } else { &PLAIN },
            width,
            starts,
            out: Vec::new(),
            containers: Vec::new(),
            inline: Vec::new(),
            inline_source: 0,
            looks: vec![Look::default()],
            heading: None,
            code: None,
            table: None,
            list_depth: 0,
            numbers: Vec::new(),
            gap: false,
            source_text: text,
        }
    }

    fn line_of(&self, byte: usize) -> usize {
        self.starts.partition_point(|&start| start <= byte).saturating_sub(1)
    }

    fn look(&self) -> Look {
        *self.looks.last().expect("a base look")
    }

    fn push_look(&mut self, change: impl FnOnce(&mut Look)) {
        let mut look = self.look();
        change(&mut look);
        self.looks.push(look);
    }

    fn pop_look(&mut self) {
        if self.looks.len() > 1 {
            self.looks.pop();
        }
    }

    /// Words for the block being built, starting it if nothing had.
    fn text(&mut self, text: &str, at: usize) {
        if self.inline.is_empty() {
            self.inline_source = self.line_of(at);
        }
        let look = self.look();
        self.inline.push(Span { text: text.to_string(), look });
    }

    fn marker(&self, text: &str) -> Span {
        Span { text: text.to_string(), look: Look { kind: Kind::Marker, ..Look::default() } }
    }

    fn event(&mut self, event: Event, at: usize) {
        // Inside a code block every piece of text is code, whatever it says.
        if let Some((code, _)) = self.code.as_mut() {
            match event {
                Event::Text(text) => {
                    code.push_str(&text);
                    return;
                }
                Event::End(TagEnd::CodeBlock) => {}
                _ => return,
            }
        }

        match event {
            Event::Start(tag) => self.start(tag, at),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text, at),
            Event::Code(text) => {
                self.push_look(|look| look.kind = Kind::Code);
                self.text(&text, at);
                self.pop_look();
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.push_look(|look| look.kind = Kind::Code);
                self.text(&text, at);
                self.pop_look();
            }
            Event::Html(text) | Event::InlineHtml(text) => {
                self.push_look(|look| look.dim = true);
                self.text(text.trim_end_matches('\n'), at);
                self.pop_look();
                // An HTML block arrives a line at a time, and each line was a
                // line: `<details>` on its own should stay on its own.
                if text.ends_with('\n') && self.table.is_none() {
                    self.inline.push(Span { text: "\n".into(), look: Look::default() });
                }
            }
            Event::FootnoteReference(name) => {
                self.push_look(|look| look.dim = true);
                self.text(&format!("[{name}]"), at);
                self.pop_look();
            }
            Event::SoftBreak => self.text(" ", at),
            Event::HardBreak => {
                self.inline.push(Span { text: "\n".into(), look: Look::default() });
            }
            Event::Rule => {
                self.flush();
                self.blank_if_gap();
                let rule = self.marks.rule.repeat(self.room());
                let line = self.line_of(at);
                self.emit(vec![self.marker(&rule)], line);
                self.gap = true;
            }
            Event::TaskListMarker(done) => {
                // The item's bullet becomes a box. It has not been drawn yet:
                // the marker comes before the item's first word.
                let mark = if done { self.marks.checked } else { self.marks.unchecked };
                if let Some(item) = self.containers.last_mut()
                    && !item.started
                {
                    let width = str_width(mark) + 1;
                    item.first = vec![Span {
                        text: format!("{mark} "),
                        look: Look { kind: Kind::Marker, ..Look::default() },
                    }];
                    item.rest = vec![Span { text: " ".repeat(width), look: Look::default() }];
                }
            }
        }
    }

    fn start(&mut self, tag: Tag, at: usize) {
        match tag {
            Tag::Paragraph => {
                self.flush();
            }
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(level);
                self.push_look(|look| {
                    look.kind = Kind::Heading;
                    look.bold = true;
                });
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.blank_if_gap();
                let bar = format!("{} ", self.marks.quote);
                let span = self.marker(&bar);
                self.containers.push(Container {
                    first: vec![span.clone()],
                    rest: vec![span],
                    started: false,
                });
                self.push_look(|look| look.italic = true);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let line = self.line_of(at);
                self.code = Some((String::new(), line));
                self.blank_if_gap();
                if !language.is_empty() {
                    let label = Span {
                        text: language,
                        look: Look { kind: Kind::Marker, dim: true, ..Look::default() },
                    };
                    self.emit(vec![label], line);
                }
            }
            Tag::HtmlBlock => self.flush(),
            Tag::List(first) => {
                self.flush();
                // A list inside an item is part of that item, not a new block
                // after it: no blank line before it.
                if self.list_depth == 0 {
                    self.blank_if_gap();
                }
                self.list_depth += 1;
                self.numbers.push(first);
            }
            Tag::Item => {
                self.flush();
                let bullet = match self.numbers.last_mut() {
                    Some(Some(number)) => {
                        let text = format!("{number}.");
                        *number += 1;
                        text
                    }
                    _ => self.marks.bullets[(self.list_depth.saturating_sub(1)) % 3].to_string(),
                };
                let width = str_width(&bullet) + 1;
                self.containers.push(Container {
                    first: vec![self.marker(&format!("{bullet} "))],
                    rest: vec![Span { text: " ".repeat(width), look: Look::default() }],
                    started: false,
                });
            }
            Tag::FootnoteDefinition(name) => {
                self.flush();
                self.blank_if_gap();
                let label = format!("[{name}] ");
                let width = str_width(&label);
                self.containers.push(Container {
                    first: vec![Span {
                        text: label,
                        look: Look { kind: Kind::Marker, dim: true, ..Look::default() },
                    }],
                    rest: vec![Span { text: " ".repeat(width), look: Look::default() }],
                    started: false,
                });
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                self.flush();
            }
            Tag::Table(alignments) => {
                self.flush();
                self.blank_if_gap();
                self.table = Some(Table { alignments, rows: Vec::new(), source: self.line_of(at) });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.inline.clear(),
            Tag::Emphasis => self.push_look(|look| look.italic = true),
            Tag::Strong => self.push_look(|look| look.bold = true),
            // No strike-through attribute to draw with, and the next best
            // thing to crossed out is faded.
            Tag::Strikethrough => self.push_look(|look| look.dim = true),
            Tag::Superscript | Tag::Subscript => self.push_look(|_| {}),
            Tag::Link { .. } => self.push_look(|look| {
                look.kind = Kind::Link;
                look.underline = true;
            }),
            Tag::Image { .. } => {
                self.push_look(|look| look.dim = true);
                self.text("[image: ", at);
            }
            Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                self.gap = true;
            }
            TagEnd::Heading(level) => {
                self.pop_look();
                self.heading = None;
                self.blank_if_gap();
                let source = self.inline_source;
                let spans = std::mem::take(&mut self.inline);
                let lines = self.wrap(&spans);
                let widest = lines.iter().map(|line| spans_width(line)).max().unwrap_or(0);
                for line in lines {
                    self.emit(line, source);
                }
                // The two biggest get a rule under them, as they would on a
                // page: a colour alone does not tell a title from a section.
                let under = match level {
                    HeadingLevel::H1 => Some(self.marks.double),
                    HeadingLevel::H2 => Some(self.marks.rule),
                    _ => None,
                };
                if let Some(mark) = under {
                    let rule = mark.repeat(widest.max(1));
                    self.emit(vec![self.marker(&rule)], source);
                }
                self.gap = true;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.pop_look();
                self.containers.pop();
                self.gap = true;
            }
            TagEnd::CodeBlock => {
                let (code, start) = self.code.take().unwrap_or_default();
                let look = Look { kind: Kind::Code, ..Look::default() };
                // A fenced block's first line of code is the line after the
                // fence; an indented one starts where it starts.
                let first = match self.source_text[self.starts[start.min(self.starts.len() - 1)]..]
                    .trim_start()
                    .starts_with(['`', '~'])
                {
                    true => start + 1,
                    false => start,
                };
                for (index, line) in code.trim_end_matches('\n').split('\n').enumerate() {
                    let text = format!("  {}", line.replace('\t', "    "));
                    self.emit(vec![Span { text, look }], first + index);
                }
                self.gap = true;
            }
            TagEnd::HtmlBlock => {
                self.flush();
                self.gap = true;
            }
            TagEnd::List(_) => {
                self.flush();
                self.list_depth = self.list_depth.saturating_sub(1);
                self.numbers.pop();
                if self.list_depth == 0 {
                    self.gap = true;
                }
            }
            TagEnd::Item => {
                // A tight item's words come straight inside it, with no
                // paragraph to end them.
                self.flush();
                self.containers.pop();
            }
            TagEnd::FootnoteDefinition => {
                self.flush();
                self.containers.pop();
                self.gap = true;
            }
            TagEnd::DefinitionList | TagEnd::DefinitionListTitle | TagEnd::DefinitionListDefinition => {
                self.flush();
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.draw_table(table);
                }
                self.gap = true;
            }
            TagEnd::TableHead | TagEnd::TableRow => {}
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(cell);
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::Link => self.pop_look(),
            TagEnd::Image => {
                let look = self.look();
                self.inline.push(Span { text: "]".into(), look });
                self.pop_look();
            }
            TagEnd::MetadataBlock(_) => {}
        }
    }

    /// The columns left for text once the containers have had theirs.
    fn room(&self) -> usize {
        let prefix: usize = self.containers.iter().map(|c| spans_width(&c.rest)).sum();
        self.width.saturating_sub(prefix).max(4)
    }

    /// Draw the words collected so far as a wrapped block.
    fn flush(&mut self) {
        if self.inline.iter().all(|span| span.text.trim().is_empty()) {
            self.inline.clear();
            return;
        }
        self.blank_if_gap();
        let spans = std::mem::take(&mut self.inline);
        let source = self.inline_source;
        for line in self.wrap(&spans) {
            self.emit(line, source);
        }
    }

    fn blank_if_gap(&mut self) {
        if !std::mem::take(&mut self.gap) || self.out.is_empty() {
            return;
        }
        // Inside a quote a blank line still has the bar down it, so a quote of
        // two paragraphs reads as one quote. A list item's blank line is just
        // blank - and does not spend the bullet of an item not yet drawn.
        let mut spans = Vec::new();
        for container in &self.containers {
            spans.extend(container.rest.iter().cloned());
        }
        let source = self.out.last().map_or(0, |line| line.source);
        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
        let spans = match text.trim().is_empty() {
            true => Vec::new(),
            false => spans,
        };
        self.out.push(Line { spans, source });
    }

    /// One line, with whatever the containers it is inside draw in front of it.
    fn emit(&mut self, body: Vec<Span>, source: usize) {
        let mut spans = Vec::new();
        for container in &mut self.containers {
            match container.started {
                true => spans.extend(container.rest.iter().cloned()),
                false => {
                    spans.extend(container.first.iter().cloned());
                    container.started = true;
                }
            }
        }
        spans.extend(body);
        self.out.push(Line { spans, source });
    }

    /// Break styled words into lines of the room there is. A word too long for
    /// a line of its own is cut, since there is nowhere else for it to go.
    fn wrap(&self, spans: &[Span]) -> Vec<Vec<Span>> {
        let room = self.room();
        let mut lines = Vec::new();
        let mut line: Vec<Span> = Vec::new();
        let mut used = 0;
        let mut space: Option<Look> = None;

        for span in spans {
            if span.text == "\n" {
                lines.push(std::mem::take(&mut line));
                used = 0;
                space = None;
                continue;
            }
            for piece in split_words(&span.text) {
                if piece.chars().all(char::is_whitespace) {
                    if used > 0 {
                        space = Some(span.look);
                    }
                    continue;
                }
                let mut word = piece.to_string();
                loop {
                    let width = str_width(&word);
                    let gap = usize::from(space.is_some());
                    if used > 0 && used + gap + width > room {
                        lines.push(std::mem::take(&mut line));
                        used = 0;
                        space = None;
                    }
                    if let Some(look) = space.take() {
                        push_span(&mut line, " ", look);
                        used += 1;
                    }
                    if str_width(&word) <= room {
                        push_span(&mut line, &word, span.look);
                        used += str_width(&word);
                        break;
                    }
                    // Longer than a whole line: as much as fits, and the rest
                    // goes round again onto the next.
                    let (head, tail) = split_at_width(&word, room.saturating_sub(used).max(1));
                    push_span(&mut line, head, span.look);
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                    word = tail.to_string();
                }
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        lines
    }

    fn draw_table(&mut self, table: Table) {
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let mut widths = vec![1; columns];
        for row in &table.rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(spans_width(cell));
            }
        }
        // Too wide for the pane: the widest column gives way first, a column
        // at a time, so a table of one long description and three short
        // numbers keeps its numbers whole.
        let separators = 3 * (columns - 1) + 2;
        while widths.iter().sum::<usize>() + separators > self.room() {
            let widest = (0..columns).max_by_key(|&i| widths[i]).expect("a column");
            if widths[widest] <= 3 {
                break;
            }
            widths[widest] -= 1;
        }

        let column = self.marks.column;
        for (index, row) in table.rows.iter().enumerate() {
            let mut spans = Vec::new();
            for (at, width) in widths.iter().enumerate() {
                if at > 0 {
                    spans.push(self.marker(&format!(" {column} ")));
                } else {
                    spans.push(Span { text: " ".into(), look: Look::default() });
                }
                let empty = Vec::new();
                let cell = row.get(at).unwrap_or(&empty);
                let alignment = table.alignments.get(at).copied().unwrap_or(Alignment::None);
                spans.extend(fit_cell(cell, *width, alignment, index == 0));
            }
            self.emit(spans, table.source + index + usize::from(index > 0));
            // The rule under the header.
            if index == 0 {
                let rule: Vec<String> = widths.iter().map(|w| self.marks.rule.repeat(*w)).collect();
                let joint = format!("{}{}{}", self.marks.rule, self.marks.cross, self.marks.rule);
                let text = format!("{}{}", self.marks.rule, rule.join(&joint));
                self.emit(vec![self.marker(&text)], table.source + 1);
            }
        }
    }
}

/// A cell's spans cut or padded to exactly `width`, aligned as the table's
/// header row asked. The header is bold, as a header is.
fn fit_cell(cell: &[Span], width: usize, alignment: Alignment, header: bool) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut used = 0;
    for span in cell {
        let mut look = span.look;
        look.bold |= header;
        let room = width - used;
        if room == 0 {
            break;
        }
        let text = span.text.replace('\n', " ");
        if str_width(&text) <= room {
            used += str_width(&text);
            spans.push(Span { text, look });
        } else {
            // Cut with an ellipsis, which says the cell went on.
            let (head, _) = split_at_width(&text, room.saturating_sub(1));
            let head = format!("{head}…");
            used += str_width(&head);
            spans.push(Span { text: head, look });
            break;
        }
    }
    let pad = width.saturating_sub(used);
    let (before, after) = match alignment {
        Alignment::Right => (pad, 0),
        Alignment::Center => (pad / 2, pad - pad / 2),
        Alignment::Left | Alignment::None => (0, pad),
    };
    let blank = |n: usize| Span { text: " ".repeat(n), look: Look::default() };
    let mut out = vec![blank(before)];
    out.extend(spans);
    out.push(blank(after));
    out.retain(|span| !span.text.is_empty());
    out
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|span| str_width(&span.text)).sum()
}

/// Add text to a line, joining it onto the last span when that has the same
/// look: a paragraph of plain words should be one span, not two hundred.
fn push_span(line: &mut Vec<Span>, text: &str, look: Look) {
    match line.last_mut() {
        Some(last) if last.look == look => last.text.push_str(text),
        _ => line.push(Span { text: text.to_string(), look }),
    }
}

/// Words and the runs of whitespace between them, both kept, in order.
fn split_words(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (at, c) in text.char_indices() {
        let space = c.is_whitespace();
        match in_space {
            Some(was) if was != space => {
                pieces.push(&text[start..at]);
                start = at;
            }
            _ => {}
        }
        in_space = Some(space);
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

/// The longest start of `text` no wider than `width`, and what is left.
fn split_at_width(text: &str, width: usize) -> (&str, &str) {
    let mut used = 0;
    for (at, cluster) in crate::view::clusters(text) {
        let w = crate::view::cluster_width(cluster, 0);
        if used + w > width {
            let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
            return text.split_at(byte);
        }
        used += w;
    }
    (text, "")
}

/// The preview pane's state: which buffer it shows, and the lines it last
/// rendered, so an unchanged buffer is not parsed again on every frame.
pub struct Preview {
    /// Which buffer, as an index into the editor's views. Pinned to the one it
    /// was opened on, so a jump into a code file from the notes leaves the
    /// notes beside you rather than a pane that has nothing to say.
    pub view: usize,
    /// Where the pane was last scrolled to, for the frames where the cursor
    /// is in some other buffer and there is nothing to follow.
    pub top: std::cell::Cell<usize>,
    cache: RefCell<Option<Cached>>,
}

struct Cached {
    revision: u64,
    width: usize,
    glyphs: bool,
    lines: Rc<Vec<Line>>,
}

impl Preview {
    pub fn new(view: usize) -> Self {
        Preview { view, top: std::cell::Cell::new(0), cache: RefCell::new(None) }
    }

    /// The rendered lines, from the cache when neither the buffer nor the
    /// width has changed since they were made.
    pub fn lines(&self, text: &ropey::Rope, revision: u64, width: usize, glyphs: bool) -> Rc<Vec<Line>> {
        if let Some(cached) = self.cache.borrow().as_ref()
            && cached.revision == revision
            && cached.width == width
            && cached.glyphs == glyphs
        {
            return cached.lines.clone();
        }
        let lines = Rc::new(render(&text.to_string(), width, glyphs));
        *self.cache.borrow_mut() = Some(Cached { revision, width, glyphs, lines: lines.clone() });
        lines
    }

    /// The first rendered line that came from `source` or after it - which is
    /// the line to put beside the cursor. A cursor on a blank line between
    /// two paragraphs gets the second of them.
    pub fn row_for(lines: &[Line], source: usize) -> usize {
        match lines.iter().position(|line| line.source >= source) {
            Some(row) => row,
            None => lines.len().saturating_sub(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(markdown: &str, width: usize) -> Vec<String> {
        render(markdown, width, true).iter().map(Line::text).collect()
    }

    fn find<'a>(lines: &'a [Line], needle: &str) -> &'a Span {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.text.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not in the preview"))
    }

    #[test]
    fn the_markup_goes_and_the_words_stay() {
        let lines = render("Some **bold**, some *slanted*, some `code`.\n", 80, true);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text(), "Some bold, some slanted, some code.");
        assert!(find(&lines, "bold").look.bold);
        assert!(find(&lines, "slanted").look.italic);
        assert_eq!(find(&lines, "code").look.kind, Kind::Code);
    }

    #[test]
    fn a_paragraph_wraps_to_the_pane_and_keeps_its_source_line() {
        let text = "intro\n\none two three four five six seven eight nine ten\n";
        let lines = render(text, 20, true);
        assert!(lines.iter().all(|line| str_width(&line.text()) <= 20), "{lines:#?}");
        // Every line of the wrapped paragraph came from source line 2.
        let wrapped: Vec<&Line> = lines.iter().filter(|l| l.source == 2).collect();
        assert!(wrapped.len() > 1, "it wrapped");
        assert_eq!(lines[1].text(), "", "a blank line between paragraphs");
    }

    #[test]
    fn headings_are_titles_and_the_big_ones_are_underlined() {
        let lines = texts("# Title\n\n## Section\n\n### Smaller\n", 40);
        assert_eq!(lines[0], "Title");
        assert_eq!(lines[1], "═════");
        assert!(lines.contains(&"Section".to_string()));
        assert!(lines.contains(&"───────".to_string()));
        // A third-level heading has no rule: just the title colour.
        let at = lines.iter().position(|l| l == "Smaller").expect("the heading");
        assert!(lines.get(at + 1).is_none_or(|l| !l.contains('─')));

        let styled = render("# Title\n", 40, true);
        assert_eq!(find(&styled, "Title").look.kind, Kind::Heading);
    }

    #[test]
    fn lists_get_bullets_and_a_hanging_indent() {
        let lines = texts("- one\n- two that is long enough to wrap round\n  - nested\n", 20);
        assert_eq!(lines[0], "• one");
        assert!(lines[1].starts_with("• two"));
        // The wrapped part of the item sits under its words, not its bullet.
        assert!(lines[2].starts_with("  "), "{lines:?}");
        assert!(lines.iter().any(|l| l.trim_start().starts_with("◦ nested")), "{lines:?}");

        let numbered = texts("3. three\n4. four\n", 40);
        assert_eq!(numbered, ["3. three", "4. four"]);
    }

    #[test]
    fn tasks_are_boxes() {
        let lines = texts("- [ ] to do\n- [x] done\n", 40);
        assert_eq!(lines, ["☐ to do", "☑ done"]);
    }

    #[test]
    fn a_quote_has_a_bar_down_all_of_it() {
        let lines = texts("> first paragraph\n>\n> second one\n", 40);
        assert!(lines.iter().all(|l| l.starts_with('│')), "{lines:?}");
        let styled = render("> quoted\n", 40, true);
        assert!(find(&styled, "quoted").look.italic);
    }

    #[test]
    fn code_is_left_as_code_with_its_language_named() {
        let text = "```rust\nfn main() {\n    let x = 1;\n}\n```\n";
        let lines = render(text, 12, true);
        let texts: Vec<String> = lines.iter().map(Line::text).collect();
        assert_eq!(texts[0], "rust");
        assert_eq!(texts[1], "  fn main() {");
        // Not wrapped, even past the width: wrapped code is not code.
        assert_eq!(texts[2], "      let x = 1;");
        // And each line of code knows its own line of the source.
        assert_eq!(lines[1].source, 1);
        assert_eq!(lines[2].source, 2);
    }

    #[test]
    fn a_table_is_lined_up() {
        let text = "| name | n |\n|------|--:|\n| jack | 1 |\n| a longer one | 22 |\n";
        let lines = texts(text, 60);
        assert_eq!(lines[0], " name         │  n");
        assert!(lines[1].contains('┼'), "a rule under the header: {lines:?}");
        assert_eq!(lines[2], " jack         │  1");
        assert_eq!(lines[3], " a longer one │ 22");

        let styled = render(text, 60, true);
        assert!(find(&styled, "name").look.bold, "the header is bold");
    }

    #[test]
    fn a_table_too_wide_for_the_pane_gives_way_at_its_widest_column() {
        let text = "| id | description |\n|---|---|\n| 1 | a very long description of a thing |\n";
        let lines = texts(text, 24);
        assert!(lines.iter().all(|l| str_width(l) <= 24), "{lines:?}");
        assert!(lines[2].starts_with(" 1  │"), "the short column is whole: {lines:?}");
        assert!(lines[2].ends_with('…'), "the long one says it was cut: {lines:?}");
    }

    #[test]
    fn links_keep_their_words_and_lose_their_address() {
        let lines = render("see [the docs](https://example.com/a/long/path)\n", 80, true);
        assert_eq!(lines[0].text(), "see the docs");
        let link = find(&lines, "the docs");
        assert_eq!(link.look.kind, Kind::Link);
        assert!(link.look.underline);
    }

    #[test]
    fn noglyphs_draws_the_scaffolding_in_ascii() {
        let text = "# T\n\n- a\n- [x] b\n\n> q\n\n---\n";
        for line in texts_plain(text) {
            assert!(line.is_ascii(), "{line:?}");
        }
    }

    fn texts_plain(markdown: &str) -> Vec<String> {
        render(markdown, 40, false).iter().map(Line::text).collect()
    }

    #[test]
    fn a_word_longer_than_the_pane_is_cut_rather_than_lost() {
        let lines = texts("aaaaaaaaaaaaaaaaaaaaaaaaa\n", 10);
        assert!(lines.iter().all(|l| str_width(l) <= 10), "{lines:?}");
        assert_eq!(lines.concat(), "aaaaaaaaaaaaaaaaaaaaaaaaa");
    }

    #[test]
    fn the_row_beside_the_cursor_is_the_first_from_its_line() {
        let lines = render("# A\n\npara one\n\npara two\n", 40, true);
        let row = Preview::row_for(&lines, 4);
        assert_eq!(lines[row].text(), "para two");
        // A cursor on the blank line between gets the paragraph after it.
        let row = Preview::row_for(&lines, 3);
        assert_eq!(lines[row].text(), "para two");
    }

    #[test]
    fn the_cache_is_used_until_the_buffer_or_the_width_changes() {
        let preview = Preview::new(0);
        let rope = ropey::Rope::from_str("# hi\n");
        let a = preview.lines(&rope, 1, 40, true);
        let b = preview.lines(&rope, 1, 40, true);
        assert!(Rc::ptr_eq(&a, &b), "the same render, not a second one");
        let c = preview.lines(&rope, 2, 40, true);
        assert!(!Rc::ptr_eq(&a, &c), "an edit renders again");
        let d = preview.lines(&rope, 2, 30, true);
        assert!(!Rc::ptr_eq(&c, &d), "and so does a resize");
    }
}
