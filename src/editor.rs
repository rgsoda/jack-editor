use anyhow::Result;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use crate::buffer::Document;
use crate::complete::{self, Completion, Pick};
use crate::jump::{Jump, Jumps};
use crate::keys::BINDINGS;
use crate::object::{self, Object};
use crate::picker::{Item, Outcome, Picker, Source};
use crate::search::{self, Search};
use crate::stream::{self, Message, Sign};
use crate::register::{RegisterValue, Registers};
use crate::syntax::Highlights;
use crate::theme::Theme;
use crate::view::{self, Find, Indent, Move, Selection, TAB_WIDTH, View};

/// How many characters of a word bring the completion popup up on its own.
/// Two, because one character narrows a buffer to hundreds of words and three
/// is most of a short name already typed.
const DEFAULT_AUTOCOMPLETE: usize = 2;

/// A line of text being typed into the status line. Only search uses it so
/// far; `:` would be the second.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// `/` and `?`: a pattern, previewed as it is typed.
    Search { backward: bool },
    /// `:`: a command, run when it is accepted.
    Command,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
    /// Where the cursor and viewport were when it opened, so cancelling can
    /// put them back after the incremental preview has moved them.
    origin: (usize, usize),
}

impl Prompt {
    /// The character the prompt starts with, which is also how you can tell
    /// which way the search is going.
    pub fn sigil(&self) -> char {
        match self.kind {
            PromptKind::Search { backward: true } => '?',
            PromptKind::Search { backward: false } => '/',
            PromptKind::Command => ':',
        }
    }

    fn backward(&self) -> bool {
        matches!(self.kind, PromptKind::Search { backward: true })
    }

    fn is_search(&self) -> bool {
        matches!(self.kind, PromptKind::Search { .. })
    }
}

/// Whether the open buffers are listed along the top: never, when there is
/// more than one of them, or always.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Tabline {
    Off,
    #[default]
    Auto,
    Always,
}

impl Tabline {
    pub fn name(&self) -> &'static str {
        match self {
            Tabline::Off => "off",
            Tabline::Auto => "auto",
            Tabline::Always => "always",
        }
    }

    fn parse(value: &str) -> Option<Tabline> {
        Some(match value {
            "off" | "no" | "never" => Tabline::Off,
            "auto" => Tabline::Auto,
            "always" | "yes" => Tabline::Always,
            _ => return None,
        })
    }
}

/// What the gutter shows. Relative numbering is worth its cost when a count
/// is how you aim a motion (`5j`), but it is a cost: every cursor move
/// repaints every number, where absolute numbering repaints only on scroll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Numbers {
    Off,
    #[default]
    Absolute,
    /// Distance from the cursor's line, which is what a count needs.
    Relative,
    /// Relative, but the cursor's own line shows its absolute number.
    Hybrid,
}

impl Numbers {
    pub fn name(self) -> &'static str {
        match self {
            Numbers::Off => "off",
            Numbers::Absolute => "absolute",
            Numbers::Relative => "relative",
            Numbers::Hybrid => "hybrid",
        }
    }

    fn next(self) -> Self {
        match self {
            Numbers::Off => Numbers::Absolute,
            Numbers::Absolute => Numbers::Relative,
            Numbers::Relative => Numbers::Hybrid,
            Numbers::Hybrid => Numbers::Off,
        }
    }

    /// What goes in the gutter for `line`, given where the cursor is. Both
    /// count from zero; what is shown counts from one.
    pub fn label(self, line: usize, cursor: usize) -> Option<usize> {
        match self {
            Numbers::Off => None,
            Numbers::Absolute => Some(line + 1),
            Numbers::Relative => Some(line.abs_diff(cursor)),
            Numbers::Hybrid if line == cursor => Some(line + 1),
            Numbers::Hybrid => Some(line.abs_diff(cursor)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Keys are commands. The cursor sits *on* a character rather than
    /// between two, which is why normal-mode motions clamp.
    #[default]
    Normal,
    /// Keys are text.
    Insert,
    /// Like normal, but motions drag one end of a selection instead of
    /// collapsing it, and a command acts on that selection at once.
    Visual,
    /// Visual, snapped to whole lines.
    VisualLine,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine)
    }
}

/// The editor: the open views, and the state that is shared across all of them.
///
/// Anything that belongs to one document - its text, cursor, scroll position,
/// undo history and syntax tree - lives in a `View`. Anything the user thinks
/// of as belonging to the session - the mode, the registers, the theme - lives
/// here, so registers survive switching files and undo does not.
pub struct Editor {
    views: Vec<View>,
    current: usize,
    pub width: usize,
    /// Text rows only; the status line is not part of this.
    pub height: usize,
    pub mode: Mode,
    pub registers: Registers,
    pub theme: Theme,
    pub numbers: Numbers,
    /// Strip trailing whitespace when writing. On by default, and every save
    /// says how many lines it touched, so it is never silent.
    pub trim_on_save: bool,
    /// Whether the open buffers are listed along the top.
    pub tabline: Tabline,
    /// Indent a new line the way the grammar says, rather than copying the
    /// line above. Only does anything where there is an indent query.
    pub autoindent: bool,
    /// Emacs chords in insert mode. Off by default: this is a vim-flavoured
    /// editor and half of these keys already mean something else.
    pub emacs: bool,
    /// How many characters of a word bring the completion popup up on their
    /// own. Zero waits for `^n`.
    pub autocomplete: usize,
    /// One step of indentation: how wide, and tabs or spaces.
    pub indent: Indent,
    /// Draw the status line with Nerd Font glyphs. Off is plain ASCII, for a
    /// terminal whose font has not been patched.
    pub glyphs: bool,
    /// Set by `:q`, read by the run loop. `Some(true)` is `:q!`.
    pub quit: Option<bool>,
    /// Show git signs in the gutter.
    pub signs_enabled: bool,
    /// The diff we are waiting on: which request, and which view asked.
    signs_token: u64,
    signs_for: usize,
    /// Transient status-line text, cleared on the next keypress.
    pub message: String,
    /// The picker, when one is open. While it is, it owns the keyboard.
    pub picker: Option<Picker>,
    /// The last `f`, `F`, `t` or `T`, which is what `;` and `,` repeat. Shared
    /// across buffers, as the search is.
    pub last_find: Option<Find>,
    /// Where the cursor was before each jump, for `^o` and `^i`.
    pub jumps: Jumps,
    /// The insert-mode completion popup, while one is open.
    pub completion: Option<Completion>,
    /// The word the popup has nothing more to say about: dismissed with `esc`,
    /// or offered nothing when it was asked. It does not come up on its own
    /// again until you are on a different word - otherwise dismissing it only
    /// buys you one keystroke of quiet, and a name nothing matches costs a
    /// gather per character.
    dismissed: Option<usize>,
    /// The status-line prompt, when one is open. It owns the keyboard too.
    pub prompt: Option<Prompt>,
    pub search: Search,
    /// Identifies the open picker, so a background job that outlives it can be
    /// told from the one feeding the picker now.
    token: Arc<AtomicU64>,
    /// Where background jobs send their results. `None` outside a run loop,
    /// which is how the tests drive a picker without spawning threads.
    jobs: Option<Sender<Message>>,
}

impl Editor {
    pub fn scratch() -> Self {
        Editor::with_views(vec![View::new(Document::scratch())])
    }

    /// Open every path, sharing one editor. A path that is already open is not
    /// opened twice.
    pub fn open<P: AsRef<Path>>(paths: &[P]) -> Result<Self> {
        if paths.is_empty() {
            return Ok(Editor::scratch());
        }

        let mut editor = Editor::with_views(vec![View::new(Document::open(&paths[0])?)]);
        editor.attach_syntax(0);
        for path in &paths[1..] {
            editor.open_file(path)?;
        }
        editor.switch_to(0);
        Ok(editor)
    }

    fn with_views(views: Vec<View>) -> Self {
        let (theme, warning) = Theme::load_user();
        Editor {
            views,
            current: 0,
            width: 80,
            height: 24,
            mode: Mode::default(),
            registers: Registers::default(),
            numbers: Numbers::default(),
            trim_on_save: true,
            glyphs: true,
            tabline: Tabline::Auto,
            autoindent: true,
            emacs: false,
            autocomplete: DEFAULT_AUTOCOMPLETE,
            dismissed: None,
            indent: Indent { width: TAB_WIDTH, tabs: true },
            quit: None,
            signs_enabled: true,
            signs_token: 0,
            signs_for: 0,
            theme,
            message: warning.unwrap_or_default(),
            picker: None,
            last_find: None,
            jumps: Jumps::default(),
            completion: None,
            prompt: None,
            search: Search::default(),
            token: Arc::new(AtomicU64::new(0)),
            jobs: None,
        }
    }

    fn attach_syntax(&mut self, index: usize) {
        let Editor { views, theme, message, .. } = self;
        if let Some(warning) = views[index].attach_syntax(theme) {
            *message = warning;
        }
    }

    // --- search -------------------------------------------------------

    pub fn open_search(&mut self, backward: bool) {
        self.open_prompt(PromptKind::Search { backward });
    }

    pub fn open_command(&mut self) {
        self.open_prompt(PromptKind::Command);
    }

    fn open_prompt(&mut self, kind: PromptKind) {
        let view = self.view();
        self.prompt = Some(Prompt {
            kind,
            input: String::new(),
            origin: (view.sel.head, view.scroll_top),
        });
    }

