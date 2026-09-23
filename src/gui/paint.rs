//! The cell grid, painted into pixels.
//!
//! The same `Surface` the terminal frontend sends as escape codes, drawn with
//! a font instead. Everything here is about keeping the grid a grid: each cell
//! is shaped on its own and drawn at its own column, so a Nerd Font glyph in
//! the status line or an emoji in a comment cannot push the rest of the line
//! sideways the way it would if a row were shaped as one piece of text.
//!
//! Shaping a cell is the expensive part, so it is done once per distinct
//! cluster and style and kept: after a second of typing, painting a frame is
//! blending bytes.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, Style as Slant,
    SwashCache, Weight,
};

use crate::screen::{Style, Surface};

use super::colors::{self, Rgb};

/// What the window draws with, and the pixel sizes that follow from it.
pub struct Painter {
    fonts: FontSystem,
    swash: SwashCache,
    /// One buffer, re-used for every cluster there is to shape.
    scratch: Buffer,
    glyphs: HashMap<Key, Glyph>,
    family: String,
    /// Whether that family is one the machine actually has. A shaper answers
    /// a name it does not know by quietly using something else, which is how
    /// a misspelt `guifont` becomes a proportional font and a line of boxes.
    matched: bool,
    /// Which family covers a character the chosen font has no glyph for,
    /// worked out once per character. `None` means nothing on the machine
    /// has it, and a box is the honest answer.
    covering: HashMap<char, Option<String>>,
    size: f32,
    /// One cell, in pixels: the advance of the font's `M` and its line height.
    pub cell: (usize, usize),
    pub fg: Rgb,
    pub bg: Rgb,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    cluster: String,
    bold: bool,
    italic: bool,
}

/// A rasterised cluster: the pixels it covers, relative to the top left of
/// its cell. Sparse, because a glyph covers a third of its box at most and
/// this is walked, never indexed.
#[derive(Default)]
struct Glyph {
    dots: Vec<Dot>,
}

struct Dot {
    x: i32,
    y: i32,
    alpha: u8,
    /// The pixel's own colour, for a glyph that has one - an emoji. A plain
    /// glyph is a coverage mask and takes the text colour instead.
    color: Option<Rgb>,
}

/// How tall a line is against the font size. Terminals sit around here, and
/// it is the difference between text that reads and text that crowds.
const LINE_HEIGHT: f32 = 1.25;

impl Painter {
    /// A painter at `size` pixels in `family`. The family is a name as
    /// fontconfig knows it - "JetBrainsMono Nerd Font" - or `monospace` for
    /// whatever the system calls its default.
    pub fn new(family: &str, size: f32) -> Painter {
        let family = resolve(family);
        let mut fonts = FontSystem::new();
        let matched = installed(&fonts, &family);
        let metrics = Metrics::new(size, (size * LINE_HEIGHT).round());
        let mut scratch = Buffer::new(&mut fonts, metrics);
        // Room for one cluster and no wrapping: a cell is a cell.
        scratch.set_size(&mut fonts, Some(size * 8.0), Some(metrics.line_height));
        let mut painter = Painter {
            fonts,
            swash: SwashCache::new(),
            scratch,
            glyphs: HashMap::new(),
            family,
            matched,
            covering: HashMap::new(),
            size,
            cell: (1, 1),
            fg: colors::DEFAULT_FG,
            bg: colors::DEFAULT_BG,
        };
        painter.cell = painter.measure();
        painter
    }

