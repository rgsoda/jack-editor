use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{
    Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{Clear, ClearType};
use std::io::Write;

/// Marks the columns covered by the tail of a double-width character. Never
/// drawn on its own; the lead cell paints the whole span.
const CONTINUATION: char = '\0';

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub reverse: bool,
}

impl Style {
    /// Overlay `other` on top of `self`: set colors and enabled attributes win,
    /// unset ones fall through. This is how a selection tints a cell without
    /// discarding its syntax color.
    pub fn patch(self, other: Style) -> Style {
        Style {
            fg: other.fg.or(self.fg),
            bg: other.bg.or(self.bg),
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            underline: self.underline || other.underline,
            dim: self.dim || other.dim,
            reverse: self.reverse || other.reverse,
        }
    }
}

/// How many bytes of a grapheme cluster a cell keeps after its first
/// character. Enough for a family emoji; a cluster longer than this is cut,
/// and no terminal was going to draw that as one glyph anyway.
const TAIL: usize = 28;

/// The rest of a grapheme cluster after its first character: combining marks,
/// a zero-width joiner and what it joins, a variation selector. Held inline
/// rather than in a `String` so a cell stays `Copy` and a screen stays one
/// flat array.
#[derive(Clone, Copy)]
pub struct Tail {
    bytes: [u8; TAIL],
    len: u8,
}

const NO_TAIL: Tail = Tail { bytes: [0; TAIL], len: 0 };

