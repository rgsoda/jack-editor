use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::editor::{Editor, Mode};
use crate::object;
use crate::register::SYSTEM;
use crate::view::{Find, Move, Reveal, Screen, Selection};
use crate::window::Direction;

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
    Binding { keys: "f{c} F{c}", what: "to the next, previous {c} on the line", mode: "normal" },
    Binding { keys: "t{c} T{c}", what: "up to it, from either side", mode: "normal" },
    Binding { keys: "; ,", what: "repeat the last f/t, reverse it", mode: "normal" },
    Binding { keys: "0 ^ $", what: "line start, first non-blank, line end", mode: "normal" },
    Binding { keys: "gg G", what: "first line, last line", mode: "normal" },
    Binding { keys: "/ ?", what: "search forward, backward", mode: "normal" },
    Binding { keys: ";", what: "a command: w q e set noh, or a line number", mode: "normal" },
    Binding { keys: ":", what: "a command: w q e set noh, or a line number", mode: "normal" },
    Binding { keys: "n N", what: "repeat the search, reverse it", mode: "normal" },
    Binding { keys: "*", what: "search for the word under the cursor", mode: "normal" },
    Binding { keys: "%", what: "jump to the matching bracket", mode: "normal" },
    Binding { keys: "gd gD", what: "go to the definition: in scope, in the file", mode: "normal" },
    Binding { keys: "^o ^i", what: "back, forward along the jump list", mode: "normal" },
    Binding { keys: "{n}G", what: "go to line n", mode: "normal" },
    Binding { keys: "^d ^u", what: "half page down, up", mode: "normal" },
    Binding { keys: "{ }", what: "paragraph back, forward (d} y{ too)", mode: "normal" },
    Binding { keys: "zz zt zb", what: "this line to the middle, top, bottom", mode: "normal" },
    Binding { keys: "H M L", what: "top, middle, bottom of the screen", mode: "normal" },
    Binding { keys: "^e ^y", what: "scroll a line down, up, cursor still", mode: "normal" },
    Binding { keys: "pgdn pgup", what: "page down, up", mode: "normal" },

    Binding { keys: "i I", what: "insert here, at first non-blank", mode: "normal" },
    Binding { keys: "a A", what: "insert after cursor, at line end", mode: "normal" },
    Binding { keys: "o O", what: "open a line below, above", mode: "normal" },
    Binding { keys: "x", what: "delete character", mode: "normal" },
    Binding { keys: "r{c} ~", what: "replace the character, swap its case", mode: "normal" },
    Binding { keys: "J", what: "join the line below onto this one", mode: "normal" },
    Binding { keys: "D C", what: "delete, change to line end", mode: "normal" },
    Binding { keys: "dd d{motion}", what: "delete lines, over a motion", mode: "normal" },
    Binding { keys: "dG dgg d{n}G", what: "over lines, to the ends of the file", mode: "normal" },
    Binding { keys: "cc c{motion}", what: "change lines, over a motion", mode: "normal" },
    Binding { keys: "yy Y y{motion}", what: "yank lines, over a motion", mode: "normal" },
    Binding { keys: "p P", what: "put after, before the cursor", mode: "normal" },
    Binding { keys: ">> << >{motion}", what: "indent, dedent lines", mode: "normal" },
    Binding { keys: "== ={motion}", what: "re-indent: ask the grammar where lines go", mode: "normal" },
    Binding { keys: "gcc gc{motion}", what: "comment lines out, or back in", mode: "normal" },
    Binding { keys: "d c y + iw aw", what: "the word under the cursor, with its space", mode: "normal" },
    Binding { keys: "d c y + iW aW", what: "the same, counting punctuation as word", mode: "normal" },
    Binding { keys: "d c y + i\" i' i`", what: "inside the quotes (a\" takes them too)", mode: "normal" },
    Binding { keys: "d c y + i( i[ i{ i<", what: "inside the brackets (a( takes them too)", mode: "normal" },
    Binding { keys: "d c y + ip ap", what: "the paragraph, with the blank line after", mode: "normal" },
    Binding { keys: "\"x", what: "use register x (\"X appends)", mode: "normal" },
    Binding { keys: "\"+y \"+p", what: "yank to, put from the system clipboard", mode: "normal" },
    Binding { keys: ".", what: "do the last change again ({n}. counts it anew)", mode: "normal" },
    Binding { keys: "u ^r", what: "undo, redo", mode: "normal" },
    Binding { keys: "{count}", what: "repeat the next command", mode: "normal" },
    Binding { keys: "esc", what: "abandon a command, stop highlighting matches", mode: "normal" },

    Binding { keys: "v V", what: "select characters, whole lines", mode: "normal" },
    Binding { keys: "gv", what: "select what was selected last", mode: "normal" },
    Binding { keys: "shift+arrows", what: "select, entering visual mode", mode: "normal" },
    Binding { keys: "gn gp", what: "next, previous buffer", mode: "normal" },
    Binding { keys: "^w s ^w v", what: "split the window: below, beside", mode: "normal" },
    Binding { keys: "^w h j k l", what: "go to the window left, below, above, right", mode: "normal" },
    Binding { keys: "^w w ^w W", what: "next, previous window", mode: "normal" },
    Binding { keys: "^w c ^w q", what: "close the window (q quits the last)", mode: "normal" },
    Binding { keys: "^w o", what: "close every other window", mode: "normal" },
    Binding { keys: "{n}gn", what: "go to buffer n", mode: "normal" },

    Binding { keys: "<space>b", what: "pick a buffer", mode: "normal" },
    Binding { keys: "<space>f", what: "pick a file", mode: "normal" },
    Binding { keys: "<space>s", what: "grep the working directory", mode: "normal" },
    Binding { keys: "^c ^x ^v", what: "copy, cut, paste the line (the system clipboard)", mode: "normal" },
    Binding { keys: "<space>d", what: "pick a definition in this buffer", mode: "normal" },
    Binding { keys: "<space>?", what: "this help", mode: "normal" },
    Binding { keys: "<space>n", what: "cycle line numbers", mode: "normal" },

    Binding { keys: "^c ^x ^v", what: "copy, cut, paste over the selection", mode: "visual" },
    Binding { keys: "any motion", what: "drag the selection", mode: "visual" },
    Binding { keys: "o", what: "swap which end moves", mode: "visual" },
    Binding { keys: "iw i\" i( ip ...", what: "select a text object (a for around)", mode: "visual" },
    Binding { keys: "v V", what: "characters, lines, or back to normal", mode: "visual" },
    Binding { keys: "d x", what: "delete the selection", mode: "visual" },
    Binding { keys: "J", what: "join the selected lines", mode: "visual" },
    Binding { keys: "c s", what: "delete it and start typing", mode: "visual" },
    Binding { keys: "y", what: "yank the selection", mode: "visual" },
    Binding { keys: "> <", what: "indent, dedent the lines ({n} steps)", mode: "visual" },
    Binding { keys: ":", what: "a command over the selection ('<,'>)", mode: "visual" },
    Binding { keys: "=", what: "re-indent the lines", mode: "visual" },
    Binding { keys: "gc", what: "comment the lines out, or back in", mode: "visual" },
    Binding { keys: "p P", what: "replace it with a register", mode: "visual" },
    Binding { keys: "D X Y C S", what: "the same, on whole lines", mode: "visual" },
    Binding { keys: "esc", what: "back to normal mode", mode: "visual" },

    Binding { keys: "esc", what: "back to normal mode", mode: "insert" },
    Binding { keys: "^n ^p", what: "complete the word: next, previous", mode: "insert" },
    Binding { keys: "(typing)", what: "the popup comes up on its own (:set autocomplete)", mode: "insert" },
    Binding { keys: "up down", what: "next, previous completion", mode: "insert" },
    Binding { keys: "enter tab ^y", what: "accept the selected completion", mode: "insert" },
    Binding { keys: "^t ^d", what: "indent, dedent this line", mode: "insert" },
    Binding { keys: "^v ^c ^x", what: "paste here, copy the line, cut the line", mode: "insert" },
    Binding { keys: "^a ^e ^f ^b ^n ^p", what: "emacs: motions (:set emacs)", mode: "insert" },
    Binding { keys: "M-f M-b", what: "emacs: word forward, back", mode: "insert" },
    Binding { keys: "^k ^u ^w M-d", what: "emacs: kill to line end, start, word", mode: "insert" },
    Binding { keys: "^y ^t ^g", what: "emacs: put back, transpose, normal mode", mode: "insert" },
    Binding { keys: "M-/", what: "emacs: complete the word", mode: "insert" },
    Binding { keys: "esc ^e", what: "close the completion popup", mode: "insert" },
    Binding { keys: "shift+arrows", what: "select while typing", mode: "insert" },
    Binding { keys: "backspace delete", what: "delete a grapheme, or the selection", mode: "insert" },
    Binding { keys: "enter", what: "split the line, keeping the indent", mode: "insert" },

    Binding { keys: "tab shift-tab", what: "complete the command, option or path", mode: "command" },
    Binding { keys: ":w [path] :w!", what: "write, or write over a changed file", mode: "command" },
    Binding { keys: ":q :q! :wq :x", what: "quit, discard changes, write and quit", mode: "command" },
    Binding { keys: ":e path :e!", what: "open a file, reload this one", mode: "command" },
    Binding { keys: ":sp [path] :vs [path]", what: "split: below, beside - this file or another", mode: "command" },
    Binding { keys: ":close :only", what: "close this window, every other window", mode: "command" },
    Binding { keys: ":q (windows)", what: "with more than one window, closes this one", mode: "command" },
    Binding { keys: ":s/old/new/", what: "substitute on this line (g: every match)", mode: "command" },
    Binding { keys: ":%s/old/new/g", what: "the whole file ({n},{m}s and '<,'> too)", mode: "command" },
    Binding { keys: ":s//new/", what: "an empty pattern is the last search", mode: "command" },
    Binding { keys: ":set number", what: "nonumber, relativenumber, hybrid", mode: "command" },
    Binding { keys: ":set trim", what: "notrim: strip trailing space on save", mode: "command" },
    Binding { keys: ":set glyphs", what: "noglyphs: nerd font status line, or ascii", mode: "command" },
    Binding { keys: ":config", what: "open the config file, writing the defaults first", mode: "command" },
    Binding { keys: ":set shiftwidth=4", what: "how wide one indent step is", mode: "command" },
    Binding { keys: ":set expandtab", what: "noexpandtab: indent with spaces or tabs", mode: "command" },
    Binding { keys: ":set emacs", what: "noemacs: emacs chords in insert mode", mode: "command" },
    Binding { keys: ":set autoindent", what: "noautoindent: indent new lines by the grammar", mode: "command" },
    Binding { keys: ":set autocomplete=2", what: "noautocomplete: word length that pops the list", mode: "command" },
    Binding { keys: ":set semicolon=find", what: "command: make ; open the command line", mode: "command" },
    Binding { keys: ":set tabline=auto", what: "off, auto, always: list buffers along the top", mode: "command" },
    Binding { keys: ":noh", what: "stop highlighting matches", mode: "command" },
    Binding { keys: ":{n}", what: "go to line n", mode: "command" },

    Binding { keys: "enter esc", what: "accept, cancel the search", mode: "prompt" },
    Binding { keys: "backspace ^w ^u", what: "delete a character, word, all", mode: "prompt" },

    Binding { keys: "any character", what: "narrow the list, or search", mode: "picker" },
    Binding { keys: "^n ^p tab arrows", what: "next, previous match", mode: "picker" },
    Binding { keys: "enter", what: "choose", mode: "picker" },
    Binding { keys: "backspace ^w ^u", what: "delete a character, word, the query", mode: "picker" },
    Binding { keys: "esc ^c", what: "close", mode: "picker" },
];

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Continue,
    /// `force` is `:q!`: leave even with unsaved changes.
    Quit { force: bool },
}

/// What a half-typed command is waiting for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// An operator waiting for the motion it applies to: `d`, `c`, `y`, and
    /// the two that move lines sideways rather than taking them away.
    Delete,
    Change,
    Yank,
    Indent,
    Dedent,
    /// `=`, which asks the grammar where the lines belong.
    Reindent,
    /// `gc`, which comments the lines out, or back in.
    Comment,
    /// The `g` prefix, waiting for `gg` and the rest. `operator` is the key of
    /// the operator waiting for it - `dgg` deletes to the top of the file -
    /// or `None` when the `g` command is the whole of it.
    Go { operator: Option<char> },
    /// `r`, waiting for the character to put there.
    Replace,
    /// `z`, waiting for where to put the cursor's line: `zz`, `zt`, `zb`.
    Reveal,
    /// The space leader, waiting for which picker to open.
    Leader,
    /// `^w`, waiting for what to do with the windows.
    Window,
    /// The `"` prefix, waiting for the register name.
    Register,
    /// `f`, `F`, `t` or `T`, waiting for the character to look for.
    /// `operator` is the key of the operator it will be handed to, or `None`
    /// when the find is the whole command.
    Find { operator: Option<char>, till: bool, backward: bool },
    /// `i` or `a`, waiting for the key that names the text object. `operator`
    /// is the key of the operator the object will be handed to, or `None` in
    /// visual mode, where selecting it is the whole command.
    Object { operator: Option<char>, around: bool },
}

/// Input state that outlives a single keypress: a count being typed, and any
/// operator or prefix waiting for the rest of its command.
#[derive(Default)]
pub struct Keys {
    count: Option<usize>,
    pending: Option<Pending>,
    /// The keys of the command being typed, kept so `.` can do it again. A
    /// command is its keystrokes: replaying them repeats whatever they meant,
    /// including the text an insert put in, and nothing has to describe what
    /// it did in some second language.
    recording: Vec<KeyEvent>,
    /// What the document looked like when the recording began. A command that
    /// leaves it alone was a movement, and `.` is not interested in it.
    recorded_at: Option<(usize, usize, usize)>,
    /// The last command that changed the buffer, ready to be replayed.
    change: Vec<KeyEvent>,
    /// True while `.` is replaying: nothing is recorded, or the repeat would
    /// become the thing to repeat.
    replaying: bool,
    /// Set by the commands that change the buffer without being a change you
    /// would want done twice - undo and redo.
    not_a_change: bool,
    /// Whether an undo group is open for the command being typed.
    grouped: bool,
    /// The register `"x` named for the command being typed.
    register: Option<char>,
}