    /// The size of a cell, from the font itself: the advance of a character
    /// every monospace font has, and the line height asked for.
    fn measure(&mut self) -> (usize, usize) {
        let attrs = attrs(&self.family, false, false);
        self.scratch.set_text(&mut self.fonts, "M", &attrs, Shaping::Advanced);
        self.scratch.shape_until_scroll(&mut self.fonts, false);
        let advance = self
            .scratch
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|glyph| glyph.w))
            .unwrap_or(self.size * 0.6);
        let height = self.scratch.metrics().line_height;
        (advance.ceil().max(1.0) as usize, height.ceil().max(1.0) as usize)
    }

    /// Grow or shrink the text, as `ctrl` and the wheel or `+`/`-` ask. The
    /// cache goes with it: every glyph in it was rasterised at the old size.
    pub fn set_size(&mut self, size: f32) {
        self.size = size.clamp(6.0, 72.0);
        let metrics = Metrics::new(self.size, (self.size * LINE_HEIGHT).round());
        self.scratch.set_metrics(&mut self.fonts, metrics);
        self.scratch.set_size(&mut self.fonts, Some(self.size * 8.0), Some(metrics.line_height));
        self.glyphs.clear();
        self.cell = self.measure();
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    /// The font being drawn with, and whether it is the one that was asked
    /// for. A window says so when it is not, rather than letting you wonder
    /// why the glyphs went missing.
    pub fn matched(&self) -> bool {
        self.matched
    }

    /// Every font family on the machine, for `:guifonts` to offer. Sorted,
    /// and without the duplicates a family with six weights would bring.
    pub fn families() -> Vec<String> {
        let fonts = FontSystem::new();
        let mut families: Vec<String> = fonts
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        families.sort_by_key(|name| name.to_lowercase());
        families.dedup();
        families
    }

    /// How many cells fit in a window of this many pixels. At least one of
    /// each: a window dragged to nothing still has an editor in it.
    pub fn grid(&self, width: usize, height: usize) -> (usize, usize) {
        ((width / self.cell.0).max(1), (height / self.cell.1).max(1))
    }

    /// Paint a whole frame. `cursor` is in cells, and `bar` says which shape
    /// it takes - a bar in insert mode, a block everywhere else, as the
    /// terminal frontend asks the terminal for.
    pub fn paint(
        &mut self,
        surface: &Surface,
        pixels: &mut [u32],
        size: (usize, usize),
        cursor: (usize, usize),
        bar: bool,
    ) {
        let (width, height) = size;
        // The window is rarely a whole number of cells; what is left over at
        // the right and the bottom is background.
        pixels.fill(self.bg.pixel());

        let (cols, rows) = surface.size();
        for row in 0..rows {
            for column in 0..cols {
                let cell = surface.get(column, row);
                let at_cursor = (column, row) == cursor && !bar;
                let (fg, bg) = self.colors(cell.style, at_cursor);
                let origin = (column * self.cell.0, row * self.cell.1);
                self.fill(pixels, size, origin, self.cell, bg);

                // A continuation cell is the right half of a wide glyph: its
                // background belongs to it, its character does not.
                let mut text = String::new();
                if cell.ch != '\0' {
                    text.push(cell.ch);
                    text.push_str(cell.tail.as_str());
                }
                if !text.trim().is_empty() {
                    self.draw_cluster(pixels, size, origin, &text, fg, bg, cell.style);
                }
                if cell.style.underline {
                    let y = origin.1 + self.cell.1.saturating_sub(2);
                    self.fill(pixels, size, (origin.0, y), (self.cell.0, 1), fg);
                }
            }
        }

        if bar {
            // Two pixels down the left of the cell, which is where a terminal
            // puts it and what an insert cursor has always looked like.
            let origin = (cursor.0 * self.cell.0, cursor.1 * self.cell.1);
            let bar_width = (self.cell.0 / 6).max(2);
            self.fill(pixels, (width, height), origin, (bar_width, self.cell.1), self.fg);
        }
    }

    /// A cell's two colours, with `reverse` and `dim` applied - and with the
    /// cursor counting as one more reverse, which is how a block cursor shows
    /// the character it is sitting on rather than hiding it.
    fn colors(&self, style: Style, at_cursor: bool) -> (Rgb, Rgb) {
        let mut fg = colors::rgb(style.fg, self.fg);
        let mut bg = colors::rgb(style.bg, self.bg);
        if style.dim {
            fg = fg.dimmed(bg);
        }
        if style.reverse != at_cursor {
            std::mem::swap(&mut fg, &mut bg);
        }
        (fg, bg)
    }

    fn fill(&self, pixels: &mut [u32], size: (usize, usize), at: (usize, usize), of: (usize, usize), color: Rgb) {
        let (width, height) = size;
        let pixel = color.pixel();
        for y in at.1..(at.1 + of.1).min(height) {
            let row = y * width;
            for x in at.0..(at.0 + of.0).min(width) {
                pixels[row + x] = pixel;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cluster(
        &mut self,
        pixels: &mut [u32],
        size: (usize, usize),
        origin: (usize, usize),
        cluster: &str,
        fg: Rgb,
        bg: Rgb,
        style: Style,
    ) {
        let key = Key { cluster: cluster.to_string(), bold: style.bold, italic: style.italic };
        if !self.glyphs.contains_key(&key) {
            let glyph = self.rasterise(cluster, style.bold, style.italic);
            self.glyphs.insert(key.clone(), glyph);
        }
        let glyph = &self.glyphs[&key];
        let (width, height) = size;
        for dot in &glyph.dots {
            let x = origin.0 as i32 + dot.x;
            let y = origin.1 as i32 + dot.y;
            if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
                continue;
            }
            let at = y as usize * width + x as usize;
            // What is already there, not the cell's background: a glyph that
            // overhangs its cell blends with whatever it overhangs.
            let under = pixel_rgb(pixels[at]);
            let color = dot.color.unwrap_or(fg);
            let _ = bg;
            pixels[at] = color.over(under, dot.alpha).pixel();
        }
    }

    /// Shape and rasterise one cluster, once. The result is in pixels
    /// relative to the top left of the cell it will be drawn in.
    fn rasterise(&mut self, cluster: &str, bold: bool, italic: bool) -> Glyph {
        let mut family = self.family.clone();
        self.shape(&family, cluster, bold, italic);
        // Nothing in the font and nothing the shaper's own fallback list
        // reached: glyph zero, which is the box. Before drawing that, ask
        // every font on the machine whether it has the character - which is
        // how a Nerd Font glyph gets drawn in a window whose text font has
        // never heard of it.
        if self.missing() && let Some(character) = cluster.chars().next()
            && let Some(other) = self.covering(character)
        {
            family = other;
            self.shape(&family, cluster, bold, italic);
        }

        let mut glyph = Glyph::default();
        // White, so that a mask glyph comes back as coverage and a colour
        // glyph as itself: the two are told apart by what arrives.
        let base = TextColor::rgba(255, 255, 255, 255);
        let _ = &family;
        self.scratch.draw(&mut self.fonts, &mut self.swash, base, |x, y, w, h, color| {
            let (r, g, b, a) = (color.r(), color.g(), color.b(), color.a());
            if a == 0 {
                return;
            }
            let own = ((r, g, b) != (255, 255, 255)).then_some(Rgb(r, g, b));
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    glyph.dots.push(Dot { x: x + dx, y: y + dy, alpha: a, color: own });
                }
            }
        });
        glyph
    }
}