    /// Route a key to the open prompt. Typing searches as you go, so the match
    /// is on screen before you commit to it.
    pub fn prompt_input(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => return self.cancel_prompt(),
            KeyCode::Char('c') if ctrl => return self.cancel_prompt(),
            KeyCode::Enter => return self.accept_prompt(),
            KeyCode::Backspace => {
                // Backspacing the prompt empty cancels it, as in vim: there is
                // nothing left to search for.
                if prompt.input.pop().is_none() {
                    return self.cancel_prompt();
                }
            }
            KeyCode::Char('u') if ctrl => prompt.input.clear(),
            KeyCode::Char('w') if ctrl => {
                let end = prompt.input.trim_end_matches(|c: char| !c.is_alphanumeric());
                let cut = end.rfind(|c: char| !c.is_alphanumeric()).map_or(0, |i| i + 1);
                prompt.input.truncate(cut);
            }
            KeyCode::Char(c) if !ctrl => prompt.input.push(c),
            _ => return,
        }
        // Only a search shows its answer as you type; a command waits.
        if self.prompt.as_ref().is_some_and(Prompt::is_search) {
            self.preview_search();
        }
    }

    /// Show where the pattern typed so far would take you, without committing.
    fn preview_search(&mut self) {
        let Some(prompt) = self.prompt.as_ref() else {
            return;
        };
        let (pattern, backward, origin) = (prompt.input.clone(), prompt.backward(), prompt.origin);

        if let Err(err) = self.search.set_pattern(&pattern) {
            self.message = err;
            return;
        }
        self.message.clear();
        self.search.highlight = true;
        self.search.backward = backward;

        // A half-typed pattern often matches nothing; that is not worth saying
        // until enter is pressed, but the cursor should go back either way.
        match self.search.find(&self.view().doc, origin.0, backward) {
            Some(hit) => self.jump_to(hit.start),
            None => self.restore_origin(origin),
        }
    }

    fn accept_prompt(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        if prompt.kind == PromptKind::Command {
            return self.run_command(&prompt.input.clone());
        }
        let origin = self.jump_at(prompt.origin.0);
        if prompt.input.is_empty() {
            // A bare `/` repeats the last search, as vim does.
            self.search.backward = prompt.backward();
            self.search_again(prompt.backward(), 1);
            return;
        }
        if self.search.find(&self.view().doc, prompt.origin.0, prompt.backward()).is_none() {
            self.restore_origin(prompt.origin);
            self.message = format!("pattern not found: {}", prompt.input);
            return;
        }
        // The preview has already moved the cursor; what the jump list wants is
        // where the search was typed from.
        self.jumps.push(origin);
    }

    fn cancel_prompt(&mut self) {
        match self.prompt.take() {
            // A cancelled search puts back what its preview moved.
            Some(prompt) if prompt.is_search() => {
                self.restore_origin(prompt.origin);
                self.search.highlight = false;
            }
            _ => {}
        }
        self.message.clear();
    }

    fn restore_origin(&mut self, origin: (usize, usize)) {
        let view = self.view_mut();
        view.sel = Selection::point(origin.0);
        view.scroll_top = origin.1;
        self.clamp_cursor();
    }

    /// Where the cursor is now, as a jump-list entry.
    pub fn here(&self) -> Jump {
        let (line, column) = self.cursor_coords();
        Jump { view: self.current, line, column }
    }

    /// A jump-list entry for a position in the current buffer. Search needs
    /// this: by the time `enter` is pressed the cursor is already on the hit
    /// the preview took it to, and the place worth remembering is the one the
    /// search was typed from.
    fn jump_at(&self, at: usize) -> Jump {
        let view = self.view();
        let line = view.doc.char_to_line(at);
        Jump { view: self.current, line, column: at - view.doc.line_to_char(line) }
    }

    /// Remember where the cursor is before a command moves it somewhere else.
    /// Called by the commands vim calls jumps - `gg`, `G`, `:{n}`, the
    /// searches, `*`, `%`, and opening something from a picker - and by
    /// nothing else, which is what keeps `^o` a list of places worth going
    /// back to rather than a history of every key pressed.
    pub fn push_jump(&mut self) {
        let here = self.here();
        self.jumps.push(here);
    }

    /// `^o`: back to where the last jump started.
    pub fn jump_back(&mut self) {
        let here = self.here();
        match self.jumps.back(here) {
            Some(jump) => self.go_to_jump(jump),
            None => self.message = "at the oldest jump".into(),
        }
    }

    /// `^i`: forward again, undoing a `^o`.
    pub fn jump_forward(&mut self) {
        match self.jumps.forward() {
            Some(jump) => self.go_to_jump(jump),
            None => self.message = "at the newest jump".into(),
        }
    }

    /// Put the cursor back on a remembered position. The line and column are
    /// clamped rather than trusted: the file has been edited since, and landing
    /// near where you meant beats refusing to go.
    fn go_to_jump(&mut self, jump: Jump) {
        self.switch_to(jump.view);
        let view = self.view_mut();
        let line = jump.line.min(view.last_line());
        let column = jump.column.min(view.doc.line_len_chars(line));
        view.sel = Selection::point(view.doc.line_to_char(line) + column);
        self.clamp_cursor();
    }

    fn jump_to(&mut self, at: usize) {
        self.view_mut().sel = Selection::point(at);
        self.clamp_cursor();
    }

    /// `n` repeats the last search the way it was going; `N` turns it around.
    pub fn search_repeat(&mut self, reverse: bool, count: usize) {
        self.search_again(self.search.backward != reverse, count);
    }

    /// The next match from where the cursor is now.
    pub fn search_again(&mut self, backward: bool, count: usize) {
        if !self.search.is_set() {
            self.message = "no previous search".into();
            return;
        }
        self.search.highlight = true;

        // One jump-list entry for the whole command, put there by the first
        // hit: `3n` is one jump, and a search that finds nothing is none.
        let origin = self.here();
        let mut wrapped = false;
        for step in 0..count {
            let from = self.view().sel.head;
            match self.search.find(&self.view().doc, from, backward) {
                Some(hit) => {
                    wrapped |= hit.wrapped;
                    if step == 0 {
                        self.jumps.push(origin);
                    }
                    self.jump_to(hit.start);
                }
                None => {
                    self.message = format!("pattern not found: {}", self.search.pattern);
                    return;
                }
            }
        }
        if wrapped {
            self.message = match backward {
                true => "search hit top, continuing at bottom".into(),
                false => "search hit bottom, continuing at top".into(),
            };
        }
    }

    /// `*`: search for the word the cursor is on.
    pub fn search_word_under_cursor(&mut self) {
        let Some(word) = self.view().word_under_cursor() else {
            self.message = "no word under the cursor".into();
            return;
        };
        if let Err(err) = self.search.set_pattern(&search::word_pattern(&word)) {
            self.message = err;
            return;
        }
        self.search.highlight = true;
        self.search.backward = false;
        self.search_again(false, 1);
    }

    /// `f` `F` `t` `T`: put the cursor on the `count`th target along this
    /// line. `extend` drags a visual selection rather than collapsing it.
    pub fn move_to_char(&mut self, find: Find, count: usize, extend: bool) {
        let Some(dest) = self.find_target(find, count) else {
            return;
        };
        let sel = &mut self.view_mut().sel;
        sel.head = dest;
        if !extend {
            sel.anchor = dest;
        }
        self.clamp_cursor();
    }

    /// The same as the target of an operator - `df,` and `ct)`. The range runs
    /// from the cursor to the target and is half-open, so `f` includes the
    /// character it lands on and `t` stops before it.
    pub fn select_to_char(&mut self, find: Find, count: usize) -> bool {
        let at = self.view().sel.head;
        let Some(dest) = self.find_target(find, count) else {
            return false;
        };
        let sel = &mut self.view_mut().sel;
        match find.backward {
            true => (sel.anchor, sel.head) = (dest, at),
            false => (sel.anchor, sel.head) = (at, dest + 1),
        }
        true
    }

    fn find_target(&mut self, find: Find, count: usize) -> Option<usize> {
        let at = self.view().sel.head;
        let found = self.view().find_char(at, find, count);
        if found.is_none() {
            self.message = format!("no {} on this line", find.target);
        }
        found
    }

    /// Remember a find so `;` and `,` can repeat it. Only a literal `f`, `F`,
    /// `t` or `T` does this: repeating a find must not rewrite what is being
    /// repeated, or `,` `,` would walk in one direction.
    pub fn remember_find(&mut self, find: Find) {
        self.last_find = Some(find);
    }

    /// `;` and `,` as a motion.
    pub fn repeat_to_char(&mut self, reverse: bool, count: usize, extend: bool) {
        let Some(find) = self.repeated_find(reverse) else {
            return;
        };
        let count = self.repeat_count(find, count);
        self.move_to_char(find, count, extend);
    }

    /// `;` and `,` as an operator's motion: `d;`.
    pub fn select_repeat(&mut self, reverse: bool, count: usize) -> bool {
        let Some(find) = self.repeated_find(reverse) else {
            return false;
        };
        let count = self.repeat_count(find, count);
        self.select_to_char(find, count)
    }

    /// Repeating a till that is already against its target means the next one:
    /// `t,` then `;` should reach the following comma rather than sitting
    /// where it is. Vim does this too - a `;` that cannot move is wasted.
    fn repeat_count(&self, find: Find, count: usize) -> usize {
        let at = self.view().sel.head;
        match find.till && self.view().find_char(at, find, count) == Some(at) {
            true => count + 1,
            false => count,
        }
    }

    /// What `;` repeats, or `,` repeats the other way.
    fn repeated_find(&mut self, reverse: bool) -> Option<Find> {
        let Some(find) = self.last_find else {
            self.message = "no previous find".into();
            return None;
        };
        Some(match reverse {
            true => Find { backward: !find.backward, ..find },
            false => find,
        })
    }

    /// `gd`: the definition of the word under the cursor, and `gD` for the
    /// file's own rather than a binding in scope. A jump, so `^o` comes back.
    pub fn goto_definition(&mut self, local: bool) {
        let Some(word) = self.view().word_under_cursor() else {
            self.message = "no word under the cursor".into();
            return;
        };
        let at = self.view().sel.head;
        let Some(found) = self.view().definition(&word, at, local) else {
            self.message = format!("no definition of {word}");
            return;
        };
        if found == at {
            self.message = format!("{word} is defined here");
            return;
        }
        self.push_jump();
        self.jump_to(found);
    }

    /// Run a `:` command. Unknown commands say so rather than doing nothing,
    /// which is the difference between a typo and a missing feature.
    /// Run `~/.config/soda_edit/init`: one command per line, written as it
    /// would be typed after `:` - `set number`, `set emacs`. Blank lines and
    /// `#` comments are skipped. Missing is not an error; the whole point is
    /// that it need not exist.
    pub fn load_config(&mut self) {
        let Some(path) = crate::theme::config_dir().map(|dir| dir.join("init")) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        self.apply_config(&text);
    }

    /// The commands in a config file, in order. Stops at the first line that
    /// had anything to say, because the line after it would overwrite the
    /// complaint in the status line before anyone saw it.
    pub fn apply_config(&mut self, text: &str) {
        for (number, line) in text.lines().enumerate() {
            let line = line.trim().trim_start_matches(':');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            self.run_command(line);
            if !self.message.is_empty() {
                self.message = format!("init line {}: {}", number + 1, self.message);
                return;
            }
        }
    }

    pub fn run_command(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        let (name, argument) = match line.split_once(char::is_whitespace) {
            Some((name, rest)) => (name, rest.trim()),
            None => (line, ""),
        };
        // `:42` goes to line 42, as it does everywhere.
        if let Ok(number) = name.parse::<usize>() {
            self.push_jump();
            self.goto_line(number.saturating_sub(1));
            self.clamp_cursor();
            return;
        }

        let force = name.ends_with('!');
        match (name.trim_end_matches('!'), argument) {
            ("w" | "write", "") => self.write(None, force),
            ("w" | "write", path) => self.write(Some(path.into()), force),
            ("q" | "quit", _) => self.quit = Some(force),
            ("wq" | "x", _) => {
                self.write(None, force);
                if self.message.starts_with("wrote") {
                    self.quit = Some(force);
                }
            }
            ("e" | "edit", "") => self.reload(force),
            ("e" | "edit", path) => {
                if let Err(err) = self.open_file(path) {
                    self.message = format!("{err:#}");
                }
            }
            ("set", option) => self.set_option(option),
            ("noh" | "nohlsearch", _) => self.clear_search_highlight(),
            (other, _) => self.message = format!("not a command: {other}"),
        }
    }

    fn set_option(&mut self, option: &str) {
        // Options that take a value: `:set shiftwidth=2`.
        if let Some((name, value)) = option.split_once('=') {
            match (name, value.parse::<usize>()) {
                ("shiftwidth" | "sw", Ok(width)) if width > 0 && width <= 16 => {
                    self.indent.width = width;
                }
                ("autocomplete" | "ac", Ok(chars)) if chars <= 16 => {
                    self.autocomplete = chars;
                }
                ("autocomplete" | "ac", _) => {
                    self.message = format!("autocomplete wants 0 to 16, not {value:?}");
                }
                ("tabline", _) => match Tabline::parse(value) {
                    Some(tabline) => self.tabline = tabline,
                    None => self.message = "tabline wants off, auto or always".into(),
                },
                ("shiftwidth" | "sw", _) => {
                    self.message = format!("shiftwidth wants 1 to 16, not {value:?}");
                }
                _ => self.message = format!("not an option: {name}"),
            }
            return;
        }
        match option {
            "tabline" => self.tabline = Tabline::Always,
            "notabline" => self.tabline = Tabline::Off,
            "autoindent" | "ai" => self.autoindent = true,
            "noautoindent" | "noai" => self.autoindent = false,
            "emacs" => self.emacs = true,
            "noemacs" => self.emacs = false,
            "autocomplete" | "ac" => self.autocomplete = DEFAULT_AUTOCOMPLETE,
            "noautocomplete" | "noac" => self.autocomplete = 0,
            "expandtab" | "et" => self.indent.tabs = false,
            "noexpandtab" | "noet" => self.indent.tabs = true,
            "number" | "nu" => self.numbers = Numbers::Absolute,
            "nonumber" | "nonu" => self.numbers = Numbers::Off,
            "relativenumber" | "rnu" => self.numbers = Numbers::Relative,
            "hybrid" => self.numbers = Numbers::Hybrid,
            "trim" => self.trim_on_save = true,
            "notrim" => self.trim_on_save = false,
            "glyphs" => self.glyphs = true,
            "noglyphs" => self.glyphs = false,
            "signs" => self.signs_enabled = true,
            "nosigns" => {
                self.signs_enabled = false;
                self.view_mut().signs.clear();
            }
            "" => {
                self.message = format!(
                    "number={} trim={} signs={} glyphs={} shiftwidth={} expandtab={} autoindent={} emacs={} tabline={} autocomplete={}",
                    self.numbers.name(),
                    self.trim_on_save,
                    self.signs_enabled,
                    self.glyphs,
                    self.indent.width,
                    !self.indent.tabs,
                    self.autoindent,
                    self.emacs,
                    self.tabline.name(),
                    self.autocomplete
                );
            }
            other => self.message = format!("not an option: {other}"),
        }
    }

    /// `%`: jump to the bracket matching the one under the cursor.
    pub fn jump_to_matching_bracket(&mut self) {
        let head = self.view().sel.head;
        match self.view().matching_bracket(head) {
            Some(at) => {
                self.push_jump();
                let extend = self.mode.is_visual();
                let view = self.view_mut();
                match extend {
                    true => view.sel.head = at,
                    false => view.sel = Selection::point(at),
                }
                self.clamp_cursor();
            }
            None => self.message = "no bracket under the cursor".into(),
        }
    }

    /// The pair to paint: the bracket under the cursor and the one it matches.
    pub fn bracket_pair(&self) -> Option<(usize, usize)> {
        let head = self.view().sel.head;
        self.view().matching_bracket(head).map(|other| (head, other))
    }

    /// `esc` in normal mode stops painting the matches.
    pub fn clear_search_highlight(&mut self) {
        self.search.highlight = false;
    }

    // --- picker -------------------------------------------------------

    /// Where background jobs should send their results. Set once, by the run
    /// loop that owns the other end.
    pub fn set_jobs(&mut self, jobs: Sender<Message>) {
        self.jobs = Some(jobs);
    }

    /// Open a picker, retiring any job feeding the previous one.
    fn open_picker(&mut self, picker: Picker) -> u64 {
        self.picker = Some(picker);
        self.retire()
    }

    /// Invalidate whatever job is running: its results stop being wanted, and
    /// it stops as soon as it notices.
    fn retire(&self) -> u64 {
        self.token.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn token(&self) -> u64 {
        self.token.load(Ordering::Relaxed)
    }

    /// The open buffers, in the order they were opened. The current one is
    /// marked, as is any with unsaved changes.
    pub fn open_buffer_picker(&mut self) {
        let current = self.current;
        let items = self
            .views
            .iter()
            .enumerate()
            .map(|(id, view)| Item {
                text: view.doc.display_name().to_string(),
                target: String::new(),
                detail: match (id == current, view.is_modified()) {
                    (true, true) => "% [+]".into(),
                    (true, false) => "%".into(),
                    (false, true) => "[+]".into(),
                    (false, false) => String::new(),
                },
                id,
            })
            .collect();
        self.open_picker(Picker::new(Source::Buffers, items));
    }

    /// Every key, searchable by the key or by what it does.
    pub fn open_help_picker(&mut self) {
        let width = BINDINGS.iter().map(|b| b.keys.chars().count()).max().unwrap_or(0);
        let items = BINDINGS
            .iter()
            .map(|binding| Item {
                // The keys are part of the text so that both they and the
                // description are searchable.
                text: format!("{:width$}  {}", binding.keys, binding.what),
                detail: binding.mode.to_string(),
                id: 0,
                target: String::new(),
            })
            .collect();
        self.open_picker(Picker::new(Source::Help, items));
    }

    /// Lines matching a pattern, anywhere under the working directory. Nothing
    /// runs until something is typed: the pattern *is* the query.
    pub fn open_grep_picker(&mut self) {
        self.open_picker(Picker::live(Source::Grep));
    }

    /// Every file under the working directory. The walk runs on its own thread
    /// and streams in, so the picker is usable on a big tree immediately.
    pub fn open_file_picker(&mut self) {
        let token = self.open_picker(Picker::streaming(Source::Files));
        let Some(jobs) = self.jobs.clone() else {
            return;
        };
        match std::env::current_dir() {
            Ok(root) => stream::spawn_walk(root, token, Arc::clone(&self.token), jobs),
            Err(err) => self.message = format!("{err:#}"),
        }
    }

    /// Run the search behind the grep picker again, retiring the one before it.
    /// Every keystroke lands here, which is why cancellation has to be cheap.
    fn search(&mut self, pattern: String) {
        let token = self.retire();
        if pattern.is_empty() {
            return;
        }
        let Some(jobs) = self.jobs.clone() else {
            return;
        };
        match std::env::current_dir() {
            Ok(root) => stream::spawn_grep(root, pattern, token, Arc::clone(&self.token), jobs),
            Err(err) => self.message = format!("{err:#}"),
        }
    }

    /// A batch of streamed candidates. Batches from a retired job are dropped.
    pub fn stream_items(&mut self, token: u64, items: Vec<String>, done: bool) {
        if token != self.token() {
            return;
        }
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let source = picker.source;
        let items = items.into_iter().map(|text| match source {
            Source::Grep => grep_item(text),
            _ => Item { detail: String::new(), id: 0, target: text.clone(), text },
        });
        picker.extend(items, done);
    }

    /// A background job gave up. Its message belongs in the status line, not in
    /// the list, and only if it is still the job we are waiting on.
    pub fn job_failed(&mut self, token: u64, error: String) {
        if token != self.token() {
            return;
        }
        self.message = error;
        // Whatever the picker has is all it is getting.
        if let Some(picker) = self.picker.as_mut() {
            picker.mark_complete();
        }
    }

    /// Route a key to the open picker. The caller has already checked that one
    /// is open.
    pub fn picker_input(&mut self, key: crossterm::event::KeyEvent) {
        let rows = self.height;
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        match picker.input(key, rows) {
            Outcome::Continue => {}
            // Retiring the token stops a walk still in flight from feeding
            // whatever picker opens next.
            Outcome::Cancel => {
                self.picker = None;
                self.retire();
            }
            Outcome::Search(pattern) => self.search(pattern),
            Outcome::Confirm(source, choice) => {
                self.picker = None;
                self.retire();
                // Where the picker was opened from, put on the jump list by
                // whatever the choice turns out to reach - and not at all when
                // the file will not open.
                let origin = self.here();
                match source {
                    // Help is a list to read; choosing a line just closes it.
                    Source::Help => {}
                    Source::Buffers => {
                        self.jumps.push(origin);
                        self.switch_to(choice.id);
                    }
                    Source::Files => match self.open_file(&choice.target) {
                        Ok(()) => self.jumps.push(origin),
                        Err(err) => self.message = format!("{err:#}"),
                    },
                    Source::Grep => match self.open_file(&choice.target) {
                        // Line numbers count from one; lines here count from zero.
                        Ok(()) => {
                            self.jumps.push(origin);
                            self.goto_line(choice.id.saturating_sub(1));
                            self.clamp_cursor();
                        }
                        Err(err) => self.message = format!("{err:#}"),
                    },
                }
            }
        }
    }

    // --- buffers ------------------------------------------------------

    /// Put the selection over the text object at the cursor, for `diw` and
    /// friends. Outside visual mode the selection is left half-open, the way an
    /// operator's motion leaves it; in visual mode the cursor sits on the last
    /// character, because that is where visual mode's cursor lives.
    pub fn select_object(&mut self, object: Object, around: bool) -> bool {
        let view = self.view();
        let Some((start, end)) = object::resolve(&view.doc, view.sel.head, object, around) else {
            return false;
        };
        let visual = self.mode.is_visual();
        let view = self.view_mut();
        view.sel.anchor = start;
        view.sel.head = match visual && end > start {
            true => view.grapheme_left(end),
            false => end,
        };
        true
    }

    pub fn view(&self) -> &View {
        &self.views[self.current]
    }

    pub fn view_mut(&mut self) -> &mut View {
        &mut self.views[self.current]
    }

    pub fn views(&self) -> &[View] {
        &self.views
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    /// True if *any* open view has unsaved changes, which is what quitting
    /// needs to know.
    pub fn any_modified(&self) -> bool {
        self.views.iter().any(View::is_modified)
    }

    pub fn switch_to(&mut self, index: usize) {
        if index < self.views.len() {
            self.current = index;
            self.clamp_cursor();
        }
    }

    pub fn next_view(&mut self) {
        let next = (self.current + 1) % self.views.len();
        self.switch_to(next);
    }

    pub fn previous_view(&mut self) {
        let previous = (self.current + self.views.len() - 1) % self.views.len();
        self.switch_to(previous);
    }

    /// Open a file, or switch to it if it is already open.
    pub fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        let canonical = path.canonicalize().ok();
        let existing = self.views.iter().position(|view| match &view.doc.path {
            Some(open) => {
                open == path || (canonical.is_some() && open.canonicalize().ok() == canonical)
            }
            None => false,
        });

        if let Some(index) = existing {
            self.switch_to(index);
            return Ok(());
        }

        self.views.push(View::new(Document::open(path)?));
        let index = self.views.len() - 1;
        self.attach_syntax(index);
        self.switch_to(index);
        Ok(())
    }

    // --- delegated to the current view --------------------------------

    /// The screen row the text starts on: one down when the buffers are
    /// listed above it, otherwise the top of the terminal.
    pub fn top(&self) -> usize {
        match self.show_tabline() {
            true => 1,
            false => 0,
        }
    }

    pub fn show_tabline(&self) -> bool {
        match self.tabline {
            Tabline::Off => false,
            Tabline::Auto => self.views.len() > 1,
            Tabline::Always => true,
        }
    }

    pub fn set_viewport(&mut self, width: usize, height: usize) {
        self.width = width.max(1);
        self.height = height.max(1);
    }

    /// Ask git how the current buffer differs from the last commit, but only
    /// when something has actually changed since the last time we asked, and
    /// never mid-keystroke while typing.
    pub fn refresh_signs(&mut self) {
        if !self.signs_enabled || self.mode == Mode::Insert {
            return;
        }
        let revision = self.view().revision();
        if self.view().signs_revision == Some(revision) {
            return;
        }
        self.view_mut().signs_revision = Some(revision);

        let (Some(path), Some(jobs)) = (self.view().doc.path.clone(), self.jobs.clone()) else {
            return;
        };
        self.signs_token += 1;
        self.signs_for = self.current;
        let text = self.view().doc.text.to_string();
        stream::spawn_git_diff(path, text, self.signs_token, jobs);
    }

    pub fn set_signs(&mut self, token: u64, signs: Vec<(usize, Sign)>) {
        if token == self.signs_token {
            self.views[self.signs_for].signs = signs.into_iter().collect();
        }
    }

    pub fn cycle_numbers(&mut self) {
        self.numbers = self.numbers.next();
        self.message = format!("line numbers: {}", self.numbers.name());
    }

    /// Columns the gutter takes. Sized from the whole buffer rather than what
    /// is on screen, so it does not twitch between 99 and 100 while scrolling.
    /// One column for the git sign, if signs are on, then the numbers.
    pub fn sign_width(&self) -> usize {
        usize::from(self.signs_enabled)
    }

    pub fn gutter_width(&self) -> usize {
        let numbers = match self.numbers {
            Numbers::Off => 0,
            // A space either side of the number.
            _ => (self.view().doc.len_lines()).max(1).to_string().len().max(2) + 2,
        };
        self.sign_width() + numbers
    }

    /// What is left for text once the gutter has taken its columns.
    pub fn text_width(&self) -> usize {
        self.width.saturating_sub(self.gutter_width()).max(1)
    }

    pub fn highlights(&self, range: Range<usize>) -> Highlights {
        self.view().highlights(range, &self.theme)
    }

    pub fn cursor_coords(&self) -> (usize, usize) {
        self.view().cursor_coords()
    }

    /// Where the terminal cursor goes. An open picker takes it: the user is
    /// typing a query, not editing text.
    pub fn cursor_screen(&self) -> (u16, u16) {
        // A prompt puts the cursor on the status line, after what is typed.
        if let Some(prompt) = self.prompt.as_ref() {
            return (
                1 + prompt.input.chars().count() as u16,
                (self.top() + self.height) as u16,
            );
        }
        let top = self.top() as u16;
        match self.picker.as_ref() {
            Some(picker) => {
                let (x, y) = picker.cursor_screen(self.height);
                (x, y + top)
            }
            None => {
                let (x, y) = self.view().cursor_screen();
                (x + self.gutter_width() as u16, y + top)
            }
        }
    }

    pub fn is_modified(&self) -> bool {
        self.view().is_modified()
    }

    pub fn last_line(&self) -> usize {
        self.view().last_line()
    }

    pub fn move_cursor(&mut self, m: Move, extend: bool) {
        let height = self.height;
        self.view_mut().move_cursor(m, extend, height);
    }

    pub fn scroll_to_cursor(&mut self) {
        let (width, height) = (self.text_width(), self.height);
        self.view_mut().scroll_to_cursor(width, height);
    }

    pub fn goto_line(&mut self, line: usize) {
        self.view_mut().goto_line(line);
    }

    /// Jump to a line without letting go of the other end of the selection.
    pub fn goto_line_extending(&mut self, line: usize) {
        let anchor = self.view().sel.anchor;
        self.view_mut().goto_line(line);
        self.view_mut().sel.anchor = anchor;
        self.clamp_cursor();
    }

    /// `^n` / `^p`: offer what could finish the word at the cursor. `backward`
    /// is `^p`, which opens on the last candidate rather than the first.
    pub fn open_completion(&mut self, backward: bool) {
        let at = self.view().sel.head;
        let pick = match backward {
            true => Pick::Last,
            false => Pick::First,
        };
        self.completion = Completion::new(self.view(), at, pick);
        self.dismissed = None;
        if self.completion.is_none() {
            self.message = "no completions".into();
        }
    }

    /// Called after every character typed in insert mode: bring the popup up
    /// by itself once the word is long enough to be worth finishing.
    ///
    /// Nothing is selected when it opens, so typing straight past it changes
    /// nothing - the popup is a list of what you *could* press `^n` for, not a
    /// guess at what you meant. No message either: a suggestion that has
    /// nothing to suggest should say nothing at all.
    pub fn suggest_completion(&mut self) {
        if self.autocomplete == 0 || self.mode != Mode::Insert || self.completion.is_some() {
            return;
        }
        let at = self.view().sel.head;
        let view = self.view();
        // The word so far, and nothing to offer until there is enough of it to
        // narrow the buffer down.
        let mut start = at;
        while start > 0 && complete::is_word(view.doc.text.char(start - 1)) {
            start -= 1;
        }
        if at - start < self.autocomplete || self.dismissed == Some(start) {
            return;
        }
        // Prose, where offering to finish every word in the file is noise.
        if view.in_comment_or_string(start) {
            return;
        }
        self.completion = Completion::new(view, at, Pick::Nothing);
        // Nothing to offer, and there never will be: the candidates come from
        // a window that does not move while you type, and a longer prefix can
        // only match fewer of them. Remembering that is what keeps typing a
        // new name in a big file from gathering once per keystroke.
        if self.completion.is_none() {
            self.dismissed = Some(start);
        }
    }

    pub fn completion_step(&mut self, forward: bool) {
        if let Some(completion) = self.completion.as_mut() {
            completion.step(forward);
        }
    }

    /// `esc`: put the popup away, and remember the word it was over so it
    /// stays away until you start another one.
    pub fn close_completion(&mut self) {
        self.dismissed = self.completion.take().map(|completion| completion.start);
    }

    /// Follow the buffer as it changes under the popup: a typed character
    /// narrows the list, a deleted one widens it, and running out closes it.
    pub fn update_completion(&mut self) {
        let at = self.view().sel.head;
        let alive = match self.completion.as_mut() {
            Some(completion) => {
                let view = &self.views[self.current];
                completion.update(view, at)
            }
            None => return,
        };
        if !alive {
            self.completion = None;
        }
    }

    /// Replace the half-typed word with the selected candidate, as one edit so
    /// a single undo takes the whole completion back.
    ///
    /// False when there is nothing selected - a popup that came up on its own
    /// and has not been stepped into. The key that asked then goes on to mean
    /// what it usually means, which is how `enter` stays `enter`.
    pub fn accept_completion(&mut self) -> bool {
        let Some(text) = self
            .completion
            .as_ref()
            .and_then(|completion| completion.selected_text())
            .map(str::to_string)
        else {
            return false;
        };
        let start = self.completion.take().expect("checked above").start;
        let at = self.view().sel.head;
        let cursor = start + text.chars().count();
        self.view_mut().edit_at(start, at - start, &text, Some(cursor));
        true
    }

    /// `>>` and `<<`, `>` over a motion, `>` in visual mode: move lines
    /// sideways by whole steps of indentation.
    pub fn shift_lines(&mut self, out: bool, first: usize, last: usize, levels: usize) {
        let indent = self.indent;
        self.view_mut().shift_lines(first, last, out, levels, indent);
    }

    /// The lines `count` lines from the cursor down, for `3>>`.
    pub fn shift_count(&mut self, out: bool, count: usize) {
        let (line, _) = self.view().cursor_coords();
        self.shift_lines(out, line, line + count - 1, 1);
    }

    /// The lines a selection covers, for visual `>` and for `>ip`. The range
    /// is half-open at both: a selection that stops at the start of a line has
    /// not reached into it.
    pub fn shift_selection(&mut self, out: bool, levels: usize) {
        let (first, last) = self.selection_lines();
        self.shift_lines(out, first, last, levels);
    }

    /// The first and last line a selection or an object reaches. Half-open at
    /// the end: a range that stops at the start of a line has not reached into
    /// it.
    fn selection_lines(&self) -> (usize, usize) {
        let (start, end) = self.selection_range().unwrap_or_else(|| self.view().sel.range());
        let view = self.view();
        let (first, _) = view.doc.coords(start);
        let (last, column) = view.doc.coords(end);
        match column == 0 && last > first {
            true => (first, last - 1),
            false => (first, last),
        }
    }

    /// `=`: put lines where the grammar says they belong.
    pub fn reindent_lines(&mut self, first: usize, last: usize) {
        let indent = self.indent;
        let last = last.min(self.view().last_line());
        let mut targets = Vec::new();
        for line in first..=last {
            if let Some(level) = self.view().indent_level(line) {
                targets.push((line, level * indent.width));
            }
        }
        if !self.view().has_indent_rules() {
            self.message = "no indent rules for this file".into();
            return;
        }
        self.view_mut().set_indents(&targets, indent);
    }

    /// The lines a selection covers, for visual `=` and `=ip`.
    pub fn reindent_selection(&mut self) {
        let (first, last) = self.selection_lines();
        self.reindent_lines(first, last);
    }

    /// The lines a motion covered, for `=j`.
    pub fn reindent_motion(&mut self) {
        let view = self.view();
        let (first, _) = view.doc.coords(view.sel.anchor.min(view.sel.head));
        let (last, _) = view.doc.coords(view.sel.anchor.max(view.sel.head));
        self.reindent_lines(first, last);
    }

    /// Re-indent the line the cursor is on. `merge` makes it part of the edit
    /// before it, which is what the auto-indent after `enter` wants.
    fn reindent_current_line(&mut self, merge: bool) {
        if !self.autoindent || !self.view().has_indent_rules() {
            return;
        }
        let indent = self.indent;
        let (line, _) = self.view().cursor_coords();
        let column = match self.view().indent_level(line) {
            Some(level) => level * indent.width,
            None => match self.guessed_indent_column(line) {
                Some(column) => column,
                None => return,
            },
        };
        self.view_mut().set_line_indent(line, column, indent, merge);
    }

    /// What to indent a line to when the tree cannot say - which is most of
    /// the time while you are typing, because an unclosed brace is an error
    /// node and not a block. The old rule, and a good one: the line above,
    /// plus a step if it opened something, minus one if this line closes it.
    fn guessed_indent_column(&self, line: usize) -> Option<usize> {
        let view = self.view();
        let previous = (0..line)
            .rev()
            .map(|l| view.doc.line_str(l))
            .find(|text| !text.trim().is_empty())?;

        let leading = previous.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let mut column = view::display_col(&previous, leading);
        if previous.trim_end().ends_with(['{', '[', '(']) {
            column += self.indent.width;
        }
        if view.doc.line_str(line).trim_start().starts_with(['}', ']', ')']) {
            column = column.saturating_sub(self.indent.width);
        }
        Some(column)
    }

    /// The lines a motion covered, for `>j`. A motion ends *on* a line rather
    /// than before it, which is why `>j` moves two lines and not one.
    pub fn shift_motion(&mut self, out: bool) {
        let view = self.view();
        let (first, _) = view.doc.coords(view.sel.anchor.min(view.sel.head));
        let (last, _) = view.doc.coords(view.sel.anchor.max(view.sel.head));
        self.shift_lines(out, first, last, 1);
    }

    /// Cut from the cursor to wherever `motion` lands, into the unnamed
    /// register so `^y` - or `p` - puts it back. The emacs kills are all this
    /// with a different motion.
    pub fn kill(&mut self, motion: Move) {
        let head = self.view().sel.head;
        self.view_mut().sel.anchor = head;
        self.move_cursor(motion, true);
        let (start, end) = self.view().sel.range();
        self.cut_range(None, start, end, false);
    }

    /// `^k`: to the end of the line, or the line break itself when there is
    /// nothing left on the line - which is how emacs joins the next line up.
    pub fn kill_to_line_end(&mut self) {
        let view = self.view();
        let (line, column) = view.doc.coords(view.sel.head);
        match column < view.doc.line_len_chars(line) {
            true => self.kill(Move::LineEnd),
            false => {
                let head = self.view().sel.head;
                let end = self.view().grapheme_right(head);
                self.cut_range(None, head, end, false);
            }
        }
    }

    /// `^y`: put the last kill back at the cursor, as typing it would.
    pub fn yank_kill(&mut self) {
        let value = self.registers.get(None);
        if value.is_empty() {
            self.message = "nothing to put".into();
            return;
        }
        let text = value.text.clone();
        self.insert(&text);
    }

    /// `^t`: swap the two characters around the cursor, as emacs does - which
    /// at the end of a line means the two before it.
    pub fn transpose_chars(&mut self) {
        let view = self.view();
        let (line, column) = view.doc.coords(view.sel.head);
        // Mid-line, emacs steps over the character after the cursor before
        // swapping, so `ab|cd` leaves you with `abdc|`.
        if column < view.doc.line_len_chars(line) {
            self.move_cursor(Move::Right, false);
        }
        let head = self.view().sel.head;
        let middle = self.view().grapheme_left(head);
        let start = self.view().grapheme_left(middle);
        if start == middle || middle == head {
            return;
        }
        let swapped = format!(
            "{}{}",
            self.view().doc.slice_str(middle, head),
            self.view().doc.slice_str(start, middle)
        );
        self.view_mut().edit_at(start, head - start, &swapped, Some(head));
    }

    /// `tab` in insert mode: a literal tab, or spaces to the next stop.
    pub fn insert_tab(&mut self) {
        let at = self.view().cursor_display_col();
        let text = self.indent.tab(at);
        self.insert(&text);
    }

    /// `^t` and `^d` in insert mode: shift the line the cursor is on without
    /// leaving insert mode or moving the cursor off its word.
    pub fn shift_current_line(&mut self, out: bool) {
        let (line, column) = self.view().cursor_coords();
        let before = self.view().doc.line_len_chars(line);
        let indent = self.indent;
        self.view_mut().shift_lines(line, line, out, 1, indent);
        // The cursor keeps its place in the text rather than its column.
        let after = self.view().doc.line_len_chars(line);
        let base = self.view().doc.line_to_char(line);
        let moved = (column + after).saturating_sub(before).min(after);
        self.view_mut().sel = Selection::point(base + moved);
    }

    pub fn insert(&mut self, text: &str) {
        self.view_mut().insert(text);
        // A closing bracket typed at the start of a line belongs under what it
        // closes, and until it is typed there is nothing to tell the grammar
        // where that is. Only when it is the whole line so far, so a bracket
        // typed in the middle of an expression is left alone.
        if matches!(text, "}" | "]" | ")") && self.line_is_only_indent_before(1) {
            self.reindent_current_line(false);
        }
    }

    /// True when everything before the cursor on this line is whitespace, not
    /// counting the last `tail` characters.
    fn line_is_only_indent_before(&self, tail: usize) -> bool {
        let view = self.view();
        let (line, column) = view.cursor_coords();
        let text = view.doc.line_str(line);
        column >= tail
            && text
                .chars()
                .take(column - tail)
                .all(|c| c == ' ' || c == '\t')
    }

    pub fn insert_newline(&mut self) {
        self.view_mut().insert_newline();
        // The copied indent is the fallback; where the grammar has an opinion
        // it replaces it, folded into the same undo step.
        self.reindent_current_line(true);
    }

    pub fn delete_backward(&mut self) {
        self.view_mut().delete_backward();
    }

    pub fn delete_forward(&mut self) {
        self.view_mut().delete_forward();
    }

    pub fn set_mode(&mut self, mode: Mode) {
        // The popup belongs to insert mode, whichever way you leave it.
        if mode != Mode::Insert {
            self.completion = None;
        }
        // Leaving insert mode with something selected keeps the selection and
        // hands it to visual mode, so `shift`-arrow, `esc`, `y` does what it
        // looks like it should. Without this the selection is still on screen
        // but no longer means anything.
        if self.mode == Mode::Insert && mode == Mode::Normal && !self.view().sel.is_empty() {
            self.adopt_insert_selection();
            return;
        }
        if self.mode == Mode::Insert && mode == Mode::Normal {
            self.view_mut().step_back_from_insert();
        }
        // Entering visual starts a selection here, and leaving it drops
        // whatever was selected - either way the selection collapses onto the
        // cursor. Switching between `v` and `V` keeps it.
        if self.mode.is_visual() != mode.is_visual() {
            let head = self.view().sel.head;
            self.view_mut().sel = Selection::point(head);
        }
        self.mode = mode;
        self.clamp_cursor();
    }

    /// Insert mode is the only one that lets the cursor sit past the last
    /// character of a line.
    pub fn clamp_cursor(&mut self) {
        if self.mode != Mode::Insert {
            self.view_mut().clamp_cursor();
        }
    }

    /// Carry a selection made in insert mode over into visual mode. Insert
    /// mode's selection stops one past its last character and visual mode's
    /// sits on it, so whichever end is the far one steps back.
    fn adopt_insert_selection(&mut self) {
        let view = self.view_mut();
        if view.sel.head > view.sel.anchor {
            view.sel.head = view.grapheme_left(view.sel.head);
        } else {
            view.sel.anchor = view.grapheme_left(view.sel.anchor);
        }
        self.mode = Mode::Visual;
        self.clamp_cursor();
    }

    /// Put the cursor on the other end of the selection, so the end you are
    /// dragging is the other one.
    pub fn swap_selection_ends(&mut self) {
        let sel = &mut self.view_mut().sel;
        std::mem::swap(&mut sel.anchor, &mut sel.head);
        self.clamp_cursor();
    }

    /// What is selected, as characters, or `None` if nothing is.
    ///
    /// Visual mode's selection includes the character under the cursor, which
    /// an anchor-to-head range does not, and `V` widens it to whole lines.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let view = self.view();
        let (start, end) = view.sel.range();
        match self.mode {
            Mode::Visual => {
                let end = view.grapheme_right(end.max(start));
                Some((start, end.min(view.doc.len_chars())))
            }
            Mode::VisualLine => {
                let (first, _) = view.doc.coords(start);
                let (last, _) = view.doc.coords(end);
                let from = view.doc.line_to_char(first);
                // Up to the start of the line after, so the newline goes with
                // the line - which is what makes a linewise put a whole line.
                let to = match last + 1 < view.doc.len_lines() {
                    true => view.doc.line_to_char(last + 1),
                    false => view.doc.len_chars(),
                };
                Some((from, to))
            }
            _ => match view.sel.is_empty() {
                true => None,
                false => Some((start, end)),
            },
        }
    }

    pub fn undo(&mut self) {
        if !self.view_mut().undo() {
            self.message = "nothing to undo".into();
        }
    }

    pub fn redo(&mut self) {
        if !self.view_mut().redo() {
            self.message = "nothing to redo".into();
        }
    }

    pub fn save(&mut self) {
        self.write(None, false);
    }

    /// Write the buffer, optionally to a new path. `force` overrides the guard
    /// against overwriting a file that has changed behind our back.
    pub fn write(&mut self, path: Option<PathBuf>, force: bool) {
        if let Some(path) = path {
            self.view_mut().doc.set_path(path);
        }
        if !force && self.view().doc.changed_on_disk() {
            self.message = format!(
                "{} changed on disk - :w! to overwrite, :e! to reload",
                self.view().doc.display_name()
            );
            return;
        }

        let trimmed = match self.trim_on_save {
            true => self.view_mut().trim_trailing_whitespace(),
            false => 0,
        };
        // The cursor may have been sitting in the spaces that just went.
        self.clamp_cursor();
        let name = self.view().doc.display_name().to_string();
        match self.view_mut().save() {
            Ok(()) => {
                self.message = match trimmed {
                    0 => format!("wrote {name}"),
                    1 => format!("wrote {name}, trimmed 1 line"),
                    n => format!("wrote {name}, trimmed {n} lines"),
                }
            }
            Err(err) => self.message = format!("{err:#}"),
        }
    }

    /// `:e` with no argument: read the file again. Refuses to throw away
    /// unsaved changes unless told twice.
    pub fn reload(&mut self, force: bool) {
        if !force && self.is_modified() {
            self.message = "unsaved changes - :e! to reload anyway".into();
            return;
        }
        let name = self.view().doc.display_name().to_string();
        match self.view_mut().doc.reload() {
            Ok(()) => self.message = format!("reloaded {name}"),
            Err(err) => self.message = format!("{err:#}"),
        }
        self.clamp_cursor();
    }

    // --- commands that touch both a view and the registers -------------

    /// The current view and the registers, borrowed at once.
    fn view_and_registers(&mut self) -> (&mut View, &mut Registers) {
        let Editor { views, current, registers, .. } = self;
        (&mut views[*current], registers)
    }

    pub fn delete_selection(&mut self, register: Option<char>) {
        let (start, end) = self.view().sel.range();
        self.cut_range(register, start, end, false);
        self.clamp_cursor();
    }

    /// Delete what visual mode has selected, and leave visual mode. Whether it
    /// is remembered as lines or as characters is what `v` and `V` decide.
    pub fn delete_visual(&mut self, register: Option<char>) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let linewise = self.mode == Mode::VisualLine;
        self.cut_range(register, start, end, linewise);
        self.set_mode(Mode::Normal);
    }

    pub fn yank_visual(&mut self, register: Option<char>) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let linewise = self.mode == Mode::VisualLine;
        let (view, registers) = self.view_and_registers();
        let text = view.doc.slice_str(start, end);
        registers.record_yank(register, RegisterValue { text, linewise });
        // The cursor lands at the start of what was yanked, as vim does.
        view.sel = Selection::point(start);
        self.set_mode(Mode::Normal);
    }

    /// Replace the selection with a register's contents. What was there goes
    /// to the register the delete would have used, so it can be put back.
    pub fn put_over_visual(&mut self, register: Option<char>) {
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let value = self.registers.get(register);
        if value.is_empty() {
            self.message = "nothing to put".into();
            self.set_mode(Mode::Normal);
            return;
        }

        let linewise = self.mode == Mode::VisualLine;
        self.cut_range(None, start, end, linewise);
        self.set_mode(Mode::Normal);

        match (value.linewise, linewise) {
            // Lines replacing lines: the cut took the newline with it, so the
            // text goes back in at the start of the line it left behind.
            (true, true) => self.view_mut().put_lines_at(start, &value.text),
            (true, false) => self.view_mut().put_lines(&value.text, false),
            (false, _) => self.view_mut().put_inline_at(start, &value.text),
        }
        self.clamp_cursor();
    }

    fn cut_range(&mut self, register: Option<char>, start: usize, end: usize, linewise: bool) {
        if end <= start {
            return;
        }
        let (view, registers) = self.view_and_registers();
        let text = view.cut(start, end);
        registers.record_delete(register, RegisterValue { text, linewise });
    }

    pub fn yank_selection(&mut self, register: Option<char>) {
        let (view, registers) = self.view_and_registers();
        let (start, end) = view.sel.range();
        let text = view.doc.slice_str(start, end);
        registers.record_yank(register, RegisterValue::charwise(text));
        // Yanking leaves the cursor at the start of what was yanked.
        view.sel = Selection::point(start);
        self.clamp_cursor();
    }

    /// `count` characters from the cursor, stopping at the end of the line so
    /// `x` can never join two lines.
    pub fn delete_chars(&mut self, register: Option<char>, count: usize) {
        let (view, registers) = self.view_and_registers();
        let (line, _) = view.cursor_coords();
        let line_end = view.doc.line_to_char(line) + view.doc.line_len_chars(line);
        let start = view.sel.head;
        let mut end = start;
        for _ in 0..count {
            if end >= line_end {
                break;
            }
            end = view.grapheme_right(end);
        }
        if end > start {
            let text = view.cut(start, end);
            registers.record_delete(register, RegisterValue::charwise(text));
        }
        self.clamp_cursor();
    }

    /// From the cursor to the end of the line, as `D` and `C` do. The caller
    /// clamps: `D` leaves the cursor on a character, but `C` goes on to insert
    /// at the position the deleted text started from.
    pub fn delete_to_line_end(&mut self, register: Option<char>) {
        let (view, registers) = self.view_and_registers();
        let (line, _) = view.cursor_coords();
        let start = view.sel.head;
        let end = view.doc.line_to_char(line) + view.doc.line_len_chars(line);
        if end > start {
            let text = view.cut(start, end);
            registers.record_delete(register, RegisterValue::charwise(text));
        }
    }

    /// Whole lines, as `dd` does.
    pub fn delete_lines(&mut self, register: Option<char>, count: usize) {
        let (view, registers) = self.view_and_registers();
        let (line_start, end) = view.line_range(count);
        let text = view.doc.slice_str(line_start, end);
        registers.record_delete(register, RegisterValue::linewise(text));

        // If the range does not already end with a newline we are deleting the
        // last line of the buffer, so take the newline *before* it instead -
        // otherwise the line is emptied rather than removed.
        let ends_with_newline = end > line_start && view.doc.text.char(end - 1) == '\n';
        let start = match !ends_with_newline && line_start > 0 {
            true => line_start - 1,
            false => line_start,
        };

        view.cut(start, end);
        self.move_cursor(Move::FirstNonBlank, false);
        self.clamp_cursor();
    }

    /// Whole lines, as `yy` does.
    pub fn yank_lines(&mut self, register: Option<char>, count: usize) {
        let (view, registers) = self.view_and_registers();
        let (start, end) = view.line_range(count);
        let text = view.doc.slice_str(start, end);
        registers.record_yank(register, RegisterValue::linewise(text));
        if count > 1 {
            self.message = format!("{count} lines yanked");
        }
    }

    /// Clear the lines but keep one, with its indent, as `cc` does.
    pub fn change_lines(&mut self, register: Option<char>, count: usize) {
        let (view, registers) = self.view_and_registers();
        let (line, _) = view.cursor_coords();
        let last = view.doc.len_lines().saturating_sub(1);
        let end_line = (line + count - 1).min(last);
        let indent: String = view
            .doc
            .line_str(line)
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();

        let start = view.doc.line_to_char(line);
        let end = view.doc.line_to_char(end_line) + view.doc.line_len_chars(end_line);
        let text = view.doc.slice_str(start, end);
        registers.record_delete(register, RegisterValue::linewise(text));

        let cursor = start + indent.chars().count();
        view.edit_at(start, end - start, &indent, Some(cursor));
        self.set_mode(Mode::Insert);
    }

    /// Put a register's contents back. `after` is `p`, otherwise `P`.
    pub fn put(&mut self, register: Option<char>, count: usize, after: bool) {
        let value = self.registers.get(register);
        if value.is_empty() {
            self.message = "nothing to put".into();
            return;
        }
        let text = value.text.repeat(count);

        if value.linewise {
            self.view_mut().put_lines(&text, after);
        } else {
            self.view_mut().put_inline(&text, after);
        }
    }

    pub fn open_line_below(&mut self) {
        self.move_cursor(Move::LineEnd, false);
        self.set_mode(Mode::Insert);
        self.insert_newline();
    }

    pub fn open_line_above(&mut self) {
        let view = self.view_mut();
        let (line, _) = view.cursor_coords();
        let indent: String = view
            .doc
            .line_str(line)
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        let start = view.doc.line_to_char(line);
        let cursor = start + indent.chars().count();
        view.edit_at(start, 0, &format!("{indent}\n"), Some(cursor));
        self.set_mode(Mode::Insert);
    }
}


