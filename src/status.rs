use crate::editor::{Editor, Mode};
use crate::keys::Keys;
use crate::screen::Style;
use crate::stream::Sign;
use crate::syntax::language_for_path;

/// One coloured block of the status line. Powerline drawing needs the pieces
/// kept apart until the end: the separator between two blocks is painted in
/// one's background over the other's.
pub struct Segment {
    pub text: String,
    pub style: Style,
}

/// The status line as blocks, left to right on each side, plus whatever
/// message wants the space between them.
pub struct Status {
    pub left: Vec<Segment>,
    pub right: Vec<Segment>,
    pub message: String,
}

/// The characters the status line is drawn with. Two sets: the pretty one
/// needs a patched font, so `:set noglyphs` swaps in the ASCII it degrades to.
pub struct Glyphs {
    /// Whether this set needs a patched font, which is also what decides
    /// whether file-type icons are worth drawing.
    pub nerd: bool,
    /// The wedge that points the way the text reads, and its hairline version
    /// for when both sides share a background.
    pub section_right: &'static str,
    pub section_left: &'static str,
    pub thin_right: &'static str,
    pub thin_left: &'static str,
    pub modified: &'static str,
    pub line: &'static str,
    pub column: &'static str,
    pub scratch: &'static str,
    pub added: &'static str,
    pub modified_sign: &'static str,
    pub deleted: &'static str,
    /// Shown where tabs scrolled off the left of the line.
    pub truncated: &'static str,
}

/// Nerd Font code points, all from the Powerline and Devicons ranges.
pub const NERD: Glyphs = Glyphs {
    nerd: true,
    section_right: "\u{e0b0}",
    section_left: "\u{e0b2}",
    thin_right: "\u{e0b1}",
    thin_left: "\u{e0b3}",
    modified: "\u{25cf}",
    line: "\u{e0a1}",
    column: "\u{e0a3}",
    scratch: "\u{f15b}",
    added: "\u{f457}",
    modified_sign: "\u{f459}",
    deleted: "\u{f458}",
    truncated: "\u{e0b3}",
};

pub const PLAIN: Glyphs = Glyphs {
    nerd: false,
    section_right: "",
    section_left: "",
    thin_right: "|",
    thin_left: "|",
    modified: "+",
    line: "ln",
    column: "col",
    scratch: "",
    added: "+",
    modified_sign: "~",
    deleted: "-",
    truncated: "<",
};

pub fn glyphs(editor: &Editor) -> &'static Glyphs {
    match editor.glyphs {
        true => &NERD,
        false => &PLAIN,
    }
}

/// The icon for a language, by the name the grammar registry uses.
fn language_icon(language: Option<&str>, glyphs: &Glyphs) -> &'static str {
    if !glyphs.nerd {
        return "";
    }
    match language {
        Some("rust") => "\u{e7a8}",
        Some("html") => "\u{e736}",
        Some("javascript") => "\u{e781}",
        _ => "\u{f15b}",
    }
}

/// The open buffers, one block each, for the line along the top. The number
/// on a tab is what `{n}gn` takes, which is the only way to get there without
/// the picker.
pub fn tabs(editor: &Editor) -> Vec<Segment> {
    let glyphs = glyphs(editor);
    let base = editor.theme.style("ui.tabline");
    let selected = editor.theme.style("ui.tabline.selected");

    editor
        .views()
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let language = language_for_path(view.doc.path.as_deref()).map(|l| l.name);
            let icon = match view.doc.path.is_some() {
                true => language_icon(language, glyphs),
                false => glyphs.scratch,
            };
            let mut text = format!("{} ", index + 1);
            if !icon.is_empty() {
                text.push_str(icon);
                text.push(' ');
            }
            text.push_str(view.doc.display_name());
            if view.is_modified() {
                text.push(' ');
                text.push_str(glyphs.modified);
            }
            let style = match index == editor.current_index() {
                true => selected,
                false => base,
            };
            Segment { text, style }
        })
        .collect()
}

