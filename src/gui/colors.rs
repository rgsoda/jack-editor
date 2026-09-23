//! Terminal colours, in a window that has no terminal to ask.
//!
//! A theme may say `"blue"`, or `"117"`, or `"#8be9fd"`. Only the last of
//! those is a colour: the other two are questions the terminal emulator
//! answers out of its own palette, and there isn't one here. So the window
//! ships a palette - the sixteen names and the 256 indices - and a theme that
//! spells its colours out gets exactly what it asked for either way.

use crossterm::style::Color;

/// What text and its background are when the theme says nothing, which for
/// ordinary text it does: in a terminal that means "whatever the terminal is
/// set to". Dracula's own two, since the theme jack ships is that palette.
pub const DEFAULT_FG: Rgb = Rgb(0xf8, 0xf8, 0xf2);
pub const DEFAULT_BG: Rgb = Rgb(0x28, 0x2a, 0x36);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// The pixel a software framebuffer wants: `0RGB`, which is what both
    /// softbuffer and every windowing system underneath it use.
    pub fn pixel(self) -> u32 {
        ((self.0 as u32) << 16) | ((self.1 as u32) << 8) | self.2 as u32
    }

    /// `self` over `under` at `alpha`. Glyph coverage is anti-aliasing, so
    /// every edge pixel of every letter goes through here.
    pub fn over(self, under: Rgb, alpha: u8) -> Rgb {
        let mix = |top: u8, bottom: u8| {
            let top = top as u32 * alpha as u32;
            let bottom = bottom as u32 * (255 - alpha as u32);
            ((top + bottom) / 255) as u8
        };
        Rgb(mix(self.0, under.0), mix(self.1, under.1), mix(self.2, under.2))
    }

    /// Two thirds of the way to the background: what `dim` means here. A
    /// terminal has a faint attribute; a framebuffer has to do it itself.
    pub fn dimmed(self, toward: Rgb) -> Rgb {
        self.over(toward, 160)
    }
}

/// The first sixteen, which is where a named colour lands. Dracula's ANSI
/// palette, so `"blue"` in the shipped theme is the blue the theme was
/// written against rather than a terminal's idea of blue.
const BASE: [Rgb; 16] = [
    Rgb(0x21, 0x22, 0x2c), // black
    Rgb(0xff, 0x55, 0x55), // dark red
    Rgb(0x50, 0xfa, 0x7b), // dark green
    Rgb(0xf1, 0xfa, 0x8c), // dark yellow
    Rgb(0xbd, 0x93, 0xf9), // dark blue
    Rgb(0xff, 0x79, 0xc6), // dark magenta
    Rgb(0x8b, 0xe9, 0xfd), // dark cyan
    Rgb(0xf8, 0xf8, 0xf2), // grey
    Rgb(0x62, 0x72, 0xa4), // dark grey
    Rgb(0xff, 0x6e, 0x6e), // red
    Rgb(0x69, 0xff, 0x94), // green
    Rgb(0xff, 0xff, 0xa5), // yellow
    Rgb(0xd6, 0xac, 0xff), // blue
    Rgb(0xff, 0x92, 0xdf), // magenta
    Rgb(0xa4, 0xff, 0xff), // cyan
    Rgb(0xff, 0xff, 0xff), // white
];

/// A palette index, by the same arithmetic every terminal uses: sixteen
/// names, then a 6×6×6 colour cube, then twenty-four greys.
pub fn ansi(index: u8) -> Rgb {
    match index {
        0..=15 => BASE[index as usize],
        16..=231 => {
            let index = index as u32 - 16;
            let level = |n: u32| match n {
                0 => 0,
                n => (55 + n * 40) as u8,
            };
            Rgb(level(index / 36), level((index / 6) % 6), level(index % 6))
        }
        _ => {
            let grey = 8 + (index as u32 - 232) * 10;
            Rgb(grey as u8, grey as u8, grey as u8)
        }
    }
}

/// What a theme's colour is on screen. `None` is the terminal's default,
/// which here is the window's.
pub fn rgb(color: Option<Color>, default: Rgb) -> Rgb {
    let Some(color) = color else {
        return default;
    };
    match color {
        Color::Rgb { r, g, b } => Rgb(r, g, b),
        Color::AnsiValue(index) => ansi(index),
        Color::Black => BASE[0],
        Color::DarkRed => BASE[1],
        Color::DarkGreen => BASE[2],
        Color::DarkYellow => BASE[3],
        Color::DarkBlue => BASE[4],
        Color::DarkMagenta => BASE[5],
        Color::Grey => BASE[7],
        Color::DarkGrey => BASE[8],
        Color::Red => BASE[9],
        Color::Green => BASE[10],
        Color::Yellow => BASE[11],
        Color::Blue => BASE[12],
        Color::Magenta => BASE[13],
        Color::Cyan => BASE[14],
        Color::DarkCyan => BASE[6],
        Color::White => BASE[15],
        Color::Reset => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_theme_that_spells_a_colour_out_gets_that_colour() {
        assert_eq!(rgb(Some(Color::Rgb { r: 0x8b, g: 0xe9, b: 0xfd }), DEFAULT_FG), Rgb(0x8b, 0xe9, 0xfd));
        assert_eq!(rgb(None, DEFAULT_BG), DEFAULT_BG);
        assert_eq!(rgb(Some(Color::Reset), DEFAULT_BG), DEFAULT_BG);
    }

    #[test]
    fn a_name_is_the_bright_half_of_the_pair_and_dark_is_the_other() {
        // As crossterm names them: `Red` is the bright one, `DarkRed` is not.
        assert_eq!(rgb(Some(Color::Red), DEFAULT_FG), BASE[9]);
        assert_eq!(rgb(Some(Color::DarkRed), DEFAULT_FG), BASE[1]);
        assert_eq!(rgb(Some(Color::AnsiValue(9)), DEFAULT_FG), BASE[9]);
    }

    #[test]
    fn the_palette_is_the_arithmetic_every_terminal_uses() {
        assert_eq!(ansi(16), Rgb(0, 0, 0), "the cube starts at black");
        assert_eq!(ansi(231), Rgb(255, 255, 255), "and ends at white");
        assert_eq!(ansi(196), Rgb(255, 0, 0), "pure red, where xterm has it");
        assert_eq!(ansi(232), Rgb(8, 8, 8), "the greys start just off black");
        assert_eq!(ansi(255), Rgb(238, 238, 238));
    }

    #[test]
    fn blending_covers_the_ends_and_the_middle() {
        let (white, black) = (Rgb(255, 255, 255), Rgb(0, 0, 0));
        assert_eq!(white.over(black, 255), white, "fully covered");
        assert_eq!(white.over(black, 0), black, "not covered at all");
        assert_eq!(white.over(black, 128), Rgb(128, 128, 128));
        // Dim is toward the background, not toward black.
        assert_eq!(white.dimmed(white), white);
    }

    #[test]
    fn a_pixel_is_the_channels_where_the_framebuffer_wants_them() {
        assert_eq!(Rgb(0x28, 0x2a, 0x36).pixel(), 0x00282a36);
    }
}