impl Painter {
    /// Shape one cluster into the scratch buffer.
    fn shape(&mut self, family: &str, cluster: &str, bold: bool, italic: bool) {
        let attrs = attrs(family, bold, italic);
        self.scratch.set_text(&mut self.fonts, cluster, &attrs, Shaping::Advanced);
        self.scratch.shape_until_scroll(&mut self.fonts, false);
    }

    /// Whether what was just shaped came out as the missing-glyph box.
    fn missing(&self) -> bool {
        self.scratch
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|glyph| glyph.glyph_id == 0))
            .unwrap_or(false)
    }

    /// The first family on the machine with a glyph for `character`, asked
    /// once per character and remembered. Monospace fonts first, so a status
    /// line's icons come from a terminal font rather than a decorative one.
    fn covering(&mut self, character: char) -> Option<String> {
        if let Some(known) = self.covering.get(&character) {
            return known.clone();
        }
        let mut faces: Vec<(cosmic_text::fontdb::ID, bool)> = self
            .fonts
            .db()
            .faces()
            .map(|face| (face.id, face.monospaced))
            .collect();
        faces.sort_by_key(|(_, monospaced)| !monospaced);

        let mut found = None;
        for (id, _) in faces {
            let Some(font) = self.fonts.get_font(id) else {
                continue;
            };
            if font.as_swash().charmap().map(character) != 0 {
                found = self
                    .fonts
                    .db()
                    .face(id)
                    .and_then(|face| face.families.first().map(|(name, _)| name.clone()));
                break;
            }
        }
        self.covering.insert(character, found.clone());
        found
    }
}

/// Whether the machine has a family by this name. Matched the way a person
/// would: case and spaces are not the difference between two fonts.
fn installed(fonts: &FontSystem, family: &str) -> bool {
    if matches!(family, "monospace" | "") {
        // Not a family but a request for whatever is default, which always
        // resolves to something.
        return true;
    }
    let wanted = simplified(family);
    fonts
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(name, _)| simplified(name) == wanted))
}

fn simplified(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()
}

