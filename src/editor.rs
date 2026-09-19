use anyhow::Result;
use regex::RegexBuilder;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use crate::buffer::Document;
use crate::clipboard;
use crate::command;
use crate::comment::{self, Toggled};
use crate::complete::{self, Completion, Pick};
use crate::info::{self, Info};
use crate::jump::{Jump, Jumps};
use crate::keys::BINDINGS;
use crate::object::{self, Object};
use crate::picker::{Choice, Item, Open, Outcome, Picker, Source};
use crate::search::{self, Search};
use crate::substitute;
use crate::stream::{self, Message, Sign};
use crate::register::{Kind, RegisterValue, Registers, SYSTEM};
use crate::theme::Theme;
use crate::view::{self, Find, Indent, Move, Reveal, Screen, Selection, TAB_WIDTH, View};
use crate::window::{self, Direction, Layout, Rect, Window};

mod block;
mod git;
mod lsp;
mod replace;
mod surround;

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
    /// `gR`: the new name for the thing under the cursor. It starts with the
    /// old name written in, since a rename is usually a word being adjusted
    /// rather than replaced.
    Rename,
    /// `^r` in the grep picker: what the pattern it searched for becomes, in
    /// every file. The pattern waits in `Editor::replacing`.
    Replace,
}

/// How often the disk is looked at for open files that changed behind our
/// back. A stat per buffer is nothing; once a keystroke would still be
/// wasted, and once a second is quicker than anyone switches panes.
const DISK_LOOK: std::time::Duration = std::time::Duration::from_secs(1);

pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
    /// Where the cursor and viewport were when it opened, so cancelling can
    /// put them back after the incremental preview has moved them.
    origin: (usize, usize),
    /// What `tab` is cycling through, while it is being pressed. Dropped by
    /// the next key that is not another `tab`, so the list on screen is always
    /// the list for what is written.
    pub completion: Option<Completing>,
}

/// A `:` completion being cycled: where in the input the replacement goes,
/// what could go there, and which one is showing.
pub struct Completing {
    start: usize,
    pub matches: Vec<String>,
    pub selected: usize,
}

impl Prompt {
    /// What the prompt starts with, which is also how you can tell which way
    /// a search is going. A rename says so in a word: it is not a command and
    /// not a search, and a lone `:` would claim it was one of them.
    pub fn prefix(&self) -> &'static str {
        match self.kind {
            PromptKind::Search { backward: true } => "?",
            PromptKind::Search { backward: false } => "/",
            PromptKind::Command => ":",
            PromptKind::Rename => "rename> ",
            PromptKind::Replace => "replace with> ",
        }
    }

    /// The whole line as it is drawn, which is also what the cursor's column
    /// is counted along: one string, so the two cannot disagree.
    pub fn line(&self) -> String {
        format!("{}{}", self.prefix(), self.input)
    }

    fn backward(&self) -> bool {
        matches!(self.kind, PromptKind::Search { backward: true })
    }

    fn is_search(&self) -> bool {
        matches!(self.kind, PromptKind::Search { .. })
    }
}

/// What `;` does. Vim repeats the last `f`/`t` with it, but `;` is where the
/// finger already is and `:` is what it is usually reaching for, so this is a
/// remap common enough to be worth an option rather than an init file full of
/// them.
/// The dog in the status line. It runs while you are typing - a step on every
/// key, so it goes as fast as you do - and sits in the middle when you stop.
///
/// No timer and no thread: the only clock this needs is the keyboard, and the
/// one moment nothing is arriving is the moment the dog should be sitting
/// down, which the run loop notices by waiting with a timeout.
#[derive(Default)]
pub struct Dog {
    /// How far along its lane the dog is. Counts keys, and survives a rest so
    /// that the dog sits where it stopped; zero means it has never run.
    pub steps: usize,
    pub running: bool,
}