pub fn build(editor: &Editor, keys: &Keys) -> Status {
    let glyphs = glyphs(editor);
    let bar = editor.theme.style("ui.statusline");
    let view = editor.view();
    let language = language_for_path(view.doc.path.as_deref()).map(|l| l.name);

    let mode_style = editor.theme.style(match editor.mode {
        Mode::Normal => "ui.mode.normal",
        Mode::Insert => "ui.mode.insert",
        Mode::Visual | Mode::VisualLine => "ui.mode.visual",
    });

    let mut left = vec![Segment { text: editor.mode.name().to_string(), style: mode_style }];

    // The file: its icon, its name, a dot while it has unsaved changes, and
    // which of several buffers it is.
    let icon = match view.doc.path.is_some() {
        true => language_icon(language, glyphs),
        false => glyphs.scratch,
    };
    let mut file = String::new();
    if !icon.is_empty() {
        file.push_str(icon);
        file.push(' ');
    }
    file.push_str(view.doc.display_name());
    if editor.is_modified() {
        file.push(' ');
        file.push_str(glyphs.modified);
    }
    // Which buffer this is, unless the line along the top is already saying.
    if editor.views().len() > 1 && !editor.show_tabline() {
        file.push_str(&format!(" {}/{}", editor.current_index() + 1, editor.views().len()));
    }
    left.push(Segment { text: file, style: editor.theme.style("ui.statusline.file") });

    // What git would say about the buffer, out of the signs already computed
    // for the gutter. Counted, not recomputed.
    let (mut added, mut modified, mut deleted) = (0, 0, 0);
    for sign in view.signs.values() {
        match sign {
            Sign::Added => added += 1,
            Sign::Modified => modified += 1,
            Sign::Deleted => deleted += 1,
        }
    }
    for (count, glyph, key) in [
        (added, glyphs.added, "ui.gutter.added"),
        (modified, glyphs.modified_sign, "ui.gutter.modified"),
        (deleted, glyphs.deleted, "ui.gutter.deleted"),
    ] {
        if count > 0 {
            // On the bar's own background, so these read as one group.
            let style = Style { bg: bar.bg, ..editor.theme.style(key) };
            left.push(Segment { text: format!("{glyph}{count}"), style });
        }
    }

    let info = editor.theme.style("ui.statusline.info");
    let mut right = Vec::new();
    let pending = keys.pending_text();
    if !pending.is_empty() {
        right.push(Segment { text: pending, style: Style { bold: true, ..info } });
    }
    if let Some(language) = language {
        right.push(Segment { text: language.to_string(), style: info });
    }

    let (line, column) = editor.cursor_coords();
    let last = editor.last_line();
    right.push(Segment { text: percentage(line, last), style: info });
    right.push(Segment {
        text: format!("{} {}  {} {}", glyphs.line, line + 1, glyphs.column, column + 1),
        style: editor.theme.style("ui.statusline.position"),
    });

    Status { left, right, message: editor.message.clone() }
}

/// How far down the file the cursor is, in vim's words rather than a number
/// when it is at either end.
fn percentage(line: usize, last: usize) -> String {
    match (line, last) {
        (_, 0) => "all".to_string(),
        (0, _) => "top".to_string(),
        (line, last) if line == last => "bot".to_string(),
        (line, last) => format!("{}%", line * 100 / last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_of_the_file_are_named_rather_than_numbered() {
        assert_eq!(percentage(0, 0), "all");
        assert_eq!(percentage(0, 99), "top");
        assert_eq!(percentage(99, 99), "bot");
        assert_eq!(percentage(50, 100), "50%");
    }

    #[test]
    fn the_line_reads_mode_first_and_position_last() {
        let mut editor = Editor::scratch();
        editor.view_mut().doc.text = ropey::Rope::from_str("one\ntwo\n");
        let status = build(&editor, &Keys::default());

        assert_eq!(status.left[0].text, "NORMAL");
        assert!(status.left[1].text.contains("[scratch]"), "{}", status.left[1].text);
        let position = &status.right.last().unwrap().text;
        assert!(position.ends_with("1"), "{position}");
        // No path, so no language segment to name.
        assert_eq!(status.right.len(), 2);
    }

    #[test]
    fn a_half_typed_command_takes_a_segment_of_its_own() {
        let editor = Editor::scratch();
        let mut keys = Keys::default();
        keys.handle(&mut Editor::scratch(), crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('d'),
            crossterm::event::KeyModifiers::NONE,
        ));
        let status = build(&editor, &keys);
        assert_eq!(status.right[0].text, "d");
    }

    #[test]
    fn the_plain_set_is_pure_ascii() {
        let plain = [
            PLAIN.section_right, PLAIN.section_left, PLAIN.thin_right, PLAIN.thin_left,
            PLAIN.modified, PLAIN.line, PLAIN.column, PLAIN.scratch,
            PLAIN.added, PLAIN.modified_sign, PLAIN.deleted,
        ];
        for text in plain {
            assert!(text.is_ascii(), "{text:?} is not ascii");
        }
    }
}