/// What `monospace` means on this machine.
///
/// Not what the font stack thinks: what fontconfig has been *told*, which is
/// the font the terminal jack usually runs in is using. Ask it, and a window
/// opened with no font configured comes up in the font the rest of the
/// desktop is already in - Nerd Font glyphs in the status line and all.
/// Anywhere without fontconfig, the shaper's own default stands.
fn resolve(family: &str) -> String {
    if !matches!(family, "monospace" | "") {
        return family.to_string();
    }
    let asked = std::process::Command::new("fc-match")
        .args(["--format=%{family[0]}", "monospace"])
        .output();
    match asked {
        Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
            Ok(name) if !name.trim().is_empty() => name.trim().to_string(),
            _ => "monospace".to_string(),
        },
        _ => "monospace".to_string(),
    }
}

/// How a cluster is to be shaped: the configured family, and the weight and
/// slant the cell's style asks for. A font without a bold or an italic face
/// gets a synthesised one, which is what a terminal does too.
fn attrs<'a>(family: &'a str, bold: bool, italic: bool) -> Attrs<'a> {
    let family = match family {
        "monospace" | "" => Family::Monospace,
        name => Family::Name(name),
    };
    let mut attrs = Attrs::new().family(family);
    if bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if italic {
        attrs = attrs.style(Slant::Italic);
    }
    attrs
}