impl Keys {
    pub fn handle(&mut self, editor: &mut Editor, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // An open picker or prompt owns the keyboard, including the chords
        // below: `^s` while typing a pattern is an `s`, not a save.
        if editor.picker.is_some() {
            self.forget(editor);
            editor.picker_input(key);
            return Action::Continue;
        }
        if editor.prompt.is_some() {
            self.forget(editor);
            editor.prompt_input(key);
            // `:q` has to reach the run loop, which is the only thing that can
            // actually stop.
            return match editor.quit.take() {
                Some(force) => Action::Quit { force },
                None => Action::Continue,
            };
        }

        // Chords that mean the same thing in either mode.
        if ctrl {
            match key.code {
                KeyCode::Char('q') => return Action::Quit { force: false },
                KeyCode::Char('s') => {
                    editor.save();
                    return Action::Continue;
                }
                _ => {}
            }
        }

        // A command typed with `"+` writes to the system clipboard, whatever
        // command it turns out to be: the yanks and the deletes both, and
        // without every one of them having to know. Compared rather than
        // assumed, so that `"+p` - which only reads - does not copy back out
        // what it just pasted.
        let watched = (self.register == Some(SYSTEM)).then(|| editor.system_register());

        // `.` is the one key that must not end up in the recording: it is the
        // command that plays it back. Any other reading of `.` - the character
        // `f.` is looking for, the one `r.` puts down - has a pending state
        // waiting for it, and is recorded like anything else.
        let repeat = !self.replaying
            && !ctrl
            && key.code == KeyCode::Char('.')
            && editor.mode == Mode::Normal
            && self.pending.is_none();
        if !self.replaying && !repeat {
            if self.recording.is_empty() {
                self.recorded_at = Some(editor.revision());
                // The same boundary serves undo: one command, one step.
                editor.begin_undo_group();
                self.grouped = true;
            }
            self.recording.push(key);
        }

        match editor.mode {
            Mode::Normal => self.normal(editor, key, ctrl),
            Mode::Insert => insert(editor, key, ctrl),
            Mode::Visual | Mode::VisualLine => self.visual(editor, key, ctrl),
        }

        if !self.replaying {
            self.remember(editor, repeat);
        }

        if let Some(before) = watched
            && editor.system_register() != before
        {
            editor.push_clipboard();
        }
        // `^w q` on the last window is a quit, like `:q`.
        match editor.quit.take() {
            Some(force) => Action::Quit { force },
            None => Action::Continue,
        }
    }

    /// Throw away the half-recorded command: it was not a change, or it was
    /// one nobody wants repeated. Closes its undo step either way, since the
    /// command is over whatever became of its keys.
    fn forget(&mut self, editor: &mut Editor) {
        self.recording.clear();
        self.recorded_at = None;
        if self.grouped {
            editor.end_undo_group();
            self.grouped = false;
        }
    }

    /// After a key: decide whether what has been recorded is finished, and
    /// whether it changed anything.
    fn remember(&mut self, editor: &mut Editor, repeat: bool) {
        if repeat || std::mem::take(&mut self.not_a_change) {
            self.forget(editor);
            return;
        }
        // Still being typed: a count or a register on its own, an operator
        // waiting for its motion, a find waiting for its character, insert and
        // visual mode, which end when the mode does. Every one of these keeps
        // its keys - the count especially, since `.` repeats `2dw` as `2dw`.
        if self.count.is_some()
            || self.register.is_some()
            || self.pending.is_some()
            || editor.mode != Mode::Normal
            || editor.picker.is_some()
            || editor.prompt.is_some()
        {
            return;
        }
        if self.recording.is_empty() {
            return;
        }
        if self.recorded_at != Some(editor.revision()) {
            self.change = std::mem::take(&mut self.recording);
        }
        self.forget(editor);
    }