/// Split a `path:line:text` hit back into an item. The path and line become
/// what confirming it jumps to; the text is what is matched and shown.
fn grep_item(hit: String) -> Item {
    let mut parts = hit.splitn(3, ':');
    let path = parts.next().unwrap_or_default().to_string();
    let line: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(1);
    let text = parts.next().unwrap_or_default().trim_start().to_string();
    Item {
        text,
        detail: format!("{path}:{line}"),
        id: line,
        target: path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{char_col_at_display, display_col};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;
    use ropey::Rope;

    fn editor(text: &str) -> Editor {
        let mut e = Editor::scratch();
        e.view_mut().doc.text = Rope::from_str(text);
        e
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        assert_eq!(display_col("\tx", 1), 4);
        assert_eq!(display_col("ab\tx", 3), 4);
        assert_eq!(display_col("abcd\tx", 5), 8);
    }

    #[test]
    fn wide_chars_count_two_columns() {
        assert_eq!(display_col("日本", 2), 4);
        assert_eq!(char_col_at_display("日本", 2), 1);
    }

    #[test]
    fn horizontal_movement_steps_over_whole_graphemes() {
        // "e" + combining acute is one grapheme, two chars.
        let mut e = editor("e\u{301}x");
        e.move_cursor(Move::Right, false);
        assert_eq!(e.view().sel.head, 2);
        e.move_cursor(Move::Left, false);
        assert_eq!(e.view().sel.head, 0);
    }

    #[test]
    fn horizontal_movement_crosses_line_boundaries() {
        let mut e = editor("ab\ncd\n");
        e.view_mut().sel = Selection::point(2); // end of line 1
        e.move_cursor(Move::Right, false);
        assert_eq!(e.cursor_coords(), (1, 0));
        e.move_cursor(Move::Left, false);
        assert_eq!(e.cursor_coords(), (0, 2));
    }

    #[test]
    fn vertical_movement_keeps_the_goal_column() {
        let mut e = editor("long line\n\nlong line\n");
        e.view_mut().sel = Selection::point(7); // column 7 on line 0
        e.move_cursor(Move::Down, false); // empty line, snaps to column 0
        assert_eq!(e.cursor_coords(), (1, 0));
        e.move_cursor(Move::Down, false); // back out to the goal column
        assert_eq!(e.cursor_coords(), (2, 7));
    }

    #[test]
    fn movement_at_the_buffer_edges_is_clamped() {
        let mut e = editor("ab\n");
        e.move_cursor(Move::Left, false);
        assert_eq!(e.view().sel.head, 0);
        e.move_cursor(Move::FileEnd, false);
        let end = e.view().sel.head;
        e.move_cursor(Move::Right, false);
        assert_eq!(e.view().sel.head, end);
    }

    #[test]
    fn shift_movement_extends_from_the_anchor() {
        let mut e = editor("abcdef\n");
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, true);
        e.move_cursor(Move::Right, true);
        assert_eq!(e.view().sel.range(), (1, 3));
        e.move_cursor(Move::Right, false);
        assert!(e.view().sel.is_empty());
    }

    fn type_str(e: &mut Editor, text: &str) {
        for ch in text.chars() {
            e.insert(&ch.to_string());
        }
    }

    #[test]
    fn typing_then_undo_restores_text_and_cursor() {
        let mut e = editor("hello\n");
        e.move_cursor(Move::LineEnd, false);
        let before = e.view().sel.head;
        type_str(&mut e, " there");
        assert_eq!(e.view().doc.text.to_string(), "hello there\n");

        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "hello\n");
        assert_eq!(e.view().sel.head, before);

        e.redo();
        assert_eq!(e.view().doc.text.to_string(), "hello there\n");
    }

    #[test]
    fn a_run_of_typing_is_one_undo_step() {
        let mut e = editor("");
        type_str(&mut e, "abc");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "");
    }

    #[test]
    fn a_newline_ends_the_coalescing_run() {
        let mut e = editor("");
        type_str(&mut e, "ab");
        e.insert_newline();
        type_str(&mut e, "cd");

        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "ab\n");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "ab");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "");
    }

    #[test]
    fn a_run_of_backspaces_is_one_undo_step() {
        let mut e = editor("abcdef");
        e.move_cursor(Move::FileEnd, false);
        e.delete_backward();
        e.delete_backward();
        e.delete_backward();
        assert_eq!(e.view().doc.text.to_string(), "abc");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "abcdef");
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut e = editor("abcdef");
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, true);
        e.move_cursor(Move::Right, true);
        e.insert("X");
        assert_eq!(e.view().doc.text.to_string(), "aXdef");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "abcdef");
        assert_eq!(e.view().sel.range(), (1, 3));
    }

    #[test]
    fn backspace_deletes_a_whole_grapheme() {
        let mut e = editor("xe\u{301}");
        e.move_cursor(Move::FileEnd, false);
        e.delete_backward();
        assert_eq!(e.view().doc.text.to_string(), "x");
    }

    #[test]
    fn enter_carries_the_leading_indent() {
        let mut e = editor("    foo\n");
        e.move_cursor(Move::LineEnd, false);
        e.insert_newline();
        assert_eq!(e.view().doc.text.to_string(), "    foo\n    \n");

        // Splitting inside the leading whitespace: the indent measured is only
        // what is before the cursor, and it is prepended to the text that moves
        // down, so the tail keeps its original indentation.
        let mut e = editor("    foo\n");
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, false);
        e.insert_newline();
        assert_eq!(e.view().doc.text.to_string(), "  \n    foo\n");
    }

    #[test]
    fn editing_after_undo_drops_the_redo_stack() {
        let mut e = editor("");
        type_str(&mut e, "abc");
        e.undo();
        type_str(&mut e, "xyz");
        e.redo();
        assert_eq!(e.view().doc.text.to_string(), "xyz");
    }

    #[test]
    fn deleting_backward_at_the_start_of_the_buffer_is_a_no_op() {
        let mut e = editor("abc");
        e.delete_backward();
        assert_eq!(e.view().doc.text.to_string(), "abc");
        assert!(!e.is_modified());
    }

    #[test]
    fn each_buffer_keeps_its_own_undo_history() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.set_mode(Mode::Insert);
        e.insert("x");
        assert_eq!(e.view().doc.text.to_string(), "xone\n");

        // Undo in the other buffer must not reach across.
        e.next_view();
        e.undo();
        assert_eq!(e.message, "nothing to undo");
        assert_eq!(e.view().doc.text.to_string(), "two\n");

        e.previous_view();
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "one\n");
    }

    #[test]
    fn registers_are_shared_across_buffers() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.yank_lines(None, 1);
        e.next_view();
        e.put(None, 1, true);
        assert_eq!(e.view().doc.text.to_string(), "two\none\n");
        assert_eq!(e.current_index(), 1);
    }

    #[test]
    fn opening_a_file_twice_switches_to_it_instead() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one.clone(), two]).unwrap();
        e.next_view();
        assert_eq!(e.current_index(), 1);

        e.open_file(&one).unwrap();
        assert_eq!(e.views().len(), 2);
        assert_eq!(e.current_index(), 0);
        assert_eq!(e.views()[0].doc.path.as_ref(), Some(&one));
    }

    #[test]
    fn opening_a_file_from_the_picker_is_a_jump_back_out_of() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "alpha\nbeta\ngamma\n");
        let two = write_file(&dir, "two.txt", "delta\n");

        let mut e = Editor::open(&[one]).unwrap();
        e.goto_line(1);
        assert_eq!(e.cursor_coords(), (1, 0));

        e.open_file_picker();
        e.stream_items(e.token(), vec![two.to_string_lossy().into_owned()], true);
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(e.current_index(), 1);

        // The buffer has to come back as well as the line.
        e.jump_back();
        assert_eq!(e.current_index(), 0);
        assert_eq!(e.cursor_coords(), (1, 0));
        e.jump_forward();
        assert_eq!(e.current_index(), 1);
    }

    #[test]
    fn quitting_is_guarded_by_any_modified_buffer() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.set_mode(Mode::Insert);
        e.insert("x");
        // The clean buffer is current, but the dirty one still blocks quitting.
        e.next_view();
        assert!(!e.is_modified());
        assert!(e.any_modified());
    }

    #[test]
    fn switching_buffers_clamps_the_cursor_for_normal_mode() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "long line\n");
        let two = write_file(&dir, "two.txt", "hi\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.move_cursor(Move::LineEnd, false);
        e.next_view();
        e.previous_view();
        // Still on a character, not past the end of the line.
        assert_eq!(e.cursor_coords(), (0, 8));
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "soda_edit_buffers_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn the_buffer_picker_marks_the_current_and_modified_buffers() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.set_mode(Mode::Insert);
        e.insert("x");
        e.set_mode(Mode::Normal);
        e.open_buffer_picker();

        let picker = e.picker.as_ref().unwrap();
        let rows: Vec<(&str, &str)> = picker
            .matches()
            .iter()
            .map(|m| {
                let item = picker.item(m);
                (item.text.as_str(), item.detail.as_str())
            })
            .collect();
        assert_eq!(rows, [("one.txt", "% [+]"), ("two.txt", "")]);
    }

    #[test]
    fn choosing_from_the_buffer_picker_switches_to_that_buffer() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.open_buffer_picker();
        for c in "two".chars() {
            e.picker_input(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(e.picker.is_none());
        assert_eq!(e.current_index(), 1);
        assert_eq!(e.view().doc.text.to_string(), "two\n");
    }

    #[test]
    fn cancelling_the_picker_leaves_the_buffer_alone() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two]).unwrap();
        e.open_buffer_picker();
        e.picker_input(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        e.picker_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(e.picker.is_none());
        assert_eq!(e.current_index(), 0);
    }

    #[test]
    fn the_picker_takes_the_cursor_while_it_is_open() {
        let mut e = Editor::scratch();
        e.set_viewport(80, 20);
        let text_cursor = e.cursor_screen();
        e.open_buffer_picker();
        assert_ne!(e.cursor_screen(), text_cursor);
        // On the prompt row, past "buffer> ".
        assert_eq!(e.cursor_screen(), (8, 10));
    }

    #[test]
    fn choosing_from_the_file_picker_opens_the_file() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one]).unwrap();
        e.open_file_picker();
        // No job sender in tests, so feed the walk's batch by hand.
        e.stream_items(e.token(), vec![two.to_string_lossy().into_owned()], true);
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(e.picker.is_none());
        assert_eq!(e.views().len(), 2);
        assert_eq!(e.view().doc.text.to_string(), "two\n");
    }

    #[test]
    fn choosing_a_file_that_is_already_open_switches_to_it() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::open(&[one, two.clone()]).unwrap();
        e.open_file_picker();
        e.stream_items(e.token(), vec![two.to_string_lossy().into_owned()], true);
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(e.views().len(), 2);
        assert_eq!(e.current_index(), 1);
    }

    #[test]
    fn a_batch_from_a_closed_picker_is_dropped() {
        let mut e = Editor::scratch();
        e.open_file_picker();
        let stale = e.token();

        e.picker_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        e.open_file_picker();
        e.stream_items(stale, vec!["stale.rs".into()], true);
        e.stream_items(e.token(), vec!["fresh.rs".into()], true);

        let picker = e.picker.as_ref().unwrap();
        assert_eq!(picker.item_count(), 1);
        assert_eq!(picker.item(&picker.matches()[0]).text, "fresh.rs");
    }

    #[test]
    fn choosing_something_that_cannot_be_read_reports_it() {
        let dir = tempdir();
        let mut e = Editor::scratch();
        e.open_file_picker();
        // A directory is not something the walk offers, but nothing stops a
        // source from handing over a path that will not open.
        e.stream_items(e.token(), vec![dir.to_string_lossy().into_owned()], true);
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(e.views().len(), 1);
        assert!(e.message.contains("reading"), "{}", e.message);
    }

    #[test]
    fn a_grep_hit_splits_into_a_line_and_a_place() {
        let item = grep_item("src/main.rs:42:    let x = 1;".into());
        assert_eq!(item.text, "let x = 1;");
        assert_eq!(item.detail, "src/main.rs:42");
        assert_eq!(item.target, "src/main.rs");
        assert_eq!(item.id, 42);
    }

    #[test]
    fn a_grep_hit_keeps_the_colons_in_the_matched_line() {
        let item = grep_item("src/main.rs:7:use std::path::{Path, PathBuf};".into());
        assert_eq!(item.text, "use std::path::{Path, PathBuf};");
        assert_eq!(item.id, 7);
    }

    #[test]
    fn choosing_a_grep_hit_opens_the_file_at_that_line() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "alpha\nbeta\ngamma\n");

        let mut e = Editor::open(&[one]).unwrap();
        e.open_grep_picker();
        let hit = format!("{}:3:gamma", two.to_string_lossy());
        e.stream_items(e.token(), vec![hit], true);
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(e.views().len(), 2);
        assert_eq!(e.view().doc.display_name(), "two.txt");
        assert_eq!(e.cursor_coords(), (2, 0));
    }

    #[test]
    fn a_failing_search_reports_itself_once_and_only_while_it_is_current() {
        let mut e = Editor::scratch();
        e.open_grep_picker();
        let stale = e.token();

        e.job_failed(stale, "bad pattern: unclosed group".into());
        assert_eq!(e.message, "bad pattern: unclosed group");
        // And the list stops claiming that more is on the way.
        assert!(e.picker.as_ref().unwrap().is_complete());

        // Typing again retires that search; its complaint is no longer news.
        e.message.clear();
        e.picker_input(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        e.job_failed(stale, "bad pattern: unclosed group".into());
        assert_eq!(e.message, "");
    }

    #[test]
    fn writing_refuses_to_overwrite_a_file_that_changed_underneath() {
        let dir = tempdir();
        let path = write_file(&dir, "one.txt", "original\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_mode(Mode::Insert);
        e.insert("mine ");
        e.set_mode(Mode::Normal);

        // Something else writes the file while we are editing it.
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(&path, "theirs\n").unwrap();

        e.write(None, false);
        assert!(e.message.contains("changed on disk"), "{}", e.message);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");

        // Told twice, it writes.
        e.write(None, true);
        assert!(e.message.starts_with("wrote"), "{}", e.message);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine original\n");
    }

    #[test]
    fn reloading_refuses_to_throw_away_unsaved_changes() {
        let dir = tempdir();
        let path = write_file(&dir, "one.txt", "original\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_mode(Mode::Insert);
        e.insert("mine ");
        e.set_mode(Mode::Normal);
        std::fs::write(&path, "theirs\n").unwrap();

        e.reload(false);
        assert!(e.message.contains(":e! to reload"), "{}", e.message);
        assert_eq!(e.view().doc.text.to_string(), "mine original\n");

        e.reload(true);
        assert_eq!(e.view().doc.text.to_string(), "theirs\n");
        // And the file is no longer considered changed underneath us.
        e.write(None, false);
        assert!(e.message.starts_with("wrote"), "{}", e.message);
    }

    #[test]
    fn saving_trims_trailing_whitespace_and_says_how_much() {
        let dir = tempdir();
        let path = write_file(&dir, "one.txt", "one   \ntwo\nthree\t\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();

        e.save();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\nthree\n");
        assert!(e.message.contains("trimmed 2 lines"), "{}", e.message);

        // One undo puts every trimmed line back.
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "one   \ntwo\nthree\t\n");
    }

    #[test]
    fn trimming_can_be_turned_off() {
        let dir = tempdir();
        let path = write_file(&dir, "one.txt", "one   \n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.run_command("set notrim");
        e.save();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one   \n");
        assert_eq!(e.message, "wrote one.txt");
    }

    #[test]
    fn trimming_keeps_the_cursor_on_its_line() {
        let dir = tempdir();
        let path = write_file(&dir, "one.txt", "one   \ntwo   \nthree\n");
        let mut e = Editor::open(&[path]).unwrap();
        e.goto_line(1);
        e.move_cursor(Move::LineEnd, false);
        e.clamp_cursor();
        assert_eq!(e.cursor_coords(), (1, 5));

        e.save();
        // It was sitting in the spaces that went, so it lands on the last
        // character that is left - still on line 1.
        assert_eq!(e.cursor_coords(), (1, 2));
    }

    #[test]
    fn write_with_a_name_writes_somewhere_else() {
        let dir = tempdir();
        let path = write_file(&dir, "one.txt", "hello\n");
        let other = dir.join("copy.txt");

        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.run_command(&format!("w {}", other.to_string_lossy()));
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "hello\n");
        // The buffer now belongs to the new path.
        assert_eq!(e.view().doc.display_name(), "copy.txt");
    }

    #[test]
    fn quit_commands_reach_the_run_loop() {
        let mut e = Editor::scratch();
        e.run_command("q");
        assert_eq!(e.quit, Some(false));
        e.quit = None;
        e.run_command("q!");
        assert_eq!(e.quit, Some(true));
    }

    #[test]
    fn write_and_quit_does_not_quit_if_the_write_failed() {
        let mut e = Editor::scratch();
        // A scratch buffer has no file name to write to.
        e.run_command("wq");
        assert_eq!(e.quit, None);
        assert!(e.message.contains("no file name"), "{}", e.message);
    }
}