impl Tail {
    /// The bytes of `rest`, as many as fit on a character boundary.
    fn of(rest: &str) -> Tail {
        let mut end = rest.len().min(TAIL);
        while end > 0 && !rest.is_char_boundary(end) {
            end -= 1;
        }
        let mut bytes = [0; TAIL];
        bytes[..end].copy_from_slice(&rest.as_bytes()[..end]);
        Tail { bytes, len: end as u8 }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_str(&self) -> &str {
        // Only ever written from a `&str` on a character boundary.
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or_default()
    }
}

impl PartialEq for Tail {
    fn eq(&self, other: &Tail) -> bool {
        self.as_str() == other.as_str()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Cell {
    /// The first character of what is drawn here, which for all but a handful
    /// of cells is the whole of it.
    pub ch: char,
    pub tail: Tail,
    pub style: Style,
}

const BLANK: Cell = Cell {
    ch: ' ',
    tail: NO_TAIL,
    style: Style {
        fg: None,
        bg: None,
        bold: false,
        italic: false,
        underline: false,
        dim: false,
        reverse: false,
    },
};

/// A grid of cells: what the terminal should show.
pub struct Surface {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl Surface {
    pub fn new(width: usize, height: usize) -> Self {
        Surface { width, height, cells: vec![BLANK; width * height] }
    }

    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn clear(&mut self) {
        self.cells.fill(BLANK);
    }

    fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells = vec![BLANK; width * height];
    }

    pub fn get(&self, x: usize, y: usize) -> Cell {
        self.cells[y * self.width + x]
    }

    /// Paint `ch` at (x, y) across `width` columns. Off-surface writes and
    /// zero-width characters are dropped.
    pub fn put(&mut self, x: usize, y: usize, ch: char, width: usize, style: Style) {
        self.write(x, y, ch, NO_TAIL, width, style);
    }

    /// One grapheme cluster in one cell: the character, and whatever rides
    /// along with it. `e` and a combining acute are one cell, not two.
    pub fn put_cluster(&mut self, x: usize, y: usize, cluster: &str, width: usize, style: Style) {
        let mut chars = cluster.chars();
        let Some(ch) = chars.next() else {
            return;
        };
        self.write(x, y, ch, Tail::of(chars.as_str()), width, style);
    }

    fn write(&mut self, x: usize, y: usize, ch: char, tail: Tail, width: usize, style: Style) {
        if width == 0 || x >= self.width || y >= self.height {
            return;
        }
        self.cells[y * self.width + x] = Cell { ch, tail, style };
        for column in x + 1..(x + width).min(self.width) {
            self.cells[y * self.width + column] = Cell { ch: CONTINUATION, tail: NO_TAIL, style };
        }
    }
}

/// Double-buffered terminal output: draw a whole frame into the back surface,
/// then send only the cells that differ from what the terminal already shows.
pub struct Screen {
    front: Surface,
    back: Surface,
    needs_clear: bool,
}

impl Screen {
    pub fn new() -> Self {
        Screen { front: Surface::new(0, 0), back: Surface::new(0, 0), needs_clear: true }
    }

    /// Start a frame and hand back the surface to draw it on.
    pub fn begin(&mut self, width: usize, height: usize) -> &mut Surface {
        if self.back.size() != (width, height) {
            self.front.resize(width, height);
            self.back.resize(width, height);
            // The terminal's contents after a resize are anyone's guess.
            self.needs_clear = true;
        }
        self.back.clear();
        &mut self.back
    }

    /// Send the frame and leave the cursor at `cursor`.
    pub fn present(&mut self, out: &mut impl Write, cursor: (u16, u16)) -> Result<()> {
        queue!(out, Hide)?;
        if self.needs_clear {
            // Clearing makes the terminal match a blank front surface, so the
            // diff below still only writes the cells that have content.
            queue!(out, Clear(ClearType::All))?;
            self.front.clear();
            self.needs_clear = false;
        }

        self.diff(out)?;

        queue!(out, ResetColor, MoveTo(cursor.0, cursor.1), Show)?;
        std::mem::swap(&mut self.front, &mut self.back);
        Ok(())
    }

    fn diff(&mut self, out: &mut impl Write) -> Result<()> {
        let (width, height) = self.back.size();
        // What the terminal is currently set to, so we only emit changes.
        let mut style: Option<Style> = None;
        let mut at: Option<(usize, usize)> = None;

        for y in 0..height {
            let mut x = 0;
            while x < width {
                if self.back.get(x, y).ch == CONTINUATION {
                    x += 1;
                    continue;
                }

                // A wide character is redrawn as a unit, so a change in either
                // of its columns repaints the whole thing.
                let mut span = 1;
                while x + span < width && self.back.get(x + span, y).ch == CONTINUATION {
                    span += 1;
                }
                let changed =
                    (0..span).any(|k| self.front.get(x + k, y) != self.back.get(x + k, y));

                if changed {
                    let cell = self.back.get(x, y);
                    if at != Some((x, y)) {
                        queue!(out, MoveTo(x as u16, y as u16))?;
                    }
                    if style != Some(cell.style) {
                        apply_style(out, cell.style)?;
                        style = Some(cell.style);
                    }
                    queue!(out, Print(cell.ch))?;
                    if !cell.tail.is_empty() {
                        queue!(out, Print(cell.tail.as_str()))?;
                    }
                    // Terminals defer the wrap after the last column, so the
                    // cursor's position there is not something we can predict.
                    at = if x + span >= width { None } else { Some((x + span, y)) };
                }

                x += span;
            }
        }

        Ok(())
    }
}

fn apply_style(out: &mut impl Write, style: Style) -> Result<()> {
    // A full SGR reset first, so we never have to track what to switch off.
    queue!(out, ResetColor)?;
    if let Some(fg) = style.fg {
        queue!(out, SetForegroundColor(fg))?;
    }
    if let Some(bg) = style.bg {
        queue!(out, SetBackgroundColor(bg))?;
    }
    for (enabled, attribute) in [
        (style.bold, Attribute::Bold),
        (style.italic, Attribute::Italic),
        (style.underline, Attribute::Underlined),
        (style.dim, Attribute::Dim),
        (style.reverse, Attribute::Reverse),
    ] {
        if enabled {
            queue!(out, SetAttribute(attribute))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw a frame and return the text it actually sent, with the escape
    /// sequences stripped out.
    fn frame(screen: &mut Screen, width: usize, height: usize, draw: impl FnOnce(&mut Surface)) -> String {
        draw(screen.begin(width, height));
        let mut out = Vec::new();
        screen.present(&mut out, (0, 0)).unwrap();
        visible(&out)
    }

    fn visible(bytes: &[u8]) -> String {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch != '\x1b' {
                out.push(ch);
                continue;
            }
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        }
        out
    }

    fn write(surface: &mut Surface, x: usize, y: usize, text: &str, style: Style) {
        for (i, ch) in text.chars().enumerate() {
            surface.put(x + i, y, ch, 1, style);
        }
    }

    #[test]
    fn a_glyph_made_of_several_characters_goes_out_whole() {
        let mut screen = Screen::new();
        let coder = "\u{1f469}\u{200d}\u{1f4bb}";
        let sent = frame(&mut screen, 4, 1, |s| {
            s.put_cluster(0, 0, coder, 2, Style::default());
            s.put_cluster(2, 0, "e\u{301}", 1, Style::default());
        });
        assert_eq!(sent, format!("{coder}{}", "e\u{301}"));
        // The cell knows the whole cluster, not just its first character.
        let surface = screen.begin(4, 1);
        surface.put_cluster(0, 0, coder, 2, Style::default());
        assert_eq!(surface.get(0, 0).ch, '\u{1f469}');
        assert_eq!(surface.get(0, 0).tail.as_str(), "\u{200d}\u{1f4bb}");
        assert_eq!(surface.get(1, 0).ch, CONTINUATION, "and it covers both its columns");
    }

    #[test]
    fn a_glyph_that_changed_is_sent_again_although_it_starts_the_same() {
        // Two clusters that share a first character: the cell has to compare
        // the whole of what it holds, or the second frame sends nothing.
        let mut screen = Screen::new();
        frame(&mut screen, 2, 1, |s| s.put_cluster(0, 0, "e\u{301}", 1, Style::default()));
        let next = frame(&mut screen, 2, 1, |s| s.put_cluster(0, 0, "e\u{308}", 1, Style::default()));
        assert_eq!(next, "e\u{308}");
    }

    #[test]
    fn a_frame_identical_to_the_last_one_sends_nothing() {
        let mut screen = Screen::new();
        let first = frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hello", Style::default()));
        assert_eq!(first, "hello");

        let second = frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hello", Style::default()));
        assert_eq!(second, "");
    }

    #[test]
    fn only_the_changed_cells_are_sent() {
        let mut screen = Screen::new();
        frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hello", Style::default()));

        let next = frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hellp", Style::default()));
        assert_eq!(next, "p");
    }

    #[test]
    fn a_style_change_alone_counts_as_a_change() {
        let mut screen = Screen::new();
        frame(&mut screen, 3, 1, |s| write(s, 0, 0, "abc", Style::default()));

        let red = Style { fg: Some(Color::Red), ..Style::default() };
        let next = frame(&mut screen, 3, 1, |s| {
            write(s, 0, 0, "a", Style::default());
            write(s, 1, 0, "b", red);
            write(s, 2, 0, "c", Style::default());
        });
        assert_eq!(next, "b");
    }

    #[test]
    fn a_wide_character_is_repainted_as_a_unit() {
        let mut screen = Screen::new();
        // A wide char covers columns 0 and 1.
        frame(&mut screen, 4, 1, |s| {
            s.put(0, 0, '日', 2, Style::default());
            write(s, 2, 0, "ab", Style::default());
        });

        // Overwriting only its second column still has to redraw the character,
        // or the terminal is left with half a glyph.
        let next = frame(&mut screen, 4, 1, |s| {
            write(s, 0, 0, "xy", Style::default());
            write(s, 2, 0, "ab", Style::default());
        });
        assert_eq!(next, "xy");

        let back = frame(&mut screen, 4, 1, |s| {
            s.put(0, 0, '日', 2, Style::default());
            write(s, 2, 0, "ab", Style::default());
        });
        assert_eq!(back, "日");
    }

    #[test]
    fn a_resize_repaints_the_whole_screen() {
        let mut screen = Screen::new();
        frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hello", Style::default()));

        let resized = frame(&mut screen, 5, 2, |s| {
            write(s, 0, 0, "hello", Style::default());
            write(s, 0, 1, "world", Style::default());
        });
        assert_eq!(resized, "helloworld");
    }

    #[test]
    fn cells_that_became_blank_are_erased() {
        let mut screen = Screen::new();
        frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hello", Style::default()));

        // Nothing clears the line for us, so the diff must send the spaces
        // itself. The leading "h" is unchanged; "i" replaces "e".
        let next = frame(&mut screen, 5, 1, |s| write(s, 0, 0, "hi", Style::default()));
        assert_eq!(next, "i   ");
    }

    #[test]
    fn patching_overlays_set_fields_and_keeps_the_rest() {
        let syntax = Style { fg: Some(Color::Green), italic: true, ..Style::default() };
        let selection = Style { bg: Some(Color::Blue), ..Style::default() };

        let merged = syntax.patch(selection);
        // The selection tints the cell without losing its syntax color.
        assert_eq!(merged.fg, Some(Color::Green));
        assert_eq!(merged.bg, Some(Color::Blue));
        assert!(merged.italic);
    }

    #[test]
    fn patching_lets_the_overlay_win_on_a_conflict() {
        let a = Style { fg: Some(Color::Green), ..Style::default() };
        let b = Style { fg: Some(Color::Red), ..Style::default() };
        assert_eq!(a.patch(b).fg, Some(Color::Red));
    }
}