    /// `.`: the last command that changed the buffer, done again here. A count
    /// replaces the one it was typed with, as vim does - `3.` is that edit
    /// three times, not three repeats of it.
    fn repeat_change(&mut self, editor: &mut Editor, count: Option<usize>) {
        if self.change.is_empty() {
            editor.message = "nothing to repeat".into();
            return;
        }
        let keys = match count {
            Some(n) => {
                let mut rest = self.change.as_slice();
                // Only a leading count is replaced: a `0` at the front is the
                // motion to the start of the line, and no count starts with it.
                while let Some(KeyEvent { code: KeyCode::Char(c), .. }) = rest.first()
                    && c.is_ascii_digit()
                    && (*c != '0' || rest.len() < self.change.len())
                {
                    rest = &rest[1..];
                }
                let count = n.to_string();
                let digits = count
                    .chars()
                    .map(|c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
                digits.chain(rest.iter().copied()).collect()
            }
            None => self.change.clone(),
        };

        self.count = None;
        self.register = None;
        self.pending = None;
        self.replaying = true;
        // The repeat is one command, so it is one undo step - even when the
        // keys it plays back are an insert that made several.
        editor.begin_undo_group();
        for key in keys {
            self.handle(editor, key);
        }
        editor.end_undo_group();
        self.replaying = false;
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
        // A half-typed find, waiting for the character to look for: `dt` is
        // what the user has pressed and so what the indicator should say.
        // Drawing runs on every frame, so there is nothing here that can fail.
        if let Some(Pending::Find { operator, till, backward }) = self.pending {
            text.push_str(&operator.map(operator_text).unwrap_or_default());
            text.push(match (till, backward) {
                (false, false) => 'f',
                (false, true) => 'F',
                (true, false) => 't',
                (true, true) => 'T',
            });
            return text;
        }
        if let Some(Pending::Object { operator, around }) = self.pending {
            text.push_str(&operator.map(operator_text).unwrap_or_default());
            text.push(match around {
                true => 'a',
                false => 'i',
            });
            return text;
        }
        text.push_str(match self.pending {
            Some(Pending::Delete) => "d",
            Some(Pending::Change) => "c",
            Some(Pending::Yank) => "y",
            Some(Pending::Indent) => ">",
            Some(Pending::Dedent) => "<",
            Some(Pending::Reindent) => "=",
            Some(Pending::Comment) => "gc",
            Some(Pending::Go { .. }) => "g",
            Some(Pending::Replace) => "r",
            Some(Pending::Reveal) => "z",
            Some(Pending::Leader) => "<space>",
            Some(Pending::Window) => "^w",
            Some(Pending::Register) => "\"",
            // Both are handled above, and neither is worth a panic in the
            // middle of a redraw if a later one ever isn't.
            Some(Pending::Find { .. }) | Some(Pending::Object { .. }) => {
                debug_assert!(false, "handled above");
                ""
            }
            None => "",
        });
        text
    }

    fn normal(&mut self, editor: &mut Editor, key: KeyEvent, ctrl: bool) {
        if key.code == KeyCode::Esc {
            self.count = None;
            self.pending = None;
            self.register = None;
            editor.clear_search_highlight();
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
            // `r` is the only command that takes a bare character, so this
            // comes before anything that reads keys as commands.
            Some(Pending::Replace) => {
                if let KeyCode::Char(c) = key.code
                    && !ctrl
                    && !editor.replace_char(c, count.unwrap_or(1))
                {
                    editor.message = "not that many characters on the line".into();
                }
                self.finish();
            }
            Some(Pending::Reveal) => {
                reveal(editor, key.code);
                self.finish();
            }
            // An operator waiting on `g` wants a line to work to, not a jump:
            // `dgg` deletes from here to the top of the file.
            Some(Pending::Go { operator: Some(operator) }) => {
                if key.code == KeyCode::Char('g') {
                    self.operate_lines(editor, operator, count.unwrap_or(1) - 1);
                }
                self.finish();
            }
            Some(Pending::Go { operator: None }) => {
                match key.code {
                    KeyCode::Char('g') => {
                        editor.push_jump();
                        editor.goto_line(count.unwrap_or(1) - 1);
                        editor.clamp_cursor();
                    }
                    KeyCode::Char('v') => editor.reselect(),
                    // An operator of its own, so it keeps the count typed
                    // before it and waits for a motion: `3gcc`, `gcap`.
                    KeyCode::Char('c') => {
                        self.pending = Some(Pending::Comment);
                        return;
                    }
                    KeyCode::Char('d') => editor.goto_definition(true),
                    KeyCode::Char('D') => editor.goto_definition(false),
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
            Some(Pending::Window) => {
                window_command(editor, key);
                // Moving between windows changes nothing, and `.` must not
                // take it for a change because the buffer it is in did.
                self.not_a_change = true;
                self.finish();
            }
            Some(Pending::Leader) => {
                match key.code {
                    KeyCode::Char('b') => editor.open_buffer_picker(),
                    KeyCode::Char('f') => editor.open_file_picker(),
                    KeyCode::Char('s') => editor.open_grep_picker(),
                    KeyCode::Char('d') => editor.open_symbol_picker(),
                    KeyCode::Char('?') => editor.open_help_picker(),
                    KeyCode::Char('n') => editor.cycle_numbers(),
                    _ => {}
                }
                self.finish();
            }
            Some(Pending::Register) => unreachable!("handled above"),
            Some(Pending::Find { operator, till, backward }) => {
                self.apply_find(editor, operator, till, backward, key, count.unwrap_or(1));
                self.finish();
            }
            Some(Pending::Object { operator, around }) => {
                self.apply_object(editor, operator, around, key);
                self.finish();
            }
            // `f` and friends after an operator are the same motion they are
            // on their own; the operator waits for the character too.
            Some(operator) if find_for(key.code, ctrl).is_some() => {
                let (till, backward) = find_for(key.code, ctrl).expect("checked above");
                self.pending = Some(Pending::Find {
                    operator: Some(operator_key(operator)),
                    till,
                    backward,
                });
            }
            // `;` and `,` repeat the last find, and an operator can take that
            // as its motion too: `d;` deletes to wherever `;` would have gone.
            Some(operator)
                if matches!(key.code, KeyCode::Char(';') | KeyCode::Char(',')) && !ctrl =>
            {
                let reverse = key.code == KeyCode::Char(',') && !editor.semicolon_is_command();
                if editor.select_repeat(reverse, count.unwrap_or(1)) {
                    self.operate(editor, Some(operator_key(operator)));
                }
                self.finish();
            }
            // `g` after an operator is `gg`, which is a motion to a line
            // rather than a jump: the operator waits for the second `g`.
            Some(operator) if key.code == KeyCode::Char('g') && !ctrl => {
                self.pending = Some(Pending::Go { operator: Some(operator_key(operator)) });
            }
            // `i` and `a` after an operator are not the insert commands: they
            // start a text object, and the operator waits for its name.
            Some(operator) if matches!(key.code, KeyCode::Char('i') | KeyCode::Char('a')) => {
                self.pending = Some(Pending::Object {
                    operator: Some(operator_key(operator)),
                    around: key.code == KeyCode::Char('a'),
                });
            }
            Some(operator) => {
                self.operator(editor, operator, key, ctrl, count.unwrap_or(1));
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

        if let Some(Pending::Find { operator, till, backward }) = self.pending {
            self.pending = None;
            self.apply_find(editor, operator, till, backward, key, repeat);
            self.finish();
            return;
        }

        if let Some(Pending::Object { operator, around }) = self.pending {
            self.pending = None;
            self.apply_object(editor, operator, around, key);
            self.finish();
            return;
        }

        if self.pending == Some(Pending::Reveal) {
            self.pending = None;
            reveal(editor, key.code);
            self.finish();
            return;
        }

        if let Some(Pending::Go { .. }) = self.pending.take() {
            match key.code {
                KeyCode::Char('g') => editor.goto_line_extending(count.unwrap_or(1) - 1),
                KeyCode::Char('c') => {
                    editor.comment_selection();
                    editor.set_mode(Mode::Normal);
                }
                _ => {}
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
            // The clipboard chords come before the letters they share: `^x` is
            // a cut where `x` is a delete, and a ctrl that fell through to the
            // letter would be the wrong one of the two.
            KeyCode::Char('c') if ctrl => editor.clip_copy(),
            KeyCode::Char('x') if ctrl => editor.clip_cut(),
            KeyCode::Char('v') if ctrl => editor.clip_paste(),

            KeyCode::Char('v') => editor.set_mode(match editor.mode {
                Mode::Visual => Mode::Normal,
                _ => Mode::Visual,
            }),
            KeyCode::Char('V') => editor.set_mode(match editor.mode {
                Mode::VisualLine => Mode::Normal,
                _ => Mode::VisualLine,
            }),
            KeyCode::Char('o') => editor.swap_selection_ends(),
            KeyCode::Char(';') | KeyCode::Char(',') => {
                let reverse = key.code == KeyCode::Char(',') && !editor.semicolon_is_command();
                editor.repeat_to_char(reverse, repeat, true);
            }
            _ if find_for(key.code, ctrl).is_some() => {
                let (till, backward) = find_for(key.code, ctrl).expect("checked above");
                self.pending = Some(Pending::Find { operator: None, till, backward });
                return;
            }
            // A count here is levels, not lines: `3>` moves the selection three
            // steps, as vim does.
            KeyCode::Char('>') => {
                editor.shift_selection(true, repeat);
                editor.set_mode(Mode::Normal);
            }
            KeyCode::Char('<') => {
                editor.shift_selection(false, repeat);
                editor.set_mode(Mode::Normal);
            }
            KeyCode::Char('=') => {
                editor.reindent_selection();
                editor.set_mode(Mode::Normal);
            }
            KeyCode::Char('%') => editor.jump_to_matching_bracket(),
            KeyCode::Char('J') => editor.join_visual(),
            KeyCode::Char('z') => {
                self.pending = Some(Pending::Reveal);
                return;
            }
            KeyCode::Char('H') => editor.goto_screen_line(Screen::Top, repeat, true),
            KeyCode::Char('M') => editor.goto_screen_line(Screen::Middle, repeat, true),
            KeyCode::Char('L') => editor.goto_screen_line(Screen::Bottom, repeat, true),
            // `:` over a selection writes the range in for you, as vim does.
            KeyCode::Char(':') => {
                editor.open_command_over_selection();
                return;
            }

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

            KeyCode::Char('i') | KeyCode::Char('a') => {
                self.pending = Some(Pending::Object {
                    operator: None,
                    around: key.code == KeyCode::Char('a'),
                });
                return;
            }
            KeyCode::Char('g') => {
                self.pending = Some(Pending::Go { operator: None });
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
        // Before the motions, where `^w` would be read as `w`.
        if ctrl && key.code == KeyCode::Char('w') {
            self.pending = Some(Pending::Window);
            return;
        }
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
            // Before the plain letters: `i` and `o` would otherwise match
            // whether or not control was held.
            //
            // `^i` and `tab` are the same byte in a terminal, so they are the
            // same key here whether you think of it as vim's or not.
            KeyCode::Char('c') if ctrl => editor.clip_copy(),
            KeyCode::Char('x') if ctrl => editor.clip_cut(),
            KeyCode::Char('v') if ctrl => editor.clip_paste(),
            KeyCode::Char('o') if ctrl => editor.jump_back(),
            KeyCode::Char('i') if ctrl => editor.jump_forward(),
            KeyCode::Tab => editor.jump_forward(),

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

            // The view rather than the cursor: `z` puts the cursor's line
            // somewhere on the screen, `^e` and `^y` scroll under it, and
            // `H M L` jump to what is already showing.
            KeyCode::Char('z') if !ctrl => {
                self.pending = Some(Pending::Reveal);
                return;
            }
            KeyCode::Char('e') if ctrl => editor.scroll_lines(true, repeat),
            KeyCode::Char('y') if ctrl => editor.scroll_lines(false, repeat),
            KeyCode::Char('H') => editor.goto_screen_line(Screen::Top, repeat, false),
            KeyCode::Char('M') => editor.goto_screen_line(Screen::Middle, repeat, false),
            KeyCode::Char('L') => editor.goto_screen_line(Screen::Bottom, repeat, false),

            // `J` joins, `~` swaps case, `r` waits for the character to put
            // where the cursor is. Small commands, all of them counted.
            KeyCode::Char('J') => editor.join_lines(repeat),
            KeyCode::Char('~') => editor.toggle_case(repeat),
            // `^r` is redo, and lives further down: a chord is not the
            // character `r` is waiting for.
            KeyCode::Char('r') if !ctrl => {
                self.pending = Some(Pending::Replace);
                return;
            }

            KeyCode::Char(':') => editor.open_command(),
            KeyCode::Char('/') => editor.open_search(false),
            KeyCode::Char('?') => editor.open_search(true),
            KeyCode::Char('n') => editor.search_repeat(false, repeat),
            KeyCode::Char('N') => editor.search_repeat(true, repeat),
            KeyCode::Char('*') => editor.search_word_under_cursor(),
            KeyCode::Char('%') => editor.jump_to_matching_bracket(),
            // With `:set semicolon=command` this key is the command line, and
            // `,` takes over repeating the find forwards - the other half of
            // the remap people write by hand.
            KeyCode::Char(';') if editor.semicolon_is_command() => editor.open_command(),
            KeyCode::Char(';') | KeyCode::Char(',') => {
                let reverse = key.code == KeyCode::Char(',') && !editor.semicolon_is_command();
                editor.repeat_to_char(reverse, repeat, false);
            }

            // Undo and redo change the buffer without being a change: `.`
            // after a `u` repeats what you undid, not the undo.
            KeyCode::Char('u') => {
                editor.undo();
                self.not_a_change = true;
            }
            KeyCode::Char('r') if ctrl => {
                editor.redo();
                self.not_a_change = true;
            }
            // The last change, again.
            KeyCode::Char('.') => self.repeat_change(editor, count),

            KeyCode::Char('d') => self.pending = Some(Pending::Delete),
            KeyCode::Char('c') => self.pending = Some(Pending::Change),
            KeyCode::Char('y') => self.pending = Some(Pending::Yank),
            KeyCode::Char('>') => self.pending = Some(Pending::Indent),
            KeyCode::Char('<') => self.pending = Some(Pending::Dedent),
            KeyCode::Char('=') => self.pending = Some(Pending::Reindent),
            KeyCode::Char('g') => self.pending = Some(Pending::Go { operator: None }),
            KeyCode::Char(' ') => self.pending = Some(Pending::Leader),
            _ if find_for(key.code, ctrl).is_some() => {
                let (till, backward) = find_for(key.code, ctrl).expect("checked above");
                self.pending = Some(Pending::Find { operator: None, till, backward });
            }
            KeyCode::Char('G') => {
                // Bare `G` goes to the last line, `{n}G` to line n.
                editor.push_jump();
                let line = count.map_or(editor.last_line(), |n| n - 1);
                editor.goto_line(line);
                editor.clamp_cursor();
            }
            _ => {}
        }
    }

    /// `d`/`c` followed by the motion they apply to. `dd` and `cc` act on whole
    /// lines; everything else selects across the motion and operates on that.
    /// The second half of `diw`: `key` names the object, `operator` the thing
    /// to do with it. In visual mode there is no operator and the object simply
    /// becomes the selection.
    fn apply_object(&mut self, editor: &mut Editor, operator: Option<char>, around: bool, key: KeyEvent) {
        let KeyCode::Char(name) = key.code else {
            return;
        };
        let Some(object) = object::from_key(name) else {
            return;
        };
        if !editor.select_object(object, around) {
            return;
        }
        self.operate(editor, operator);
    }

    /// `f`, `F`, `t` or `T`, once the character to look for has been typed.
    /// With an operator it covers the range it would have moved over; without
    /// one it is a motion, which in visual mode drags the selection.
    fn apply_find(
        &mut self,
        editor: &mut Editor,
        operator: Option<char>,
        till: bool,
        backward: bool,
        key: KeyEvent,
        count: usize,
    ) {
        let KeyCode::Char(target) = key.code else {
            return;
        };
        let find = Find { target, till, backward };
        editor.remember_find(find);
        match operator {
            Some(operator) => {
                if editor.select_to_char(find, count) {
                    self.operate(editor, Some(operator));
                }
            }
            None => editor.move_to_char(find, count, editor.mode.is_visual()),
        }
    }

    /// `dG`, `=gg`, `y5G`: an operator over whole lines, from the line the
    /// cursor is on to another one. Linewise whichever way it runs, which is
    /// what vim does with the motions that cross the file.
    fn operate_lines(&mut self, editor: &mut Editor, operator: char, target: usize) {
        let target = target.min(editor.last_line());
        let (line, _) = editor.cursor_coords();
        let (first, last) = (line.min(target), line.max(target));
        let count = last - first + 1;
        let at = editor.view().sel.head;
        editor.goto_line(first);
        match operator {
            // A yank leaves the cursor where it was, unlike the rest: nothing
            // was taken away for it to fall into.
            'y' => {
                editor.yank_lines(self.register, count);
                editor.view_mut().sel = Selection::point(at);
                editor.clamp_cursor();
            }
            'c' => {
                editor.change_lines(self.register, count);
                editor.set_mode(Mode::Insert);
            }
            '>' => editor.shift_count(true, count),
            '<' => editor.shift_count(false, count),
            '=' => editor.reindent_lines(first, last),
            COMMENT => editor.comment_lines(first, last),
            _ => editor.delete_lines(self.register, count),
        }
    }

    /// Hand a selection - a text object's, or a find's - to the operator that
    /// asked for it.
    fn operate(&mut self, editor: &mut Editor, operator: Option<char>) {
        match operator {
            Some('>') => editor.shift_selection(true, 1),
            Some('<') => editor.shift_selection(false, 1),
            Some('=') => editor.reindent_selection(),
            Some(COMMENT) => editor.comment_selection(),
            Some('y') => editor.yank_selection(self.register),
            Some('c') => {
                // An empty object - `ci(` on `()` - deletes nothing, but the
                // point of the command is still to start typing there.
                editor.delete_selection(self.register);
                editor.set_mode(Mode::Insert);
            }
            Some(_) => editor.delete_selection(self.register),
            // Visual mode keeps the selection it just made.
            None => {}
        }
    }

    fn operator(&mut self, editor: &mut Editor, operator: Pending, key: KeyEvent, ctrl: bool, count: usize) {
        // A chord is not a motion, and `^d` in particular reaches here as a
        // plain `d`, which would make `d^d` delete lines. An operator waiting
        // for a motion and handed a chord gives up instead.
        if ctrl {
            return;
        }
        let doubled = matches!(
            (operator, key.code),
            (Pending::Delete, KeyCode::Char('d'))
                | (Pending::Change, KeyCode::Char('c'))
                | (Pending::Yank, KeyCode::Char('y'))
                | (Pending::Indent, KeyCode::Char('>'))
                | (Pending::Dedent, KeyCode::Char('<'))
                | (Pending::Reindent, KeyCode::Char('='))
                | (Pending::Comment, KeyCode::Char('c'))
        );
        if doubled {
            match operator {
                Pending::Delete => editor.delete_lines(self.register, count),
                Pending::Change => editor.change_lines(self.register, count),
                Pending::Yank => editor.yank_lines(self.register, count),
                Pending::Indent => editor.shift_count(true, count),
                Pending::Dedent => editor.shift_count(false, count),
                Pending::Reindent => {
                    let (line, _) = editor.view().cursor_coords();
                    editor.reindent_lines(line, line + count - 1);
                }
                Pending::Comment => {
                    let (line, _) = editor.view().cursor_coords();
                    editor.comment_lines(line, line + count - 1);
                }
                Pending::Go { .. }
                | Pending::Replace
                | Pending::Reveal
                | Pending::Register
                | Pending::Leader
                | Pending::Window
                | Pending::Find { .. }
                | Pending::Object { .. } => {}
            }
            return;
        }

        // `H`, `M` and `L` are lines on the screen, and an operator over one
        // is linewise, as vim has it: `dL` deletes to the bottom of what is
        // showing.
        if let Some(which) = screen_for(key.code) {
            let target = editor.screen_line(which, self.count.unwrap_or(1));
            self.operate_lines(editor, operator_key(operator), target);
            return;
        }

        // `G` is a line, not a motion: `dG` deletes to the end of the file and
        // `d5G` to line five, both linewise. The raw count is what tells them
        // apart, so it is read here rather than taken as a repeat.
        if key.code == KeyCode::Char('G') {
            let target = self.count.map_or(editor.last_line(), |n| n - 1);
            self.operate_lines(editor, operator_key(operator), target);
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
            // `>j` moves this line and the next one, and leaves them there.
            Pending::Indent => editor.shift_motion(true),
            Pending::Dedent => editor.shift_motion(false),
            Pending::Reindent => editor.reindent_motion(),
            Pending::Comment => editor.comment_motion(),
            _ => editor.delete_selection(self.register),
        }
        if operator == Pending::Change {
            editor.set_mode(Mode::Insert);
        }
    }
}

/// The key that started an operator, for echoing a half-typed command back.
fn operator_key(operator: Pending) -> char {
    match operator {
        Pending::Change => 'c',
        Pending::Yank => 'y',
        Pending::Indent => '>',
        Pending::Dedent => '<',
        Pending::Reindent => '=',
        Pending::Comment => COMMENT,
        _ => 'd',
    }
}

/// `gc` is two keys, and the operators travel as one character: this is the
/// one that stands for it. Not a key any operator is typed with, so it cannot
/// be mistaken for one.
const COMMENT: char = '#';

/// An operator as it was typed, for the indicator.
fn operator_text(operator: char) -> String {
    match operator {
        COMMENT => "gc".to_string(),
        other => other.to_string(),
    }
}

/// The second key of a `^w`, with or without control still held: vim takes
/// `^w ^v` for `^w v`, and so does every hand that has not let go yet.
fn window_command(editor: &mut Editor, key: KeyEvent) {
    match key.code {
        KeyCode::Char('s') | KeyCode::Char('S') => editor.split_window(false, None),
        KeyCode::Char('v') => editor.split_window(true, None),
        KeyCode::Char('h') | KeyCode::Left => editor.focus_direction(Direction::Left),
        KeyCode::Char('j') | KeyCode::Down => editor.focus_direction(Direction::Down),
        KeyCode::Char('k') | KeyCode::Up => editor.focus_direction(Direction::Up),
        KeyCode::Char('l') | KeyCode::Right => editor.focus_direction(Direction::Right),
        KeyCode::Char('w') => editor.focus_next(true),
        KeyCode::Char('W') => editor.focus_next(false),
        KeyCode::Char('c') => {
            editor.close_window();
        }
        KeyCode::Char('q') => {
            if !(editor.windows_open() > 1 && editor.close_window()) {
                editor.quit = Some(false);
            }
        }
        KeyCode::Char('o') => editor.only_window(),
        _ => {}
    }
}

/// The second key of a `z`: where the cursor's line should end up. Anything
/// else is not one of these commands and does nothing, as in vim.
fn reveal(editor: &mut Editor, code: KeyCode) {
    match code {
        KeyCode::Char('z') => editor.reveal(Reveal::Middle),
        KeyCode::Char('t') => editor.reveal(Reveal::Top),
        KeyCode::Char('b') => editor.reveal(Reveal::Bottom),
        _ => {}
    }
}

/// `H`, `M`, `L` as an operator's motion.
fn screen_for(code: KeyCode) -> Option<Screen> {
    match code {
        KeyCode::Char('H') => Some(Screen::Top),
        KeyCode::Char('M') => Some(Screen::Middle),
        KeyCode::Char('L') => Some(Screen::Bottom),
        _ => None,
    }
}

/// Emacs chords in insert mode, under `:set emacs`. True when the key was one
/// of them and insert mode should not also see it.
///
/// Only insert mode: normal mode is the whole point of a modal editor, and
/// `^d` there already means half a page. `M-/` completes, because emacs calls
/// that dabbrev-expand and `^n` is busy being a motion.
fn emacs(editor: &mut Editor, key: KeyEvent, ctrl: bool, alt: bool) -> bool {
    match (key.code, ctrl, alt) {
        (KeyCode::Char('a'), true, _) => editor.move_cursor(Move::LineStart, false),
        (KeyCode::Char('e'), true, _) => editor.move_cursor(Move::LineEnd, false),
        (KeyCode::Char('f'), true, _) => editor.move_cursor(Move::Right, false),
        (KeyCode::Char('b'), true, _) => editor.move_cursor(Move::Left, false),
        (KeyCode::Char('n'), true, _) => editor.move_cursor(Move::Down, false),
        (KeyCode::Char('p'), true, _) => editor.move_cursor(Move::Up, false),
        (KeyCode::Char('f'), _, true) => editor.move_cursor(Move::WordForward, false),
        (KeyCode::Char('b'), _, true) => editor.move_cursor(Move::WordBack, false),

        (KeyCode::Char('d'), true, _) => editor.delete_forward(),
        (KeyCode::Char('h'), true, _) => editor.delete_backward(),
        (KeyCode::Char('k'), true, _) => editor.kill_to_line_end(),
        (KeyCode::Char('u'), true, _) => editor.kill(Move::LineStart),
        (KeyCode::Char('w'), true, _) => editor.kill(Move::WordBack),
        (KeyCode::Char('d'), _, true) => editor.kill(Move::WordForward),
        (KeyCode::Backspace, _, true) => editor.kill(Move::WordBack),
        (KeyCode::Char('y'), true, _) => editor.yank_kill(),
        (KeyCode::Char('t'), true, _) => editor.transpose_chars(),

        // `^g` is emacs for "never mind", which here means normal mode.
        (KeyCode::Char('g'), true, _) => editor.set_mode(Mode::Normal),
        (KeyCode::Char('/'), _, true) => editor.open_completion(false),
        _ => return false,
    }
    true
}

/// Insert mode with the completion popup open. True when the key was the
/// popup's own and insert mode should not also see it.
fn completing(editor: &mut Editor, key: KeyEvent, ctrl: bool) -> bool {
    match (key.code, ctrl) {
        (KeyCode::Char('n'), true) | (KeyCode::Down, _) => editor.completion_step(true),
        (KeyCode::Char('p'), true) | (KeyCode::Up, _) => editor.completion_step(false),
        // `tab` accepts because every other editor taught everyone that; `^y`
        // accepts because vim taught the rest of us. `enter` accepts rather
        // than splitting the line - but only once something is selected, so a
        // popup that came up by itself never swallows a newline.
        (KeyCode::Char('y'), true) | (KeyCode::Tab, _) | (KeyCode::Enter, _) => {
            return editor.accept_completion();
        }
        // Esc closes the popup and leaves you typing rather than leaving insert
        // mode: one escape, one thing undone.
        (KeyCode::Char('e'), true) | (KeyCode::Esc, _) => editor.close_completion(),
        // Typing and deleting keep it open, and it re-filters afterwards.
        (KeyCode::Char(_), false) | (KeyCode::Backspace, _) => return false,
        // Anything else dismisses it and then means what it usually means.
        _ => {
            editor.close_completion();
            return false;
        }
    }
    true
}

/// Whether a key starts a find, and how it stops: `f` and `F` land on the
/// character, `t` and `T` one short of it; `F` and `T` look backwards.
fn find_for(code: KeyCode, ctrl: bool) -> Option<(bool, bool)> {
    if ctrl {
        return None;
    }
    match code {
        KeyCode::Char('f') => Some((false, false)),
        KeyCode::Char('F') => Some((false, true)),
        KeyCode::Char('t') => Some((true, false)),
        KeyCode::Char('T') => Some((true, true)),
        _ => None,
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
        KeyCode::Char('{') => Move::ParagraphBack,
        KeyCode::Char('}') => Move::ParagraphForward,
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

    // The completion popup gets first refusal on the keyboard, but only for the
    // keys that are its own: typing and deleting go through as normal and the
    // popup follows along afterwards.
    if editor.completion.is_some() && completing(editor, key, ctrl) {
        return;
    }

    // Emacs chords next, when they are turned on: half of them are keys insert
    // mode already uses, and with `:set emacs` the emacs meaning wins.
    if editor.emacs && emacs(editor, key, ctrl, alt) {
        editor.update_completion();
        return;
    }

    match key.code {
        KeyCode::Char('c') if ctrl => editor.clip_copy(),
        KeyCode::Char('x') if ctrl => editor.clip_cut(),
        KeyCode::Char('v') if ctrl => editor.clip_paste(),
        KeyCode::Char('n') if ctrl => editor.open_completion(false),
        KeyCode::Char('p') if ctrl => editor.open_completion(true),
        KeyCode::Char('t') if ctrl => editor.shift_current_line(true),
        KeyCode::Char('d') if ctrl => editor.shift_current_line(false),
        KeyCode::Esc => editor.set_mode(Mode::Normal),
        KeyCode::Char(c) if !ctrl && !alt => editor.insert(&c.to_string()),
        KeyCode::Enter => editor.insert_newline(),
        KeyCode::Tab => editor.insert_tab(),
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

    // Typing and deleting change the word under the popup - and typing a word
    // character is also what brings one up in the first place.
    editor.update_completion();
    if let KeyCode::Char(c) = key.code
        && !ctrl
        && !alt
        && crate::complete::is_word(c)
    {
        editor.suggest_completion();
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

        /// The same, under a file name, so there is a grammar to ask.
        fn file(name: &str, text: &str) -> Self {
            let mut vim = Vim::new(text);
            let view = vim.editor.view_mut();
            view.doc.path = Some(name.into());
            view.attach_syntax(&crate::theme::Theme::built_in());
            vim
        }

        fn rust(text: &str) -> Self {
            Vim::file("demo.rs", text)
        }

        /// Put the cursor on a line and column, both counting from one.
        /// These tests are about what a command finds, not about how the
        /// cursor got there.
        fn at(&mut self, line: usize, column: usize) -> &mut Self {
            let view = self.editor.view_mut();
            let start = view.doc.line_to_char(line - 1);
            view.sel = crate::view::Selection::point(start + column - 1);
            self
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
                        // `<<` would otherwise read as the start of a key name.
                        "lt" => KeyEvent::new(KeyCode::Char('<'), KeyModifiers::NONE),
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
                            } else if let Some(name) = other.strip_prefix("M-") {
                                let c = name.chars().next().unwrap();
                                let code = match name {
                                    "bs" => KeyCode::Backspace,
                                    _ => KeyCode::Char(c),
                                };
                                KeyEvent::new(code, KeyModifiers::ALT)
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
    fn control_o_goes_back_to_where_a_jump_started() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\nfive\n");
        vim.press("jj");
        assert_eq!(vim.cursor(), (3, 1));
        vim.press("G");
        assert_eq!(vim.cursor(), (5, 1));

        vim.press("<C-o>");
        assert_eq!(vim.cursor(), (3, 1));
        // And forward again, to where `^o` was pressed from.
        vim.press("<C-i>");
        assert_eq!(vim.cursor(), (5, 1));
    }

    #[test]
    fn the_jump_list_keeps_the_column_too() {
        let mut vim = Vim::new("hello there\nsecond\nthird\n");
        vim.press("lllll");
        assert_eq!(vim.cursor(), (1, 6));
        vim.press("G<C-o>");
        assert_eq!(vim.cursor(), (1, 6));
    }

    #[test]
    fn walking_around_is_not_a_jump() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        // `j` and `w` move; they do not go anywhere.
        vim.press("jw");
        vim.press("<C-o>");
        assert_eq!(vim.editor.message, "at the oldest jump");
    }

    #[test]
    fn a_search_remembers_where_it_was_typed_from() {
        let mut vim = Vim::new("alpha\nbeta\ngamma\ndelta\n");
        vim.press("j");
        vim.press("/delta<cr>");
        assert_eq!(vim.cursor(), (4, 1));
        // Not line 4 and not line 1: the line the search was typed from.
        vim.press("<C-o>");
        assert_eq!(vim.cursor(), (2, 1));
    }

    #[test]
    fn a_search_that_finds_nothing_is_not_a_jump() {
        let mut vim = Vim::new("alpha\nbeta\n");
        vim.press("j/nowhere<cr>");
        vim.press("<C-o>");
        assert_eq!(vim.editor.message, "at the oldest jump");
    }

    #[test]
    fn a_jump_that_has_drifted_lands_near_rather_than_nowhere() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\nfive\nsix\n");
        vim.press("4G");
        vim.press("gg");
        // Three lines go from above where the jump was taken.
        vim.press("dddddd");
        vim.press("<C-o>");
        // Line 4 is now past the end, so it clamps onto the last line instead
        // of refusing to go.
        assert_eq!(vim.cursor(), (3, 1));
    }

    #[test]
    fn a_new_jump_forgets_the_way_forward() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\nfive\n");
        vim.press("jjG");
        vim.press("<C-o>");
        assert_eq!(vim.cursor(), (3, 1));
        // Somewhere else from here, and line 5 is no longer ahead of us.
        vim.press("gg");
        vim.press("<C-i>");
        assert_eq!(vim.editor.message, "at the newest jump");
    }

    #[test]
    fn gd_finds_a_let_binding_in_the_same_function() {
        let mut vim = Vim::rust("fn main() {\n    let total = 1;\n    show(total);\n}\n");
        vim.at(3, 10).press("gd");
        assert_eq!(vim.cursor(), (2, 9));
    }

    #[test]
    fn gd_finds_a_parameter() {
        let mut vim = Vim::rust("fn twice(count: usize) -> usize {\n    count + count\n}\n");
        vim.at(2, 5).press("gd");
        assert_eq!(vim.cursor(), (1, 10));
    }

    #[test]
    fn the_nearest_binding_wins_over_the_one_it_shadows() {
        let text = "fn main() {\n    let value = 1;\n    {\n        let value = 2;\n        use_it(value);\n    }\n}\n";
        let mut vim = Vim::rust(text);
        vim.at(5, 16).press("gd");
        // The inner `let`, not the outer one.
        assert_eq!(vim.cursor(), (4, 13));
    }

    #[test]
    fn gd_finds_a_function_the_file_defines() {
        let mut vim = Vim::rust("fn helper() {}\n\nfn main() {\n    helper();\n}\n");
        vim.at(4, 5).press("gd");
        assert_eq!(vim.cursor(), (1, 4));
    }

    #[test]
    fn gd_finds_a_type() {
        let mut vim = Vim::rust("struct Widget;\n\nfn make() -> Widget {\n    Widget\n}\n");
        vim.at(4, 5).press("gd");
        assert_eq!(vim.cursor(), (1, 8));
    }

    #[test]
    fn gd_is_a_jump_so_control_o_comes_back() {
        let mut vim = Vim::rust("fn helper() {}\n\nfn main() {\n    helper();\n}\n");
        vim.at(4, 5).press("gd");
        assert_eq!(vim.cursor().0, 1);
        vim.press("<C-o>");
        assert_eq!(vim.cursor(), (4, 5));
    }

    #[test]
    fn capital_gd_skips_the_binding_and_takes_the_file_definition() {
        let text = "fn value() {}\n\nfn main() {\n    let value = 1;\n    use_it(value);\n}\n";
        let mut vim = Vim::rust(text);
        vim.at(5, 12).press("gd");
        assert_eq!(vim.cursor().0, 4, "gd finds the let");
        vim.at(5, 12).press("gD");
        assert_eq!(vim.cursor().0, 1, "gD finds the fn");
    }

    #[test]
    fn gd_without_a_grammar_looks_backwards_for_the_word() {
        // No file name, so no parser: this is the tier that is only a search.
        let mut vim = Vim::new("total = 1\nsomething else\nprint total\n");
        vim.at(3, 7).press("gd");
        assert_eq!(vim.cursor(), (1, 1));
    }

    #[test]
    fn gd_says_so_when_there_is_nothing_to_find() {
        let mut vim = Vim::rust("fn main() {\n    missing();\n}\n");
        vim.at(2, 5).press("gd");
        assert_eq!(vim.editor.message, "no definition of missing");

        let mut vim = Vim::rust("fn main() {}\n");
        vim.press("$").press("gd");
        assert_eq!(vim.editor.message, "no word under the cursor");
    }

    #[test]
    fn gd_on_the_definition_itself_says_so_rather_than_jumping() {
        let mut vim = Vim::rust("fn helper() {}\n\nfn main() {\n    helper();\n}\n");
        vim.at(1, 4).press("gd");
        assert_eq!(vim.editor.message, "helper is defined here");
    }

    #[test]
    fn gd_works_in_javascript_from_the_grammars_own_queries() {
        // JavaScript's crate ships both queries, so this language needed no
        // file of ours at all.
        let mut vim = Vim::file("demo.js", "function helper() {}\n\nfunction main() {\n  helper();\n}\n");
        vim.at(4, 3).press("gd");
        assert_eq!(vim.cursor(), (1, 10));

        let text = "function main() {\n  const total = 1;\n  show(total);\n}\n";
        let mut vim = Vim::file("demo.js", text);
        vim.at(3, 8).press("gd");
        assert_eq!(vim.cursor(), (2, 9));
    }

    #[test]
    fn a_definition_in_a_big_file_is_found_between_keystrokes() {
        // The items tier reads the whole tree, because a function can be
        // defined anywhere in the file. The bindings tier reads only the item
        // the cursor is in, because that is as far as a binding reaches - and
        // that is the difference between the two numbers below.
        let mut text = String::from("fn needle(count: usize) -> usize { count }\n");
        while text.len() < 800_000 {
            text.push_str("fn filler(count: usize) -> usize { count + 1 }\n");
        }
        text.push_str("fn main() {\n    needle(1);\n}\n");
        let mut vim = Vim::rust(&text);
        // `last_line` counts from zero; `at` counts from one, like the editor
        // shows it.
        let call = vim.editor.last_line();

        // A parameter, in a function that is one line long however big the
        // file is.
        vim.at(2, 36);
        let start = std::time::Instant::now();
        let local = vim.editor.view().definition("count", vim.editor.view().sel.head, true);
        let binding = start.elapsed();
        assert!(local.is_some());

        // A function, which could be anywhere.
        vim.at(call, 5);
        let start = std::time::Instant::now();
        vim.press("gd");
        let item = start.elapsed();

        assert_eq!(vim.cursor().0, 1, "the needle is on the first line");
        // 25µs and 45ms release on this file; the bound is for a debug build
        // on a slow machine.
        assert!(binding.as_millis() < 50, "a binding took {binding:?}");
        assert!(item.as_millis() < 800, "an item took {item:?}");
    }

    #[test]
    fn f_and_capital_f_land_on_the_character() {
        let mut vim = Vim::new("one, two, three\n");
        vim.press("f,");
        assert_eq!(vim.cursor(), (1, 4));
        vim.press("f,");
        assert_eq!(vim.cursor(), (1, 9));
        vim.press("F,");
        assert_eq!(vim.cursor(), (1, 4));
    }

    #[test]
    fn t_and_capital_t_stop_one_short() {
        let mut vim = Vim::new("one, two, three\n");
        vim.press("t,");
        assert_eq!(vim.cursor(), (1, 3));
        vim.press("$");
        vim.press("T,");
        assert_eq!(vim.cursor(), (1, 10));
    }

    #[test]
    fn a_count_takes_the_nth_one() {
        let mut vim = Vim::new("a.b.c.d.e\n");
        vim.press("3f.");
        assert_eq!(vim.cursor(), (1, 6));
        vim.press("2F.");
        assert_eq!(vim.cursor(), (1, 2));
    }

    #[test]
    fn a_find_stops_at_the_end_of_the_line() {
        let mut vim = Vim::new("one two\nthree, four\n");
        vim.press("f,");
        // The comma is on the next line, which `f` does not reach.
        assert_eq!(vim.cursor(), (1, 1));
        assert_eq!(vim.editor.message, "no , on this line");
    }

    #[test]
    fn an_operator_takes_a_find_as_its_motion() {
        let mut vim = Vim::new("one, two, three\n");
        vim.press("df,");
        assert_eq!(vim.text(), " two, three\n");

        let mut vim = Vim::new("one, two, three\n");
        vim.press("dt,");
        assert_eq!(vim.text(), ", two, three\n");

        // Backwards, from the end of the word `three`.
        let mut vim = Vim::new("one, two, three\n");
        vim.press("$dF,");
        assert_eq!(vim.text(), "one, twoe\n");
    }

    #[test]
    fn change_with_a_find_leaves_you_typing() {
        let mut vim = Vim::new("call(a, b);\n");
        vim.press("ct)");
        assert_eq!(vim.editor.mode, Mode::Insert);
        assert_eq!(vim.text(), ");\n");
    }

    #[test]
    fn yank_with_a_find_takes_what_it_covers() {
        let mut vim = Vim::new("one, two\n");
        vim.press("yf,");
        assert_eq!(vim.editor.registers.get(None).text, "one,");
    }

    #[test]
    fn semicolon_repeats_and_comma_reverses() {
        let mut vim = Vim::new("a.b.c.d\n");
        vim.press("f.");
        assert_eq!(vim.cursor(), (1, 2));
        vim.press(";");
        assert_eq!(vim.cursor(), (1, 4));
        vim.press(";");
        assert_eq!(vim.cursor(), (1, 6));
        vim.press(",");
        assert_eq!(vim.cursor(), (1, 4));
        // Reversing does not change what is being repeated: another `;` still
        // goes the way the original `f` went.
        vim.press(";");
        assert_eq!(vim.cursor(), (1, 6));
    }

    #[test]
    fn paragraph_motions_walk_between_the_blank_lines() {
        let mut vim = Vim::new("one\ntwo\n\nthree\nfour\n\n\nfive\n");
        vim.press("}");
        assert_eq!(vim.cursor(), (3, 1), "the blank line after the first block");
        vim.press("}");
        assert_eq!(vim.cursor(), (6, 1));
        vim.press("{");
        assert_eq!(vim.cursor(), (3, 1), "a run of blanks counts once");

        // They are motions, so an operator takes them.
        let mut vim = Vim::new("one\ntwo\n\nthree\n");
        vim.press("d}");
        assert_eq!(vim.text(), "\nthree\n");

        // And they stop at the ends rather than running off.
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("{");
        assert_eq!(vim.cursor(), (1, 1));
        // Past the end is the last line, where `clamp_cursor` leaves anything
        // that runs off the bottom - column one, not the final character.
        vim.press("}}");
        assert_eq!(vim.cursor(), (2, 1));
    }

    fn lines(count: usize) -> String {
        (1..=count).map(|n| format!("line {n}\n")).collect()
    }

    #[test]
    fn a_split_shows_the_same_buffer_with_a_cursor_of_its_own() {
        let mut vim = Vim::new(&lines(50));
        vim.editor.set_viewport(81, 23);
        vim.press("5G<C-w>s");
        assert_eq!(vim.editor.windows_open(), 2);
        assert_eq!(vim.cursor(), (5, 1), "the new window starts where the old one was");
        // Each half has its own share of the screen: 24 rows, two status lines.
        assert_eq!(vim.editor.height, 11);

        vim.press("40G<C-w>k");
        assert_eq!(vim.cursor(), (5, 1), "the top window kept its place");
        vim.press("<C-w>j");
        assert_eq!(vim.cursor(), (40, 1));
        vim.press("<C-w>w");
        assert_eq!(vim.cursor(), (5, 1), "and w cycles back round");
    }

    #[test]
    fn side_by_side_windows_go_left_and_right() {
        let mut vim = Vim::new(&lines(10));
        vim.editor.set_viewport(81, 23);
        vim.press("<C-w>v");
        assert_eq!(vim.editor.width, 40);
        assert_eq!(vim.editor.window_rect(vim.editor.focus()).x, 41, "the new one is on the right");
        vim.press("3G<C-w>h");
        assert_eq!(vim.editor.focus(), 0);
        assert_eq!(vim.cursor(), (1, 1));
        vim.press("<C-w>l");
        assert_eq!(vim.cursor(), (3, 1));
    }

    #[test]
    fn an_edit_in_one_window_carries_the_others_cursor_along() {
        let mut vim = Vim::new("a\nb\nc\nd\n");
        vim.press("3G<C-w>s");
        // Two new lines above, from the other window.
        vim.press("ggOone<esc>otwo<esc>");
        vim.press("<C-w>k");
        assert_eq!(vim.cursor(), (5, 1), "still on c");
        let (line, _) = vim.editor.cursor_coords();
        assert_eq!(vim.editor.view().doc.line_str(line), "c");

        // And deleting the line it was on leaves it where the line was.
        vim.press("<C-w>jGkdd<C-w>k");
        assert_eq!(vim.text(), "one\ntwo\na\nb\nd\n");
        assert_eq!(vim.cursor(), (5, 1));
    }

    #[test]
    fn undo_in_one_window_is_the_buffers_undo() {
        let mut vim = Vim::new("text\n");
        vim.press("<C-w>vx<C-w>hx");
        assert_eq!(vim.text(), "xt\n");
        vim.press("u");
        assert_eq!(vim.text(), "ext\n", "one command, one undo, whichever window");
        vim.press("u");
        assert_eq!(vim.text(), "text\n");
    }

    #[test]
    fn closing_windows_and_quitting() {
        let mut vim = Vim::new(&lines(10));
        vim.editor.set_viewport(81, 23);
        vim.press("<C-w>s<C-w>v");
        assert_eq!(vim.editor.windows_open(), 3);

        vim.press(":q<cr>");
        assert_eq!(vim.editor.windows_open(), 2, ":q closes a window while there are others");
        vim.press("<C-w>c");
        assert_eq!(vim.editor.windows_open(), 1);
        vim.press("<C-w>c");
        assert_eq!(vim.editor.message, "the last window cannot be closed");

        vim.press("<C-w>s<C-w>s<C-w>o");
        assert_eq!(vim.editor.windows_open(), 1);
        assert_eq!(vim.editor.height, 23, "the one left has the screen back");

        // `^w q` on the last window is a quit.
        vim.press("<C-w>");
        let action = vim.keys.handle(&mut vim.editor, KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(action, Action::Quit { force: false });
    }

    #[test]
    fn a_split_needs_room() {
        let mut vim = Vim::new("text\n");
        // Three rows of text and a status line: four rows, and 20 columns.
        vim.editor.set_viewport(20, 3);
        vim.press("<C-w>v");
        assert_eq!(vim.editor.windows_open(), 1);
        assert_eq!(vim.editor.message, "no room to split");
        vim.press("<C-w>s");
        assert_eq!(vim.editor.windows_open(), 2, "two windows of a line and a status line each");
        vim.press("<C-w>s");
        assert_eq!(vim.editor.windows_open(), 2);
    }

    #[test]
    fn a_command_that_changes_buffer_closes_its_undo_group_where_it_opened_it() {
        let dir = std::env::temp_dir().join(format!("jack_group_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "abc\n").unwrap();
        std::fs::write(&b, "xyz\n").unwrap();

        let mut vim = Vim { editor: Editor::open(&[&a, &b]).unwrap(), keys: Keys::default() };
        // `gn` opens its group on a and finishes on b. `:e` comes back
        // without a command of that kind, so nothing else closes a's.
        vim.press("gn");
        vim.press(&format!(":e {}<cr>", a.display()));
        vim.press("xx");
        assert_eq!(vim.text(), "c\n");
        vim.press("u");
        assert_eq!(vim.text(), "bc\n", "two commands are still two undos");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn moving_between_windows_is_not_a_change_to_repeat() {
        let mut vim = Vim::new("abcdef\n");
        vim.press("x<C-w>v<C-w>w.");
        assert_eq!(vim.text(), "cdef\n");
        assert_eq!(vim.keys.pending_text(), "");
    }

    #[test]
    fn gcc_comments_a_line_and_the_same_keys_bring_it_back() {
        let mut vim = Vim::rust("fn main() {\n    one();\n    two();\n}\n");
        vim.at(2, 7).press("gcc");
        assert_eq!(vim.text(), "fn main() {\n    // one();\n    two();\n}\n");
        // The first non-blank of the line, which is now the marker.
        assert_eq!(vim.cursor(), (2, 5));

        vim.press("gcc");
        assert_eq!(vim.text(), "fn main() {\n    one();\n    two();\n}\n");

        // A count is lines, lined up at the shallowest of them.
        vim.at(1, 1).press("3gcc");
        assert_eq!(vim.text(), "// fn main() {\n//     one();\n//     two();\n}\n");
        assert_eq!(vim.editor.message, "3 lines commented");
    }

    #[test]
    fn gc_takes_motions_objects_and_selections() {
        let text = "a = 1\nb = 2\n\nc = 3\n";
        let mut vim = Vim::file("demo.py", text);
        vim.press("gcj");
        assert_eq!(vim.text(), "# a = 1\n# b = 2\n\nc = 3\n");
        vim.press("u");
        assert_eq!(vim.text(), text, "one command, one undo");

        vim.press("gcap");
        assert_eq!(vim.text(), "# a = 1\n# b = 2\n\nc = 3\n", "the blank line stays blank");
        vim.press("u");

        vim.press("gggcG");
        assert_eq!(vim.text(), "# a = 1\n# b = 2\n\n# c = 3\n");
        vim.press("u");

        vim.press("ggjVgc");
        assert_eq!(vim.text(), "a = 1\n# b = 2\n\nc = 3\n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn gc_is_a_change_that_dot_repeats() {
        let mut vim = Vim::rust("one\ntwo\nthree\n");
        vim.press("gccj.");
        assert_eq!(vim.text(), "// one\n// two\nthree\n");
    }

    #[test]
    fn gc_without_a_marker_says_so_and_changes_nothing() {
        let mut vim = Vim::new("plain\n");
        vim.press("gcc");
        assert_eq!(vim.text(), "plain\n");
        assert_eq!(vim.editor.message, "no comment marker for this file");
        assert_eq!(vim.keys.pending_text(), "");
    }

    #[test]
    fn z_puts_the_cursors_line_where_you_ask() {
        let mut vim = Vim::new(&"line\n".repeat(100));
        vim.editor.set_viewport(80, 20);
        vim.press("50G");
        let line = vim.editor.cursor_coords().0;

        vim.press("zt");
        assert_eq!(vim.editor.view().scroll_top, line - 3, "scrolloff kept above");
        vim.press("zz");
        assert_eq!(vim.editor.view().scroll_top, line - 10);
        vim.press("zb");
        assert_eq!(vim.editor.view().scroll_top, line + 4 - 20);

        // The cursor did not move for any of it.
        assert_eq!(vim.editor.cursor_coords().0, line);
    }

    #[test]
    fn h_m_l_go_to_what_is_on_the_screen() {
        let mut vim = Vim::new(&"line\n".repeat(100));
        vim.editor.set_viewport(80, 20);
        vim.press("50Gzt");
        let top = vim.editor.view().scroll_top;

        vim.press("H");
        assert_eq!(vim.editor.cursor_coords().0, top + 3, "scrolloff from the top");
        vim.press("L");
        assert_eq!(vim.editor.cursor_coords().0, top + 20 - 1 - 3);
        vim.press("M");
        assert_eq!(vim.editor.cursor_coords().0, (top + top + 19) / 2);

        // A count is how far in from the edge.
        vim.press("3H");
        assert_eq!(vim.editor.cursor_coords().0, top + 3 + 2);

        // An operator over one is linewise.
        let mut vim = Vim::new(&"line\n".repeat(100));
        vim.editor.set_viewport(80, 20);
        vim.press("ggdL");
        // Lines 1 to 17: the bottom of the screen is 20 rows less the three
        // of scrolloff, and the range includes the line it started on.
        assert_eq!(vim.text().lines().count(), 100 - 17, "to the bottom of the screen");
    }

    #[test]
    fn ctrl_e_and_ctrl_y_scroll_under_the_cursor() {
        let mut vim = Vim::new(&"line\n".repeat(100));
        vim.editor.set_viewport(80, 20);
        vim.press("50Gzz");
        let (top, line) = (vim.editor.view().scroll_top, vim.editor.cursor_coords().0);

        vim.press("<C-e>");
        assert_eq!(vim.editor.view().scroll_top, top + 1);
        assert_eq!(vim.editor.cursor_coords().0, line, "the cursor stayed put");

        // Until it would be scrolled off, when it comes along.
        vim.press("20<C-e>");
        assert!(vim.editor.cursor_coords().0 > line);

        vim.press("<C-y>");
        assert_eq!(vim.editor.view().scroll_top, top + 20);
    }

    #[test]
    fn dot_does_the_last_change_again() {
        // An operator and a motion.
        let mut vim = Vim::new("one two three four\n");
        vim.press("dw");
        assert_eq!(vim.text(), "two three four\n");
        vim.press(".");
        assert_eq!(vim.text(), "three four\n");
        vim.press(".");
        assert_eq!(vim.text(), "four\n");

        // An insert: the text it put in comes with it, because the keys are
        // the command.
        let mut vim = Vim::new("a\nb\n");
        vim.press("ohello<esc>");
        assert_eq!(vim.text(), "a\nhello\nb\n");
        vim.press(".");
        assert_eq!(vim.text(), "a\nhello\nhello\nb\n");

        // The small verbs too.
        let mut vim = Vim::new("abcd\n");
        vim.press("rx");
        vim.press("l.");
        assert_eq!(vim.text(), "xxcd\n");
    }

    #[test]
    fn a_movement_is_not_a_change_to_repeat() {
        let mut vim = Vim::new("one two\nthree four\n");
        vim.press("x");
        assert_eq!(vim.text(), "ne two\nthree four\n");

        // Moving about, searching, changing mode: none of it displaces the
        // change that `.` still has to give back.
        vim.press("jw");
        vim.press("v<esc>");
        vim.press(".");
        assert_eq!(vim.text(), "ne two\nthree our\n");
    }

    #[test]
    fn undo_is_not_the_change_that_dot_repeats() {
        let mut vim = Vim::new("one two\n");
        vim.press("dw");
        assert_eq!(vim.text(), "two\n");
        vim.press("u");
        assert_eq!(vim.text(), "one two\n");

        // Vim's rule: `.` after a `u` does the edit again rather than undoing
        // something else. Where it lands is wherever undo left the cursor -
        // here the end of what came back, because undo restores the selection
        // the change was made over - so it takes the second word this time.
        vim.press(".");
        assert_eq!(vim.text(), "one ");
        assert_ne!(vim.text(), "one two\n", "not another undo");
    }

    #[test]
    fn a_count_on_dot_replaces_the_one_it_was_typed_with() {
        let mut vim = Vim::new("one two three four five six\n");
        vim.press("2dw");
        assert_eq!(vim.text(), "three four five six\n");
        vim.press("3.");
        assert_eq!(vim.text(), "six\n");

        // Without a count it keeps the one it had.
        let mut vim = Vim::new("one two three four five\n");
        vim.press("2dw");
        vim.press(".");
        assert_eq!(vim.text(), "five\n");
    }

    #[test]
    fn dot_with_nothing_to_repeat_says_so() {
        let mut vim = Vim::new("one\n");
        vim.press(".");
        assert_eq!(vim.text(), "one\n");
        assert!(!vim.editor.message.is_empty());
    }

    #[test]
    fn a_dot_that_is_an_argument_is_not_the_repeat() {
        // `f.` is looking for a full stop, and `r.` puts one down. Neither is
        // the repeat command, because both have something waiting for a key.
        let mut vim = Vim::new("one.two\n");
        vim.press("df.");
        assert_eq!(vim.text(), "two\n");

        let mut vim = Vim::new("abc\n");
        vim.press("r.");
        assert_eq!(vim.text(), ".bc\n");
        // And that replace is now the change to repeat.
        vim.press("l.");
        assert_eq!(vim.text(), "..c\n");
    }

    #[test]
    fn j_joins_the_line_below_onto_this_one() {
        let mut vim = Vim::new("one\n    two\nthree\nfour\n");
        vim.press("J");
        assert_eq!(vim.text(), "one two\nthree\nfour\n", "the indent goes with it");
        // The count is the lines taking part, not the joins: `3J` makes one
        // line out of three.
        vim.press("3J");
        assert_eq!(vim.text(), "one two three four\n");

        // Nothing below to join, and it says so rather than eating the line.
        let mut vim = Vim::new("only\n");
        vim.press("J");
        assert_eq!(vim.text(), "only\n");
        assert!(!vim.editor.message.is_empty());

        // A closing bracket sits against what it closes, and a line that
        // already ends in a space does not get another.
        let mut vim = Vim::new("call(a,\n    )\n");
        vim.press("J");
        assert_eq!(vim.text(), "call(a,)\n");
    }

    #[test]
    fn visual_j_joins_everything_selected() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\n");
        vim.press("Vjj");
        vim.press("J");
        assert_eq!(vim.text(), "one two three\nfour\n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn r_replaces_characters_and_tilde_swaps_their_case() {
        let mut vim = Vim::new("hello\n");
        vim.press("rj");
        assert_eq!(vim.text(), "jello\n");
        vim.press("3rx");
        assert_eq!(vim.text(), "xxxlo\n");
        // The cursor sits on the last one replaced, so `~` carries on from
        // there rather than from where the command started.
        vim.press("~");
        assert_eq!(vim.text(), "xxXlo\n");

        // More than the line has left is refused outright, not done in part.
        let mut vim = Vim::new("ab\n");
        vim.press("5rz");
        assert_eq!(vim.text(), "ab\n");
        assert!(!vim.editor.message.is_empty());

        // `~` takes a count too, and stops at the end of the line.
        let mut vim = Vim::new("abc\n");
        vim.press("9~");
        assert_eq!(vim.text(), "ABC\n");
    }

    #[test]
    fn gv_brings_back_the_last_selection() {
        let mut vim = Vim::new("one two three\n");
        vim.press("vee");
        vim.press("<esc>");
        vim.press("gv");
        assert_eq!(vim.editor.mode, Mode::Visual);
        vim.press("d");
        assert_eq!(vim.text(), " three\n");

        // Nothing selected yet says so rather than selecting something.
        let mut vim = Vim::new("one\n");
        vim.press("gv");
        assert_eq!(vim.editor.mode, Mode::Normal);
        assert!(!vim.editor.message.is_empty());
    }

    #[test]
    fn an_operator_reaches_the_ends_of_the_file() {
        // `dG` and `dgg` are linewise, and take the line the cursor is on.
        let mut vim = Vim::new("one\ntwo\nthree\nfour\n");
        vim.press("jdG");
        assert_eq!(vim.text(), "one\n");

        let mut vim = Vim::new("one\ntwo\nthree\nfour\n");
        vim.press("jjdgg");
        assert_eq!(vim.text(), "four\n");

        // A count is a line number for both of them.
        let mut vim = Vim::new("one\ntwo\nthree\nfour\n");
        vim.press("d2G");
        assert_eq!(vim.text(), "three\nfour\n");

        // Every operator, not just `d`.
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("yG");
        assert_eq!(vim.editor.registers.get(None).text, "one\ntwo\nthree\n");
        assert_eq!(vim.cursor(), (1, 1), "a yank leaves the cursor alone");

        let mut vim = Vim::new("fn a() {\nlet x = 1;\n}\n");
        vim.press("gg=G");
        assert_eq!(vim.text(), "fn a() {\nlet x = 1;\n}\n", "no grammar, no change");
    }

    #[test]
    fn a_till_already_against_its_target_takes_nothing() {
        // Vim's rule: `dtb` on the `a` of `ab` has nowhere to go, so it does
        // nothing. Taking the character under the cursor instead would make
        // pressing `dt,` twice eat the line a character at a time.
        let mut vim = Vim::new("ab cd, efg\n");
        vim.press("dtb");
        assert_eq!(vim.text(), "ab cd, efg\n");

        // The same find that does have somewhere to go still works, and doing
        // it a second time is the no-op rather than a second bite.
        vim.press("dt,");
        assert_eq!(vim.text(), ", efg\n");
        vim.press("dt,");
        assert_eq!(vim.text(), ", efg\n");

        // `c` must not open insert mode for a change it cannot make.
        vim.press("ctb");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn repeating_a_till_moves_on_rather_than_sticking() {
        // The classic `t` trap: the cursor is already beside the comma, so a
        // repeat that meant "the same one" would never move again.
        let mut vim = Vim::new("a, b, c, d\n");
        vim.press("t,");
        assert_eq!(vim.cursor(), (1, 1));
        vim.press(";");
        assert_eq!(vim.cursor(), (1, 4));
        vim.press(";");
        assert_eq!(vim.cursor(), (1, 7));
    }

    #[test]
    fn semicolon_can_be_the_command_line_instead() {
        let mut vim = Vim::new("a.b.c.d\n");
        vim.editor.run_command("set semicolon=command");
        vim.press(";");
        assert!(vim.editor.prompt.is_some(), "the command line is open");
        vim.press("<esc>");

        // `,` takes over repeating, forwards.
        vim.press("f.");
        assert_eq!(vim.cursor(), (1, 2));
        vim.press(",");
        assert_eq!(vim.cursor(), (1, 4));
        // And an operator takes it: from the second dot through the third.
        vim.press("d,");
        assert_eq!(vim.text(), "a.bd\n");
    }

    #[test]
    fn the_command_line_semicolon_runs_a_command() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.editor.run_command("set semicolon=command");
        vim.press(";3<cr>");
        assert_eq!(vim.cursor(), (3, 1));
    }

    #[test]
    fn repeating_before_any_find_says_so() {
        let mut vim = Vim::new("a.b\n");
        vim.press(";");
        assert_eq!(vim.editor.message, "no previous find");
    }

    #[test]
    fn an_operator_can_take_the_repeat_too() {
        let mut vim = Vim::new("a.b.c\n");
        vim.press("f.");
        vim.press("d;");
        // From the first dot through the second, as vim does.
        assert_eq!(vim.text(), "ac\n");
    }

    #[test]
    fn a_find_drags_a_visual_selection() {
        let mut vim = Vim::new("one, two, three\n");
        vim.press("vf,");
        assert_eq!(vim.cursor(), (1, 4));
        vim.press("y");
        assert_eq!(vim.editor.registers.get(None).text, "one,");

        let mut vim = Vim::new("one, two, three\n");
        vim.press("vt,;y");
        assert_eq!(vim.editor.registers.get(None).text, "one, two");
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
    fn every_half_typed_command_has_something_to_show() {
        // The indicator is drawn on every frame, so a state it cannot describe
        // is a crash rather than a blank: a pending find used to panic the
        // editor the moment it redrew, which is any pause between `dt` and the
        // character to look for.
        let states = [
            ("d", "d"), ("c", "c"), ("y", "y"), (">", ">"), ("<lt>", "<"),
            ("=", "="), ("g", "g"), (" ", "<space>"), ("\"", "\""),
            ("f", "f"), ("F", "F"), ("t", "t"), ("T", "T"),
            ("dt", "dt"), ("cf", "cf"), ("yT", "yT"), ("2dF", "2dF"),
            ("di", "di"), ("ca", "ca"),
            ("gc", "gc"), ("3gc", "3gc"), ("gct", "gct"), ("gci", "gci"),
        ];
        for (keys, shown) in states {
            let mut vim = Vim::new("one, two\n");
            vim.press(keys);
            assert_eq!(vim.keys.pending_text(), shown, "after {keys}");
        }
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
            // A picker, a setting changed, or - for a key that has nothing to
            // show in a scratch buffer - a word about why.
            let did_something = vim.editor.picker.is_some()
                || vim.editor.numbers != numbers
                || !vim.editor.message.is_empty();
            assert!(did_something, "{} did nothing", binding.keys);
        }
    }

    #[test]
    fn slash_opens_a_prompt_that_owns_the_keyboard() {
        let mut vim = Vim::new("one two\n");
        vim.press("/");
        assert!(vim.editor.prompt.is_some());
        // `d` here is a character of the pattern, not a delete.
        vim.press("dd");
        assert_eq!(vim.editor.prompt.as_ref().unwrap().input, "dd");
        assert_eq!(vim.editor.view().doc.text.to_string(), "one two\n");
    }

    #[test]
    fn a_search_moves_the_cursor_to_the_match() {
        let mut vim = Vim::new("one two three\n");
        vim.press("/three<cr>");
        assert!(vim.editor.prompt.is_none());
        assert_eq!(vim.editor.cursor_coords(), (0, 8));
    }

    #[test]
    fn the_search_previews_as_it_is_typed() {
        let mut vim = Vim::new("one two three\n");
        vim.press("/thr");
        // The cursor is already on the match before enter is pressed.
        assert_eq!(vim.editor.cursor_coords(), (0, 8));
    }

    #[test]
    fn cancelling_a_search_puts_the_cursor_back() {
        let mut vim = Vim::new("one two three\n");
        vim.press("/three");
        assert_eq!(vim.editor.cursor_coords(), (0, 8));
        vim.press("<esc>");
        assert_eq!(vim.editor.cursor_coords(), (0, 0));
        assert!(!vim.editor.search.highlight);
    }

    #[test]
    fn n_and_shift_n_walk_the_matches() {
        let mut vim = Vim::new("x one x two x\n");
        vim.press("/x<cr>");
        assert_eq!(vim.editor.cursor_coords(), (0, 6));
        vim.press("n");
        assert_eq!(vim.editor.cursor_coords(), (0, 12));
        vim.press("N");
        assert_eq!(vim.editor.cursor_coords(), (0, 6));
    }

    #[test]
    fn a_count_repeats_the_search() {
        let mut vim = Vim::new("x a x b x c x\n");
        vim.press("/x<cr>");
        assert_eq!(vim.editor.cursor_coords(), (0, 4));
        vim.press("2n");
        assert_eq!(vim.editor.cursor_coords(), (0, 12));
    }

    #[test]
    fn a_search_wraps_and_says_so() {
        let mut vim = Vim::new("match\nnothing\n");
        vim.press("/match<cr>");
        vim.press("n");
        assert_eq!(vim.editor.cursor_coords(), (0, 0));
        assert!(vim.editor.message.contains("continuing at top"), "{}", vim.editor.message);
    }

    #[test]
    fn a_pattern_that_matches_nothing_says_so_and_stays_put() {
        let mut vim = Vim::new("one two\n");
        vim.press("llll");
        vim.press("/zebra<cr>");
        assert_eq!(vim.editor.cursor_coords(), (0, 4));
        assert!(vim.editor.message.contains("not found"), "{}", vim.editor.message);
    }

    #[test]
    fn star_searches_for_the_word_under_the_cursor() {
        let mut vim = Vim::new("cat category cat\n");
        vim.press("*");
        // Past `category`, which is not the whole word.
        assert_eq!(vim.editor.cursor_coords(), (0, 13));
        assert_eq!(vim.editor.search.pattern, r"\bcat\b");
    }

    #[test]
    fn a_backwards_search_goes_the_other_way() {
        let mut vim = Vim::new("one two one\n");
        vim.press("$");
        vim.press("?one<cr>");
        // The nearest match starting before the cursor, which is the one the
        // cursor is sitting inside.
        assert_eq!(vim.editor.cursor_coords(), (0, 8));
        vim.press("n");
        // `n` keeps going the way the search was going.
        assert_eq!(vim.editor.cursor_coords(), (0, 0));
        vim.press("N");
        assert_eq!(vim.editor.cursor_coords(), (0, 8));
    }

    #[test]
    fn escape_in_normal_mode_stops_highlighting_matches() {
        let mut vim = Vim::new("one two\n");
        vim.press("/two<cr>");
        assert!(vim.editor.search.highlight);
        vim.press("<esc>");
        assert!(!vim.editor.search.highlight);
        // The pattern itself is remembered, so `n` still works.
        vim.press("n");
        assert!(vim.editor.search.highlight);
    }

    #[test]
    fn n_without_a_search_says_so() {
        let mut vim = Vim::new("one\n");
        vim.press("n");
        assert_eq!(vim.editor.message, "no previous search");
    }

    #[test]
    fn backspacing_an_empty_pattern_cancels_the_search() {
        let mut vim = Vim::new("one\n");
        vim.press("/a<bs><bs>");
        assert!(vim.editor.prompt.is_none());
        assert_eq!(vim.editor.cursor_coords(), (0, 0));
    }

    #[test]
    fn a_bare_slash_repeats_the_last_search() {
        let mut vim = Vim::new("x one x two x\n");
        vim.press("/x<cr>");
        assert_eq!(vim.editor.cursor_coords(), (0, 6));
        vim.press("/<cr>");
        assert_eq!(vim.editor.cursor_coords(), (0, 12));
    }

    #[test]
    fn colon_opens_a_command_line_that_does_not_preview() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press(":3");
        // A command waits to be accepted; only a search moves as you type.
        assert_eq!(vim.editor.cursor_coords(), (0, 0));
        vim.press("<cr>");
        assert_eq!(vim.editor.cursor_coords(), (2, 0));
    }

    #[test]
    fn set_changes_an_option_and_says_so_when_it_cannot() {
        let mut vim = Vim::new("hello\n");
        vim.press(":set relativenumber<cr>");
        assert_eq!(vim.editor.numbers.name(), "relative");
        vim.press(":set nonumber<cr>");
        assert_eq!(vim.editor.numbers.name(), "off");
        vim.press(":set notrim<cr>");
        assert!(!vim.editor.trim_on_save);

        vim.press(":set wibble<cr>");
        assert_eq!(vim.editor.message, "not an option: wibble");
    }

    #[test]
    fn an_unknown_command_says_so() {
        let mut vim = Vim::new("hello\n");
        vim.press(":frobnicate<cr>");
        assert_eq!(vim.editor.message, "not a command: frobnicate");
    }

    #[test]
    fn noh_stops_highlighting_without_losing_the_pattern() {
        let mut vim = Vim::new("one two\n");
        vim.press("/two<cr>");
        assert!(vim.editor.search.highlight);
        vim.press(":noh<cr>");
        assert!(!vim.editor.search.highlight);
        assert_eq!(vim.editor.search.pattern, "two");
    }

    #[test]
    fn a_cancelled_command_leaves_everything_alone() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press(":set nonumber<esc>");
        assert!(vim.editor.prompt.is_none());
        assert_eq!(vim.editor.numbers.name(), "absolute");
    }

    #[test]
    fn percent_jumps_between_matching_brackets() {
        let mut vim = Vim::new("fn main() { let x = (1 + 2); }\n");
        vim.press("10l");                       // onto the `{`
        assert_eq!(vim.editor.cursor_coords(), (0, 10));
        vim.press("%");
        assert_eq!(vim.editor.cursor_coords(), (0, 29));
        vim.press("%");
        assert_eq!(vim.editor.cursor_coords(), (0, 10));

        // The inner pair is matched from inside the outer one.
        vim.press("10l");
        assert_eq!(vim.editor.cursor_coords(), (0, 20));
        vim.press("%");
        assert_eq!(vim.editor.cursor_coords(), (0, 26));
    }

    #[test]
    fn percent_nests() {
        let mut vim = Vim::new("((()))\n");
        vim.press("%");
        assert_eq!(vim.editor.cursor_coords(), (0, 5));
        // The next one in is a pair of its own, not the same one again.
        vim.press("h%");
        assert_eq!(vim.editor.cursor_coords(), (0, 1));
    }

    #[test]
    fn percent_off_a_bracket_says_so() {
        let mut vim = Vim::new("hello\n");
        vim.press("%");
        assert_eq!(vim.editor.message, "no bracket under the cursor");
        assert_eq!(vim.editor.cursor_coords(), (0, 0));
    }

    #[test]
    fn percent_extends_a_visual_selection() {
        let mut vim = Vim::new("(abc)\n");
        vim.press("v%");
        assert_eq!(vim.editor.selection_range(), Some((0, 5)));
    }

    #[test]
    fn change_inner_word_replaces_the_whole_word() {
        let mut vim = Vim::new("let x = foo_bar(1);");
        // `dw` from the middle of the word would leave `foo_` behind.
        vim.press("10lciwbaz<esc>");
        assert_eq!(vim.text(), "let x = baz(1);");
    }

    #[test]
    fn delete_a_word_takes_the_space_with_it() {
        let mut vim = Vim::new("one two three");
        vim.press("5ldaw");
        assert_eq!(vim.text(), "one three");
    }

    #[test]
    fn delete_inside_quotes_empties_the_string() {
        let mut vim = Vim::new("let s = \"hi\";");
        vim.press("di\"");
        assert_eq!(vim.text(), "let s = \"\";");
    }

    #[test]
    fn change_inside_an_empty_pair_still_starts_typing() {
        let mut vim = Vim::new("f()");
        vim.press("2lci(x<esc>");
        assert_eq!(vim.text(), "f(x)");
    }

    #[test]
    fn an_object_in_visual_mode_becomes_the_selection() {
        let mut vim = Vim::new("f(g(x), y)");
        vim.press("4lvi(d");
        assert_eq!(vim.text(), "f(g(), y)");

        let mut vim = Vim::new("f(g(x), y)");
        vim.press("4lva(d");
        assert_eq!(vim.text(), "f(g, y)");
    }

    #[test]
    fn an_object_obeys_the_named_register() {
        let mut vim = Vim::new("alpha beta");
        vim.press("\"ayiw$\"ap");
        assert_eq!(vim.text(), "alpha betaalpha");
    }

    #[test]
    fn delete_a_paragraph_takes_the_blank_line() {
        let mut vim = Vim::new("one\ntwo\n\nthree\n");
        vim.press("dap");
        assert_eq!(vim.text(), "three\n");
    }

    #[test]
    fn an_object_key_that_names_nothing_does_nothing() {
        let mut vim = Vim::new("hello");
        vim.press("diz");
        assert_eq!(vim.text(), "hello");
    }

    #[test]
    fn insert_keys_are_still_insert_keys_on_their_own() {
        let mut vim = Vim::new("bc");
        vim.press("ia<esc>llax<esc>");
        assert_eq!(vim.text(), "abcx");
    }

    #[test]
    fn a_chord_does_not_finish_an_operator() {
        // `^d` arrives as a plain `d`, which would otherwise read as `dd`.
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("2d<C-d>");
        assert_eq!(vim.text(), "one\ntwo\nthree\n");
    }



    #[test]
    fn a_completion_finishes_the_word_and_undoes_with_the_insert() {
        let mut vim = Vim::new("render_widget\n");
        vim.press("Gorend<C-n><tab>");
        assert_eq!(vim.text(), "render_widget\nrender_widget\n");

        // One `u` takes the whole insert, completion and all: the command was
        // `o`, and it ended at `esc`.
        vim.press("<esc>u");
        assert_eq!(vim.text(), "render_widget\n");
    }

    #[test]
    fn an_insert_is_one_undo_step() {
        // Typing, however many transactions it takes inside, is one command.
        let mut vim = Vim::new("top\n");
        vim.press("ohello there<esc>");
        assert_eq!(vim.text(), "top\nhello there\n");
        vim.press("u");
        assert_eq!(vim.text(), "top\n");

        // The same for a change: `ciw` deletes and then types, and one `u`
        // puts back what was there.
        let mut vim = Vim::new("alpha beta\n");
        vim.press("ciwgamma<esc>");
        assert_eq!(vim.text(), "gamma beta\n");
        vim.press("u");
        assert_eq!(vim.text(), "alpha beta\n");

        // Backspacing inside the insert is part of it too - the step is not
        // one direction of travel.
        let mut vim = Vim::new("x\n");
        vim.press("Aabc<bs><bs>d<esc>");
        assert_eq!(vim.text(), "xad\n");
        vim.press("u");
        assert_eq!(vim.text(), "x\n");

        // And redo puts the whole thing back.
        vim.press("<C-r>");
        assert_eq!(vim.text(), "xad\n");
    }

    #[test]
    fn a_repeat_is_one_undo_step_too() {
        let mut vim = Vim::new("top\n");
        vim.press("ohi<esc>");
        vim.press(".");
        assert_eq!(vim.text(), "top\nhi\nhi\n");
        vim.press("u");
        assert_eq!(vim.text(), "top\nhi\n", "the repeat, not half of it");
        vim.press("u");
        assert_eq!(vim.text(), "top\n");
    }

    #[test]
    fn typing_narrows_the_popup_and_running_out_closes_it() {
        let mut vim = Vim::new("alpha album\n");
        vim.press("Goal<C-n>");
        assert!(vim.editor.completion.is_some());
        // `alb` still matches `album`...
        vim.press("b");
        assert!(vim.editor.completion.is_some());
        // ...and `albz` matches nothing.
        vim.press("z");
        assert!(vim.editor.completion.is_none());
        assert_eq!(vim.text(), "alpha album\nalbz\n");
    }

    #[test]
    fn escape_closes_the_popup_without_leaving_insert_mode() {
        let mut vim = Vim::new("alpha\n");
        vim.press("Goal<C-n>");
        assert!(vim.editor.completion.is_some());
        vim.press("<esc>");
        assert!(vim.editor.completion.is_none());
        assert_eq!(vim.editor.mode, Mode::Insert);
        // The second escape is the one that leaves.
        vim.press("<esc>");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn a_word_with_nothing_to_finish_it_says_so() {
        let mut vim = Vim::new("alpha\n");
        vim.press("Gozz<C-n>");
        assert!(vim.editor.completion.is_none());
        assert_eq!(vim.editor.message, "no completions");
    }


    #[test]
    fn the_arrows_walk_the_popup_and_enter_takes_one() {
        let mut vim = Vim::new("alpha album\n");
        vim.press("Goal<C-n><down>");
        assert_eq!(vim.editor.completion.as_ref().unwrap().selected(), Some(1));
        vim.press("<up>");
        assert_eq!(vim.editor.completion.as_ref().unwrap().selected(), Some(0));

        vim.press("<cr>");
        assert!(vim.editor.completion.is_none());
        assert_eq!(vim.text(), "alpha album\nalbum\n");
    }

    #[test]
    fn typing_brings_the_popup_up_by_itself() {
        let mut vim = Vim::new("album\n");
        vim.press("Goa");
        // One character is not a word yet.
        assert!(vim.editor.completion.is_none());
        vim.press("l");
        let completion = vim.editor.completion.as_ref().expect("suggested");
        // It suggests; it does not choose.
        assert_eq!(completion.selected(), None);
        assert_eq!(completion.len(), 1);
    }

    #[test]
    fn a_popup_that_came_up_by_itself_leaves_enter_and_tab_alone() {
        let mut vim = Vim::new("album\n");
        vim.press("Goal<cr>");
        assert_eq!(vim.text(), "album\nal\n\n");
        assert!(vim.editor.completion.is_none());

        // And the first arrow selects rather than stepping off the top.
        let mut vim = Vim::new("album\n");
        vim.press("Goal<down>");
        assert_eq!(vim.editor.completion.as_ref().unwrap().selected(), Some(0));
        vim.press("<cr>");
        assert_eq!(vim.text(), "album\nalbum\n");
    }

    #[test]
    fn a_dismissed_popup_stays_away_until_the_next_word() {
        let mut vim = Vim::new("album alphabet\n");
        vim.press("Goal<esc>");
        assert!(vim.editor.completion.is_none());
        // Still the same word, so it does not come back on its own...
        vim.press("p");
        assert!(vim.editor.completion.is_none());
        // ...but `^n` still works, because asking is not the same as being
        // offered.
        vim.press("<C-n><esc>");
        // And a new word starts over.
        vim.press(" al");
        assert!(vim.editor.completion.is_some());
    }

    #[test]
    fn deleting_does_not_bring_the_popup_up() {
        let mut vim = Vim::new("album\n");
        vim.press("Goalb<esc>");
        assert!(vim.editor.completion.is_none());
        vim.press("<bs>");
        assert!(vim.editor.completion.is_none());
    }

    #[test]
    fn nothing_is_suggested_inside_a_comment() {
        let mut vim = Vim::rust("fn album() {}\n// ");
        vim.press("GA");
        vim.press("al");
        assert!(vim.editor.completion.is_none(), "prose is not code");
        // Asking still works.
        vim.press("<C-n>");
        assert!(vim.editor.completion.is_some());
    }

    #[test]
    fn autocomplete_can_be_turned_off_and_its_length_set() {
        let mut vim = Vim::new("album\n");
        vim.editor.run_command("set noautocomplete");
        vim.press("Goal");
        assert!(vim.editor.completion.is_none());

        let mut vim = Vim::new("album\n");
        vim.editor.run_command("set autocomplete=4");
        vim.press("Goalb");
        assert!(vim.editor.completion.is_none());
        vim.press("u");
        assert!(vim.editor.completion.is_some());
    }

    #[test]
    fn typing_a_new_name_into_a_big_file_stays_cheap() {
        // Gathering candidates is milliseconds on a file this size, and
        // suggesting happens on a keystroke. A name nothing in the file
        // matches is the bad case: without remembering that, every further
        // character would gather the whole window again.
        let mut text = String::new();
        while text.len() < 800_000 {
            text.push_str("fn render_widget(count: usize) -> usize { count + 1 }\n");
        }
        let mut vim = Vim::rust(&text);

        let start = std::time::Instant::now();
        vim.press("Gozzaphod");
        let elapsed = start.elapsed();

        assert!(vim.editor.completion.is_none(), "nothing matches zzaphod");
        assert!(elapsed.as_millis() < 500, "typing took {elapsed:?}");
    }

    #[test]
    fn ctrl_c_copies_the_line_and_ctrl_v_puts_it_back() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("<C-c>");
        // Copying a line looks like nothing happening, so it says so.
        assert_eq!(vim.editor.message, "copied");
        vim.press("<C-v>");
        // A line copied is a line pasted onto a line of its own, not into the
        // middle of the one the cursor was on.
        assert_eq!(vim.text(), "one\none\ntwo\n");
    }

    #[test]
    fn the_plus_register_is_the_system_clipboard() {
        let mut vim = Vim::new("hello world\n");
        vim.press("\"+yw");
        assert_eq!(crate::clipboard::paste().as_deref(), Some("hello "));

        // And the other way: what is on the clipboard is what `"+p` puts.
        crate::clipboard::copy("elsewhere");
        vim.press("$\"+p");
        assert_eq!(vim.text(), "hello worldelsewhere\n");
    }

    #[test]
    fn a_delete_to_the_plus_register_copies_too() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("\"+dd");
        assert_eq!(vim.text(), "two\n");
        assert_eq!(crate::clipboard::paste().as_deref(), Some("one\n"));
    }

    #[test]
    fn a_visual_yank_to_the_plus_register_copies() {
        let mut vim = Vim::new("hello world\n");
        vim.press("wve\"+y");
        assert_eq!(crate::clipboard::paste().as_deref(), Some("world"));
    }

    #[test]
    fn putting_from_the_plus_register_does_not_copy_anything_back() {
        let mut vim = Vim::new("one\n");
        crate::clipboard::copy("outside");
        vim.press("\"+p");
        // `p` puts after the cursor, which is still on the first character.
        assert_eq!(vim.text(), "ooutsidene\n");
        // The clipboard is untouched by a read - `"+p` is not a copy.
        assert_eq!(crate::clipboard::paste().as_deref(), Some("outside"));
    }

    #[test]
    fn a_shift_arrow_selection_is_what_the_chords_copy() {
        // Shift and an arrow starts visual mode in normal mode, and selects
        // without leaving insert mode while typing. Both are selections, and
        // `^c` takes the selection rather than the line either way.
        let mut vim = Vim::new("hello world\n");
        vim.press("<S-right><S-right><S-right><S-right><C-c>");
        // Visual mode covers the character under the cursor, so four steps
        // right is five characters.
        assert_eq!(crate::clipboard::paste().as_deref(), Some("hello"));

        let mut vim = Vim::new("hello world\n");
        vim.press("i<S-right><S-right><S-right><S-right><S-right><C-c>");
        assert_eq!(crate::clipboard::paste().as_deref(), Some("hello"));
        assert_eq!(vim.editor.mode, Mode::Insert);
        // Copying while typing leaves the cursor where it was, so you can go
        // on typing from there.
        assert_eq!(vim.editor.cursor_coords(), (0, 5));
    }

    #[test]
    fn ctrl_x_while_typing_cuts_the_selection_and_ctrl_v_puts_it_back() {
        let mut vim = Vim::new("hello world\n");
        vim.press("A<S-left><S-left><S-left><S-left><S-left><C-x>");
        assert_eq!(vim.text(), "hello \n");
        assert_eq!(vim.editor.mode, Mode::Insert);
        // And back in at the cursor, not as a line.
        vim.press("<C-v>");
        assert_eq!(vim.text(), "hello world\n");
    }

    #[test]
    fn pasting_over_a_selection_while_typing_replaces_it() {
        let mut vim = Vim::new("hello world\n");
        crate::clipboard::copy("there");
        vim.press("A<S-left><S-left><S-left><S-left><S-left><C-v>");
        assert_eq!(vim.text(), "hello there\n");
    }

    #[test]
    fn ctrl_x_cuts_the_line() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("j<C-x>");
        assert_eq!(vim.text(), "one\nthree\n");
        vim.press("<C-v>");
        assert_eq!(vim.text(), "one\nthree\ntwo\n");
    }

    #[test]
    fn the_chords_take_the_selection_when_there_is_one() {
        let mut vim = Vim::new("hello world\n");
        vim.press("ve<C-c>");
        // Copying leaves visual mode, as it does everywhere else these keys
        // work.
        assert_eq!(vim.editor.mode, Mode::Normal);
        assert_eq!(crate::clipboard::paste().as_deref(), Some("hello"));

        vim.press("$<C-v>");
        assert_eq!(vim.text(), "hello worldhello\n");
    }

    #[test]
    fn ctrl_x_in_visual_mode_cuts_the_selection() {
        let mut vim = Vim::new("hello world\n");
        vim.press("wve<C-x>");
        assert_eq!(vim.text(), "hello \n");
        assert_eq!(vim.editor.mode, Mode::Normal);
    }

    #[test]
    fn ctrl_v_in_insert_mode_types_the_clipboard_in() {
        let mut vim = Vim::new("say \n");
        // As if it had been copied somewhere else entirely.
        crate::clipboard::copy("when");
        vim.press("A<C-v> so");
        assert_eq!(vim.text(), "say when so\n");
        assert_eq!(vim.editor.mode, Mode::Insert);
    }

    #[test]
    fn a_copy_here_is_a_paste_anywhere_else() {
        let mut vim = Vim::new("keep me\n");
        vim.press("<C-c>");
        // What the session's clipboard would have been handed.
        assert_eq!(crate::clipboard::paste().as_deref(), Some("keep me\n"));
    }

    #[test]
    fn pasting_nothing_says_so() {
        let mut vim = Vim::new("one\n");
        crate::clipboard::copy("");
        vim.press("<C-v>");
        assert_eq!(vim.text(), "one\n");
        assert_eq!(vim.editor.message, "nothing to put");
    }

    #[test]
    fn listing_the_definitions_in_a_big_file_stays_cheap() {
        // One tags query over the whole file, which is the whole cost of the
        // symbol picker: it is gathered once when the list opens, and typing
        // into it only filters what is already in hand.
        let mut text = String::new();
        let mut line = 0;
        while text.len() < 800_000 {
            text.push_str(&format!("fn render_{line}(count: usize) -> usize {{ count + 1 }}\n"));
            line += 1;
        }
        let mut vim = Vim::rust(&text);

        let start = std::time::Instant::now();
        vim.press("<space>d");
        let elapsed = start.elapsed();

        let picker = vim.editor.picker.as_ref().expect("a picker");
        assert_eq!(picker.matches().len(), line);
        assert!(elapsed.as_millis() < 500, "listing took {elapsed:?}");
    }

    #[test]
    fn enter_is_a_newline_again_once_the_popup_is_gone() {
        let mut vim = Vim::new("alpha\n");
        vim.press("Goal<C-n><esc><cr>x");
        assert_eq!(vim.text(), "alpha\nal\nx\n");
    }


    #[test]
    fn shifting_lines_moves_them_by_one_step() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press(">>");
        assert_eq!(vim.text(), "\tone\ntwo\nthree\n");
        vim.press("<lt><lt>");
        assert_eq!(vim.text(), "one\ntwo\nthree\n");
    }

    #[test]
    fn a_count_shifts_that_many_lines() {
        let mut vim = Vim::new("one\ntwo\nthree\nfour\n");
        vim.press("3>>");
        assert_eq!(vim.text(), "\tone\n\ttwo\n\tthree\nfour\n");
    }

    #[test]
    fn shifting_is_one_undo_step_and_lands_on_the_first_non_blank() {
        let mut vim = Vim::new("one\ntwo\n");
        vim.press("2>>");
        assert_eq!(vim.cursor(), (1, 2));
        vim.press("u");
        assert_eq!(vim.text(), "one\ntwo\n");
    }

    #[test]
    fn dedent_takes_what_indent_there_is_and_no_more() {
        let mut vim = Vim::new("  two spaces\n\t\ttwo tabs\nnone\n");
        vim.press("3<lt><lt>");
        // Two spaces is less than one step, so the line lands at the margin.
        assert_eq!(vim.text(), "two spaces\n\ttwo tabs\nnone\n");
    }

    #[test]
    fn blank_lines_are_left_where_they_are() {
        let mut vim = Vim::new("one\n\n   \ntwo\n");
        vim.press("4>>");
        assert_eq!(vim.text(), "\tone\n\n   \n\ttwo\n");
    }

    #[test]
    fn visual_shifts_the_selected_lines_and_a_count_is_levels() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press("Vj>");
        assert_eq!(vim.text(), "\tone\n\ttwo\nthree\n");
        assert_eq!(vim.editor.mode, Mode::Normal);

        let mut vim = Vim::new("one\ntwo\n");
        vim.press("Vj3>");
        assert_eq!(vim.text(), "\t\t\tone\n\t\t\ttwo\n");
    }

    #[test]
    fn a_motion_or_an_object_says_which_lines_to_shift() {
        let mut vim = Vim::new("one\ntwo\nthree\n");
        vim.press(">j");
        assert_eq!(vim.text(), "\tone\n\ttwo\nthree\n");

        let mut vim = Vim::new("one\ntwo\n\nthree\n");
        vim.press(">ip");
        assert_eq!(vim.text(), "\tone\n\ttwo\n\nthree\n");
    }

    #[test]
    fn expandtab_and_shiftwidth_decide_what_a_step_is() {
        let mut vim = Vim::new("one\n");
        vim.press(":set expandtab<cr>:set sw=2<cr>>>");
        assert_eq!(vim.text(), "  one\n");
        // And `tab` follows the same setting.
        vim.press("A<tab>x<esc>");
        assert_eq!(vim.text(), "  one x\n");
    }

    #[test]
    fn insert_mode_shifts_the_line_without_moving_off_the_word() {
        let mut vim = Vim::new("one\n");
        vim.press("A<C-t>");
        assert_eq!(vim.text(), "\tone\n");
        // Still at the end of the word, not pushed along by the tab.
        assert_eq!(vim.editor.mode, Mode::Insert);
        vim.press("x<C-d>");
        assert_eq!(vim.text(), "onex\n");
    }


    #[test]
    fn an_object_shifts_only_the_lines_it_covers() {
        // `ap` reaches to the start of the line after the paragraph, which is
        // not part of it.
        let mut vim = Vim::new("one\ntwo\n\nthree\n");
        vim.press(">ap");
        assert_eq!(vim.text(), "\tone\n\ttwo\n\nthree\n");
    }


    #[test]
    fn emacs_motions_move_the_cursor_in_insert_mode() {
        let mut vim = Vim::new("one two\nthree\n");
        vim.press(":set emacs<cr>i");
        vim.press("<C-e>");
        assert_eq!(vim.cursor(), (1, 8));
        vim.press("<C-a>");
        assert_eq!(vim.cursor(), (1, 1));
        vim.press("<C-f><C-f>");
        assert_eq!(vim.cursor(), (1, 3));
        vim.press("<C-n>");
        assert_eq!(vim.cursor(), (2, 3));
        vim.press("<M-b>");
        assert_eq!(vim.cursor(), (2, 1));
    }

    #[test]
    fn emacs_kills_go_to_the_register_and_come_back() {
        let mut vim = Vim::new("hello world\n");
        vim.press(":set emacs<cr>i<C-e><C-u>");
        assert_eq!(vim.text(), "\n");
        vim.press("<C-y>");
        assert_eq!(vim.text(), "hello world\n");

        // `^k` at the end of a line takes the line break, which joins the next.
        let mut vim = Vim::new("one\ntwo\n");
        vim.press(":set emacs<cr>i<C-e><C-k>");
        assert_eq!(vim.text(), "onetwo\n");
    }

    #[test]
    fn emacs_kills_a_word_at_a_time() {
        let mut vim = Vim::new("alpha beta gamma\n");
        vim.press(":set emacs<cr>A<C-w>");
        assert_eq!(vim.text(), "alpha beta \n");
        vim.press("<C-a><M-d>");
        assert_eq!(vim.text(), "beta \n");
    }

    #[test]
    fn transpose_swaps_the_two_characters_before_the_cursor() {
        let mut vim = Vim::new("teh\n");
        vim.press(":set emacs<cr>A<C-t>");
        assert_eq!(vim.text(), "the\n");
    }

    #[test]
    fn the_emacs_chords_are_off_until_they_are_asked_for() {
        // `^t` is indent until `:set emacs` makes it transpose.
        let mut vim = Vim::new("ab\n");
        vim.press("A<C-t>");
        assert_eq!(vim.text(), "\tab\n");
    }

    #[test]
    fn a_config_file_is_commands_one_to_a_line() {
        let mut vim = Vim::new("one\n");
        vim.editor.apply_config("# how I like it\n\nset emacs\n:set sw=2\nset expandtab\n");
        assert!(vim.editor.emacs);
        assert_eq!(vim.editor.indent.width, 2);

        // `set emacs` took, so `^t` transposes rather than indenting.
        vim.press("A<C-t>");
        assert_eq!(vim.text(), "oen\n");
    }

    #[test]
    fn a_config_line_that_goes_wrong_says_which_one() {
        let mut vim = Vim::new("one\n");
        vim.editor.apply_config("set number\nset wobble\nset emacs\n");
        assert_eq!(vim.editor.message, "init line 2: not an option: wobble");
        // And it stopped there, rather than carrying on past the mistake.
        assert!(!vim.editor.emacs);
    }


    #[test]
    fn reindent_puts_a_line_where_the_grammar_says() {
        let mut vim = Vim::rust("fn main() {\nlet x = 1;\n}\n");
        vim.press("j==");
        assert_eq!(vim.text(), "fn main() {\n\tlet x = 1;\n}\n");
    }

    #[test]
    fn reindent_over_a_motion_or_an_object_takes_the_whole_block() {
        let mut vim = Vim::rust("fn main() {\n  let x = 1;\n      if x > 0 {\nlet y = 2;\n}\n}\n");
        vim.press("=ip");
        assert_eq!(
            vim.text(),
            "fn main() {\n\tlet x = 1;\n\tif x > 0 {\n\t\tlet y = 2;\n\t}\n}\n"
        );
    }

    #[test]
    fn a_closing_brace_comes_back_out_on_its_own_line() {
        let mut vim = Vim::rust("fn main() {\n\tif a {\n\t\tb();\n\t\t}\n}\n");
        vim.press("jjj==");
        assert_eq!(vim.text(), "fn main() {\n\tif a {\n\t\tb();\n\t}\n}\n");
    }

    #[test]
    fn enter_indents_the_new_line_by_the_grammar() {
        let mut vim = Vim::rust("fn main() {\n}\n");
        vim.press("A<cr>x");
        assert_eq!(vim.text(), "fn main() {\n\tx\n}\n");
    }

    #[test]
    fn typing_a_closing_brace_takes_the_line_back_out() {
        let mut vim = Vim::rust("fn main() {\n");
        vim.press("A<cr>x();<cr>}");
        assert_eq!(vim.text(), "fn main() {\n\tx();\n}\n");
    }

    #[test]
    fn the_auto_indent_after_enter_is_part_of_the_same_undo() {
        let mut vim = Vim::rust("fn main() {\n}\n");
        vim.press("A<cr><esc>u");
        assert_eq!(vim.text(), "fn main() {\n}\n");
    }

    #[test]
    fn without_a_grammar_a_new_line_keeps_the_indent_it_had() {
        let mut vim = Vim::new("\t\tone\n");
        vim.press("A<cr>two<esc>");
        assert_eq!(vim.text(), "\t\tone\n\t\ttwo\n");

        // And `=` says so rather than doing something arbitrary.
        vim.press("==");
        assert_eq!(vim.editor.message, "no indent rules for this file");
    }

    #[test]
    fn autoindent_can_be_turned_off() {
        let mut vim = Vim::rust("fn main() {\n}\n");
        vim.press(":set noautoindent<cr>A<cr>x");
        assert_eq!(vim.text(), "fn main() {\nx\n}\n");
    }




    #[test]
    fn the_other_grammars_have_indent_rules_too() {
        let mut vim = Vim::file("demo.js", "function f() {\nconst a = {\nb: 1,\n};\n}\n");
        vim.press("=ip");
        assert_eq!(vim.text(), "function f() {\n\tconst a = {\n\t\tb: 1,\n\t};\n}\n");

        let mut vim = Vim::file("demo.html", "<div>\n<p>hi</p>\n</div>\n");
        vim.press("=ip");
        assert_eq!(vim.text(), "<div>\n\t<p>hi</p>\n</div>\n");
    }

    #[test]
    fn a_half_typed_block_falls_back_to_the_line_above() {
        // There is no block node yet - just an error node with a loose brace -
        // so the guess is what indents this, and it has to be right because it
        // is the case that happens on every keystroke.
        let mut vim = Vim::rust("fn main() {\n");
        vim.press("A<cr>let x = 1;");
        assert_eq!(vim.text(), "fn main() {\n\tlet x = 1;\n");
    }


    #[test]
    fn re_indenting_a_big_file_is_worth_measuring() {
        let mut text = String::new();
        text.push_str("fn main() {\n");
        for i in 0..5_000 {
            text.push_str(&format!("let x{i} = {i};\n"));
        }
        text.push_str("}\n");
        let mut vim = Vim::rust(&text);

        let start = std::time::Instant::now();
        vim.editor.reindent_lines(0, 5_001);
        let elapsed = start.elapsed();
        // One query run per line, restricted to that line: about 11us each in
        // release, so `=` over a whole file is tens of milliseconds and the
        // one line `enter` re-indents is free.
        println!("5000 lines re-indented in {elapsed:?}");
        assert!(vim.text().contains("\tlet x4999"), "the lines moved");
        assert!(elapsed.as_millis() < 2_000, "{elapsed:?}");
    }

}
