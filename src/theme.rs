use anyhow::{Context, Result, bail};
use crossterm::style::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::screen::Style;

const BUILT_IN: &str = include_str!("../themes/default.toml");

/// Styles for tree-sitter capture names and the editor's own `ui.*` elements.
#[derive(Debug)]
pub struct Theme {
    styles: HashMap<String, Style>,
}

impl Theme {
    pub fn built_in() -> Theme {
        Theme::parse(BUILT_IN).expect("the built-in theme should parse")
    }

    /// The built-in theme with the user's layered over it, if they have one.
    /// Layering rather than replacing means a three-line theme file can restyle
    /// keywords without silently losing the status line. A broken user theme is
    /// reported rather than fatal.
    pub fn load_user() -> (Theme, Option<String>) {
        let built_in = Theme::built_in();
        let Some(path) = user_theme_path() else {
            return (built_in, None);
        };
        if !path.exists() {
            return (built_in, None);
        }

        match std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))
            .and_then(|text| Theme::parse(&text))
        {
            Ok(user) => {
                let mut theme = built_in;
                theme.overlay(user);
                (theme, None)
            }
            Err(err) => (built_in, Some(format!("default theme: {err:#}"))),
        }
    }

    /// Replace this theme's entries with `other`'s, key by key.
    pub fn overlay(&mut self, other: Theme) {
        self.styles.extend(other.styles);
    }

    pub fn parse(text: &str) -> Result<Theme> {
        let entries: HashMap<String, Entry> =
            toml::from_str(text).context("parsing theme")?;
        let mut styles = HashMap::with_capacity(entries.len());
        for (key, entry) in entries {
            let style = entry
                .into_style()
                .with_context(|| format!("in theme key `{key}`"))?;
            styles.insert(key, style);
        }
        Ok(Theme { styles })
    }

    /// Look up a key, falling back along the dots: `punctuation.bracket` tries
    /// `punctuation.bracket`, then `punctuation`. Unknown keys get the default
    /// style, which renders as the terminal's own colors.
    pub fn style(&self, key: &str) -> Style {
        let mut name = key;
        loop {
            if let Some(style) = self.styles.get(name) {
                return *style;
            }
            match name.rfind('.') {
                Some(dot) => name = &name[..dot],
                None => return Style::default(),
            }
        }
    }

    /// Whether the key resolves to anything, so callers can skip styling
    /// entirely rather than painting the default over every cell.
    pub fn has(&self, key: &str) -> bool {
        let mut name = key;
        loop {
            if self.styles.contains_key(name) {
                return true;
            }
            match name.rfind('.') {
                Some(dot) => name = &name[..dot],
                None => return false,
            }
        }
    }
}

fn user_theme_path() -> Option<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(config.join("soda_edit").join("theme.toml"))
}

/// A theme value is either a bare color or a table of attributes.
#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Color(String),
    Table(Table),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    fg: Option<String>,
    bg: Option<String>,
    #[serde(default)]
    bold: bool,
    #[serde(default)]
    italic: bool,
    #[serde(default)]
    underline: bool,
    #[serde(default)]
    dim: bool,
    #[serde(default)]
    reverse: bool,
}

impl Entry {
    fn into_style(self) -> Result<Style> {
        match self {
            Entry::Color(name) => Ok(Style { fg: Some(parse_color(&name)?), ..Style::default() }),
            Entry::Table(table) => Ok(Style {
                fg: table.fg.as_deref().map(parse_color).transpose()?,
                bg: table.bg.as_deref().map(parse_color).transpose()?,
                bold: table.bold,
                italic: table.italic,
                underline: table.underline,
                dim: table.dim,
                reverse: table.reverse,
            }),
        }
    }
}

