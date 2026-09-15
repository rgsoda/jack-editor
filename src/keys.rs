use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor::{Editor, Mode};
use crate::view::Move;

/// One line of the help. These are written down rather than derived from the
/// match arms below, so this is a promise the tests have to keep: every key
/// named here has to do what it says.
pub struct Binding {
    pub keys: &'static str,
    pub what: &'static str,
    pub mode: &'static str,
}

pub const BINDINGS: &[Binding] = &[
    Binding { keys: "^s", what: "save", mode: "any" },
    Binding { keys: "^q", what: "quit (twice if unsaved)", mode: "any" },

    Binding { keys: "h j k l", what: "move left, down, up, right", mode: "normal" },
    Binding { keys: "arrows", what: "move", mode: "normal" },
    Binding { keys: "w b e", what: "word forward, back, end", mode: "normal" },
    Binding { keys: "0 ^ $", what: "line start, first non-blank, line end", mode: "normal" },
    Binding { keys: "gg G", what: "first line, last line", mode: "normal" },
    Binding { keys: "{n}G", what: "go to line n", mode: "normal" },
    Binding { keys: "^d ^u", what: "half page down, up", mode: "normal" },
    Binding { keys: "pgdn pgup", what: "page down, up", mode: "normal" },

    Binding { keys: "i I", what: "insert here, at first non-blank", mode: "normal" },
    Binding { keys: "a A", what: "insert after cursor, at line end", mode: "normal" },
    Binding { keys: "o O", what: "open a line below, above", mode: "normal" },
    Binding { keys: "x", what: "delete character", mode: "normal" },
    Binding { keys: "D C", what: "delete, change to line end", mode: "normal" },
    Binding { keys: "dd d{motion}", what: "delete lines, over a motion", mode: "normal" },
    Binding { keys: "cc c{motion}", what: "change lines, over a motion", mode: "normal" },
    Binding { keys: "yy Y y{motion}", what: "yank lines, over a motion", mode: "normal" },
    Binding { keys: "p P", what: "put after, before the cursor", mode: "normal" },
    Binding { keys: "\"x", what: "use register x (\"X appends)", mode: "normal" },
    Binding { keys: "u ^r", what: "undo, redo", mode: "normal" },
    Binding { keys: "{count}", what: "repeat the next command", mode: "normal" },
    Binding { keys: "esc", what: "abandon a half-typed command", mode: "normal" },

    Binding { keys: "v V", what: "select characters, whole lines", mode: "normal" },
    Binding { keys: "shift+arrows", what: "select, entering visual mode", mode: "normal" },
    Binding { keys: "gn gp", what: "next, previous buffer", mode: "normal" },
    Binding { keys: "{n}gn", what: "go to buffer n", mode: "normal" },

    Binding { keys: "<space>b", what: "pick a buffer", mode: "normal" },
    Binding { keys: "<space>f", what: "pick a file", mode: "normal" },
    Binding { keys: "<space>s", what: "grep the working directory", mode: "normal" },
    Binding { keys: "<space>?", what: "this help", mode: "normal" },
    Binding { keys: "<space>n", what: "cycle line numbers", mode: "normal" },

    Binding { keys: "any motion", what: "drag the selection", mode: "visual" },
    Binding { keys: "o", what: "swap which end moves", mode: "visual" },
    Binding { keys: "v V", what: "characters, lines, or back to normal", mode: "visual" },
    Binding { keys: "d x", what: "delete the selection", mode: "visual" },
    Binding { keys: "c s", what: "delete it and start typing", mode: "visual" },
    Binding { keys: "y", what: "yank the selection", mode: "visual" },
    Binding { keys: "p P", what: "replace it with a register", mode: "visual" },
    Binding { keys: "D X Y C S", what: "the same, on whole lines", mode: "visual" },
    Binding { keys: "esc", what: "back to normal mode", mode: "visual" },

    Binding { keys: "esc", what: "back to normal mode", mode: "insert" },
    Binding { keys: "shift+arrows", what: "select while typing", mode: "insert" },
    Binding { keys: "backspace delete", what: "delete a grapheme, or the selection", mode: "insert" },
    Binding { keys: "enter", what: "split the line, keeping the indent", mode: "insert" },

    Binding { keys: "any character", what: "narrow the list, or search", mode: "picker" },
    Binding { keys: "^n ^p tab arrows", what: "next, previous match", mode: "picker" },
    Binding { keys: "enter", what: "choose", mode: "picker" },
    Binding { keys: "backspace ^w ^u", what: "delete a character, word, the query", mode: "picker" },
    Binding { keys: "esc ^c", what: "close", mode: "picker" },
];

pub enum Action {
    Continue,
    Quit,
}

/// What a half-typed command is waiting for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// An operator waiting for the motion it applies to: `d`, `c`, `y`.
    Delete,
    Change,
    Yank,
    /// The `g` prefix, waiting for `gg`.
    Go,
    /// The space leader, waiting for which picker to open.
    Leader,
    /// The `"` prefix, waiting for the register name.
    Register,
}

/// Input state that outlives a single keypress: a count being typed, and any
/// operator or prefix waiting for the rest of its command.
#[derive(Default)]
pub struct Keys {
    count: Option<usize>,
    pending: Option<Pending>,
    /// The register `"x` named for the command being typed.
    register: Option<char>,
}