fn pixel_rgb(pixel: u32) -> Rgb {
    Rgb((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A painter over whatever monospace font this machine has. Font files
    /// are the one thing a test here cannot ship.
    fn painter() -> Painter {
        Painter::new("monospace", 16.0)
    }

    fn painted(surface: &Surface, painter: &mut Painter, cursor: (usize, usize), bar: bool) -> (Vec<u32>, (usize, usize)) {
        let (cols, rows) = surface.size();
        let size = (cols * painter.cell.0, rows * painter.cell.1);
        let mut pixels = vec![0; size.0 * size.1];
        painter.paint(surface, &mut pixels, size, cursor, bar);
        (pixels, size)
    }

    #[test]
    fn a_font_the_machine_does_not_have_is_said_so_rather_than_swapped() {
        let painter = Painter::new("No Such Font At All", 16.0);
        assert!(!painter.matched(), "a name nothing answers to");

        // `monospace` is not a family but a request, and always resolves.
        assert!(Painter::new("monospace", 16.0).matched());

        // And a real one is matched however it is spelt and cased: a font is
        // not two fonts because someone typed a space.
        let known = Painter::families().into_iter().next().expect("some font");
        assert!(Painter::new(&known, 16.0).matched(), "{known:?}");
        assert!(Painter::new(&known.to_lowercase().replace(' ', ""), 16.0).matched(), "{known:?}");
    }

    #[test]
    fn the_machines_fonts_are_a_list_to_choose_from() {
        let families = Painter::families();
        assert!(!families.is_empty(), "a machine with a display has fonts");
        let mut sorted = families.clone();
        sorted.sort_by_key(|name| name.to_lowercase());
        assert_eq!(families, sorted, "sorted, for a picker to show");
        let mut once = families.clone();
        once.dedup();
        assert_eq!(families, once, "a family with six weights is one entry");
    }

    #[test]
    fn a_glyph_the_font_lacks_comes_from_a_font_that_has_it() {
        // A plain text font, and a character no text font has: the powerline
        // separator and the dog are both in this private-use range, which is
        // what a Nerd Font is for.
        let mut painter = Painter::new("Liberation Mono", 16.0);
        let dog = painter.rasterise("\u{f0a44}", false, false);
        let missing = painter.covering('\u{f0a44}');
        if missing.is_none() {
            // Nothing on this machine has it; a box is then the honest answer
            // and there is nothing to assert.
            return;
        }
        assert!(dog.dots.len() > 20, "it is drawn: {} pixels", dog.dots.len());
        // And the answer is remembered rather than worked out per frame.
        assert!(painter.covering.contains_key(&'\u{f0a44}'));
    }

    #[test]
    fn a_cell_is_a_whole_number_of_pixels_and_a_window_holds_what_fits() {
        let painter = painter();
        assert!(painter.cell.0 >= 4 && painter.cell.1 >= 8, "a cell of {:?}", painter.cell);
        let (cols, rows) = painter.grid(painter.cell.0 * 80 + 3, painter.cell.1 * 24 + 7);
        assert_eq!((cols, rows), (80, 24), "the leftover is not a column");
        assert_eq!(painter.grid(1, 1), (1, 1), "a window too small still has one cell");
    }

    #[test]
    fn text_is_drawn_and_blank_cells_are_left_as_background() {
        let mut painter = painter();
        let mut surface = Surface::new(4, 1);
        surface.put(0, 0, 'W', 1, Style::default());
        let (pixels, size) = painted(&surface, &mut painter, (9, 9), false);

        let ink = |column: usize| {
            let (x0, x1) = (column * painter.cell.0, (column + 1) * painter.cell.0);
            (0..size.1)
                .flat_map(|y| (x0..x1).map(move |x| (x, y)))
                .filter(|&(x, y)| pixels[y * size.0 + x] != painter.bg.pixel())
                .count()
        };
        assert!(ink(0) > 10, "the W is drawn: {} pixels", ink(0));
        assert_eq!(ink(2), 0, "and an empty cell is background");
    }

    #[test]
    fn the_theme_decides_the_colours_and_reverse_swaps_them() {
        let mut painter = painter();
        let mut surface = Surface::new(2, 1);
        let red = crossterm::style::Color::Rgb { r: 255, g: 0, b: 0 };
        let style = Style { bg: Some(red), ..Style::default() };
        surface.put(0, 0, ' ', 1, style);
        surface.put(1, 0, ' ', 1, Style { reverse: true, ..style });
        let (pixels, _) = painted(&surface, &mut painter, (9, 9), false);

        assert_eq!(pixels[0], Rgb(255, 0, 0).pixel(), "the background the theme asked for");
        // Reversed, the background becomes the text colour, which nothing is
        // drawn in on a blank cell - so the cell takes the foreground.
        assert_eq!(pixels[painter.cell.0], painter.fg.pixel());
    }

    #[test]
    fn the_cursor_is_a_block_that_shows_what_it_sits_on_and_a_bar_that_does_not() {
        let mut painter = painter();
        let mut surface = Surface::new(2, 1);
        surface.put(0, 0, 'M', 1, Style::default());
        surface.put(1, 0, 'M', 1, Style::default());

        let (block, size) = painted(&surface, &mut painter, (0, 0), false);
        let corner = block[0];
        assert_eq!(corner, painter.fg.pixel(), "the block is filled with the text colour");
        // The glyph is drawn through the block in the cell's colour, so most
        // of the cell is the cursor and some of it is not.
        let drawn = (0..size.1)
            .flat_map(|y| (0..painter.cell.0).map(move |x| (x, y)))
            .filter(|&(x, y)| block[y * size.0 + x] != painter.fg.pixel())
            .count();
        assert!(drawn > 10, "the character shows through: {drawn} pixels of it");

        // A bar leaves the cell alone but for a stripe down its left.
        let (bar, _) = painted(&surface, &mut painter, (0, 0), true);
        assert_eq!(bar[0], painter.fg.pixel(), "the stripe");
        assert_eq!(bar[painter.cell.0 - 1], painter.bg.pixel(), "the rest of the cell is not filled");
    }

    #[test]
    fn a_wide_glyph_keeps_the_grid_and_takes_its_second_cell() {
        let mut painter = painter();
        let mut surface = Surface::new(4, 1);
        // Two cells wide, as `cluster_width` measures it, so the surface puts
        // a continuation cell beside it.
        let blue = crossterm::style::Color::Rgb { r: 0, g: 0, b: 255 };
        surface.put(0, 0, '界', 2, Style { bg: Some(blue), ..Style::default() });
        let (pixels, size) = painted(&surface, &mut painter, (9, 9), false);

        let blue = Rgb(0, 0, 255).pixel();
        assert_eq!(pixels[painter.cell.0 * 2 - 1], blue, "the second cell is its background too");
        assert_eq!(pixels[painter.cell.0 * 2], painter.bg.pixel(), "and the third is not");
        assert!(size.0 == painter.cell.0 * 4);
    }

    #[test]
    fn the_size_changes_and_the_glyphs_that_were_cached_go_with_it() {
        let mut painter = painter();
        let small = painter.cell;
        painter.set_size(painter.size() * 2.0);
        assert!(painter.cell.0 > small.0 && painter.cell.1 > small.1, "{small:?} -> {:?}", painter.cell);
        assert!(painter.glyphs.is_empty(), "nothing rasterised at the old size survives");
        // And it stays sane at the ends.
        painter.set_size(1.0);
        assert!(painter.cell.0 >= 1);
    }
}