fn parse_color(name: &str) -> Result<Color> {
    if let Some(hex) = name.strip_prefix('#') {
        if hex.len() != 6 {
            bail!("`#{hex}` is not a 6-digit hex color");
        }
        let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16);
        let (r, g, b) = (channel(0..2), channel(2..4), channel(4..6));
        return match (r, g, b) {
            (Ok(r), Ok(g), Ok(b)) => Ok(Color::Rgb { r, g, b }),
            _ => bail!("`#{hex}` is not a 6-digit hex color"),
        };
    }
    if let Ok(index) = name.parse::<u8>() {
        return Ok(Color::AnsiValue(index));
    }

    Ok(match name {
        "black" => Color::Black,
        "dark_grey" | "dark_gray" => Color::DarkGrey,
        "grey" | "gray" => Color::Grey,
        "white" => Color::White,
        "red" => Color::Red,
        "dark_red" => Color::DarkRed,
        "green" => Color::Green,
        "dark_green" => Color::DarkGreen,
        "yellow" => Color::Yellow,
        "dark_yellow" => Color::DarkYellow,
        "blue" => Color::Blue,
        "dark_blue" => Color::DarkBlue,
        "magenta" => Color::Magenta,
        "dark_magenta" => Color::DarkMagenta,
        "cyan" => Color::Cyan,
        "dark_cyan" => Color::DarkCyan,
        _ => bail!("unknown color `{name}`"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_theme_parses_and_covers_the_ui() {
        let theme = Theme::built_in();
        for key in ["ui.statusline", "ui.selection", "ui.eof", "keyword", "comment"] {
            assert!(theme.has(key), "built-in theme is missing `{key}`");
        }
    }

    #[test]
    fn lookup_falls_back_along_the_dots() {
        let theme = Theme::parse(r#""variable" = "blue""#).unwrap();
        assert_eq!(theme.style("variable.parameter").fg, Some(Color::Blue));
        assert_eq!(theme.style("variable").fg, Some(Color::Blue));
        // A more specific entry wins over the general one.
        let theme = Theme::parse(
            r#"
            "variable" = "blue"
            "variable.parameter" = "red"
            "#,
        )
        .unwrap();
        assert_eq!(theme.style("variable.parameter").fg, Some(Color::Red));
    }

    #[test]
    fn an_unknown_key_resolves_to_the_default_style() {
        let theme = Theme::parse(r#""keyword" = "blue""#).unwrap();
        assert!(!theme.has("nonsense"));
        assert_eq!(theme.style("nonsense"), Style::default());
    }

    #[test]
    fn a_value_is_either_a_color_or_a_table() {
        let theme = Theme::parse(
            r#"
            "a" = "green"
            "b" = { fg = "red", bg = "black", bold = true, italic = true }
            "#,
        )
        .unwrap();
        assert_eq!(theme.style("a").fg, Some(Color::Green));
        assert!(!theme.style("a").bold);

        let b = theme.style("b");
        assert_eq!(b.fg, Some(Color::Red));
        assert_eq!(b.bg, Some(Color::Black));
        assert!(b.bold && b.italic);
        assert!(!b.underline);
    }

    #[test]
    fn colors_can_be_names_palette_indices_or_hex() {
        let theme = Theme::parse(
            r##"
            "a" = "dark_cyan"
            "b" = "117"
            "c" = "#8be9fd"
            "##,
        )
        .unwrap();
        assert_eq!(theme.style("a").fg, Some(Color::DarkCyan));
        assert_eq!(theme.style("b").fg, Some(Color::AnsiValue(117)));
        assert_eq!(
            theme.style("c").fg,
            Some(Color::Rgb { r: 0x8b, g: 0xe9, b: 0xfd })
        );
    }

    #[test]
    fn a_bad_color_names_the_key_that_contains_it() {
        let err = Theme::parse(r#""keyword" = "purplish""#).unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("keyword"), "{message}");
        assert!(message.contains("purplish"), "{message}");
    }

    #[test]
    fn a_short_hex_color_is_rejected() {
        assert!(Theme::parse(r##""keyword" = "#abc""##).is_err());
        assert!(Theme::parse(r##""keyword" = "#gggggg""##).is_err());
    }

    #[test]
    fn a_user_theme_layers_over_the_built_in_one() {
        let mut theme = Theme::built_in();
        theme.overlay(Theme::parse(r#""keyword" = "red""#).unwrap());

        // The override lands...
        assert_eq!(theme.style("keyword").fg, Some(Color::Red));
        // ...and everything the user did not mention survives.
        assert!(theme.style("ui.statusline").reverse);
        assert_eq!(theme.style("string").fg, Some(Color::Green));
    }

    #[test]
    fn a_misspelled_attribute_is_rejected_rather_than_ignored() {
        // Silently dropping `itallic` would leave the user hunting for why
        // their theme does nothing.
        assert!(Theme::parse(r#""keyword" = { fg = "red", itallic = true }"#).is_err());
    }
}