impl Keys {
    pub fn handle(&mut self, editor: &mut Editor, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // An open picker owns the keyboard, including the chords below.
        if editor.picker.is_some() {
            editor.picker_input(key);
            return Action::Continue;
        }

        // Chords that mean the same thing in either mode.
        if ctrl {
            match key.code {
                KeyCode::Char('q') => return Action::Quit,
                KeyCode::Char('s') => {
                    editor.save();
                    return Action::Continue;
                }
                _ => {}
            }
        }

        match editor.mode {
            Mode::Normal => self.normal(editor, key, ctrl),
            Mode::Insert => insert(editor, key, ctrl),
            Mode::Visual | Mode::VisualLine => self.visual(editor, key, ctrl),
        }
        Action::Continue
    }

    /// True while a command is half-typed, so the caller can show it.
    pub fn pending_text(&self) -> String {
        let mut text = String::new();
        if let Some(count) = self.count {
            text.push_str(&count.to_string());
        }
        if let Some(register) = self.register {
            text.push('"');
            text.push(register);
        }
        text.push_str(match self.pending {
            Some(Pending::Delete) => "d",
            Some(Pending::Change) => "c",
            Some(Pending::Yank) => "y",
            Some(Pending::Go) => "g",
            Some(Pending::Leader) => "<space>",
            Some(Pending::Register) => "\"",
            None => "",
        });
        text
    }