/// A case conversion that stays one character long. `ß` upper-cases to `SS`,
/// which is two, and `~` would rather leave it alone than move every column
/// after it.
fn one(mut cased: impl Iterator<Item = char>) -> Option<char> {
    match (cased.next(), cased.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Semicolon {
    /// Vim's: repeat the last find, with `,` reversing it.
    #[default]
    Find,
    /// Open the command line, as `:` does. `,` then repeats the find forwards,
    /// taking over the job `;` had - which is the other half of the remap
    /// people write by hand.
    Command,
}

impl Semicolon {
    pub fn name(&self) -> &'static str {
        match self {
            Semicolon::Find => "find",
            Semicolon::Command => "command",
        }
    }

    fn parse(value: &str) -> Option<Semicolon> {
        Some(match value {
            "find" | "repeat" => Semicolon::Find,
            "command" | "colon" | ":" => Semicolon::Command,
            _ => return None,
        })
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
    /// Visual over a rectangle rather than a range: the same columns of
    /// several lines, which is a different model and not a third variant of
    /// the other two. See `editor::block`.
    VisualBlock,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Normal => "ABNORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::VisualBlock => "V-BLOCK",
        }
    }

    pub fn is_visual(self) -> bool {
        matches!(self, Mode::Visual | Mode::VisualLine | Mode::VisualBlock)
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
    /// The buffer the focused window shows: always `windows[focus].view`.
    current: usize,
    /// Every window, focused or not. The focused one's cursor and scroll live
    /// in its `View`; the rest keep theirs here until they are focused.
    windows: Vec<Window>,
    layout: Layout,
    focus: usize,
    /// The whole screen below the buffer list: every window, and the status
    /// line each one has. `width` and `height` are the focused window's share.
    screen: (usize, usize),
    /// The buffer an undo group was opened on, so it is closed on the same one
    /// even when the command in between went to another buffer.
    grouped_view: Option<usize>,
    /// Running language servers. Indexes stay put: a stopped one is kept,
    /// marked, so the buffers pointing at it can tell.
    servers: Vec<crate::lsp::Client>,
    /// Start language servers for the files that have one.
    pub lsp_enabled: bool,
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
    /// What `;` is bound to.
    pub semicolon: Semicolon,
    /// Whether the open buffers are listed along the top.
    pub tabline: Tabline,
    /// Indent a new line the way the grammar says, rather than copying the
    /// line above. Only does anything where there is an indent query.
    pub autoindent: bool,
    /// Type `(` and get `()`, with the cursor between; type the `)` and step
    /// over the one already there.
    pub autopairs: bool,
    /// Emacs chords in insert mode. Off by default: this is a vim-flavoured
    /// editor and half of these keys already mean something else.
    pub emacs: bool,
    /// How many characters of a word bring the completion popup up on their
    /// own. Zero waits for `^n`.
    pub autocomplete: usize,
    /// One step of indentation: how wide, and tabs or spaces.
    pub indent: Indent,
    /// True while a config file is being read, so its `set` lines count as
    /// defaults rather than as overriding what each file said about itself.
    from_config: bool,
    /// Draw the status line with Nerd Font glyphs. Off is plain ASCII, for a
    /// terminal whose font has not been patched.
    pub glyphs: bool,
    /// Tint the row the cursor is on, the whole width of the screen.
    /// What visual mode was last left with, for `gv` to bring back.
    last_visual: Option<(Selection, Mode)>,
    pub cursorline: bool,
    /// The dog in the status line: whether to draw one, and what it is doing.
    pub show_dog: bool,
    pub dog: Dog,
    /// Set by `:q`, read by the run loop. `Some(true)` is `:q!`.
    pub quit: Option<bool>,
    /// What `<space>{key}` runs, as set by `:map`. The leader is the space
    /// left for you: the rest of normal mode is vim's, and a mapping that
    /// quietly took `d` or `w` away would be a different editor.
    pub leader: BTreeMap<char, String>,
    /// When the disk was last looked at for files changed behind our back.
    disk_checked: Option<std::time::Instant>,
    /// Where the cursor was left in files closed before, this run or earlier
    /// ones. Kept nowhere until the run loop says where to keep it, so a test
    /// editor never touches the real list.
    positions: crate::positions::Positions,
    /// Where undo files go, or `None` for nowhere - which, like `positions`,
    /// is what a test editor gets.
    undo_dir: Option<PathBuf>,
    /// `:set undofile`: whether history is written down at all.
    pub undofile: bool,
    /// `:set inlayhints`: types and parameter names from the language server,
    /// written into the lines.
    pub inlayhints: bool,
    /// `:set wrap`: long lines continue on the rows below rather than off
    /// the right edge.
    pub wrap: bool,
    /// The grep pattern a project-wide replace is waiting on a replacement for.
    replacing: String,
    /// A block `I`, `A` or `c` waiting for insert mode to end, so what was
    /// typed on one line can be put on the rest of them.
    pending_block: Option<block::PendingBlock>,
    /// `$` in block mode: the block runs to the end of every line rather than
    /// to a column. Dropped by any move that is not up or down, the way vim
    /// drops it, since a sideways move is a new right-hand side.
    block_to_eol: bool,
    /// A command line the run loop should hand the terminal to. The editor
    /// does not own the terminal - the renderer does - so `:!` leaves the
    /// command here rather than running it.
    pub shell: Option<String>,
    /// `^z`: the run loop should hand the terminal back and stop the process.
    /// Same reason as `shell` - the terminal is not the editor's to give away.
    pub suspend: bool,
    /// An escape sequence the run loop should write out with the next frame.
    /// A copy with no clipboard helper to run leaves an OSC 52 here, because
    /// the editor does not own stdout and the renderer does.
    pub escape: Option<String>,
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
    /// The box beside the cursor: what `K` asked the server, or the signature
    /// of the call being typed.
    pub info: Option<Info>,
    /// What the server last offered to do about the place the cursor was in,
    /// and which server offered it: the picker of titles holds an index into
    /// this, and running one may mean going back to that server for the rest
    /// of it.
    pub actions: Option<(usize, Vec<crate::lsp::CodeAction>)>,
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
            windows: vec![Window {
                view: 0,
                sel: Selection::point(0),
                top_char: 0,
                scroll_left: 0,
                mark: 0,
            }],
            layout: Layout::default(),
            focus: 0,
            screen: (80, 25),
            grouped_view: None,
            servers: Vec::new(),
            lsp_enabled: true,
            width: 80,
            height: 24,
            mode: Mode::default(),
            registers: Registers::default(),
            escape: None,
            numbers: Numbers::default(),
            trim_on_save: true,
            glyphs: true,
            last_visual: None,
            cursorline: true,
            show_dog: true,
            dog: Dog::default(),
            semicolon: Semicolon::default(),
            tabline: Tabline::Auto,
            autoindent: true,
            autopairs: true,
            emacs: false,
            autocomplete: DEFAULT_AUTOCOMPLETE,
            dismissed: None,
            indent: Indent { width: TAB_WIDTH, tabs: true },
            from_config: false,
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
            info: None,
            actions: None,
            shell: None,
            suspend: false,
            leader: BTreeMap::new(),
            disk_checked: None,
            positions: Default::default(),
            undo_dir: None,
            undofile: true,
            inlayhints: true,
            wrap: false,
            replacing: String::new(),
            pending_block: None,
            block_to_eol: false,
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

    /// `:` from visual mode, which in vim writes the selection's range in for
    /// you. Leaving visual mode is what records the selection, so this reads
    /// the same `'<,'>` that a substitute would resolve.
    pub fn open_command_over_selection(&mut self) {
        self.set_mode(Mode::Normal);
        self.open_prompt(PromptKind::Command);
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.input.push_str("'<,'>");
        }
    }

    fn open_prompt(&mut self, kind: PromptKind) {
        let view = self.view();
        self.prompt = Some(Prompt {
            kind,
            input: String::new(),
            origin: (view.sel.head, view.scroll_top),
            completion: None,
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

        // `tab` cycles the completion; every other key ends the cycling,
        // because what is offered has to be an answer to what is written.
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            return self.complete_command(key.code == KeyCode::Tab);
        }
        prompt.completion = None;

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
        self.prompt_changed();
    }

    /// What happens after the `:` or `/` line changes. Only a search shows its
    /// answer as you type; a command waits.
    fn prompt_changed(&mut self) {
        if self.prompt.as_ref().is_some_and(Prompt::is_search) {
            self.preview_search();
        }
    }

    /// `tab` on the `:` line: offer what could finish the word being typed,
    /// and cycle through the offers on every press after the first. Searching
    /// has nothing to complete against, so `tab` there is a tab.
    fn complete_command(&mut self, forward: bool) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        if prompt.kind != PromptKind::Command {
            return;
        }

        match prompt.completion.as_mut() {
            Some(completing) => {
                let count = completing.matches.len();
                completing.selected = match forward {
                    true => (completing.selected + 1) % count,
                    false => (completing.selected + count - 1) % count,
                };
            }
            None => {
                let (start, matches) = command::complete(&prompt.input);
                if matches.is_empty() {
                    return;
                }
                let selected = match forward {
                    true => 0,
                    false => matches.len() - 1,
                };
                prompt.completion = Some(Completing { start, matches, selected });
            }
        }

        let completing = prompt.completion.as_ref().expect("just set");
        prompt.input.truncate(completing.start);
        prompt.input.push_str(&completing.matches[completing.selected]);
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
        // An empty replacement is an answer - delete what matched - so unlike a
        // rename it is not taken for changing your mind.
        if prompt.kind == PromptKind::Replace {
            let pattern = std::mem::take(&mut self.replacing);
            return self.replace_in_project(&pattern, &prompt.input);
        }
        if prompt.kind == PromptKind::Rename {
            let name = prompt.input.trim().to_string();
            if name.is_empty() {
                self.message.clear();
                return;
            }
            if !self.lsp_rename(&name) {
                self.message = "no language server to rename with".into();
            }
            return;
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
        // A till that is already against its target covers nothing: `dtb` with
        // the cursor on the `a` of `ab` is a motion that cannot move, and vim
        // leaves the line alone rather than taking the character under the
        // cursor with it. Failing here is what makes that a no-op - and stops
        // `ctb` opening insert mode for a change that was never going to
        // happen.
        if find.till && dest == at {
            self.message = format!("already before {}", find.target);
            return false;
        }
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

    /// Whether `;` opens the command line rather than repeating a find.
    pub fn semicolon_is_command(&self) -> bool {
        self.semicolon == Semicolon::Command
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
    /// `gd` asks the language server when there is one, which knows about
    /// other files; `gD`, and `gd` without a server, read the tree.
    pub fn goto_definition(&mut self, local: bool) {
        if local && self.lsp_definition() {
            return;
        }
        self.goto_definition_in_tree(local);
    }

    fn goto_definition_in_tree(&mut self, local: bool) {
        let Some(word) = self.view().word_under_cursor() else {
            self.message = "no word under the cursor".into();
            return;
        };
        let at = self.view().sel.head;
        let Some(found) = self.view().definition(&word, at, local, &self.theme) else {
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
    /// Run `~/.config/jack/init`: one command per line, written as it
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
        // A config file says what to do when the file has nothing to say, so
        // its `set expandtab` is a default rather than an instruction to
        // ignore what was read out of the files already open. A `:set` typed
        // by hand is the opposite, and still wins.
        self.from_config = true;
        for (number, line) in text.lines().enumerate() {
            let line = line.trim().trim_start_matches(':');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            self.run_command(line);
            if !self.message.is_empty() {
                self.message = format!("init line {}: {}", number + 1, self.message);
                break;
            }
        }
        self.from_config = false;
    }

    pub fn run_command(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        // `:s` before anything else: it has no whitespace to split on, and a
        // pattern is allowed to contain any of the characters a command name
        // is looked up by.
        if let Some(parsed) = substitute::parse(line) {
            match parsed {
                Ok(command) => self.substitute(command),
                Err(complaint) => self.message = complaint,
            }
            return;
        }

        // `:!cmd` - a shell command, with the terminal handed over to it. The
        // whole rest of the line, quotes, pipes and all: it goes to a shell,
        // which is better at reading it than anything here would be.
        if let Some(command) = line.strip_prefix('!') {
            self.run_shell(command.trim());
            return;
        }

        // A range in front of a command that takes one. Only `:fmt` does, so
        // far, and a bare `:fmt` is the whole buffer rather than the line the
        // cursor is on - `range` cannot tell "no range" from "this line", so
        // whether it consumed anything is what says which.
        let (lines, rest) = substitute::range(line);
        let ranged = rest.len() != line.len();
        if matches!(rest.trim(), "fmt" | "format") {
            let lines = ranged.then(|| self.substitute_lines(lines)).flatten();
            if ranged && lines.is_none() {
                return;
            }
            self.format(lines);
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
            // With more than one window these close the window, as in vim;
            // the buffer stays open, so there is nothing to lose by it.
            ("q" | "quit", _) => {
                if !(self.windows.len() > 1 && self.close_window()) {
                    self.quit = Some(force);
                }
            }
            ("wa" | "wall", _) => {
                self.write_all(force);
            }
            ("wqa" | "wqall" | "xa" | "xall", _) => {
                if self.write_all(force) {
                    self.quit = Some(force);
                }
            }
            ("wq" | "x", _) => {
                self.write(None, force);
                if self.message.starts_with("wrote")
                    && !(self.windows.len() > 1 && self.close_window())
                {
                    self.quit = Some(force);
                }
            }
            ("sp" | "split", path) => self.split_window(false, Some(path).filter(|p| !p.is_empty())),
            ("vs" | "vsplit", path) => self.split_window(true, Some(path).filter(|p| !p.is_empty())),
            ("clo" | "close", _) => {
                self.close_window();
            }
            ("on" | "only", _) => self.only_window(),
            ("bd" | "bdelete", _) => self.close_buffer(force),
            ("lsp", _) => self.lsp_report(),
            ("stage", _) => self.stage_hunk(),
            ("revert", _) => self.revert_hunk(),
            ("hunk", _) => self.preview_hunk(),
            ("blame", _) => self.blame_line(),
            ("sh" | "shell", _) => self.run_shell(""),
            ("sus" | "stop" | "suspend", _) => self.suspend(),
            ("map", argument) => self.map_leader(argument),
            ("unmap", argument) => self.unmap_leader(argument),
            ("e" | "edit", "") => self.reload(force),
            ("e" | "edit", path) => {
                if let Err(err) = self.open_file(path) {
                    self.message = format!("{err:#}");
                }
            }
            ("set", option) => self.set_option(option),
            ("config", _) => self.open_config(),
            ("noh" | "nohlsearch", _) => self.clear_search_highlight(),
            (other, _) => self.message = format!("not a command: {other}"),
        }
    }

    /// `:map g !lazygit` - what `<space>g` runs. Bare `:map` lists what is
    /// mapped; `:unmap g` takes one back.
    ///
    /// One character after the leader, and a command line as you would type it
    /// after `:`. Not a key sequence and not a recording of keystrokes: a
    /// command is a thing with a name that can be listed, said back, and put
    /// in a config file, which is the whole point of having this be config.
    fn map_leader(&mut self, argument: &str) {
        if argument.is_empty() {
            self.message = match self.leader.is_empty() {
                true => "nothing mapped (:map g !lazygit)".into(),
                false => self
                    .leader
                    .iter()
                    .map(|(key, command)| format!("<space>{key} {command}"))
                    .collect::<Vec<String>>()
                    .join(", "),
            };
            return;
        }
        let (key, command) = match argument.split_once(char::is_whitespace) {
            Some((key, command)) => (key, command.trim()),
            None => (argument, ""),
        };
        let mut chars = key.chars();
        let (Some(key), None) = (chars.next(), chars.next()) else {
            self.message = format!("one key after the leader, not {key:?}");
            return;
        };
        if let Some(what) = built_in_leader(key) {
            self.message = format!("<space>{key} is {what}");
            return;
        }
        if command.is_empty() {
            self.message = "nothing to map it to".into();
            return;
        }
        // Written as it would be typed, `:` and all, or without - both spell
        // the same thing and both are what someone reaches for.
        //
        // Nothing is said when it works, because a config file is read by
        // running its lines and anything said there reads as a complaint about
        // the line - the same silence `:set` keeps.
        let command = command.trim_start_matches(':').trim().to_string();
        self.leader.insert(key, command);
    }

    fn unmap_leader(&mut self, argument: &str) {
        let key = argument.chars().next();
        if key.and_then(|key| self.leader.remove(&key)).is_none() {
            self.message = format!("{argument} is not mapped");
        }
    }

    /// What `<space>{key}` runs, when it is one of yours.
    pub fn leader_command(&self, key: char) -> Option<String> {
        self.leader.get(&key).cloned()
    }

    /// `:!cmd`, and `:sh` for a shell of your own. The terminal goes to it
    /// whole - full-screen programs work, which is the point: `:!lazygit` is
    /// the reason this exists.
    ///
    /// An empty command means the shell itself, and `exit` comes back here.
    /// `^z` and `:suspend`: stop, and come back on `fg` where you left off.
    pub fn suspend(&mut self) {
        // As with `:!`, a config file has no terminal to hand back yet, and a
        // line in one that stops the editor before it starts is nobody's idea.
        if self.from_config {
            self.message = "not from a config file".into();
            return;
        }
        self.suspend = true;
    }

    fn run_shell(&mut self, command: &str) {
        // A config file is for settings. A line in one that runs a program at
        // startup is a surprise nobody wants, and there is no terminal to hand
        // over at that point anyway.
        if self.from_config {
            self.message = "not from a config file".into();
            return;
        }
        self.shell = Some(command.to_string());
    }

    /// After another program has had the terminal: whatever it did to the
    /// files that are open here. The buffers with nothing to lose are reloaded
    /// (that is what makes `:!git checkout`, or a rebase in lazygit, show up)
    /// and the ones with unsaved changes are named rather than overwritten.
    pub fn reload_changed_files(&mut self) {
        let (mut reloaded, mut conflicts) = (Vec::new(), Vec::new());
        for index in 0..self.views.len() {
            if !self.views[index].doc.changed_on_disk() {
                continue;
            }
            let name = self.views[index].doc.display_name().to_string();
            if self.views[index].is_modified() {
                // Said once per change to the file: this runs every second,
                // and a warning repeated that often is one you stop reading.
                if self.views[index].doc.first_news_of_disk() {
                    conflicts.push(name);
                }
                continue;
            }
            if self.views[index].reload().is_ok() {
                reloaded.push(name);
            }
        }
        self.clamp_cursor();

        let say = |what: &str, names: Vec<String>| match names.len() {
            0 => String::new(),
            1 => format!("{} {what}", names[0]),
            _ => format!("{} files {what}", names.len()),
        };
        let parts: Vec<String> = [
            say("reloaded", reloaded),
            say("changed on disk - :e! to reload", conflicts),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
        // Nothing found is nothing to say, and certainly no reason to wipe
        // whatever the status line was saying before the look.
        if !parts.is_empty() {
            self.message = parts.join("; ");
        }
    }

    /// Look at the disk for files changed behind our back, at most once a
    /// second: a stat per open buffer, on the frames that happen to fall after
    /// the second is up. What changes a file is usually another program in
    /// another pane - a formatter, a `git checkout` - and the look that
    /// matters most is the one when you come back, which is `focus_gained`.
    ///
    /// Not while typing. A reload is an edit, an insert is one undo step, and
    /// a reload that landed inside one would be taken back by the `u` meant
    /// for what you typed.
    pub fn watch_disk(&mut self) {
        if self.mode == Mode::Insert {
            return;
        }
        let now = std::time::Instant::now();
        if self.disk_checked.is_some_and(|at| now.duration_since(at) < DISK_LOOK) {
            return;
        }
        self.disk_checked = Some(now);
        self.reload_changed_files();
    }

    /// The terminal has the focus back, which is the likeliest moment for a
    /// file to have changed: look now rather than when the second is up.
    pub fn focus_gained(&mut self) {
        self.disk_checked = None;
        self.watch_disk();
    }

    /// `:fmt` - hand the buffer to whatever the language server formats with.
    /// The answer comes back later and is applied then, as one undo step.
    pub fn format(&mut self, lines: Option<(usize, usize)>) {
        match self.lsp_format(lines) {
            true => self.message = "formatting...".into(),
            false => self.message = "no language server that formats this".into(),
        }
    }

    /// `:s` - the substitute command, over whatever lines the range names.
    ///
    /// One pass per line, from the last up, so that replacing text on one line
    /// cannot move the line below it out from under the next edit. Every edit
    /// is inside one undo group, so the whole command comes back with one `u`.
    fn substitute(&mut self, command: substitute::Substitute) {
        let Some((first, last)) = self.substitute_lines(command.lines) else {
            return;
        };

        // An empty pattern means the last search, which is what makes `*` and
        // then `:%s//new/g` a pair of keystrokes rather than retyping a word.
        let pattern = match command.pattern.is_empty() {
            true => self.search.pattern.clone(),
            false => command.pattern.clone(),
        };
        if pattern.is_empty() {
            self.message = "no pattern, and no previous search".into();
            return;
        }
        // The same smart case as `/`: all lower case matches either, a capital
        // means it. The `i` and `I` flags say so outright instead.
        let insensitive = command
            .flags
            .insensitive
            .unwrap_or_else(|| !pattern.chars().any(char::is_uppercase));
        let regex = match RegexBuilder::new(&pattern).case_insensitive(insensitive).build() {
            Ok(regex) => regex,
            Err(_) => {
                self.message = format!("not a pattern: {pattern}");
                return;
            }
        };
        let replacement = substitute::replacement(&command.replacement);

        let (mut changes, mut lines, mut landed) = (0, 0, None);
        self.begin_undo_group();
        for line in (first..=last).rev() {
            let text = self.view().doc.line_str(line).to_string();
            let body = text.trim_end_matches('\n');
            let here = match command.flags.global {
                true => regex.find_iter(body).count(),
                false => usize::from(regex.is_match(body)),
            };
            if here == 0 {
                continue;
            }
            changes += here;
            lines += 1;
            landed = Some(line);
            if command.flags.count_only {
                continue;
            }

            let new = match command.flags.global {
                true => regex.replace_all(body, replacement.as_str()),
                false => regex.replace(body, replacement.as_str()),
            };
            let start = self.view().doc.line_to_char(line);
            let count = body.chars().count();
            self.view_mut().edit_at(start, count, &new, Some(start));
        }
        self.end_undo_group();

        if let Some(line) = landed
            && !command.flags.count_only
        {
            // Vim leaves the cursor on the last line it changed, which reading
            // upwards means the first one found.
            self.goto_line(line);
        }
        self.clamp_cursor();
        self.message = match (changes, command.flags.count_only) {
            (0, _) => format!("not found: {pattern}"),
            (_, true) => format!("{changes} matches on {lines} lines"),
            _ => format!("{changes} substitutions on {lines} lines"),
        };
    }

    /// The first and last line a range names, zero-based and in order, or a
    /// message about why it names nothing.
    fn substitute_lines(&mut self, lines: substitute::Lines) -> Option<(usize, usize)> {
        use substitute::{Address, Lines};
        let last_line = self.last_line();
        let resolve = |address: Address| match address {
            Address::Line(number) => Some(number.saturating_sub(1).min(last_line)),
            Address::Current => Some(self.view().cursor_coords().0),
            Address::Last => Some(last_line),
            Address::VisualStart | Address::VisualEnd => {
                let (sel, _) = self.last_visual?;
                let doc = &self.view().doc;
                let (from, to) = (doc.char_to_line(sel.anchor), doc.char_to_line(sel.head));
                Some(match address {
                    Address::VisualStart => from.min(to),
                    _ => from.max(to),
                })
            }
        };
        let (first, last) = match lines {
            Lines::Current => {
                let line = self.view().cursor_coords().0;
                (line, line)
            }
            Lines::Whole => (0, last_line),
            Lines::Range(first, last) => match (resolve(first), resolve(last)) {
                (Some(first), Some(last)) => (first.min(last), first.max(last)),
                _ => {
                    self.message = "no previous selection".into();
                    return None;
                }
            },
        };
        Some((first, last.min(last_line)))
    }

    /// `:config`: open the init file for editing, writing it out first if
    /// there is not one yet. What gets written is every setting at its
    /// default with a line above it saying what it does, so the file is the
    /// documentation as well as the configuration - and, being all defaults,
    /// a fresh one changes nothing until you edit it.
    pub fn open_config(&mut self) {
        let Some(dir) = crate::theme::config_dir() else {
            self.message = "no config directory: $HOME is not set".into();
            return;
        };
        let path = dir.join("init");
        let fresh = !path.exists();
        if fresh {
            let written = std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(&path, command::default_config()));
            if let Err(err) = written {
                self.message = format!("{}: {err}", path.display());
                return;
            }
        }
        match self.open_file(&path) {
            Ok(()) if fresh => self.message = format!("wrote {}", path.display()),
            Ok(()) => {}
            Err(err) => self.message = format!("{err:#}"),
        }
    }

    /// What to indent this buffer with: what the file itself uses, and the
    /// configured default when it uses nothing - or when a `:set` has since
    /// overruled it, which is what typing one is for.
    pub fn indent(&self) -> Indent {
        self.view().indent.unwrap_or(self.indent)
    }

    /// A `:set` that names an indent option means it for every buffer, so what
    /// was read out of the files stops counting. Reopening a file reads it
    /// again, which is the way back.
    fn override_detected_indent(&mut self) {
        if self.from_config {
            return;
        }
        for view in self.views.iter_mut() {
            view.indent = None;
        }
    }

    fn set_option(&mut self, option: &str) {
        // Options that take a value: `:set shiftwidth=2`.
        if let Some((name, value)) = option.split_once('=') {
            match (name, value.parse::<usize>()) {
                ("shiftwidth" | "sw", Ok(width)) if width > 0 && width <= 16 => {
                    self.indent.width = width;
                    self.override_detected_indent();
                }
                ("autocomplete" | "ac", Ok(chars)) if chars <= 16 => {
                    self.autocomplete = chars;
                }
                ("autocomplete" | "ac", _) => {
                    self.message = format!("autocomplete wants 0 to 16, not {value:?}");
                }
                ("semicolon", _) => match Semicolon::parse(value) {
                    Some(semicolon) => self.semicolon = semicolon,
                    None => self.message = "semicolon wants find or command".into(),
                },
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
            "semicolon" => self.semicolon = Semicolon::Command,
            "nosemicolon" => self.semicolon = Semicolon::Find,
            "tabline" => self.tabline = Tabline::Always,
            "notabline" => self.tabline = Tabline::Off,
            "autoindent" | "ai" => self.autoindent = true,
            "noautoindent" | "noai" => self.autoindent = false,
            "wrap" => self.wrap = true,
            "nowrap" => self.wrap = false,
            "inlayhints" => self.inlayhints = true,
            "noinlayhints" => {
                self.inlayhints = false;
                for view in &mut self.views {
                    view.hints.clear();
                    view.hints_asked = None;
                }
            }
            "undofile" => self.undofile = true,
            "noundofile" => self.undofile = false,
            "autopairs" => self.autopairs = true,
            "noautopairs" => self.autopairs = false,
            "emacs" => self.emacs = true,
            "noemacs" => self.emacs = false,
            "lsp" => {
                self.lsp_enabled = true;
                self.lsp_turned_on();
            }
            "nolsp" => self.lsp_enabled = false,
            "autocomplete" | "ac" => self.autocomplete = DEFAULT_AUTOCOMPLETE,
            "noautocomplete" | "noac" => self.autocomplete = 0,
            "expandtab" | "et" => {
                self.indent.tabs = false;
                self.override_detected_indent();
            }
            "noexpandtab" | "noet" => {
                self.indent.tabs = true;
                self.override_detected_indent();
            }
            "number" | "nu" => self.numbers = Numbers::Absolute,
            "nonumber" | "nonu" => self.numbers = Numbers::Off,
            "relativenumber" | "rnu" => self.numbers = Numbers::Relative,
            "hybrid" => self.numbers = Numbers::Hybrid,
            "trim" => self.trim_on_save = true,
            "notrim" => self.trim_on_save = false,
            "dog" => self.show_dog = true,
            "nodog" => self.show_dog = false,
            "cursorline" => self.cursorline = true,
            "nocursorline" => self.cursorline = false,
            "glyphs" => self.glyphs = true,
            "noglyphs" => self.glyphs = false,
            "signs" => self.signs_enabled = true,
            "nosigns" => {
                self.signs_enabled = false;
                self.view_mut().signs.clear();
            }
            "" => {
                // The indent reported is the one this buffer is being edited
                // with, which is the file's own where it had one to give.
                let indent = self.indent();
                let read = match self.view().indent.is_some() {
                    true => " (read from the file)",
                    false => "",
                };
                self.message = format!(
                    "number={} cursorline={} dog={} trim={} signs={} glyphs={} shiftwidth={} expandtab={}{read} autoindent={} autopairs={} undofile={} inlayhints={} wrap={} emacs={} lsp={} tabline={} autocomplete={} semicolon={}",
                    self.numbers.name(),
                    self.cursorline,
                    self.show_dog,
                    self.trim_on_save,
                    self.signs_enabled,
                    self.glyphs,
                    indent.width,
                    !indent.tabs,
                    self.autoindent,
                    self.autopairs,
                    self.undofile,
                    self.inlayhints,
                    self.wrap,
                    self.emacs,
                    self.lsp_enabled,
                    self.tabline.name(),
                    self.autocomplete,
                    self.semicolon.name()
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

    /// `<space>S`: names across the whole project, found by the language
    /// server as you type.
    pub fn open_workspace_symbol_picker(&mut self) {
        self.open_picker(Picker::live(Source::Workspace));
    }

    /// `<space>d`: what this buffer defines, to jump to. The same tags query
    /// `gd` reads, asked for the whole file - so a language we can highlight is
    /// a language we can list.
    pub fn open_symbol_picker(&mut self) {
        let definitions = self.view().definitions(&self.theme);
        if definitions.is_empty() {
            self.message = match self.view().has_grammar() {
                true => "nothing defined in this buffer".into(),
                false => "no grammar for this file".into(),
            };
            return;
        }
        let items = definitions
            .into_iter()
            .map(|(name, kind, line)| Item {
                text: name,
                detail: format!("{kind}  {}", line + 1),
                target: String::new(),
                id: line,
            })
            .collect();
        self.open_picker(Picker::new(Source::Symbols, items));
    }

    /// `<space>l`: every line of this buffer, to fuzzy-find and jump to -
    /// which is `/` without leaving the file, and without a pattern to spell.
    /// It opens on the line the cursor is on, so `<space>l<esc>` is nothing.
    pub fn open_line_picker(&mut self) {
        let view = self.view();
        let last = view.last_line();
        let width = (last + 1).to_string().len();
        let items: Vec<Item> = (0..=last)
            .map(|line| Item {
                // Indentation is not worth matching against and not worth the
                // width; the number on the right says where the line is.
                text: view.doc.line_str(line).trim_end_matches(['\n', '\r']).trim_start().to_string(),
                detail: format!("{:>width$}", line + 1),
                target: String::new(),
                id: line,
            })
            .collect();
        let (line, _) = self.cursor_coords();
        let rows = self.area_rows();
        let mut picker = Picker::new(Source::Lines, items);
        picker.focus(line, rows);
        self.open_picker(picker);
    }

    /// `<space>e`: everything the language servers say is wrong, in every open
    /// buffer - this one's first, in the order they come in the file, then the
    /// rest. The severity is part of the text, so typing `error` is a way to
    /// leave the warnings out.
    pub fn open_diagnostics_picker(&mut self) {
        let order = std::iter::once(self.current).chain((0..self.views.len()).filter(|&i| i != self.current));
        let mut items = Vec::new();
        for index in order {
            let view = &self.views[index];
            let Some(path) = view.doc.path.as_deref() else {
                continue;
            };
            let shown = view.doc.display_name();
            for diagnostic in &view.diagnostics {
                let first = diagnostic.message.lines().next().unwrap_or_default();
                let line = view.doc.char_to_line(diagnostic.start.min(view.doc.len_chars()));
                items.push(Item {
                    text: format!("{}: {first}", diagnostic.severity.name()),
                    detail: format!("{shown}:{}", line + 1),
                    id: diagnostic.start,
                    target: path.display().to_string(),
                });
            }
        }
        if items.is_empty() {
            self.message = match self.servers.is_empty() {
                true => "no diagnostics - and no language server running".into(),
                false => "no diagnostics".into(),
            };
            return;
        }
        self.open_picker(Picker::new(Source::Diagnostics, items));
    }

    /// Every key, searchable by the key or by what it does.
    pub fn open_help_picker(&mut self) {
        let width = BINDINGS.iter().map(|b| b.keys.chars().count()).max().unwrap_or(0);
        // What `:map` has been told, alongside what jack came with: a key you
        // gave yourself is a key you want to be reminded of.
        let mapped: Vec<Item> = self
            .leader
            .iter()
            .map(|(key, command)| Item {
                text: format!("{:width$}  :{command}", format!("<space>{key}")),
                detail: "mapped".into(),
                id: 0,
                target: String::new(),
            })
            .collect();
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
            .chain(mapped)
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
        if self.picker.as_ref().is_some_and(|picker| picker.source == Source::Workspace) {
            if !self.lsp_workspace_symbols(pattern, token) {
                self.message = "no language server that knows the project's symbols".into();
                if let Some(picker) = self.picker.as_mut() {
                    picker.mark_complete();
                }
            }
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
        let rows = self.area_rows();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let outcome = picker.input(key, rows);
        self.picker_outcome(outcome);
    }

    /// What the picker asked for after a key, or after a paste into its query.
    fn picker_outcome(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Continue => {}
            // Retiring the token stops a walk still in flight from feeding
            // whatever picker opens next.
            Outcome::Cancel => {
                self.picker = None;
                self.retire();
            }
            Outcome::Search(pattern) => self.search(pattern),
            Outcome::Replace(pattern) => self.start_project_replace(pattern),
            Outcome::Confirm(source, choice, open) => {
                self.picker = None;
                self.retire();
                // Where the picker was opened from, put on the jump list by
                // whatever the choice turns out to reach - and not at all when
                // the file will not open.
                let origin = self.here();
                // A split first, and the choice then goes to the new window
                // the same way it would have gone to this one. No room for
                // one is said and nothing opens: landing in the old window
                // instead would look like the key had been ignored.
                if open != Open::Here && !matches!(source, Source::Help | Source::Actions) {
                    let before = self.windows.len();
                    self.split_window(open == Open::Beside, None);
                    if self.windows.len() == before {
                        return;
                    }
                }
                // A window already showing what was chosen is the window to
                // land in: the same file twice on screen is a window wasted,
                // and the buffer this one is showing keeps its place. An
                // explicit `^v` or `^s` asked for a new window, so it is only
                // the plain enter that goes looking.
                if open == Open::Here
                    && let Some(id) = self.window_showing(source, &choice)
                {
                    self.focus_window(id);
                }
                match source {
                    // Help is a list to read; choosing a line just closes it.
                    Source::Help => {}
                    Source::Actions => self.run_code_action(choice.id),
                    Source::Buffers => {
                        self.jumps.push(origin);
                        self.switch_to(choice.id);
                    }
                    Source::Diagnostics => match self.open_file(&choice.target) {
                        Ok(()) => {
                            self.jumps.push(origin);
                            let at = choice.id.min(self.view().doc.len_chars());
                            self.view_mut().sel = Selection::point(at);
                            self.clamp_cursor();
                        }
                        Err(err) => self.message = format!("{err:#}"),
                    },
                    Source::Symbols | Source::Lines => {
                        self.jumps.push(origin);
                        self.goto_line(choice.id);
                        self.clamp_cursor();
                    }
                    Source::Files => match self.open_file(&choice.target) {
                        Ok(()) => self.jumps.push(origin),
                        Err(err) => self.message = format!("{err:#}"),
                    },
                    Source::Grep | Source::References | Source::Workspace => match self.open_file(&choice.target) {
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

    /// `ga`: what the server offers to do about where the cursor is. In visual
    /// mode the selection is what it is asked about, which is what a refactor
    /// over a few lines needs.
    pub fn code_actions(&mut self) {
        if !self.lsp_code_actions() {
            self.message = "no language server for this buffer".into();
        }
    }

    /// `gr`: every use of the name under the cursor, from the language server.
    /// There is no tree-sitter fallback: what one file can see is the file's
    /// own uses, which `*` already finds and which is not what this is for.
    pub fn references(&mut self) {
        if !self.lsp_references() {
            self.message = "no language server for this buffer".into();
        }
    }

    /// `gR`: ask what to rename the name under the cursor to. The prompt opens
    /// with the old name in it, and the server is asked when it is accepted.
    pub fn start_rename(&mut self) {
        let word = self.view().word_under_cursor().unwrap_or_default();
        self.open_prompt(PromptKind::Rename);
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.input.push_str(&word);
        }
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
            self.windows[self.focus].view = index;
            self.refresh_watched();
            self.clamp_cursor();
        }
    }

    // --- windows ------------------------------------------------------

    pub fn windows_open(&self) -> usize {
        self.windows.len()
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    /// Every window's rectangle, and the lines between side-by-side ones.
    pub fn window_rects(&self) -> (Vec<(usize, Rect)>, Vec<Rect>) {
        let (width, height) = self.screen;
        self.layout.rects(Rect { x: 0, y: self.top(), width, height })
    }

    pub fn window_rect(&self, id: usize) -> Rect {
        let (rects, _) = self.window_rects();
        rects.into_iter().find(|(window, _)| *window == id).map(|(_, rect)| rect).unwrap_or_default()
    }

    /// Text rows of the whole screen below the buffer list, less the bottom
    /// status line: what a picker opens over, whatever the windows are.
    pub fn area_rows(&self) -> usize {
        self.screen.1.saturating_sub(1).max(1)
    }

    /// A window's cursor and scroll: from its buffer when it is the focused
    /// one, otherwise its own copy, carried through whatever was done to the
    /// buffer in the meantime.
    pub fn window_state(&self, id: usize) -> (&View, Selection, usize, usize) {
        let window = &self.windows[id];
        let view = &self.views[window.view];
        match id == self.focus {
            true => (view, view.sel, view.scroll_top, view.scroll_left),
            false => {
                let (_, sel, top, left) = self.window_state_unfocused(id);
                (view, sel, top, left)
            }
        }
    }

    /// The focused window's size is what every scroll and screen motion works
    /// in, so it follows the layout whenever that changes.
    fn fit_focus(&mut self) {
        let rect = self.window_rect(self.focus);
        self.width = rect.width.max(1);
        self.height = rect.text_height().max(1);
    }

    /// Copy the focused window's cursor and scroll out of its buffer.
    fn store_focus(&mut self) {
        let view = &self.views[self.current];
        self.windows[self.focus] = Window {
            view: self.current,
            sel: view.sel,
            top_char: view.doc.line_to_char(view.scroll_top.min(view.last_line())),
            scroll_left: view.scroll_left,
            mark: view.log_mark(),
        };
    }

    /// And into it, for the window that has just been focused.
    fn load_focus(&mut self) {
        let (_, sel, top, left) = self.window_state_unfocused(self.focus);
        self.current = self.windows[self.focus].view;
        let view = &mut self.views[self.current];
        view.sel = sel;
        view.scroll_top = top;
        view.scroll_left = left;
        self.fit_focus();
        self.clamp_cursor();
    }

    fn window_state_unfocused(&self, id: usize) -> (usize, Selection, usize, usize) {
        let window = &self.windows[id];
        let view = &self.views[window.view];
        let sel = Selection {
            anchor: view.carry(window.mark, window.sel.anchor),
            head: view.carry(window.mark, window.sel.head),
        };
        let top = view.doc.char_to_line(view.carry(window.mark, window.top_char));
        (window.view, sel, top, window.scroll_left)
    }

    /// A buffer shown in two windows logs its edits, so the one not being
    /// typed in can follow them; one shown once does not need to. Every other
    /// window's copy is brought up to date first, since a log that stops is a
    /// log nobody can catch up from.
    fn refresh_watched(&mut self) {
        for id in 0..self.windows.len() {
            if id == self.focus {
                continue;
            }
            let (_, sel, _, _) = self.window_state_unfocused(id);
            let window = self.windows[id];
            let view = &self.views[window.view];
            let top_char = view.carry(window.mark, window.top_char);
            self.windows[id] = Window { sel, top_char, mark: view.log_mark(), ..window };
        }
        for index in 0..self.views.len() {
            let showing = self.windows.iter().filter(|window| window.view == index).count();
            self.views[index].set_watched(showing > 1);
        }
    }

    /// The smallest a window is split down to: a status line and a row of text
    /// either way, and room for a gutter and a word side by side.
    const MIN_WIDTH: usize = 12;
    const MIN_HEIGHT: usize = 2;

    /// `:split` and `:vsplit`, `^w s` and `^w v`: the focused window in two,
    /// the new half below or to the right, showing the same place in the same
    /// buffer - or `path`, when one is given - and focused.
    pub fn split_window(&mut self, vertical: bool, path: Option<&str>) {
        let rect = self.window_rect(self.focus);
        let room = match vertical {
            // Two windows and the line between them.
            true => rect.width > 2 * Self::MIN_WIDTH,
            false => rect.height >= 2 * Self::MIN_HEIGHT,
        };
        if !room {
            self.message = "no room to split".into();
            return;
        }
        self.store_focus();
        let new = self.windows.len();
        self.windows.push(self.windows[self.focus]);
        self.layout.split(self.focus, new, vertical);
        self.focus = new;
        self.load_focus();
        self.refresh_watched();
        if let Some(path) = path
            && let Err(err) = self.open_file(path)
        {
            self.message = format!("{err:#}");
        }
    }

    /// `:close`, `^w c`: the focused window goes and the one before it takes
    /// focus. The buffer stays open - closing a window never loses work - and
    /// the last window cannot be closed this way.
    pub fn close_window(&mut self) -> bool {
        if self.windows.len() == 1 {
            self.message = "the last window cannot be closed".into();
            return false;
        }
        let (rects, _) = self.window_rects();
        let order: Vec<usize> = rects.iter().map(|(id, _)| *id).collect();
        let index = order.iter().position(|id| *id == self.focus).unwrap_or(0);
        let next = match index {
            0 => order[1],
            _ => order[index - 1],
        };
        let closing = self.focus;
        self.windows.remove(closing);
        self.layout.remove(closing);
        self.focus = match next > closing {
            true => next - 1,
            false => next,
        };
        self.load_focus();
        self.refresh_watched();
        true
    }

    /// `:only`, `^w o`: every window but this one.
    pub fn only_window(&mut self) {
        self.store_focus();
        self.windows = vec![self.windows[self.focus]];
        self.layout = Layout::default();
        self.focus = 0;
        self.load_focus();
        self.refresh_watched();
    }

    /// The window, other than this one, already showing what a picker's
    /// choice points at - by buffer index for the buffer picker, by path for
    /// the sources that name a file. Sources that stay in this buffer, and
    /// the ones that open nothing, point at no window.
    fn window_showing(&self, source: Source, choice: &Choice) -> Option<usize> {
        let view = match source {
            Source::Buffers => choice.id,
            Source::Files | Source::Grep | Source::Diagnostics | Source::References | Source::Workspace => {
                let path = crate::editor::lsp::absolute(Path::new(&choice.target));
                self.views.iter().position(|view| view.doc.path.as_deref().map(crate::editor::lsp::absolute) == Some(path.clone()))?
            }
            Source::Help | Source::Actions | Source::Symbols | Source::Lines => return None,
        };
        (0..self.windows.len()).find(|&id| id != self.focus && self.windows[id].view == view)
    }

    pub fn focus_window(&mut self, id: usize) {
        if id == self.focus || id >= self.windows.len() {
            return;
        }
        self.store_focus();
        self.focus = id;
        self.load_focus();
    }

    /// `^w h j k l`: the window beside this one, level with the cursor.
    pub fn focus_direction(&mut self, direction: Direction) {
        let (rects, _) = self.window_rects();
        let (x, y) = self.cursor_screen();
        if let Some(id) = window::neighbour(&rects, self.focus, direction, (x as usize, y as usize)) {
            self.focus_window(id);
        }
    }

    /// `^w w` and `^w W`: the next window in screen order, or the previous.
    pub fn focus_next(&mut self, forward: bool) {
        let (rects, _) = self.window_rects();
        let order: Vec<usize> = rects.iter().map(|(id, _)| *id).collect();
        let index = order.iter().position(|id| *id == self.focus).unwrap_or(0);
        let count = order.len();
        let next = match forward {
            true => order[(index + 1) % count],
            false => order[(index + count - 1) % count],
        };
        self.focus_window(next);
    }

    /// `:bdelete`, `<space>x`: the buffer in this window goes. Every window
    /// showing it moves to the buffer before it in the list; closing the last
    /// one leaves an empty scratch buffer, as starting with no file does.
    /// Unsaved changes stop it unless forced.
    pub fn close_buffer(&mut self, force: bool) {
        let closing = self.current;
        let name = self.views[closing].doc.display_name().to_string();
        if !force && self.views[closing].is_modified() {
            self.message = format!("{name} has unsaved changes - :bd! to close it anyway");
            return;
        }
        self.store_focus();
        self.remember_position(closing);
        self.lsp_closed(closing);

        let last = self.views.len() == 1;
        match last {
            true => self.views[0] = View::new(Document::scratch()),
            false => {
                self.views.remove(closing);
            }
        }
        let shift = |index: usize| match index > closing {
            true => index - 1,
            false => index,
        };
        let replacement = closing.saturating_sub(1).min(self.views.len() - 1);
        for id in 0..self.windows.len() {
            let window = self.windows[id];
            self.windows[id] = match window.view == closing {
                true => {
                    let view = &self.views[replacement];
                    Window {
                        view: replacement,
                        sel: view.sel,
                        top_char: view.doc.line_to_char(view.scroll_top.min(view.last_line())),
                        scroll_left: view.scroll_left,
                        mark: view.log_mark(),
                    }
                }
                false => Window { view: shift(window.view), ..window },
            };
        }

        self.jumps.forget(closing);
        if !last {
            self.grouped_view = self.grouped_view.filter(|index| *index != closing).map(shift);
        }
        match self.signs_for == closing {
            // The diff on its way is for text that is not open any more.
            true => self.signs_token += 1,
            false => self.signs_for = shift(self.signs_for),
        }
        self.last_visual = None;
        self.completion = None;
        self.dismissed = None;

        self.load_focus();
        self.refresh_watched();
        self.message = format!("closed {name}");
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

        let document = Document::open(path)?;
        // The buffer the editor starts with is somewhere to stand, not
        // something to keep: opening the first file takes its place, so
        // `jack .` leaves one tab rather than two. In place, because the
        // jump list addresses buffers by index.
        if self.views.len() == 1 && self.views[0].is_empty_scratch() {
            self.views[0] = View::new(document);
            self.attach_syntax(0);
            self.restore_position(0);
            self.read_undo(0);
            self.switch_to(0);
            return Ok(());
        }

        self.views.push(View::new(document));
        let index = self.views.len() - 1;
        self.attach_syntax(index);
        self.restore_position(index);
        self.read_undo(index);
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

    /// The screen the windows share: `height` rows of text and a status line
    /// below them, as a single window has it.
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        self.screen = (width.max(1), height.max(1) + 1);
        self.fit_focus();
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

    pub fn set_signs(&mut self, token: u64, signs: Vec<(usize, Sign)>, hunks: Vec<stream::Hunk>) {
        if token == self.signs_token {
            let view = &mut self.views[self.signs_for];
            view.signs = signs.into_iter().collect();
            view.hunks = hunks;
            // What the diff was taken of - which is what the hunks' line
            // numbers are about, whatever has been typed since.
            view.hunks_revision = view.signs_revision;
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
        self.gutter_width_for(self.view())
    }

    pub fn gutter_width_for(&self, view: &View) -> usize {
        let numbers = match self.numbers {
            Numbers::Off => 0,
            // A space either side of the number.
            _ => (view.doc.len_lines()).max(1).to_string().len().max(2) + 2,
        };
        self.sign_width() + numbers
    }

    /// The width lines wrap at, when they wrap.
    pub fn wrap_width(&self) -> Option<usize> {
        self.wrap.then(|| self.text_width())
    }

    /// What is left for text once the gutter has taken its columns.
    pub fn text_width(&self) -> usize {
        self.width.saturating_sub(self.gutter_width()).max(1)
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
                crate::ui::str_width(&prompt.line()) as u16,
                (self.top() + self.area_rows()) as u16,
            );
        }
        match self.picker.as_ref() {
            Some(picker) => {
                let (x, y) = picker.cursor_screen(self.area_rows());
                (x, y + self.top() as u16)
            }
            None => {
                let rect = self.window_rect(self.focus);
                let (x, y) = self.view().cursor_screen(self.wrap_width());
                (x + (rect.x + self.gutter_width()) as u16, y + rect.y as u16)
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
        // Only up and down keep a `$` block: anything sideways is a new right
        // edge, and there is nothing left of `$` to keep.
        if self.block_to_eol && !matches!(m, Move::Up | Move::Down | Move::PageUp | Move::PageDown | Move::HalfPageUp | Move::HalfPageDown) {
            self.block_to_eol = false;
        }
        let (height, wrap) = (self.height, self.wrap_width());
        self.view_mut().move_cursor(m, extend, height, wrap);
    }

    /// `zt`, `zz`, `zb`: the cursor's line put at the top, middle or bottom of
    /// the screen, without moving the cursor off it.
    pub fn reveal(&mut self, where_to: Reveal) {
        let height = self.height;
        let wrap = self.wrap_width();
        self.view_mut().reveal(where_to, height, wrap);
    }

    /// `^e` and `^y`: scroll without moving the cursor, until the cursor would
    /// be scrolled off and has to come along.
    pub fn scroll_lines(&mut self, down: bool, count: usize) {
        let wrap = self.wrap_width();
        let height = self.view().lines_on_screen(self.height, wrap);
        self.view_mut().scroll_lines(down, count, height, wrap);
    }

    /// `H`, `M`, `L`: the line at the top, middle or bottom of what is on
    /// screen. Zero-based, for the caller to move to or operate over.
    pub fn screen_line(&self, which: Screen, count: usize) -> usize {
        self.view().screen_line(which, count, self.view().lines_on_screen(self.height, self.wrap_width()))
    }

    /// `H`, `M`, `L` as a motion: a jump, so `^o` comes back from it.
    pub fn goto_screen_line(&mut self, which: Screen, count: usize, extend: bool) {
        let line = self.screen_line(which, count);
        if !extend {
            self.push_jump();
        }
        match extend {
            true => self.goto_line_extending(line),
            false => self.goto_line(line),
        }
        self.clamp_cursor();
    }

    pub fn scroll_to_cursor(&mut self) {
        let (width, height, wrap) = (self.text_width(), self.height, self.wrap);
        if std::mem::take(&mut self.view_mut().centre) {
            self.view_mut().reveal(Reveal::Middle, height, wrap.then_some(width));
        }
        self.view_mut().scroll_to_cursor(width, height, wrap);
    }

    // --- undo that outlives the editor ---------------------------------

    /// Say where undo files are kept, and read back the history of the
    /// buffers already open.
    pub fn set_undo_dir(&mut self, dir: Option<PathBuf>) {
        self.undo_dir = dir;
        for index in 0..self.views.len() {
            self.read_undo(index);
        }
    }

    /// A buffer just opened takes the history written at its last save, if
    /// that history is about the text it opened with.
    fn read_undo(&mut self, index: usize) {
        let (Some(dir), true) = (self.undo_dir.as_deref(), self.undofile) else {
            return;
        };
        let view = &self.views[index];
        let Some(path) = view.doc.path.as_deref() else {
            return;
        };
        // Only over a buffer nothing has been done to yet.
        if view.history().depth() > 0 {
            return;
        }
        if let Some(history) = crate::undofile::read(dir, path, &view.doc.text) {
            self.views[index].restore_history(history);
        }
    }

    /// After a write: the history, next to the text it is now about.
    fn write_undo(&mut self) {
        let (Some(dir), true) = (self.undo_dir.as_deref(), self.undofile) else {
            return;
        };
        let view = self.view();
        let Some(path) = view.doc.path.as_deref() else {
            return;
        };
        if let Err(err) = crate::undofile::write(dir, path, &view.doc.text, view.history()) {
            self.message = format!("{}, but the undo history was not kept: {err}", self.message);
        }
    }

    // --- where the cursor was left ------------------------------------

    /// Take the list of where the cursor was left in files, and put the
    /// buffers already open back where they were.
    pub fn set_positions(&mut self, positions: crate::positions::Positions) {
        self.positions = positions;
        for index in 0..self.views.len() {
            self.restore_position(index);
        }
    }

    /// A file just opened goes back to where its cursor was left - the line
    /// and column, clamped to what the file is now, since it may well have
    /// changed since.
    fn restore_position(&mut self, index: usize) {
        let view = &self.views[index];
        let Some(path) = view.doc.path.as_deref() else {
            return;
        };
        let Some((line, column)) = self.positions.get(path) else {
            return;
        };
        let line = line.min(view.doc.len_lines().saturating_sub(1));
        // On a character, not past the last one: a file opens in normal mode.
        let at = view.doc.line_to_char(line) + column.min(view.doc.line_len_chars(line).saturating_sub(1));
        let view = &mut self.views[index];
        view.sel = Selection::point(at);
        view.centre = true;
    }

    /// Note where the cursor is in a buffer that is about to go.
    fn remember_position(&mut self, index: usize) {
        let view = &self.views[index];
        if let Some(path) = view.doc.path.as_deref() {
            let (line, column) = view.cursor_coords();
            self.positions.set(path, line, column);
        }
    }

    /// On the way out: every open buffer's cursor, written down.
    pub fn save_positions(&mut self) {
        for index in 0..self.views.len() {
            self.remember_position(index);
        }
        if let Err(err) = self.positions.save() {
            eprintln!("jack: could not remember cursor positions: {err}");
        }
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
        // The buffer answers now; the server answers in a moment, into the
        // same popup - or into a new one, if the buffer had nothing.
        self.lsp_complete(complete::word_start(self.view(), at), None);
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
        self.lsp_complete(start, None);
        // Nothing to offer, and there never will be - from the buffer. The
        // server may still answer, and `dismissed` is not in its way: what it
        // stops is gathering the buffer's words again, once per keystroke.
        if self.completion.is_none() {
            self.dismissed = Some(start);
        }
    }

    /// A character the server asked to be told about - a `.`, most of the
    /// time. There is no word here to complete, so there is nothing to show
    /// until the answer lands; `completion_answer` opens the popup then.
    pub fn suggest_from_server(&mut self, typed: char) {
        if self.autocomplete == 0 || self.mode != Mode::Insert {
            return;
        }
        if !self.completion_triggers().contains(&typed) {
            return;
        }
        self.completion = None;
        self.dismissed = None;
        self.lsp_complete(self.view().sel.head, Some(typed));
    }

    /// `K`: what the server says the thing under the cursor is, in a box
    /// beside it. The answer comes back later, which is why this only says
    /// that it asked.
    pub fn hover(&mut self) {
        self.info = None;
        match self.lsp_hover() {
            true => self.message = "asking...".into(),
            false => self.message = "no language server for this buffer".into(),
        }
    }

    /// A character the server wants to show a signature after - a `(`, or a
    /// `,` moving on to the next argument. Nothing appears until the answer
    /// lands, and nothing at all when the server has none to give.
    pub fn signature_hint(&mut self, typed: char) {
        if self.mode != Mode::Insert || !self.signature_triggers().contains(&typed) {
            return;
        }
        self.lsp_signature(Some(typed));
    }

    /// The box goes away with the next key, the way a message does - but only
    /// the answer to a `K`. A signature is for the call being typed, so it
    /// stands while you type it.
    pub fn dismiss_hover(&mut self) {
        if self.info.as_ref().is_some_and(|info| info.kind == info::Kind::Hover) {
            self.info = None;
        }
    }

    /// A signature is about the call it was asked in: back out past the
    /// bracket that opened the call and it is about nothing.
    pub fn update_info(&mut self) {
        if let Some(info) = self.info.as_ref()
            && info.kind == info::Kind::Signature
            && self.view().sel.head < info.anchor
        {
            self.info = None;
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
        let indent = self.indent();
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
        let indent = self.indent();
        let last = last.min(self.view().last_line());
        let mut targets = Vec::new();
        for line in first..=last {
            if let Some(level) = self.view().indent_level(line, &self.theme) {
                targets.push((line, level.column(indent.width)));
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

    /// `gc`: comment lines out, or back in when they all already are.
    pub fn comment_lines(&mut self, first: usize, last: usize) {
        let Some(marker) = comment::marker_for(self.view().doc.path.as_deref()) else {
            self.message = "no comment marker for this file".into();
            return;
        };
        // A count is worth reporting once it is more than the line you can see.
        match self.view_mut().toggle_comments(first, last, marker) {
            Toggled::Commented(n) if n > 1 => self.message = format!("{n} lines commented"),
            Toggled::Uncommented(n) if n > 1 => self.message = format!("{n} lines uncommented"),
            _ => {}
        }
    }

    /// The lines a selection covers, for visual `gc` and `gcap`.
    pub fn comment_selection(&mut self) {
        let (first, last) = self.selection_lines();
        self.comment_lines(first, last);
    }

    /// The lines a motion covered, for `gcj`.
    pub fn comment_motion(&mut self) {
        let view = self.view();
        let (first, _) = view.doc.coords(view.sel.anchor.min(view.sel.head));
        let (last, _) = view.doc.coords(view.sel.anchor.max(view.sel.head));
        self.comment_lines(first, last);
    }

    /// Re-indent the line the cursor is on. `merge` makes it part of the edit
    /// before it, which is what the auto-indent after `enter` wants.
    fn reindent_current_line(&mut self, merge: bool) {
        if !self.autoindent || !self.view().has_indent_rules() {
            return;
        }
        let indent = self.indent();
        let (line, _) = self.view().cursor_coords();
        let grammar = self
            .view()
            .indent_level(line, &self.theme)
            .map(|level| level.column(indent.width));
        let guess = self.guessed_indent_column(line);

        // On a line with something on it the grammar is the authority: it can
        // see what the line *is*, and a closing brace belongs under its opener
        // whatever came before it.
        //
        // On a blank one it cannot. The line the cursor just opened has no
        // content to place, and a tree that parses without it is a tree that
        // has not been told about it yet: `a:` in YAML is a complete mapping
        // pair until something appears under it, so the grammar says nought
        // steps for a line that is plainly one step in. So the deeper of the
        // two wins there, which is the guess saying a step is owed and the
        // grammar never contradicting it - it only ever knows less.
        let blank = self.view().doc.line_str(line).trim().is_empty();
        let column = match (grammar, guess) {
            (Some(grammar), Some(guess)) if blank => grammar.max(guess),
            (Some(grammar), _) => grammar,
            (None, Some(guess)) => guess,
            (None, None) => return,
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
        // A colon opens a block in as many languages as a brace does - a
        // Python `def`, a YAML key, a `case` in C or Go - and a line that ends
        // with one is a line that has opened something whatever the grammar
        // has managed to parse so far.
        if previous.trim_end().ends_with(['{', '[', '(', ':']) {
            column += self.indent().width;
        }
        if view.doc.line_str(line).trim_start().starts_with(['}', ']', ')']) {
            column = column.saturating_sub(self.indent().width);
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
        let text = self.indent().tab(at);
        self.insert(&text);
    }

    /// `^t` and `^d` in insert mode: shift the line the cursor is on without
    /// leaving insert mode or moving the cursor off its word.
    pub fn shift_current_line(&mut self, out: bool) {
        let (line, column) = self.view().cursor_coords();
        let before = self.view().doc.line_len_chars(line);
        let indent = self.indent();
        self.view_mut().shift_lines(line, line, out, 1, indent);
        // The cursor keeps its place in the text rather than its column.
        let after = self.view().doc.line_len_chars(line);
        let base = self.view().doc.line_to_char(line);
        let moved = (column + after).saturating_sub(before).min(after);
        self.view_mut().sel = Selection::point(base + moved);
    }

    /// Text the terminal handed over in one piece, between the markers a
    /// bracketed paste is wrapped in. It is text, not keystrokes: nothing in
    /// it is a command, and none of it goes through the auto-indent - which is
    /// the whole point, because indented code typed a character at a time is
    /// how a paste ends up as a staircase.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() {
            return;
        }

        // A prompt and a picker are one line each, so a multi-line paste into
        // one is taken as its first line: the alternative is a query with a
        // newline in it, which can match nothing.
        let line = text.lines().next().unwrap_or_default();
        if let Some(picker) = self.picker.as_mut() {
            let outcome = picker.extend_query(line);
            self.picker_outcome(outcome);
            return;
        }
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.input.push_str(line);
            prompt.completion = None;
            self.prompt_changed();
            return;
        }

        self.begin_undo_group();
        match self.mode {
            Mode::Insert => self.view_mut().insert(&text),
            // In normal mode a paste goes in beside the cursor, like `p`: a
            // paste that ends in a newline is whole lines and belongs on their
            // own, and anything else belongs where the cursor is.
            _ => match text.ends_with('\n') {
                true => self.view_mut().put_lines(&text, true),
                false => self.view_mut().put_inline(&text, true),
            },
        }
        self.end_undo_group();
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

    /// A character typed in insert mode, with the brackets and quotes it
    /// comes in pairs with looked after.
    ///
    /// Three things, and each of them only where it cannot get in the way:
    ///
    /// - An opening bracket gets its closer, when what follows is the end of
    ///   the line, a space or another closer. Before a word it does not: `(`
    ///   typed to wrap something already there would otherwise leave a `)`
    ///   stranded in front of it.
    /// - A closer steps over the same closer under the cursor, rather than
    ///   typing a second one - which is what makes typing both halves by habit
    ///   come out right.
    /// - A quote is both at once: over the same quote it steps, and otherwise
    ///   it pairs under the same rule as a bracket, and not straight after a
    ///   word character, which is an apostrophe (`don't`) or a string prefix
    ///   (`b"`, `f"`) rather than the start of a string.
    pub fn type_char(&mut self, c: char) {
        if !self.autopairs || !self.view().sel.is_empty() {
            return self.insert(&c.to_string());
        }
        let (before, after) = self.around_cursor();

        if after == Some(c) && (is_closer(c) || is_quote(c)) {
            self.move_cursor(Move::Right, false);
            return;
        }
        let pairs = match closer_of(c) {
            Some(_) if is_quote(c) => {
                !before.is_some_and(crate::complete::is_word) && self.quote_pairs(c)
            }
            Some(_) => true,
            None => false,
        };
        let room = after.is_none_or(|next| next.is_whitespace() || is_closer(next));
        if !(pairs && room) {
            return self.insert(&c.to_string());
        }
        let close = closer_of(c).expect("checked above");
        let head = self.view().sel.head;
        self.view_mut().edit_at(head, 0, &format!("{c}{close}"), Some(head + 1));
    }

    /// Whether `'` opens a string in this language. In Rust it is a lifetime
    /// far more often than a character, and `<'a>` coming out as `<'a'>`
    /// is worse than typing one quote by hand.
    fn quote_pairs(&self, c: char) -> bool {
        let language = crate::syntax::language_for_path(self.view().doc.path.as_deref()).map(|l| l.name);
        !(c == '\'' && language == Some("rust"))
    }

    /// The characters either side of the cursor, on its line.
    fn around_cursor(&self) -> (Option<char>, Option<char>) {
        let view = self.view();
        let (line, column) = view.cursor_coords();
        let text = view.doc.line_str(line);
        let before = column.checked_sub(1).and_then(|at| text.chars().nth(at));
        (before, text.chars().nth(column))
    }

    /// `backspace` in insert mode: between a pair with nothing inside, both
    /// halves go - the closer was put there for you, and leaving it behind
    /// after taking back the opener is a mess to tidy by hand.
    pub fn backspace(&mut self) {
        if self.autopairs && self.view().sel.is_empty() {
            let (before, after) = self.around_cursor();
            if let (Some(open), Some(close)) = (before, after)
                && closer_of(open) == Some(close)
            {
                let head = self.view().sel.head;
                self.view_mut().edit_at(head - 1, 2, "", Some(head - 1));
                return;
            }
        }
        self.delete_backward();
    }

    /// `enter` in insert mode: between `{` and `}` the closer goes down a line
    /// of its own and the cursor sits on an indented line between them, which
    /// is the only thing anyone ever wants to do next.
    pub fn enter(&mut self) {
        let (before, after) = self.around_cursor();
        let between = self.autopairs
            && matches!((before, after), (Some(open), Some(close)) if is_bracket(open) && closer_of(open) == Some(close));
        self.insert_newline();
        if !between {
            return;
        }
        // The closer is at the cursor, on the new line: push it down another,
        // then come back up to the end of the line in the middle.
        self.insert_newline();
        let line = self.view().cursor_coords().0 - 1;
        let end = self.view().doc.line_to_char(line) + self.view().doc.line_len_chars(line);
        self.view_mut().sel = Selection::point(end);
        self.reindent_current_line(true);
        let end = self.view().doc.line_to_char(line) + self.view().doc.line_len_chars(line);
        self.view_mut().sel = Selection::point(end);
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
        // Leaving insert mode after a block `I`, `A` or `c` puts what was
        // typed on the rest of the block's lines. It has to happen before the
        // cursor steps back off the last character typed.
        if self.mode == Mode::Insert && mode != Mode::Insert && self.pending_block.is_some() {
            self.finish_block_insert();
        }
        // The popup belongs to insert mode, whichever way you leave it, and so
        // does the signature of the call that was being typed.
        if mode != Mode::Insert {
            self.completion = None;
            self.info = None;
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
        if mode != Mode::VisualBlock {
            self.block_to_eol = false;
        }
        if self.mode.is_visual() != mode.is_visual() {
            // On the way out it is worth keeping: `gv` is the only way back to
            // a selection, and whatever happens next usually destroys it.
            if self.mode.is_visual() {
                self.last_visual = Some((self.view().sel, self.mode));
            }
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
            // A block is not a range, so this is its bounding box: the
            // commands that understand rectangles ask `block()` instead.
            Mode::VisualBlock => {
                let block = self.block()?;
                Some((block.rows.first()?.0, block.rows.last()?.1))
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

    /// A command is starting: everything it edits is one undo step. The keys
    /// know where a command begins and ends - that is what `.` is built on -
    /// so they are what drives this, and `ciwword<esc>` comes back in one `u`.
    pub fn begin_undo_group(&mut self) {
        self.view_mut().begin_undo_group();
        self.grouped_view = Some(self.current);
    }

    /// Closed on the buffer it was opened on: a command that changed window or
    /// buffer part way would otherwise leave that one grouping for ever.
    pub fn end_undo_group(&mut self) {
        let index = self.grouped_view.take().unwrap_or(self.current);
        self.views[index].end_undo_group();
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
                };
                self.lsp_saved();
                self.write_undo();
            }
            Err(err) => self.message = format!("{err:#}"),
        }
    }

    /// `:wa`: every buffer with unsaved changes and a file to write them to,
    /// written. `true` when nothing was left unwritten.
    pub fn write_all(&mut self, force: bool) -> bool {
        let was = self.current;
        let (mut wrote, mut failed) = (0, None);
        for index in 0..self.views.len() {
            let view = &self.views[index];
            if !view.is_modified() || view.doc.path.is_none() {
                continue;
            }
            self.current = index;
            self.write(None, force);
            match self.message.starts_with("wrote") {
                true => wrote += 1,
                false => {
                    failed.get_or_insert(std::mem::take(&mut self.message));
                }
            }
        }
        self.current = was;
        let unnamed = self.views.iter().filter(|view| view.is_modified() && view.doc.path.is_none()).count();
        self.message = match (failed, wrote, unnamed) {
            (Some(err), _, _) => err,
            (None, _, n) if n > 0 => format!("wrote {wrote}, but {n} unnamed buffer(s) have nowhere to go - :w name"),
            (None, 0, _) => "nothing to write".into(),
            (None, 1, _) => "wrote 1 buffer".into(),
            (None, n, _) => format!("wrote {n} buffers"),
        };
        self.message.starts_with("wrote") && unnamed == 0 || self.message == "nothing to write"
    }

    /// `:e` with no argument: read the file again. Refuses to throw away
    /// unsaved changes unless told twice.
    pub fn reload(&mut self, force: bool) {
        if !force && self.is_modified() {
            self.message = "unsaved changes - :e! to reload anyway".into();
            return;
        }
        let name = self.view().doc.display_name().to_string();
        match self.view_mut().reload() {
            Ok(()) => self.message = format!("reloaded {name}"),
            Err(err) => self.message = format!("{err:#}"),
        }
        self.clamp_cursor();
    }

    // --- the system clipboard: ^c ^x ^v ---------------------------------

    /// `^c`: the selection, or the whole line when there is none - which is
    /// what every editor with these keys does, and what makes `^c^v` a way to
    /// duplicate a line without selecting it first.
    pub fn clip_copy(&mut self) {
        match self.mode {
            Mode::Visual | Mode::VisualLine => {
                self.yank_visual(Some(SYSTEM));
                self.set_mode(Mode::Normal);
            }
            // Shift and an arrow leaves a selection without leaving insert
            // mode, and that is a selection like any other. A yank collapses
            // to the start of what it took; while typing, the cursor belongs
            // where it was, so that copying does not move you.
            _ if self.has_selection() => {
                let head = self.view().sel.head;
                self.yank_selection(Some(SYSTEM));
                if self.mode == Mode::Insert {
                    self.view_mut().sel = Selection::point(head);
                }
            }
            _ => self.yank_lines(Some(SYSTEM), 1),
        }
        self.push_clipboard();
        self.message = "copied".into();
    }

    /// What is in the `+` register now, for telling whether a command has just
    /// written to it - which is what a `"+y` has to notice.
    pub fn system_register(&self) -> RegisterValue {
        self.registers.get(Some(SYSTEM))
    }

    /// `J`: the next line pulled up onto this one with a single space where
    /// the newline was. `J` and `2J` both join one newline - vim counts the
    /// lines taking part, not the joins - and `3J` joins three lines into one.
    pub fn join_lines(&mut self, count: usize) {
        for _ in 0..count.max(2) - 1 {
            if !self.join_once() {
                self.message = "no line below to join".into();
                break;
            }
        }
    }

    /// Every line of the selection onto one, which is `J` in visual mode.
    pub fn join_visual(&mut self) {
        // The lines the two ends are on, rather than the character range: in
        // line mode the head sits at the start of the last line, and a range
        // would stop one line short of it.
        let sel = self.view().sel;
        let doc = &self.view().doc;
        let (first, last) = (doc.char_to_line(sel.anchor), doc.char_to_line(sel.head));
        let (first, last) = (first.min(last), first.max(last));
        self.set_mode(Mode::Normal);
        self.goto_line(first);
        // One join per newline inside the selection, and at least one, so `J`
        // on a single line does what it does in normal mode.
        self.join_lines(last.saturating_sub(first) + 1);
    }

    /// One join. False when there is no line below to join, which is where a
    /// count runs out.
    fn join_once(&mut self) -> bool {
        let view = self.view_mut();
        let (line, _) = view.cursor_coords();
        if line >= view.last_line() {
            return false;
        }
        let start = view.doc.line_to_char(line);
        let text = view.doc.line_str(line);
        let content = text.trim_end_matches('\n');
        let at = start + content.chars().count();

        let next = view.doc.line_str(line + 1);
        let blank = crate::buffer::indent_of(&next).chars().count();
        let end = view.doc.line_to_char(line + 1) + blank;
        let rest = next.trim_start_matches([' ', '\t']);

        // A space, except where vim does not add one: an empty line either
        // side of the join, one that already ends in space, and a closing
        // bracket, which wants to sit against what it closes.
        let space = match content.is_empty()
            || content.ends_with([' ', '\t'])
            || rest.trim_end_matches('\n').is_empty()
            || rest.starts_with(')')
        {
            true => "",
            false => " ",
        };
        view.edit_at(at, end - at, space, Some(at));
        true
    }

    /// `r`: the character under the cursor becomes another one, `count` of them
    /// at once. False - and nothing changed - when the line is too short for
    /// the count, as vim refuses rather than replacing what it can.
    pub fn replace_char(&mut self, c: char, count: usize) -> bool {
        let view = self.view_mut();
        let at = view.sel.head;
        let (line, column) = view.cursor_coords();
        let room = view.doc.line_str(line).trim_end_matches('\n').chars().count();
        if column + count > room {
            return false;
        }
        let text: String = std::iter::repeat_n(c, count).collect();
        // The cursor ends on the last character replaced, not past it.
        view.edit_at(at, count, &text, Some(at + count - 1));
        true
    }

    /// `$` in block mode: the block runs to the end of every line it covers.
    /// The cursor still goes to the end of the line it is on, so the block is
    /// where it looks like it is on the line being moved.
    pub fn block_to_end_of_line(&mut self) {
        self.move_cursor(Move::LineEnd, true);
        self.block_to_eol = true;
        self.clamp_cursor();
    }

    /// `r{c}` over a selection: every character in it becomes `c`. Line breaks
    /// are left alone - vim leaves them, and replacing them would glue the
    /// selected lines into one.
    pub fn replace_selection(&mut self, c: char) {
        let Some(rows) = self.selection_rows() else {
            return;
        };
        self.edit_rows(rows, |text| {
            text.chars().map(|ch| match ch == '\n' || ch == '\r' {
                true => ch,
                false => c,
            })
            .collect()
        });
    }

    /// `~` over a selection: every character's case swapped. A character whose
    /// case is more than one character long - `ß` upper-cases to `SS` - is
    /// left alone, as it is for the `~` that takes a count.
    pub fn toggle_case_selection(&mut self) {
        let Some(rows) = self.selection_rows() else {
            return;
        };
        self.edit_rows(rows, |text| {
            text.chars()
                .map(|c| match (c.is_lowercase(), c.is_uppercase()) {
                    (true, _) => one(c.to_uppercase()).unwrap_or(c),
                    (_, true) => one(c.to_lowercase()).unwrap_or(c),
                    _ => c,
                })
                .collect()
        });
    }

    /// What a command over a selection acts on: one range for `v` and `V`, and
    /// one per line for a block, which is not a range at all.
    fn selection_rows(&self) -> Option<Vec<(usize, usize)>> {
        match self.block() {
            Some(block) => Some(block.rows),
            None => self.selection_range().map(|range| vec![range]),
        }
    }

    /// Rewrite each row through `f`, bottom up and in one undo step, and leave
    /// visual mode with the cursor on the first thing changed.
    fn edit_rows(&mut self, rows: Vec<(usize, usize)>, f: impl Fn(&str) -> String) {
        let Some(&(at, _)) = rows.first() else {
            return;
        };
        self.begin_undo_group();
        for &(start, end) in rows.iter().rev() {
            if end <= start {
                continue;
            }
            let text = self.view().doc.slice_str(start, end);
            let new = f(&text);
            self.view_mut().edit_at(start, end - start, &new, None);
        }
        self.end_undo_group();
        self.view_mut().sel = Selection::point(at);
        self.set_mode(Mode::Normal);
        self.clamp_cursor();
    }

    /// `~`: the case of the character under the cursor swapped, and on to the
    /// next one. Characters whose case is more than one character long - `ß`
    /// upper-cases to `SS` - are left alone rather than changing the length of
    /// the line under a cursor that is counting columns.
    pub fn toggle_case(&mut self, count: usize) {
        let view = self.view_mut();
        let at = view.sel.head;
        let (line, column) = view.cursor_coords();
        let text = view.doc.line_str(line);
        let content = text.trim_end_matches('\n');
        let room = content.chars().count().saturating_sub(column).min(count);
        if room == 0 {
            return;
        }
        let swapped: String = content
            .chars()
            .skip(column)
            .take(room)
            .map(|c| match (c.is_lowercase(), c.is_uppercase()) {
                (true, _) => one(c.to_uppercase()).unwrap_or(c),
                (_, true) => one(c.to_lowercase()).unwrap_or(c),
                _ => c,
            })
            .collect();
        // Vim leaves the cursor after the last character it changed, clamped
        // back onto the line by normal mode.
        view.edit_at(at, room, &swapped, Some(at + room));
        self.clamp_cursor();
    }

    /// `gv`: the last visual selection, back again. Set when visual mode is
    /// left, so it survives the editing that usually follows.
    pub fn reselect(&mut self) {
        let Some((sel, mode)) = self.last_visual else {
            self.message = "no previous selection".into();
            return;
        };
        let last = self.view().doc.len_chars();
        // The buffer may be shorter than it was when the selection was made.
        self.view_mut().sel = Selection {
            anchor: sel.anchor.min(last),
            head: sel.head.min(last),
        };
        self.mode = mode;
        self.clamp_cursor();
    }

    /// The current buffer's state, for telling whether a command changed
    /// anything: which buffer it was, how many edits deep, and how long it is.
    /// Switching buffers counts as a change of state, which is the point: the
    /// comparison is only ever between two moments of the same command.
    pub fn revision(&self) -> (usize, usize, usize) {
        let (depth, len) = self.view().revision();
        (self.current, depth, len)
    }

    /// Where the cursor is, in a form that can be compared before and after a
    /// key: which buffer, and where in it. The dog runs on this changing
    /// rather than on the keystroke, so holding a key that goes nowhere - `l`
    /// against the end of a line, a `:w`, an `esc` - leaves it sitting.
    pub fn cursor_mark(&self) -> (usize, usize) {
        (self.current, self.view().sel.head)
    }

    /// The cursor moved: the dog takes a step.
    pub fn dog_runs(&mut self) {
        self.dog.steps = self.dog.steps.wrapping_add(1);
        self.dog.running = true;
    }

    /// Typing stopped: the dog sits down where it had got to. Keeping the
    /// step count is what puts it there rather than back in the middle, and
    /// what makes the next burst of typing carry on from the same place.
    pub fn dog_rests(&mut self) {
        self.dog.running = false;
    }

    /// True when something is selected outside visual mode - what shift and an
    /// arrow leave behind while typing. Visual mode has its own range, which
    /// covers the character under the cursor as well.
    fn has_selection(&self) -> bool {
        let sel = self.view().sel;
        sel.anchor != sel.head
    }

    /// `^x`, the same rule: the selection, or the line.
    pub fn clip_cut(&mut self) {
        match self.mode {
            Mode::Visual | Mode::VisualLine => {
                self.delete_visual(Some(SYSTEM));
                self.set_mode(Mode::Normal);
            }
            _ if self.has_selection() => self.delete_selection(Some(SYSTEM)),
            _ => self.delete_lines(Some(SYSTEM), 1),
        }
        self.push_clipboard();
        self.clamp_cursor();
    }

    /// `^v`. In insert mode the text goes in at the cursor, as typing it
    /// would; anywhere else it is a put, so a copied line lands on a line of
    /// its own rather than in the middle of the one you are on.
    pub fn clip_paste(&mut self) {
        self.pull_clipboard();
        if self.registers.get(Some(SYSTEM)).is_empty() {
            self.message = "nothing to put".into();
            if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                self.set_mode(Mode::Normal);
            }
            return;
        }
        match self.mode {
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock => self.put_over_visual(Some(SYSTEM)),
            Mode::Insert => {
                let text = self.registers.get(Some(SYSTEM)).text;
                self.insert(&text);
            }
            Mode::Normal => {
                self.put(Some(SYSTEM), 1, true);
                self.clamp_cursor();
            }
        }
    }

    /// A register's contents, asking the session's clipboard first when the
    /// register is `+`. Every read of a register that a put could name goes
    /// through here, so `"+p` is the same thing `^v` is.
    fn register_value(&mut self, name: Option<char>) -> RegisterValue {
        if name == Some(SYSTEM) {
            self.pull_clipboard();
        }
        self.registers.get(name)
    }

    /// Send the `+` register out to the session's clipboard.
    pub fn push_clipboard(&mut self) {
        let text = self.registers.get(Some(SYSTEM)).text;
        self.escape = clipboard::copy(&text);
    }

    /// Bring the session's clipboard in, so a `^v` pastes what was copied in
    /// the browser rather than what was copied here an hour ago. Text ending
    /// in a newline is taken as whole lines - which is what makes copying a
    /// line here and pasting it back put it on a line of its own.
    fn pull_clipboard(&mut self) {
        let Some(text) = clipboard::paste() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let value = match text.ends_with('\n') {
            true => RegisterValue::linewise(text),
            false => RegisterValue::charwise(text),
        };
        self.registers.set(SYSTEM, value);
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
        if self.mode == Mode::VisualBlock {
            return self.delete_block(register);
        }
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let linewise = self.mode == Mode::VisualLine;
        self.cut_range(register, start, end, linewise);
        self.set_mode(Mode::Normal);
    }

    pub fn yank_visual(&mut self, register: Option<char>) {
        if self.mode == Mode::VisualBlock {
            return self.yank_block(register);
        }
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let kind = self.register_kind();
        let (view, registers) = self.view_and_registers();
        let text = view.doc.slice_str(start, end);
        registers.record_yank(register, RegisterValue { text, kind });
        // The cursor lands at the start of what was yanked, as vim does.
        view.sel = Selection::point(start);
        self.set_mode(Mode::Normal);
    }

    /// Replace the selection with a register's contents. What was there goes
    /// to the register the delete would have used, so it can be put back.
    pub fn put_over_visual(&mut self, register: Option<char>) {
        // Over a block, what was there goes and what is put takes its place
        // as a rectangle of its own - the same two steps, spelled twice.
        if self.mode == Mode::VisualBlock {
            let value = self.register_value(register);
            if value.is_empty() {
                self.message = "nothing to put".into();
                self.set_mode(Mode::Normal);
                return;
            }
            self.begin_undo_group();
            self.delete_block(None);
            match value.is_block() {
                true => self.put_block(&value, false),
                false => {
                    let at = self.view().sel.head;
                    self.view_mut().put_inline_at(at, &value.text);
                }
            }
            self.end_undo_group();
            self.clamp_cursor();
            return;
        }
        let Some((start, end)) = self.selection_range() else {
            return;
        };
        let value = self.register_value(register);
        if value.is_empty() {
            self.message = "nothing to put".into();
            self.set_mode(Mode::Normal);
            return;
        }

        let linewise = self.mode == Mode::VisualLine;
        self.cut_range(None, start, end, linewise);
        self.set_mode(Mode::Normal);

        match (value.is_linewise(), linewise) {
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
        let kind = match linewise {
            true => Kind::Line,
            false => Kind::Char,
        };
        let (view, registers) = self.view_and_registers();
        let text = view.cut(start, end);
        registers.record_delete(register, RegisterValue { text, kind });
    }

    /// What shape the mode says a yank or a delete is.
    fn register_kind(&self) -> Kind {
        match self.mode {
            Mode::VisualLine => Kind::Line,
            Mode::VisualBlock => Kind::Block,
            _ => Kind::Char,
        }
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
        let indent = view.doc.line_indent(line);

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
        let value = self.register_value(register);
        if value.is_empty() {
            self.message = "nothing to put".into();
            return;
        }
        let text = value.text.repeat(count);

        if value.is_block() {
            self.put_block(&value, after);
            return;
        }
        if value.is_linewise() {
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
        let indent = view.doc.line_indent(line);
        let start = view.doc.line_to_char(line);
        let cursor = start + indent.chars().count();
        view.edit_at(start, 0, &format!("{indent}\n"), Some(cursor));
        self.set_mode(Mode::Insert);
    }
}


/// The closing half of a pair: a bracket's closer, or a quote itself.
fn closer_of(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '"' | '\'' | '`' => c,
        _ => return None,
    })
}

fn is_bracket(c: char) -> bool {
    matches!(c, '(' | '[' | '{')
}

fn is_closer(c: char) -> bool {
    matches!(c, ')' | ']' | '}')
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}

/// What a leader key already does, for the ones that are not yours to map.
/// The leader is the space left for you, and these are the few corners of it
/// jack has already furnished.
fn built_in_leader(key: char) -> Option<&'static str> {
    Some(match key {
        'b' => "the buffer picker",
        'f' => "the file picker",
        's' => "the search picker",
        'd' => "the symbol picker",
        'l' => "the line picker",
        'S' => "the project symbol picker",
        'h' => "what this change was",
        'B' => "who changed this line",
        'e' => "the diagnostics picker",
        '?' => "the help picker",
        'n' => "line numbers",
        'x' => "close this buffer",
        _ => return None,
    })
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

    /// Two files open, and a grep hit taken in one of them.
    fn two_files(name: &str) -> (PathBuf, PathBuf, Editor) {
        let dir = std::env::temp_dir().join(format!("jack_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
        std::fs::write(&a, "1\n2\n3\n4\n5\nneedle\n7\n").unwrap();
        std::fs::write(&b, "x\ny\n").unwrap();
        let editor = Editor::open(&[a.clone(), b.clone()]).unwrap();
        (a, b, editor)
    }

    #[test]
    fn a_hit_lands_in_the_window_that_already_has_the_file() {
        let (a, _, mut e) = two_files("useopen");
        // Window 0 reading a.txt at line 2; window 1 beside it, on b.txt.
        e.switch_to(0);
        e.goto_line(1);
        e.split_window(true, None);
        e.switch_to(1);
        assert_eq!((e.windows_open(), e.focus()), (2, 1));

        let hit = Choice { id: 6, target: a.to_string_lossy().into() };
        e.picker = Some(Picker::new(Source::Grep, Vec::new()));
        e.picker_outcome(Outcome::Confirm(Source::Grep, hit, Open::Here));

        // The window that had a.txt is the one that went to the hit, and
        // b.txt is still on screen rather than pushed out by a second a.txt.
        assert_eq!(e.focus(), 0, "focus moved to the window that had it");
        assert_eq!(e.cursor_coords(), (5, 0));
        let (other, sel, _, _) = e.window_state(1);
        assert!(other.doc.path.as_deref().unwrap().ends_with("b.txt"));
        assert_eq!(other.doc.coords(sel.head), (0, 0), "left where it was");
        // And the jump list comes back out of it, into that other window.
        e.jump_back();
        assert_eq!(e.focus(), 0);
    }

    #[test]
    fn a_split_asked_for_is_a_split_even_when_a_window_has_the_file() {
        let (a, _, mut e) = two_files("useopen_split");
        e.switch_to(0);
        e.split_window(true, None);
        e.switch_to(1);
        let hit = Choice { id: 6, target: a.to_string_lossy().into() };
        e.picker = Some(Picker::new(Source::Grep, Vec::new()));
        e.picker_outcome(Outcome::Confirm(Source::Grep, hit, Open::Beside));
        assert_eq!(e.windows_open(), 3, "^v asked for a window and gets one");
        assert_eq!(e.cursor_coords(), (5, 0));
    }

    /// A view built the way opening a file builds one, so the indentation is
    /// read from the text rather than left at the default.
    fn opened(text: &str) -> Editor {
        let mut doc = Document::scratch();
        doc.text = Rope::from_str(text);
        Editor::with_views(vec![View::new(doc)])
    }

    #[test]
    fn a_file_indented_with_spaces_is_edited_with_spaces() {
        // The default is tabs, and this file says otherwise: four spaces.
        let mut e = opened("def f():\n    a = 1\n    b = 2\n");
        assert!(e.indent.tabs, "the default has not moved");
        assert!(!e.indent().tabs, "the file won");
        assert_eq!(e.indent().width, 4);

        // Which is what an indent actually puts there - the bug that started
        // this: a tab in a file of spaces is an error in Python, not a style.
        e.goto_line(2);
        e.shift_count(true, 1);
        assert_eq!(e.view().doc.line_str(2).to_string(), "        b = 2");
        assert!(!e.view().doc.line_str(2).contains('\t'));
    }

    #[test]
    fn a_file_of_tabs_keeps_its_tabs_whatever_the_config_says() {
        let mut e = opened("fn a() {\n\tlet x = 1;\n\tlet y = 2;\n}\n");
        e.run_command("set expandtab");
        // A `:set` typed is meant, and takes this buffer with it.
        assert!(!e.indent().tabs);

        let mut e = opened("fn a() {\n\tlet x = 1;\n\tlet y = 2;\n}\n");
        assert!(e.indent().tabs);
        e.goto_line(2);
        e.shift_count(true, 1);
        assert_eq!(e.view().doc.line_str(2).to_string(), "\t\tlet y = 2;");
    }

    #[test]
    fn a_file_with_no_indentation_leaves_the_default_alone() {
        let mut e = opened("one\ntwo\n");
        assert!(e.view().indent.is_none());
        assert_eq!(e.indent().tabs, e.indent.tabs);

        // And the report says where the numbers came from.
        e.run_command("set");
        assert!(!e.message.contains("read from the file"), "{}", e.message);
        let mut e = opened("def f():\n    a = 1\n");
        e.run_command("set");
        assert!(e.message.contains("read from the file"), "{}", e.message);
    }

    #[test]
    fn a_config_file_sets_the_default_but_does_not_overrule_the_file() {
        // The generated config spells out every setting, `set noexpandtab`
        // among them. Reading it must not throw away what the open files said
        // about themselves, or detection would never survive startup.
        let mut e = opened("def f():\n    a = 1\n");
        e.apply_config("set noexpandtab\nset shiftwidth=8\n");
        assert!(e.indent.tabs, "the default moved");
        assert!(!e.indent().tabs, "the file still wins");
        assert_eq!(e.indent().width, 4);

        // The same words typed are meant, and take the buffer with them.
        e.run_command("set noexpandtab");
        assert!(e.indent().tabs);
        assert_eq!(e.indent().width, 8, "and the config's width now applies");
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

    /// Typing, as one command: the keys open an undo group around an insert,
    /// and these tests are below the keys.
    fn type_str(e: &mut Editor, text: &str) {
        e.begin_undo_group();
        for ch in text.chars() {
            e.insert(&ch.to_string());
        }
        e.end_undo_group();
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
    fn a_paste_keeps_its_own_shape_and_is_one_undo_step() {
        // Typed a character at a time, the second line would be indented by
        // the grammar and land under the first. Pasted, it arrives as written.
        let mut e = editor("");
        e.set_mode(Mode::Insert);
        e.paste("fn main() {\n    let x = 1;\n}\n");
        assert_eq!(e.view().doc.text.to_string(), "fn main() {\n    let x = 1;\n}\n");

        // One step, however many lines it was.
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "");

        // Carriage returns are the terminal's, not the text's.
        e.paste("one\r\ntwo\r");
        assert_eq!(e.view().doc.text.to_string(), "one\ntwo\n");
    }

    #[test]
    fn a_paste_in_normal_mode_goes_in_beside_the_cursor() {
        // Whole lines - it ends in a newline - go on a line of their own,
        // below the cursor's, the way `p` puts them.
        let mut e = editor("one\ntwo\n");
        e.paste("new\n");
        assert_eq!(e.view().doc.text.to_string(), "one\nnew\ntwo\n");

        // Anything else goes in the line, after the cursor.
        let mut e = editor("one\n");
        e.paste("XY");
        assert_eq!(e.view().doc.text.to_string(), "oXYne\n");

        // And none of it is read as commands: a pasted `dd` is two letters.
        let mut e = editor("one\ntwo\n");
        e.paste("dd");
        assert_eq!(e.view().doc.text.to_string(), "oddne\ntwo\n");
    }

    #[test]
    fn a_paste_into_a_prompt_is_one_line_of_it() {
        let mut e = editor("alpha\n");
        e.open_command();
        e.paste("w foo.txt\nand more");
        assert_eq!(e.prompt.as_ref().expect("a prompt").input, "w foo.txt");
        // The buffer is untouched: the prompt owns the paste.
        assert_eq!(e.view().doc.text.to_string(), "alpha\n");
    }

    #[test]
    fn a_paste_into_a_picker_filters_by_it() {
        let mut e = editor("");
        e.open_help_picker();
        e.paste("undo");
        let picker = e.picker.as_ref().expect("a picker");
        assert!(!picker.matches().is_empty(), "the help list mentions undo");
        assert!(picker.prompt_text().ends_with("undo"), "{}", picker.prompt_text());
    }

    #[test]
    fn one_command_is_one_undo_step_however_it_is_typed() {
        // A newline inside an insert does not end the step: the command does.
        let mut e = editor("");
        e.begin_undo_group();
        for ch in "ab".chars() {
            e.insert(&ch.to_string());
        }
        e.insert_newline();
        for ch in "cd".chars() {
            e.insert(&ch.to_string());
        }
        e.end_undo_group();
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "");

        // And two commands are two steps, however alike their edits look.
        let mut e = editor("");
        type_str(&mut e, "ab");
        type_str(&mut e, "cd");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "ab");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "");
    }

    #[test]
    fn a_run_of_backspaces_is_one_undo_step() {
        let mut e = editor("abcdef");
        e.move_cursor(Move::FileEnd, false);
        e.begin_undo_group();
        e.delete_backward();
        e.delete_backward();
        e.delete_backward();
        e.end_undo_group();
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
    fn closing_a_buffer_moves_its_windows_and_jumps_along() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");
        let three = write_file(&dir, "three.txt", "three\n");
        let mut e = Editor::open(&[one.clone(), two, three.clone()]).unwrap();
        e.set_viewport(80, 20);
        e.switch_to(2);
        e.push_jump();
        e.switch_to(1);
        // Two windows on two.txt, and one on three.txt.
        e.split_window(true, None);
        e.split_window(true, Some(three.to_str().unwrap()));
        e.focus_window(0);

        e.run_command("bd");
        assert_eq!(e.message, "closed two.txt");
        let names: Vec<&str> = e.views().iter().map(|v| v.doc.display_name()).collect();
        assert_eq!(names, ["one.txt", "three.txt"]);
        let shown: Vec<usize> = e.windows.iter().map(|w| w.view).collect();
        assert_eq!(shown, [0, 0, 1], "both of its windows went to the buffer before it");
        assert_eq!(e.view().doc.path.as_ref(), Some(&one));

        // The jump from three.txt still lands in three.txt.
        e.jump_back();
        assert_eq!(e.view().doc.path.as_ref(), Some(&three));
    }

    #[test]
    fn a_buffer_with_changes_is_only_closed_when_forced() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let mut e = Editor::open(&[one]).unwrap();
        type_str(&mut e, "x");
        e.close_buffer(false);
        assert!(e.message.contains("unsaved changes"), "{}", e.message);
        assert_eq!(e.views().len(), 1);
        assert_eq!(e.view().doc.display_name(), "one.txt");

        // The last buffer closed leaves somewhere to stand.
        e.run_command("bd!");
        assert_eq!(e.views().len(), 1);
        assert!(e.view().is_empty_scratch());
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

    /// A key with no modifiers, for driving a prompt.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn every_command_the_completion_offers_is_a_command() {
        // The two lists have to agree: one is what `tab` offers, the other is
        // what `:` runs.
        for command in crate::command::COMMANDS {
            let mut e = Editor::scratch();
            e.run_command(command.name);
            assert!(
                !e.message.starts_with("not a command"),
                "{}: {}",
                command.name,
                e.message
            );
        }
    }

    #[test]
    fn the_generated_config_leaves_a_fresh_editor_exactly_as_it_was() {
        // The point of the file: every line in it is what the editor already
        // does, so a config written and not edited changes nothing. It is also
        // the check that no default in the table has drifted from the code.
        let mut untouched = Editor::scratch();
        untouched.run_command("set");
        let before = std::mem::take(&mut untouched.message);

        let mut e = Editor::scratch();
        e.apply_config(&crate::command::default_config());
        assert_eq!(e.message, "", "the file runs clean");
        e.run_command("set");
        assert_eq!(e.message, before);
    }

    #[test]
    fn every_setting_in_the_table_is_reported_by_bare_set() {
        let mut e = Editor::scratch();
        e.run_command("set");
        for setting in crate::command::SETTINGS {
            assert!(
                e.message.contains(&format!("{}=", setting.name)),
                "{} is in the report: {}",
                setting.name,
                e.message
            );
        }
    }

    #[test]
    fn every_option_the_completion_offers_is_an_option() {
        for option in crate::command::option_names() {
            let mut e = Editor::scratch();
            // The ones that take a value are offered with their `=` on.
            let written = match option.ends_with('=') {
                true => format!("set {option}4"),
                false => format!("set {option}"),
            };
            e.run_command(&written);
            assert!(
                !e.message.starts_with("not an option"),
                "{written}: {}",
                e.message
            );
        }
    }

    #[test]
    fn tab_completes_the_command_line_and_cycles() {
        let mut e = Editor::scratch();
        e.open_command();
        for c in "w".chars() {
            e.prompt_input(key(KeyCode::Char(c)));
        }
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "write");
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "wq");
        e.prompt_input(key(KeyCode::Tab));
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "wqall");
        // Round the end, and back the other way.
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "write");
        e.prompt_input(key(KeyCode::BackTab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "wqall");
    }

    #[test]
    fn typing_after_a_completion_starts_a_new_one() {
        let mut e = Editor::scratch();
        e.open_command();
        for c in "set auto".chars() {
            e.prompt_input(key(KeyCode::Char(c)));
        }
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "set autocomplete=");
        // The list on screen has to be a list for what is written.
        e.prompt_input(key(KeyCode::Char('3')));
        assert!(e.prompt.as_ref().unwrap().completion.is_none());
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "set autocomplete=3");
    }

    #[test]
    fn a_path_completes_from_the_directory_it_names() {
        let dir = tempdir();
        write_file(&dir, "alpha.txt", "a\n");
        write_file(&dir, "beta.txt", "b\n");

        let mut e = Editor::scratch();
        e.open_command();
        let typed = format!("edit {}/a", dir.display());
        for c in typed.chars() {
            e.prompt_input(key(KeyCode::Char(c)));
        }
        e.prompt_input(key(KeyCode::Tab));

        // The directory already typed stays; only the last segment is
        // replaced.
        assert_eq!(
            e.prompt.as_ref().unwrap().input,
            format!("edit {}/alpha.txt", dir.display())
        );
    }

    #[test]
    fn a_search_has_nothing_to_complete() {
        let mut e = Editor::scratch();
        e.open_search(false);
        e.prompt_input(key(KeyCode::Char('w')));
        e.prompt_input(key(KeyCode::Tab));
        assert_eq!(e.prompt.as_ref().unwrap().input, "w");
    }

    #[test]
    fn the_dog_follows_the_cursor_rather_than_the_keyboard() {
        let mut e = editor("one two\nthree four\n");
        let start = e.cursor_mark();

        // Anything that moves the cursor is a step.
        e.move_cursor(Move::Down, false);
        assert_ne!(e.cursor_mark(), start, "the cursor moved");

        // Anything that does not is not: a command that writes a message, a
        // mode change, or a motion with nowhere left to go.
        let sitting = e.cursor_mark();
        e.set_mode(Mode::Insert);
        e.set_mode(Mode::Normal);
        e.message = "something happened".into();
        assert_eq!(e.cursor_mark(), sitting);

        e.goto_line(0);
        e.move_cursor(Move::LineStart, false);
        let at_start = e.cursor_mark();
        e.move_cursor(Move::Left, false);
        assert_eq!(e.cursor_mark(), at_start, "no room left to move");
    }

    #[test]
    fn substitute_changes_the_line_it_is_on_and_says_what_it_did() {
        let mut e = editor("one two one\nthree one\n");
        e.run_command("s/one/X/");
        assert_eq!(e.view().doc.text.to_string(), "X two one\nthree one\n");
        assert!(e.message.contains('1'), "{}", e.message);

        // `g` takes every match on the line, not just the first.
        let mut e = editor("one two one\nthree one\n");
        e.run_command("s/one/X/g");
        assert_eq!(e.view().doc.text.to_string(), "X two X\nthree one\n");
    }

    #[test]
    fn a_range_says_which_lines() {
        let mut e = editor("a\na\na\na\n");
        e.run_command("2,3s/a/b/");
        assert_eq!(e.view().doc.text.to_string(), "a\nb\nb\na\n");

        let mut e = editor("a\na\na\n");
        e.run_command("%s/a/b/");
        assert_eq!(e.view().doc.text.to_string(), "b\nb\nb\n");

        // `.,$` from the second line down.
        let mut e = editor("a\na\na\n");
        e.goto_line(1);
        e.run_command(".,$s/a/b/");
        assert_eq!(e.view().doc.text.to_string(), "a\nb\nb\n");

        // A line number past the end is the end, not a panic.
        let mut e = editor("a\na\n");
        e.run_command("1,99s/a/b/");
        assert_eq!(e.view().doc.text.to_string(), "b\nb\n");
    }

    #[test]
    fn the_whole_substitute_is_one_undo_step() {
        let mut e = editor("a\na\na\n");
        e.run_command("%s/a/b/");
        assert_eq!(e.view().doc.text.to_string(), "b\nb\nb\n");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "a\na\na\n");
    }

    #[test]
    fn capture_groups_and_the_whole_match_can_be_put_back() {
        let mut e = editor("alpha beta\n");
        e.run_command(r"s/(\w+) (\w+)/\2 \1/");
        assert_eq!(e.view().doc.text.to_string(), "beta alpha\n");

        let mut e = editor("total 42\n");
        e.run_command("s/[0-9]+/[&]/");
        assert_eq!(e.view().doc.text.to_string(), "total [42]\n");
    }

    #[test]
    fn an_empty_pattern_means_the_last_search() {
        let mut e = editor("one two\none three\n");
        e.search.set_pattern("one").expect("a pattern");
        e.run_command("%s//X/");
        assert_eq!(e.view().doc.text.to_string(), "X two\nX three\n");

        // And with nothing searched for either, it says so and changes
        // nothing.
        let mut e = editor("one\n");
        e.run_command("%s//X/");
        assert_eq!(e.view().doc.text.to_string(), "one\n");
        assert!(!e.message.is_empty());
    }

    #[test]
    fn case_follows_the_pattern_unless_a_flag_says_otherwise() {
        // All lower case matches either case, as `/` does.
        let mut e = editor("Foo foo\n");
        e.run_command("s/foo/x/g");
        assert_eq!(e.view().doc.text.to_string(), "x x\n");

        // A capital in the pattern means it.
        let mut e = editor("Foo foo\n");
        e.run_command("s/Foo/x/g");
        assert_eq!(e.view().doc.text.to_string(), "x foo\n");

        // `I` insists even on a lower-case pattern.
        let mut e = editor("Foo foo\n");
        e.run_command("s/foo/x/gI");
        assert_eq!(e.view().doc.text.to_string(), "Foo x\n");
    }

    #[test]
    fn n_counts_without_changing_anything() {
        let mut e = editor("one one\none\n");
        e.run_command("%s/one/x/gn");
        assert_eq!(e.view().doc.text.to_string(), "one one\none\n");
        assert!(e.message.starts_with('3'), "{}", e.message);
    }

    #[test]
    fn a_pattern_that_is_not_there_changes_nothing_and_says_so() {
        let mut e = editor("one\n");
        e.run_command("%s/zebra/x/");
        assert_eq!(e.view().doc.text.to_string(), "one\n");
        assert!(e.message.contains("zebra"), "{}", e.message);
        assert!(!e.is_modified(), "nothing to save");

        // A pattern that will not compile says that instead.
        e.run_command("%s/(unclosed/x/");
        assert!(e.message.contains("not a pattern"), "{}", e.message);
    }

    #[test]
    fn colon_over_a_selection_writes_the_range_in() {
        let mut e = editor("a\na\na\na\n");
        e.set_mode(Mode::VisualLine);
        e.move_cursor(Move::Down, true);
        e.open_command_over_selection();
        assert_eq!(e.prompt.as_ref().expect("a prompt").input, "'<,'>");
        assert_eq!(e.mode, Mode::Normal);

        e.run_command("'<,'>s/a/b/");
        assert_eq!(e.view().doc.text.to_string(), "b\nb\na\na\n");
    }

    #[test]
    fn a_completed_command_still_runs() {
        let mut e = Editor::scratch();
        e.open_command();
        for c in "set numb".chars() {
            e.prompt_input(key(KeyCode::Char(c)));
        }
        e.prompt_input(key(KeyCode::Tab));
        e.prompt_input(key(KeyCode::Enter));
        assert!(e.prompt.is_none());
        assert_eq!(e.numbers.name(), "absolute");
    }

    #[test]
    fn the_first_file_replaces_the_buffer_the_editor_started_with() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");
        let two = write_file(&dir, "two.txt", "two\n");

        let mut e = Editor::scratch();
        e.open_file(&one).unwrap();
        assert_eq!(e.views().len(), 1, "no dead [scratch] tab beside it");
        assert_eq!(e.view().doc.text.to_string(), "one\n");

        // Only the empty one, though: a second file is a second buffer.
        e.open_file(&two).unwrap();
        assert_eq!(e.views().len(), 2);
    }

    #[test]
    fn a_scratch_buffer_that_has_been_typed_into_is_kept() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "one\n");

        let mut e = Editor::scratch();
        e.insert("notes");
        e.open_file(&one).unwrap();
        assert_eq!(e.views().len(), 2, "what was typed is still open");
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
            "jack_buffers_{}_{:?}",
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
    fn the_symbol_picker_lists_what_the_buffer_defines() {
        let dir = tempdir();
        let path = write_file(
            &dir,
            "lib.rs",
            "struct Bag;\n\nimpl Bag {\n    fn take(&self) {}\n}\n\nfn main() {\n    let x = 1;\n}\n",
        );

        let mut e = Editor::open(&[path]).unwrap();
        e.open_symbol_picker();

        let picker = e.picker.as_ref().unwrap();
        let rows: Vec<(&str, &str)> = picker
            .matches()
            .iter()
            .map(|m| {
                let item = picker.item(m);
                (item.text.as_str(), item.detail.as_str())
            })
            .collect();
        // File order, not name order, and `let x` is a binding rather than a
        // definition the file offers - it is not on the list.
        assert_eq!(rows, [("Bag", "type  1"), ("take", "method  4"), ("main", "fn  7")]);
    }

    #[test]
    fn choosing_a_symbol_jumps_to_it_and_remembers_where_you_were() {
        let dir = tempdir();
        let path = write_file(&dir, "lib.rs", "fn one() {}\n\nfn two() {}\n\nfn three() {}\n");

        let mut e = Editor::open(&[path]).unwrap();
        e.open_symbol_picker();
        for c in "three".chars() {
            e.picker_input(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(e.picker.is_none());
        assert_eq!(e.cursor_coords().0, 4);
        // And `^o` goes back to the top, where the picker was opened from.
        e.jump_back();
        assert_eq!(e.cursor_coords().0, 0);
    }

    #[test]
    fn the_symbol_picker_says_so_when_there_is_nothing_to_list() {
        let dir = tempdir();
        let empty = write_file(&dir, "empty.rs", "let it be\n");
        let plain = write_file(&dir, "notes.txt", "fn not_really() {}\n");

        let mut e = Editor::open(&[empty, plain]).unwrap();
        e.open_symbol_picker();
        assert!(e.picker.is_none());
        assert_eq!(e.message, "nothing defined in this buffer");

        e.next_view();
        e.open_symbol_picker();
        assert!(e.picker.is_none());
        assert_eq!(e.message, "no grammar for this file");
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
        // On the prompt row, past " buffer> " - the leading space included,
        // which is what this used to be one short of.
        assert_eq!(e.cursor_screen(), (9, 10));
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
    fn the_leader_is_yours_to_map() {
        let mut e = Editor::scratch();
        e.run_command("map g !lazygit");
        assert_eq!(e.leader_command('g').as_deref(), Some("!lazygit"));
        assert_eq!(e.message, "", "silent, the way :set is");

        // Written with the colon, as it would be typed: the same thing.
        e.run_command("map t :!cargo test");
        assert_eq!(e.leader_command('t').as_deref(), Some("!cargo test"));

        e.run_command("map");
        assert_eq!(e.message, "<space>g !lazygit, <space>t !cargo test");

        e.run_command("unmap g");
        assert_eq!(e.leader_command('g'), None);
        e.run_command("unmap g");
        assert_eq!(e.message, "g is not mapped");
    }

    #[test]
    fn a_leader_key_jack_already_uses_is_not_yours() {
        let mut e = Editor::scratch();
        e.run_command("map f !lazygit");
        assert_eq!(e.message, "<space>f is the file picker");
        assert_eq!(e.leader_command('f'), None);

        e.run_command("map gg !lazygit");
        assert!(e.message.starts_with("one key after the leader"), "{}", e.message);
        e.run_command("map g");
        assert_eq!(e.message, "nothing to map it to");
    }

    #[test]
    fn a_config_file_can_map_the_leader_and_the_map_can_then_run_things() {
        let mut e = Editor::scratch();
        // As a config file runs it: a message would read as a complaint about
        // the line and stop the file there.
        e.from_config = true;
        e.run_command("map g !lazygit");
        assert_eq!(e.message, "");
        e.from_config = false;

        // And pressing it is what leaves the command for the run loop.
        assert_eq!(e.leader_command('g').as_deref(), Some("!lazygit"));
        e.run_command(&e.leader_command('g').expect("mapped"));
        assert_eq!(e.shell.as_deref(), Some("lazygit"));
    }

    #[test]
    fn a_bang_leaves_the_command_for_the_run_loop() {
        let mut e = Editor::scratch();
        e.run_command("!git status --short");
        assert_eq!(e.shell.as_deref(), Some("git status --short"));

        // `:sh` is the same thing with nothing to run: the shell itself.
        e.shell = None;
        e.run_command("sh");
        assert_eq!(e.shell.as_deref(), Some(""));
    }

    #[test]
    fn suspending_leaves_the_signal_for_the_run_loop() {
        // The editor does not own the terminal, so `^z` cannot stop anything
        // itself: it leaves a flag, the same way `:!` leaves a command.
        let mut e = Editor::scratch();
        assert!(!e.suspend);
        e.suspend();
        assert!(e.suspend);

        e.suspend = false;
        e.run_command("stop");
        assert!(e.suspend, "`:stop` and `:sus` are `:suspend`");

        e.suspend = false;
        e.from_config = true;
        e.run_command("suspend");
        assert!(!e.suspend, "and a config file has no terminal to hand back");
        assert_eq!(e.message, "not from a config file");
    }

    #[test]
    fn a_config_file_does_not_get_to_run_programs() {
        let mut e = Editor::scratch();
        e.from_config = true;
        e.run_command("!rm -rf /");
        assert_eq!(e.shell, None);
        assert_eq!(e.message, "not from a config file");
    }

    #[test]
    fn what_another_program_changed_is_read_back_in() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "before\n");
        let two = write_file(&dir, "two.txt", "mine\n");
        let mut e = Editor::open(&[one.clone(), two.clone()]).unwrap();

        // The second buffer has unsaved changes; the first has none.
        e.switch_to(1);
        e.set_mode(Mode::Insert);
        e.insert("typed ");
        e.set_mode(Mode::Normal);

        // Something else - a checkout, a formatter - rewrites both files.
        std::fs::write(&one, "after\n").unwrap();
        std::fs::write(&two, "theirs\n").unwrap();
        e.reload_changed_files();

        assert_eq!(e.views()[0].doc.text.to_string(), "after\n", "nothing to lose");
        assert_eq!(e.views()[1].doc.text.to_string(), "typed mine\n", "not overwritten");
        assert!(e.message.contains("one.txt reloaded"), "{}", e.message);
        assert!(e.message.contains("two.txt changed on disk"), "{}", e.message);
    }

    #[test]
    fn the_diagnostics_picker_lists_this_buffer_first_and_goes_to_the_place() {
        let dir = tempdir();
        let one = write_file(&dir, "one.rs", "let a = 1;\nlet b = 2;\n");
        let two = write_file(&dir, "two.rs", "fn f() {}\n");
        let mut e = Editor::open(&[one.clone(), two.clone()]).unwrap();
        let problem = |start: usize, severity: crate::lsp::Severity, message: &str| view::Diagnostic {
            start,
            end: start + 1,
            severity,
            message: message.into(),
            raw: serde_json::Value::Null,
        };
        e.views[0].diagnostics = vec![problem(15, crate::lsp::Severity::Warning, "unused b\nhelp: remove it")];
        e.views[1].diagnostics = vec![problem(3, crate::lsp::Severity::Error, "f is never called")];
        e.switch_to(1);

        e.open_diagnostics_picker();
        let picker = e.picker.as_ref().expect("a picker");
        let rows: Vec<(String, String)> = picker
            .matches()
            .iter()
            .map(|m| (picker.item(m).text.clone(), picker.item(m).detail.clone()))
            .collect();
        assert_eq!(
            rows,
            [
                // The buffer you are in first, the first line of each message.
                ("error: f is never called".to_string(), "two.rs:1".to_string()),
                ("warning: unused b".to_string(), "one.rs:2".to_string()),
            ]
        );

        // Typing narrows by severity as well as by what it says.
        e.paste("warn");
        e.picker_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(e.current_index(), 0, "over to the other buffer");
        assert_eq!(e.view().sel.head, 15, "on the character it is about");
    }

    #[test]
    fn no_diagnostics_is_said_rather_than_an_empty_list() {
        let mut e = Editor::scratch();
        e.open_diagnostics_picker();
        assert!(e.picker.is_none());
        assert!(e.message.starts_with("no diagnostics"), "{}", e.message);
    }

    #[test]
    fn a_file_closed_and_opened_again_opens_where_it_was_left() {
        let dir = tempdir();
        let one = write_file(&dir, "one.txt", "a\nb\ncharlie\nd\n");
        let two = write_file(&dir, "two.txt", "other\n");
        let mut e = Editor::open(&[one.clone(), two.clone()]).unwrap();

        e.goto_line(2);
        e.move_cursor(Move::Right, false);
        e.move_cursor(Move::Right, false);
        assert_eq!(e.cursor_coords(), (2, 2));
        e.close_buffer(false);
        assert!(e.views().iter().all(|view| view.doc.path.as_deref() != Some(one.as_path())));

        e.open_file(&one).unwrap();
        assert_eq!(e.cursor_coords(), (2, 2));
        assert!(e.view().centre, "and put in the middle of the screen");
        e.scroll_to_cursor();
        assert!(!e.view().centre, "once");
    }

    #[test]
    fn a_remembered_place_past_the_end_of_a_shorter_file_is_its_end() {
        let dir = tempdir();
        let path = write_file(&dir, "shrunk.txt", "only line\n");
        let mut positions = crate::positions::Positions::load(None);
        positions.set(&path, 40, 12);
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_positions(positions);
        // The last line is the empty one after the newline; the column
        // can only be where that line has a place for it.
        assert_eq!(e.cursor_coords(), (1, 0));

        let mut positions = crate::positions::Positions::load(None);
        positions.set(&path, 0, 40);
        e.set_positions(positions);
        assert_eq!(e.cursor_coords(), (0, 8), "on the last character, not past it");
    }

    #[test]
    fn undo_reaches_back_past_a_restart() {
        let dir = tempdir();
        let path = write_file(&dir, "kept.txt", "first\n");
        let store = dir.join("undo");

        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_undo_dir(Some(store.clone()));
        e.view_mut().edit_at(5, 0, " second", Some(0));
        e.view_mut().edit_at(0, 0, "> ", Some(0));
        e.save();
        assert_eq!(e.message, "wrote kept.txt");
        drop(e);

        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_undo_dir(Some(store.clone()));
        assert!(!e.view().is_modified(), "the history ends where the file is");
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "first second\n");
        assert!(e.view().is_modified());
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "first\n");
        e.redo();
        e.redo();
        assert!(!e.view().is_modified(), "and redo comes back to the saved text");
    }

    #[test]
    fn a_history_about_other_text_is_not_used() {
        let dir = tempdir();
        let path = write_file(&dir, "moved.txt", "one\n");
        let store = dir.join("undo");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_undo_dir(Some(store.clone()));
        e.view_mut().edit_at(3, 0, " two", Some(0));
        e.save();
        drop(e);

        // Something else wrote the file in between.
        std::fs::write(&path, "one two!\n").unwrap();
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_undo_dir(Some(store.clone()));
        assert_eq!(e.view().history().depth(), 0);

        // And with the setting off, nothing is written or read.
        let other = write_file(&dir, "off.txt", "x\n");
        let mut e = Editor::open(std::slice::from_ref(&other)).unwrap();
        e.undofile = false;
        e.set_undo_dir(Some(store.clone()));
        e.view_mut().edit_at(0, 0, "y", Some(0));
        e.save();
        e.undofile = true;
        let mut e = Editor::open(std::slice::from_ref(&other)).unwrap();
        e.set_undo_dir(Some(store));
        assert_eq!(e.view().history().depth(), 0);
    }

    #[test]
    fn a_reload_is_an_edit_that_undo_takes_back() {
        let dir = tempdir();
        let path = write_file(&dir, "undo.txt", "one\ntwo\nthree\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.goto_line(2);

        std::fs::write(&path, "one\nTWO, changed\nthree\n").unwrap();
        e.reload_changed_files();
        assert_eq!(e.view().doc.text.to_string(), "one\nTWO, changed\nthree\n");
        assert!(!e.is_modified(), "it is what the disk says");
        assert_eq!(e.cursor_coords().0, 2, "a change above does not move the cursor off its line");

        // The history is about this text, reload and all: `u` takes the
        // reload back rather than replaying old edits over a new file.
        e.undo();
        assert_eq!(e.view().doc.text.to_string(), "one\ntwo\nthree\n");
        assert!(e.is_modified(), "and that is no longer what the disk says");
    }

    #[test]
    fn a_conflict_on_disk_is_said_once_per_change() {
        let dir = tempdir();
        let path = write_file(&dir, "busy.txt", "mine\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.set_mode(Mode::Insert);
        e.insert("x");
        e.set_mode(Mode::Normal);

        std::fs::write(&path, "theirs\n").unwrap();
        e.reload_changed_files();
        assert!(e.message.contains("changed on disk"), "{}", e.message);

        // Looked at again a second later, with nothing new: quiet.
        e.message.clear();
        e.reload_changed_files();
        assert_eq!(e.message, "");

        // Changed again, and that is news.
        std::fs::write(&path, "theirs, again\n").unwrap();
        e.reload_changed_files();
        assert!(e.message.contains("changed on disk"), "{}", e.message);
        // And saving still refuses to overwrite it without being told.
        e.save();
        assert!(e.message.contains(":w! to overwrite"), "{}", e.message);
    }

    #[test]
    fn the_disk_is_watched_but_not_while_typing_and_not_every_frame() {
        let dir = tempdir();
        let path = write_file(&dir, "watched.txt", "first\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();

        std::fs::write(&path, "second\n").unwrap();
        e.set_mode(Mode::Insert);
        e.watch_disk();
        assert_eq!(e.view().doc.text.to_string(), "first\n", "not in the middle of an insert");

        e.set_mode(Mode::Normal);
        e.watch_disk();
        assert_eq!(e.view().doc.text.to_string(), "second\n");

        // Within the second: not looked at.
        std::fs::write(&path, "third one\n").unwrap();
        e.watch_disk();
        assert_eq!(e.view().doc.text.to_string(), "second\n");

        // Coming back to the terminal is worth a look at once.
        e.focus_gained();
        assert_eq!(e.view().doc.text.to_string(), "third one\n");
    }

    #[test]
    fn nothing_changed_underneath_is_nothing_to_say() {
        let dir = tempdir();
        let path = write_file(&dir, "quiet.txt", "as it was\n");
        let mut e = Editor::open(std::slice::from_ref(&path)).unwrap();
        e.reload_changed_files();
        assert_eq!(e.message, "");
        assert_eq!(e.view().doc.text.to_string(), "as it was\n");
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