    fn normal(&mut self, editor: &mut Editor, key: KeyEvent, ctrl: bool) {
        if key.code == KeyCode::Esc {
            self.count = None;
            self.pending = None;
            self.register = None;
            return;
        }

        // `"x` names the register for whatever command comes next, so it does
        // not end the command the way an operator or motion does.
        if self.pending == Some(Pending::Register) {
            self.pending = None;
            if let KeyCode::Char(name) = key.code {
                self.register = Some(name);
            }
            return;
        }

        // Digits build a count, except a leading `0`, which is a motion.
        if let KeyCode::Char(c) = key.code
            && !ctrl
            && c.is_ascii_digit()
            && !(c == '0' && self.count.is_none())
        {
            let digit = c.to_digit(10).unwrap() as usize;
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10) + digit);
            return;
        }

        let count = self.count;
        match self.pending.take() {
            Some(Pending::Go) => {
                match key.code {
                    KeyCode::Char('g') => {
                        editor.goto_line(count.unwrap_or(1) - 1);
                        editor.clamp_cursor();
                    }
                    // A count on `gn`/`gp` is a buffer number, as in vim's `:b`.
                    KeyCode::Char('n') => match count {
                        Some(n) => editor.switch_to(n - 1),
                        None => editor.next_view(),
                    },
                    KeyCode::Char('p') => match count {
                        Some(n) => editor.switch_to(n - 1),
                        None => editor.previous_view(),
                    },
                    _ => {}
                }
                self.finish();
            }
            Some(Pending::Leader) => {
                match key.code {
                    KeyCode::Char('b') => editor.open_buffer_picker(),
                    KeyCode::Char('f') => editor.open_file_picker(),
                    KeyCode::Char('s') => editor.open_grep_picker(),
                    KeyCode::Char('?') => editor.open_help_picker(),
                    KeyCode::Char('n') => editor.cycle_numbers(),
                    _ => {}
                }
                self.finish();
            }
            Some(Pending::Register) => unreachable!("handled above"),
            Some(operator) => {
                self.operator(editor, operator, key, count.unwrap_or(1));
                self.finish();
            }
            None => {
                self.command(editor, key, ctrl, count);
                // A command that started an operator or named a register keeps
                // them for the rest of the command.
                if self.pending.is_none() {
                    self.count = None;
                    self.register = None;
                }
            }
        }
    }

    /// Visual mode. Motions drag the head of the selection; everything else
    /// acts on the whole of it at once and drops back to normal mode.
    fn visual(&mut self, editor: &mut Editor, key: KeyEvent, ctrl: bool) {
        if key.code == KeyCode::Esc {
            self.finish();
            self.pending = None;
            editor.set_mode(Mode::Normal);
            return;
        }

        if self.pending == Some(Pending::Register) {
            self.pending = None;
            if let KeyCode::Char(name) = key.code {
                self.register = Some(name);
            }
            return;
        }

        if let KeyCode::Char(c) = key.code
            && !ctrl
            && c.is_ascii_digit()
            && !(c == '0' && self.count.is_none())
        {
            let digit = c.to_digit(10).unwrap() as usize;
            self.count = Some(self.count.unwrap_or(0).saturating_mul(10) + digit);
            return;
        }

        let count = self.count;
        let repeat = count.unwrap_or(1);

        if self.pending.take() == Some(Pending::Go) {
            if key.code == KeyCode::Char('g') {
                editor.goto_line_extending(count.unwrap_or(1) - 1);
            }
            self.finish();
            return;
        }

        // A motion drags the selection instead of collapsing it. This is the
        // whole of visual mode.
        if let Some((motion, _)) = motion_for(key.code, ctrl) {
            for _ in 0..repeat {
                editor.move_cursor(motion, true);
            }
            editor.clamp_cursor();
            self.finish();
            return;
        }

        match key.code {
            KeyCode::Char('v') => editor.set_mode(match editor.mode {
                Mode::Visual => Mode::Normal,
                _ => Mode::Visual,
            }),
            KeyCode::Char('V') => editor.set_mode(match editor.mode {
                Mode::VisualLine => Mode::Normal,
                _ => Mode::VisualLine,
            }),
            KeyCode::Char('o') => editor.swap_selection_ends(),

            KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete => {
                editor.delete_visual(self.register)
            }
            KeyCode::Char('y') => editor.yank_visual(self.register),
            KeyCode::Char('c') | KeyCode::Char('s') => {
                editor.delete_visual(self.register);
                editor.set_mode(Mode::Insert);
            }
            KeyCode::Char('p') | KeyCode::Char('P') => editor.put_over_visual(self.register),

            // The capitals act on whole lines, whichever visual mode you are in.
            KeyCode::Char('D') | KeyCode::Char('X') => {
                editor.set_mode(Mode::VisualLine);
                editor.delete_visual(self.register);
            }
            KeyCode::Char('Y') => {
                editor.set_mode(Mode::VisualLine);
                editor.yank_visual(self.register);
            }
            KeyCode::Char('C') | KeyCode::Char('S') => {
                editor.set_mode(Mode::VisualLine);
                editor.delete_visual(self.register);
                editor.open_line_above();
            }

            KeyCode::Char('g') => {
                self.pending = Some(Pending::Go);
                return;
            }
            KeyCode::Char('G') => {
                let line = count.map_or(editor.last_line(), |n| n - 1);
                editor.goto_line_extending(line);
            }
            KeyCode::Char('"') => {
                self.pending = Some(Pending::Register);
                self.count = None;
                return;
            }
            _ => {}
        }
        self.finish();
    }

    fn finish(&mut self) {
        self.count = None;
        self.register = None;
    }

    fn command(&mut self, editor: &mut Editor, key: KeyEvent, ctrl: bool, count: Option<usize>) {
        let repeat = count.unwrap_or(1);
        if let Some((motion, _)) = motion_for(key.code, ctrl) {
            // Shift with an arrow starts a selection, the way it does in every
            // editor that is not vim. `H` and `L` cannot do this: shift with a
            // letter is a different letter, and already means something else.
            let extend = key.modifiers.contains(KeyModifiers::SHIFT) && is_navigation(key.code);
            if extend {
                editor.set_mode(Mode::Visual);
            }
            for _ in 0..repeat {
                editor.move_cursor(motion, extend);
            }
            editor.clamp_cursor();
            return;
        }

        match key.code {
            KeyCode::Char('i') => editor.set_mode(Mode::Insert),
            KeyCode::Char('I') => {
                editor.move_cursor(Move::FirstNonBlank, false);
                editor.set_mode(Mode::Insert);
            }
            KeyCode::Char('a') => {
                // On an empty line there is nothing to append after.
                let (line, column) = editor.cursor_coords();
                if column < editor.view().doc.line_len_chars(line) {
                    editor.move_cursor(Move::Right, false);
                }
                editor.set_mode(Mode::Insert);
            }
            KeyCode::Char('A') => {
                editor.move_cursor(Move::LineEnd, false);
                editor.set_mode(Mode::Insert);
            }
            KeyCode::Char('o') => editor.open_line_below(),
            KeyCode::Char('O') => editor.open_line_above(),

            KeyCode::Char('x') | KeyCode::Delete => editor.delete_chars(self.register, repeat),
            KeyCode::Char('D') => {
                editor.delete_to_line_end(self.register);
                editor.clamp_cursor();
            }
            KeyCode::Char('C') => {
                editor.delete_to_line_end(self.register);
                editor.set_mode(Mode::Insert);
            }
            KeyCode::Char('Y') => editor.yank_lines(self.register, repeat),
            KeyCode::Char('p') => editor.put(self.register, repeat, true),
            KeyCode::Char('P') => editor.put(self.register, repeat, false),
            KeyCode::Char('"') => self.pending = Some(Pending::Register),

            KeyCode::Char('v') => editor.set_mode(Mode::Visual),
            KeyCode::Char('V') => editor.set_mode(Mode::VisualLine),

            KeyCode::Char('u') => editor.undo(),
            KeyCode::Char('r') if ctrl => editor.redo(),

            KeyCode::Char('d') => self.pending = Some(Pending::Delete),
            KeyCode::Char('c') => self.pending = Some(Pending::Change),
            KeyCode::Char('y') => self.pending = Some(Pending::Yank),
            KeyCode::Char('g') => self.pending = Some(Pending::Go),
            KeyCode::Char(' ') => self.pending = Some(Pending::Leader),
            KeyCode::Char('G') => {
                // Bare `G` goes to the last line, `{n}G` to line n.
                let line = count.map_or(editor.last_line(), |n| n - 1);
                editor.goto_line(line);
                editor.clamp_cursor();
            }
            _ => {}
        }
    }

    /// `d`/`c` followed by the motion they apply to. `dd` and `cc` act on whole
    /// lines; everything else selects across the motion and operates on that.
    fn operator(&mut self, editor: &mut Editor, operator: Pending, key: KeyEvent, count: usize) {
        let doubled = matches!(
            (operator, key.code),
            (Pending::Delete, KeyCode::Char('d'))
                | (Pending::Change, KeyCode::Char('c'))
                | (Pending::Yank, KeyCode::Char('y'))
        );
        if doubled {
            match operator {
                Pending::Delete => editor.delete_lines(self.register, count),
                Pending::Change => editor.change_lines(self.register, count),
                Pending::Yank => editor.yank_lines(self.register, count),
                Pending::Go | Pending::Register | Pending::Leader => {}
            }
            return;
        }

        let Some((motion, inclusive)) = motion_for(key.code, false) else {
            return;
        };

        let sel = &mut editor.view_mut().sel;
        sel.anchor = sel.head;
        for _ in 0..count {
            editor.move_cursor(motion, true);
        }
        // `e` stops *on* the last character of the word, so operating up to it
        // would leave that character behind.
        if inclusive {
            editor.move_cursor(Move::Right, true);
        }

        match operator {
            Pending::Yank => editor.yank_selection(self.register),
            _ => editor.delete_selection(self.register),
        }
        if operator == Pending::Change {
            editor.set_mode(Mode::Insert);
        }
    }
}

/// The motion a key names, and whether it includes the character it lands on.
fn motion_for(code: KeyCode, ctrl: bool) -> Option<(Move, bool)> {
    let motion = match code {
        KeyCode::Char('d') if ctrl => Move::HalfPageDown,
        KeyCode::Char('u') if ctrl => Move::HalfPageUp,
        KeyCode::Home if ctrl => Move::FileStart,
        KeyCode::End if ctrl => Move::FileEnd,
        _ if ctrl => return None,

        KeyCode::Char('h') | KeyCode::Left => Move::Left,
        KeyCode::Char('l') | KeyCode::Right => Move::Right,
        KeyCode::Char('j') | KeyCode::Down => Move::Down,
        KeyCode::Char('k') | KeyCode::Up => Move::Up,
        KeyCode::Char('w') => Move::WordForward,
        KeyCode::Char('b') => Move::WordBack,
        KeyCode::Char('e') => return Some((Move::WordEnd, true)),
        KeyCode::Char('0') | KeyCode::Home => Move::LineStart,
        KeyCode::Char('^') => Move::FirstNonBlank,
        KeyCode::Char('$') | KeyCode::End => Move::LineEnd,
        KeyCode::PageUp => Move::PageUp,
        KeyCode::PageDown => Move::PageDown,
        _ => return None,
    };
    Some((motion, false))
}

/// Keys that are only ever a movement, so shift can mean "and select" without
/// taking a command away from anything.
fn is_navigation(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
    )
}

fn insert(editor: &mut Editor, key: KeyEvent, ctrl: bool) {
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let extend = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Esc => editor.set_mode(Mode::Normal),
        KeyCode::Char(c) if !ctrl && !alt => editor.insert(&c.to_string()),
        KeyCode::Enter => editor.insert_newline(),
        KeyCode::Tab => editor.insert("\t"),
        KeyCode::Backspace => editor.delete_backward(),
        KeyCode::Delete => editor.delete_forward(),
        KeyCode::Left => editor.move_cursor(Move::Left, extend),
        KeyCode::Right => editor.move_cursor(Move::Right, extend),
        KeyCode::Up => editor.move_cursor(Move::Up, extend),
        KeyCode::Down => editor.move_cursor(Move::Down, extend),
        KeyCode::Home if ctrl => editor.move_cursor(Move::FileStart, extend),
        KeyCode::End if ctrl => editor.move_cursor(Move::FileEnd, extend),
        KeyCode::Home => editor.move_cursor(Move::LineStart, extend),
        KeyCode::End => editor.move_cursor(Move::LineEnd, extend),
        KeyCode::PageUp => editor.move_cursor(Move::PageUp, extend),
        KeyCode::PageDown => editor.move_cursor(Move::PageDown, extend),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    struct Vim {
        editor: Editor,
        keys: Keys,
    }

    impl Vim {
        fn new(text: &str) -> Self {
            let mut editor = Editor::scratch();
            editor.view_mut().doc.text = Rope::from_str(text);
            Vim { editor, keys: Keys::default() }
        }

        /// Type a key sequence. `<esc>`, `<cr>`, `<bs>`, `<tab>` and `<C-x>`
        /// name the keys that are not plain characters.
        fn press(&mut self, sequence: &str) -> &mut Self {
            let mut chars = sequence.chars().peekable();
            while let Some(ch) = chars.next() {
                let event = if ch == '<' {
                    let mut name = String::new();
                    for c in chars.by_ref() {
                        if c == '>' {
                            break;
                        }
                        name.push(c);
                    }
                    match name.as_str() {
                        "esc" => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                        "cr" => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                        "bs" => KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                        "tab" => KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
                        "space" => KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
                        "left" => KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                        "right" => KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                        "up" => KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
                        "down" => KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
                        "end" => KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
                        other => {
                            if let Some(name) = other.strip_prefix("S-") {
                                let code = match name {
                                    "left" => KeyCode::Left,
                                    "right" => KeyCode::Right,
                                    "up" => KeyCode::Up,
                                    "down" => KeyCode::Down,
                                    "home" => KeyCode::Home,
                                    "end" => KeyCode::End,
                                    _ => panic!("unknown key name"),
                                };
                                KeyEvent::new(code, KeyModifiers::SHIFT)
                            } else {
                                let c = other.strip_prefix("C-").expect("unknown key name");
                                let c = c.chars().next().unwrap();
                                KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
                            }
                        }
                    }
                } else {
                    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
                };
                self.keys.handle(&mut self.editor, event);
            }
            self
        }

        fn text(&self) -> String {
            self.editor.view().doc.text.to_string()
        }

        /// (line, column), both 1-based, as the status line shows them.
        fn cursor(&self) -> (usize, usize) {
            let (line, column) = self.editor.cursor_coords();
            (line + 1, column + 1)
        }
    }

    #[test]
    fn the_editor_starts_in_normal_mode() {
        let mut vim = Vim::new("hello\n");
        vim.press("hjkl");
        // Those are motions, not text.
        assert_eq!(vim.text(), "hello\n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn i_inserts_and_escape_returns_to_normal() {
        let mut vim = Vim::new("bc\n");
        vim.press("ia");
        assert_eq!(vim.editor.mode, Mode::Insert);
        assert_eq!(vim.text(), "abc\n");

        vim.press("<esc>");
        assert_eq!(vim.editor.mode, Mode::Normal);
        // Escape steps back onto the last character typed.
        assert_eq!(vim.cursor(), (1, 1));
    }

    #[test]
    fn a_appends_after_the_cursor() {
        let mut vim = Vim::new("ac\n");
        vim.press("ab<esc>");
        assert_eq!(vim.text(), "abc\n");
    }

    #[test]
    fn a_on_an_empty_line_does_not_jump_to_the_next_one() {
        let mut vim = Vim::new("\nsecond\n");
        vim.press("ax<esc>");
        assert_eq!(vim.text(), "x\nsecond\n");
    }

    #[test]
    fn capital_i_and_a_go_to_the_ends_of_the_line() {
        let mut vim = Vim::new("    code\n");
        vim.press("Ix<esc>");
        assert_eq!(vim.text(), "    xcode\n");

        let mut vim = Vim::new("    code\n");
        vim.press("Ax<esc>");
        assert_eq!(vim.text(), "    codex\n");
    }

    #[test]
    fn o_opens_a_line_below_and_keeps_the_indent() {
        let mut vim = Vim::new("    one\ntwo\n");
        vim.press("ox<esc>");
        assert_eq!(vim.text(), "    one\n    x\ntwo\n");
    }

    #[test]
    fn capital_o_opens_a_line_above_and_keeps_the_indent() {
        let mut vim = Vim::new("    one\n");
        vim.press("Ox<esc>");
        assert_eq!(vim.text(), "    x\n    one\n");
    }

    #[test]
    fn x_deletes_under_the_cursor_and_takes_a_count() {
        let mut vim = Vim::new("abcdef\n");
        vim.press("x");
        assert_eq!(vim.text(), "bcdef\n");

        vim.press("3x");
        assert_eq!(vim.text(), "ef\n");
    }

    #[test]
    fn the_cursor_cannot_rest_past_the_last_character() {
        let mut vim = Vim::new("abc\nlonger\n");
        vim.press("$");
        assert_eq!(vim.cursor(), (1, 3));

        // Nor can it be pushed there by deleting.
        vim.press("x");
        assert_eq!(vim.text(), "ab\nlonger\n");
        assert_eq!(vim.cursor(), (1, 2));
    }

    #[test]
    fn dd_removes_a_line_including_its_newline() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("jdd");
        assert_eq!(vim.text(), "one\nthree\n");
    }

    #[test]
    fn dd_on_the_last_line_does_not_leave_a_blank_one() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("jdd");
        assert_eq!(vim.text(), "one\n");

        // And on a buffer with no trailing newline.
        let mut vim = Vim::new("one\ntwo");
        vim.press("jdd");
        assert_eq!(vim.text(), "one");
    }

    #[test]
    fn a_count_applies_to_the_whole_operator() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\n");
        vim.press("2dd");
        assert_eq!(vim.text(), "three\nfour\n");
    }

    #[test]
    fn dw_deletes_to_the_start_of_the_next_word() {
        let mut vim = Vim::new("one two three\n");
        vim.press("dw");
        assert_eq!(vim.text(), "two three\n");
    }

    #[test]
    fn de_is_inclusive_of_the_character_it_lands_on() {
        // `e` stops on the last character of the word, so `de` has to take it.
        let mut vim = Vim::new("one two\n");
        vim.press("de");
        assert_eq!(vim.text(), " two\n");
    }

    #[test]
    fn capital_d_and_c_act_to_the_end_of_the_line() {
        let mut vim = Vim::new("keep this\n");
        vim.press("wD");
        assert_eq!(vim.text(), "keep \n");

        let mut vim = Vim::new("keep this\n");
        vim.press("wCthat<esc>");
        assert_eq!(vim.text(), "keep that\n");
    }

    #[test]
    fn cc_clears_the_line_but_keeps_its_indent() {
        let mut vim = Vim::new("    old text\nnext\n");
        vim.press("ccnew<esc>");
        assert_eq!(vim.text(), "    new\nnext\n");
    }

    #[test]
    fn cw_changes_a_word_and_leaves_you_in_insert_mode() {
        let mut vim = Vim::new("one two\n");
        vim.press("cwX");
        assert_eq!(vim.editor.mode, Mode::Insert);
        assert_eq!(vim.text(), "Xtwo\n");
    }

    #[test]
    fn word_motions_step_between_runs_of_one_class() {
        let mut vim = Vim::new("foo.bar baz\n");
        vim.press("w");
        assert_eq!(vim.cursor(), (1, 4)); // the dot
        vim.press("w");
        assert_eq!(vim.cursor(), (1, 5)); // bar
        vim.press("w");
        assert_eq!(vim.cursor(), (1, 9)); // baz
        vim.press("b");
        assert_eq!(vim.cursor(), (1, 5));
        vim.press("e");
        assert_eq!(vim.cursor(), (1, 7)); // last char of bar
    }

    #[test]
    fn line_motions() {
        let mut vim = Vim::new("    indented line\n");
        vim.press("$");
        assert_eq!(vim.cursor(), (1, 17));
        vim.press("0");
        assert_eq!(vim.cursor(), (1, 1));
        vim.press("^");
        assert_eq!(vim.cursor(), (1, 5));
    }

    #[test]
    fn gg_and_capital_g_jump_between_the_ends() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("G");
        // Not the empty line the trailing newline creates.
        assert_eq!(vim.cursor(), (3, 1));
        vim.press("gg");
        assert_eq!(vim.cursor(), (1, 1));
        vim.press("2G");
        assert_eq!(vim.cursor(), (2, 1));
    }

    #[test]
    fn zero_is_a_motion_alone_but_a_digit_after_a_count() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n");
        // `10G` is line ten, not line 1 followed by a `0` motion.
        vim.press("10G");
        assert_eq!(vim.cursor(), (10, 1));

        vim.press("$0");
        assert_eq!(vim.cursor(), (10, 1));
    }

    #[test]
    fn u_undoes_and_ctrl_r_redoes() {
        let mut vim = Vim::new("text\n");
        vim.press("ihello <esc>");
        assert_eq!(vim.text(), "hello text\n");

        vim.press("u");
        assert_eq!(vim.text(), "text\n");
        vim.press("<C-r>");
        assert_eq!(vim.text(), "hello text\n");
    }

    #[test]
    fn escape_abandons_a_half_typed_command() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("2d<esc>");
        assert_eq!(vim.text(), "one\ntwo\n");
        assert_eq!(vim.keys.pending_text(), "");

        // And the abandoned count does not leak into the next command.
        vim.press("x");
        assert_eq!(vim.text(), "ne\ntwo\n");
    }

    #[test]
    fn a_half_typed_command_is_visible() {
        let mut vim = Vim::new("one\n");
        vim.press("2d");
        assert_eq!(vim.keys.pending_text(), "2d");
        vim.press("d");
        assert_eq!(vim.keys.pending_text(), "");
    }

    #[test]
    fn insert_mode_keeps_the_arrow_keys_working() {
        let mut vim = Vim::new("ac\n");
        vim.press("i");
        vim.keys.handle(
            &mut vim.editor,
            KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        );
        vim.press("b");
        assert_eq!(vim.text(), "abc\n");
    }

    #[test]
    fn yy_then_p_duplicates_the_line_below() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("yyp");
        assert_eq!(vim.text(), "one\none\ntwo\n");
        // The cursor lands on the line that was put.
        assert_eq!(vim.cursor(), (2, 1));
    }

    #[test]
    fn capital_p_puts_the_line_above() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("jyyP");
        assert_eq!(vim.text(), "one\ntwo\ntwo\n");
        assert_eq!(vim.cursor(), (2, 1));
    }

    #[test]
    fn dd_then_p_moves_a_line() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("ddp");
        assert_eq!(vim.text(), "two\none\n");
    }

    #[test]
    fn a_linewise_put_lands_on_the_first_non_blank() {
        let mut vim = Vim::new("    indented
plain
");
        vim.press("yyp");
        assert_eq!(vim.text(), "    indented
    indented
plain
");
        assert_eq!(vim.cursor(), (2, 5));
    }

    #[test]
    fn putting_after_the_last_line_adds_the_newline_it_needs() {
        // No trailing newline, so there is no line after to start at.
        let mut vim = Vim::new("only");
        vim.press("yyp");
        assert_eq!(vim.text(), "only\nonly");
    }

    #[test]
    fn x_then_p_swaps_two_characters() {
        let mut vim = Vim::new("abc\n");
        vim.press("xp");
        assert_eq!(vim.text(), "bac\n");
    }

    #[test]
    fn a_charwise_put_goes_after_the_cursor_and_capital_p_before_it() {
        let mut vim = Vim::new("one two\n");
        vim.press("dw");
        assert_eq!(vim.text(), "two\n");
        vim.press("p");
        assert_eq!(vim.text(), "tone wo\n");

        let mut vim = Vim::new("one two\n");
        vim.press("dwP");
        assert_eq!(vim.text(), "one two\n");
    }

    #[test]
    fn a_count_repeats_the_put() {
        let mut vim = Vim::new("ab\n");
        vim.press("yy3p");
        assert_eq!(vim.text(), "ab\nab\nab\nab\n");
    }

    #[test]
    fn a_named_register_keeps_its_own_text() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("\"ayy");   // yank "one" into a
        vim.press("jdd");      // delete "two" into the unnamed register
        vim.press("\"ap");    // put a, not the deletion
        assert_eq!(vim.text(), "one\nthree\none\n");
    }

    #[test]
    fn register_zero_holds_the_last_yank_past_a_delete() {
        let mut vim = Vim::new("keep\njunk\n");
        vim.press("yy");  // "keep" yanked
        vim.press("jdd"); // "junk" deleted, unnamed register overwritten
        assert_eq!(vim.text(), "keep\n");

        vim.press("\"0p");
        assert_eq!(vim.text(), "keep\nkeep\n");
    }

    #[test]
    fn an_uppercase_register_name_appends() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("\"ayy");
        vim.press("j\"Ayy");
        vim.press("G\"ap");
        assert_eq!(vim.text(), "one\ntwo\nthree\none\ntwo\n");
    }

    #[test]
    fn yanking_leaves_the_text_alone_and_the_cursor_at_its_start() {
        let mut vim = Vim::new("one two\n");
        vim.press("wye");
        assert_eq!(vim.text(), "one two\n");
        assert_eq!(vim.cursor(), (1, 5));
        vim.press("0P");
        assert_eq!(vim.text(), "twoone two\n");
    }

    #[test]
    fn putting_an_empty_register_says_so_and_changes_nothing() {
        let mut vim = Vim::new("text\n");
        vim.press("\"zp");
        assert_eq!(vim.text(), "text\n");
        assert_eq!(vim.editor.message, "nothing to put");
    }

    #[test]
    fn a_register_name_shows_in_the_half_typed_command() {
        let mut vim = Vim::new("one\n");
        vim.press("\"");
        assert_eq!(vim.keys.pending_text(), "\"");
        vim.press("a");
        assert_eq!(vim.keys.pending_text(), "\"a");
        vim.press("2d");
        assert_eq!(vim.keys.pending_text(), "2\"ad");
        vim.press("<esc>");
        assert_eq!(vim.keys.pending_text(), "");
    }

    #[test]
    fn x_never_joins_two_lines() {
        let mut vim = Vim::new("ab\ncd\n");
        // More deletions asked for than the line has left.
        vim.press("5x");
        assert_eq!(vim.text(), "\ncd\n");
    }

    #[test]
    fn a_put_is_one_undo_step() {
        let mut vim = Vim::new("one\n");
        vim.press("yyp");
        assert_eq!(vim.text(), "one\none\n");
        vim.press("u");
        assert_eq!(vim.text(), "one\n");
    }

    #[test]
    fn space_b_opens_the_buffer_picker() {
        let mut vim = Vim::new("hello\n");
        vim.press("<space>");
        assert_eq!(vim.keys.pending_text(), "<space>");
        vim.press("b");
        assert!(vim.editor.picker.is_some());
        assert_eq!(vim.keys.pending_text(), "");
    }

    #[test]
    fn the_open_picker_swallows_normal_mode_keys() {
        let mut vim = Vim::new("hello\n");
        vim.press("<space>b");
        // `dd` would delete a line; here it is just two characters of a query.
        vim.press("dd");
        assert_eq!(vim.editor.view().doc.text.to_string(), "hello\n");
        assert_eq!(vim.editor.picker.as_ref().unwrap().query, "dd");
        vim.press("<esc>");
        assert!(vim.editor.picker.is_none());
    }

    #[test]
    fn a_space_leader_followed_by_nothing_known_is_dropped() {
        let mut vim = Vim::new("hello\n");
        vim.press("<space>z");
        assert!(vim.editor.picker.is_none());
        assert_eq!(vim.keys.pending_text(), "");
    }


    #[test]
    fn v_enters_visual_mode_and_selects_the_character_under_the_cursor() {
        let mut vim = Vim::new("hello\n");
        vim.press("v");
        assert_eq!(vim.editor.mode, Mode::Visual);
        assert_eq!(vim.editor.selection_range(), Some((0, 1)));
    }

    #[test]
    fn a_motion_in_visual_mode_drags_the_selection() {
        let mut vim = Vim::new("hello world\n");
        vim.press("ve");
        // `e` stops on the last character of the word, and it is selected.
        assert_eq!(vim.editor.selection_range(), Some((0, 5)));
        vim.press("w");
        assert_eq!(vim.editor.selection_range(), Some((0, 7)));
    }

    #[test]
    fn a_count_works_on_a_visual_motion() {
        let mut vim = Vim::new("one two three four\n");
        vim.press("v2w");
        assert_eq!(vim.editor.selection_range(), Some((0, 9)));
    }

    #[test]
    fn escape_leaves_visual_mode_and_the_selection() {
        let mut vim = Vim::new("hello\n");
        vim.press("vll<esc>");
        assert_eq!(vim.editor.mode, Mode::Normal);
        assert_eq!(vim.editor.selection_range(), None);
        assert_eq!(vim.editor.cursor_coords(), (0, 2));
    }

    #[test]
    fn v_twice_leaves_visual_mode() {
        let mut vim = Vim::new("hello\n");
        vim.press("vv");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn d_in_visual_mode_deletes_the_selection_and_returns_to_normal() {
        let mut vim = Vim::new("hello world\n");
        vim.press("vlld");
        assert_eq!(vim.editor.view().doc.text.to_string(), "lo world\n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn c_in_visual_mode_deletes_and_starts_typing() {
        let mut vim = Vim::new("hello world\n");
        vim.press("vecbye<esc>");
        assert_eq!(vim.editor.view().doc.text.to_string(), "bye world\n");
    }

    #[test]
    fn y_in_visual_mode_keeps_the_text_and_the_cursor_where_it_started() {
        let mut vim = Vim::new("hello world\n");
        vim.press("llvlly");
        assert_eq!(vim.editor.registers.get(None).text, "llo");
        assert!(!vim.editor.registers.get(None).linewise);
        assert_eq!(vim.editor.cursor_coords(), (0, 2));
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn o_swaps_which_end_of_the_selection_moves() {
        let mut vim = Vim::new("hello world\n");
        vim.press("llvll");
        assert_eq!(vim.editor.selection_range(), Some((2, 5)));
        vim.press("ohh");
        // The other end moved instead, so the selection grew to the left.
        assert_eq!(vim.editor.selection_range(), Some((0, 5)));
    }

    #[test]
    fn visual_line_mode_takes_whole_lines() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("V");
        assert_eq!(vim.editor.selection_range(), Some((0, 4)));
        vim.press("j");
        assert_eq!(vim.editor.selection_range(), Some((0, 8)));
        vim.press("d");
        assert_eq!(vim.editor.view().doc.text.to_string(), "three\n");
    }

    #[test]
    fn a_visual_line_yank_puts_back_as_lines() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("Vy");
        assert!(vim.editor.registers.get(None).linewise);
        vim.press("jp");
        assert_eq!(vim.editor.view().doc.text.to_string(), "one\ntwo\none\n");
    }

    #[test]
    fn capitals_act_on_whole_lines_from_character_visual() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("vjD");
        assert_eq!(vim.editor.view().doc.text.to_string(), "three\n");
    }

    #[test]
    fn p_in_visual_mode_replaces_the_selection() {
        let mut vim = Vim::new("hello world\n");
        vim.press("yw");                  // yank "hello " into the unnamed register
        vim.press("wvey");                // now yank "world"
        vim.press("0vep");                // replace "hello" with "world"
        assert_eq!(vim.editor.view().doc.text.to_string(), "world world\n");
    }

    #[test]
    fn a_named_register_works_in_visual_mode() {
        let mut vim = Vim::new("hello\n");
        // The register is named in visual mode, just before the operator.
        vim.press("ve");
        vim.press("\"ay");
        assert_eq!(vim.editor.registers.get(Some('a')).text, "hello");
    }

    #[test]
    fn gg_and_g_extend_the_selection_in_visual_mode() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("jvG");
        // `G` lands on the first non-blank of the last line, as it does in
        // normal mode, so the selection reaches that character and stops.
        assert_eq!(vim.editor.selection_range(), Some((4, 9)));
        vim.press("gg");
        // The head moved to the top; the anchor is still where `v` began.
        assert_eq!(vim.editor.selection_range(), Some((0, 5)));
    }

    #[test]
    fn a_selection_past_the_end_of_a_line_is_clamped() {
        let mut vim = Vim::new("hi\nlonger line\n");
        vim.press("jv$");
        assert_eq!(vim.editor.cursor_coords(), (1, 10));
        vim.press("k");
        // Line 0 is shorter, so the head clamps onto its last character.
        assert_eq!(vim.editor.cursor_coords(), (0, 1));
    }


    #[test]
    fn shift_and_an_arrow_starts_a_selection_in_normal_mode() {
        let mut vim = Vim::new("hello world\n");
        vim.press("<S-right><S-right>");
        assert_eq!(vim.editor.mode, Mode::Visual);
        assert_eq!(vim.editor.selection_range(), Some((0, 3)));
    }

    #[test]
    fn a_shifted_selection_can_be_operated_on_like_any_other() {
        let mut vim = Vim::new("hello world\n");
        vim.press("<S-end>d");
        assert_eq!(vim.editor.view().doc.text.to_string(), "\n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn a_plain_arrow_after_a_shifted_one_keeps_dragging_the_selection() {
        // Once visual mode has started, the shift is no longer what matters.
        let mut vim = Vim::new("hello world\n");
        vim.press("<S-right><right>");
        assert_eq!(vim.editor.selection_range(), Some((0, 3)));
    }

    #[test]
    fn an_unshifted_arrow_in_normal_mode_still_just_moves() {
        let mut vim = Vim::new("hello\n");
        vim.press("<right><right>");
        assert_eq!(vim.editor.mode, Mode::Normal);
        assert_eq!(vim.editor.selection_range(), None);
        assert_eq!(vim.editor.cursor_coords(), (0, 2));
    }


    #[test]
    fn leaving_insert_mode_with_a_selection_lands_in_visual_mode() {
        let mut vim = Vim::new("hello world\n");
        vim.press("i<S-right><S-right><S-right><S-right><S-right><esc>");
        assert_eq!(vim.editor.mode, Mode::Visual);
        // The same five characters that were selected while typing.
        assert_eq!(vim.editor.selection_range(), Some((0, 5)));
    }

    #[test]
    fn a_selection_made_in_insert_mode_can_be_yanked_and_put() {
        let mut vim = Vim::new("hello world\n");
        vim.press("i<S-right><S-right><S-right><S-right><S-right><esc>");
        vim.press("y");
        assert_eq!(vim.editor.registers.get(None).text, "hello");
        vim.press("$p");
        assert_eq!(vim.editor.view().doc.text.to_string(), "hello worldhello\n");
    }

    #[test]
    fn a_backwards_selection_from_insert_mode_keeps_its_characters() {
        let mut vim = Vim::new("hello world\n");
        // Start at column 5 and select back over "llo".
        vim.press("llllli<S-left><S-left><S-left><esc>");
        assert_eq!(vim.editor.mode, Mode::Visual);
        vim.press("y");
        assert_eq!(vim.editor.registers.get(None).text, "llo");
    }

    #[test]
    fn leaving_insert_mode_with_no_selection_still_just_steps_back() {
        let mut vim = Vim::new("hello\n");
        vim.press("ixy<esc>");
        assert_eq!(vim.editor.mode, Mode::Normal);
        assert_eq!(vim.editor.selection_range(), None);
        assert_eq!(vim.editor.cursor_coords(), (0, 1));
    }


    #[test]
    fn leader_question_mark_opens_the_help() {
        let mut vim = Vim::new("hello\n");
        vim.press("<space>?");
        let picker = vim.editor.picker.as_ref().expect("a picker");
        assert_eq!(picker.item_count(), BINDINGS.len());
    }

    #[test]
    fn the_help_can_be_searched_by_key_or_by_what_it_does() {
        let mut vim = Vim::new("hello\n");
        vim.press("<space>?");
        vim.press("grep");
        let picker = vim.editor.picker.as_ref().unwrap();
        let hit = picker.item(picker.matches().first().expect("a match for \"grep\""));
        assert!(hit.text.contains("<space>s"), "{}", hit.text);

        vim.press("<C-u>");
        vim.press("gn");
        let picker = vim.editor.picker.as_ref().unwrap();
        let hit = picker.item(picker.matches().first().expect("a match for \"gn\""));
        assert!(hit.text.contains("buffer"), "{}", hit.text);
    }

    #[test]
    fn choosing_a_help_line_just_closes_it() {
        let mut vim = Vim::new("hello\n");
        vim.press("<space>?");
        vim.press("<cr>");
        assert!(vim.editor.picker.is_none());
        assert_eq!(vim.editor.view().doc.text.to_string(), "hello\n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn every_documented_leader_key_opens_what_it_says() {
        // The help is written by hand, so this is the check that it has not
        // drifted from the keymap.
        for binding in BINDINGS.iter().filter(|b| b.keys.starts_with("<space>")) {
            let key = binding.keys.trim_start_matches("<space>");
            let mut vim = Vim::new("hello\n");
            let numbers = vim.editor.numbers;
            vim.press(&format!("<space>{key}"));
            let did_something = vim.editor.picker.is_some() || vim.editor.numbers != numbers;
            assert!(did_something, "{} did nothing", binding.keys);
        }
    }

}
