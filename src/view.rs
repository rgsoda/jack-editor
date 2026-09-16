use anyhow::Result;
use std::collections::HashMap;
use std::ops::Range;
use unicode_segmentation::GraphemeCursor;
use unicode_width::UnicodeWidthChar;

use crate::buffer::Document;
use crate::comment::{self, Marker, Toggled};
use crate::history::{Change, History, Transaction};
use crate::lsp::Severity;
use crate::search::Search;
use crate::stream::Sign;
use crate::syntax::{Highlights, Syntax, language_for_path};
use crate::theme::Theme;

pub const TAB_WIDTH: usize = 4;

/// What one step of indentation is: how many columns, and whether to spend
/// them on tabs or spaces.
#[derive(Clone, Copy, Debug)]
pub struct Indent {
    pub width: usize,
    pub tabs: bool,
}

impl Indent {
    /// The whitespace that reaches `column`. With tabs, as many whole tab
    /// stops as fit and spaces for the remainder - which is what a mixture of
    /// tab width and shift width leaves you with either way.
    pub fn make(&self, column: usize) -> String {
        match self.tabs {
            true => "\t".repeat(column / TAB_WIDTH) + &" ".repeat(column % TAB_WIDTH),
            false => " ".repeat(column),
        }
    }

    /// What `tab` inserts at display column `at`: enough to reach the next
    /// stop, or a literal tab.
    pub fn tab(&self, at: usize) -> String {
        match self.tabs {
            true => "\t".to_string(),
            false => " ".repeat(self.width - (at % self.width)),
        }
    }
}

/// Byte length of the first `chars` chars of `text`.
fn leading_bytes(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(i, _)| i)
}
/// Rows kept between the cursor and the top/bottom edge when scrolling.
const SCROLLOFF: usize = 3;

/// A cursor plus the region it has selected. `head` is where the cursor is
/// drawn; `anchor` is where the selection started. Equal means no selection.
///
/// Every command operates on a `Selection`, so growing this into a
/// `Vec<Selection>` for multi-cursor later is mechanical rather than a rewrite.
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn point(pos: usize) -> Self {
        Selection { anchor: pos, head: pos }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The selected char range, normalized so start <= end.
    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

/// One press of `f`, `F`, `t` or `T`: which character to look for, and how to
/// stop at it. Remembered by the editor so `;` and `,` can repeat it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Find {
    pub target: char,
    /// `t` and `T`, which stop one character short of the target.
    pub till: bool,
    /// `F` and `T`, which look back along the line rather than on.
    pub backward: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    FirstNonBlank,
    LineEnd,
    WordForward,
    WordBack,
    WordEnd,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    FileStart,
    FileEnd,
    /// `{` and `}`: the blank line before or after this paragraph.
    ParagraphBack,
    ParagraphForward,
}

/// Where `zt`, `zz` and `zb` put the line the cursor is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reveal {
    Top,
    Middle,
    Bottom,
}

/// Which visible line `H`, `M` and `L` mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Top,
    Middle,
    Bottom,
}

/// One open document and everything about how it is being looked at: where the
/// cursor is, where the viewport is scrolled to, its undo history and its
/// syntax tree.
///
/// Splitting a window would mean several views over one `Document`; for now
/// there is exactly one view per open file, and `Editor` holds the list.
pub struct View {
    pub doc: Document,
    pub sel: Selection,
    /// Display column vertical movement tries to return to, so moving down
    /// through a short line and back out keeps the original column.
    goal_col: Option<usize>,
    pub scroll_top: usize,
    pub scroll_left: usize,
    history: History,
    syntax: Option<Syntax>,
    /// Per-line git signs, and the revision they were computed for.
    pub signs: HashMap<usize, Sign>,
    pub signs_revision: Option<(usize, usize)>,
    /// What this file indents with, read from the file itself when it was
    /// opened. `None` when it had nothing to say, and the configured default
    /// is what to use.
    pub indent: Option<Indent>,
    /// Every change made since another window started showing this buffer,
    /// as (position, chars removed, chars inserted), for carrying that
    /// window's cursor through edits made here. Kept only while `watched`:
    /// a buffer in one window has nothing to carry and logs nothing.
    log: Vec<(usize, usize, usize)>,
    /// The log position of `log[0]`, so marks survive the log being cleared.
    log_base: usize,
    watched: bool,
    /// What the language server last said is wrong, in char offsets, sorted.
    /// Carried through edits like any other position, so a squiggle stays on
    /// its word while you type above it and the server has not yet answered.
    pub diagnostics: Vec<Diagnostic>,
    /// Every edit ever applied, counted: how the server's copy is known to be
    /// behind. The history's depth cannot say, since undo takes it back down.
    edits: u64,
    pub lsp: Lsp,
}

/// Whether a buffer is known to a language server.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lsp {
    /// Not looked into yet.
    #[default]
    Untried,
    /// No server for it: no file, no language, none installed, or turned off.
    Without,
    /// Open in server `server`, which has seen `version` of it, made when the
    /// buffer's edit count was `synced`.
    Open { server: usize, version: i32, synced: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
    pub message: String,
}

impl View {
    pub fn new(doc: Document) -> Self {
        View {
            indent: crate::indent::detect(&doc),
            log: Vec::new(),
            log_base: 0,
            watched: false,
            diagnostics: Vec::new(),
            edits: 0,
            lsp: Lsp::Untried,
            doc,
            sel: Selection::point(0),
            goal_col: None,
            scroll_top: 0,
            scroll_left: 0,
            history: History::new(),
            syntax: None,
            signs: HashMap::new(),
            signs_revision: None,
        }
    }

    /// Set up highlighting for the document's language, if we know it. Returns
    /// a warning to show rather than failing: a broken grammar or query should
    /// never stop you editing the file.
    pub fn attach_syntax(&mut self, theme: &Theme) -> Option<String> {
        let config = language_for_path(self.doc.path.as_deref())?;
        match Syntax::new(config, &self.doc.text, theme) {
            Ok(syntax) => {
                self.syntax = Some(syntax);
                None
            }
            Err(err) => Some(format!("highlighting off: {err:#}")),
        }
    }

    /// What the grammar calls the names in a byte range, for completion. Empty
    /// when there is no grammar for this file, which is why completion also has
    /// a tier that only knows about words.
    pub fn identifiers(&self, range: Range<usize>) -> Vec<(Range<usize>, &'static str)> {
        match &self.syntax {
            Some(syntax) => syntax.identifiers(&self.doc.text, range),
            None => Vec::new(),
        }
    }

    /// Whether there is a parser for this file at all.
    pub fn has_grammar(&self) -> bool {
        self.syntax.is_some()
    }

    /// Everything this buffer defines, as (name, kind, line). Empty without a
    /// grammar, which is what the picker reports rather than pretending.
    pub fn definitions(&self) -> Vec<(String, &'static str, usize)> {
        match &self.syntax {
            Some(syntax) => syntax
                .definitions(&self.doc.text)
                .into_iter()
                .map(|(name, kind, byte)| (name, kind, self.doc.text.byte_to_line(byte)))
                .collect(),
            None => Vec::new(),
        }
    }

    /// Where `name` is defined, as a char index: `gd` with `local`, `gD`
    /// without. Three tiers, and the last one needs no grammar at all - which
    /// is why `gd` does something sensible in a file we have no parser for.
    pub fn definition(&self, name: &str, at: usize, local: bool) -> Option<usize> {
        if let Some(syntax) = &self.syntax {
            let byte = self.doc.text.char_to_byte(at);
            if let Some(found) = syntax.definition(&self.doc.text, byte, name, local) {
                return Some(self.doc.text.byte_to_char(found));
            }
        }
        self.word_before(name, at)
    }

    /// Vim's own `gd`, near enough: the nearest earlier occurrence of the
    /// word, or the first one in the file when there is nothing above. Its own
    /// `Search` rather than the editor's, because finding a definition should
    /// not change what `n` repeats.
    fn word_before(&self, name: &str, at: usize) -> Option<usize> {
        let mut search = Search::default();
        search.set_pattern(&crate::search::word_pattern(name)).ok()?;
        // The word under the cursor is not its own definition, and with no
        // grammar to say otherwise it is the only thing we can rule out.
        let here = self.word_range(at);

        let hit = search.find(&self.doc, at, true)?;
        if !hit.wrapped && !here.contains(&hit.start) {
            return Some(hit.start);
        }
        // Nothing above the cursor: the first occurrence in the file will do.
        // Searching on from the end wraps to it, which a search from zero
        // would step over.
        let first = search.find(&self.doc, self.doc.len_chars(), false)?;
        (!here.contains(&first.start)).then_some(first.start)
    }

    /// The char range of the word `at` sits in, empty when it is not in one.
    fn word_range(&self, at: usize) -> Range<usize> {
        let text = &self.doc.text;
        let mut start = at;
        while start > 0 && is_word(text.char(start - 1)) {
            start -= 1;
        }
        let mut end = at;
        while end < text.len_chars() && is_word(text.char(end)) {
            end += 1;
        }
        start..end
    }

    /// Where `f`, `F`, `t` or `T` would put the cursor: the `count`th target
    /// on this line, or one short of it for a till. `None` when the line does
    /// not hold that many.
    ///
    /// This line and no further, which is the whole character of the motion:
    /// `f` is for getting somewhere you can see.
    pub fn find_char(&self, at: usize, find: Find, count: usize) -> Option<usize> {
        let line = self.doc.char_to_line(at);
        let start = self.doc.line_to_char(line);
        let text = self.doc.line_str(line);
        let chars: Vec<char> = text.chars().collect();
        let column = at - start;

        let mut seen = 0;
        let mut found = None;
        let columns: Vec<usize> = match find.backward {
            true => (0..column.min(chars.len())).rev().collect(),
            false => (column + 1..chars.len()).collect(),
        };
        for i in columns {
            if chars[i] == find.target {
                seen += 1;
                if seen == count {
                    found = Some(i);
                    break;
                }
            }
        }

        // A till stops beside the target rather than on it. Neither step can
        // leave the line: a forward target is at least one column on, and a
        // backward one at least one column back.
        let column = match (found?, find.till, find.backward) {
            (i, true, false) => i - 1,
            (i, true, true) => i + 1,
            (i, false, _) => i,
        };
        Some(start + column)
    }

    /// Whether the cursor is somewhere completion should keep quiet. False
    /// without a grammar: with nothing to go on, we would rather offer.
    pub fn in_comment_or_string(&self, at: usize) -> bool {
        match &self.syntax {
            Some(syntax) => syntax.in_comment_or_string(self.doc.text.char_to_byte(at)),
            None => false,
        }
    }

    pub fn highlights(&self, range: Range<usize>, theme: &Theme) -> Highlights {
        match &self.syntax {
            Some(syntax) => syntax.highlights(&self.doc.text, range, theme),
            None => Highlights::none(),
        }
    }

    /// Whether there is an indent query for this file at all, as opposed to
    /// there being one that has nothing to say about a particular line.
    pub fn has_indent_rules(&self) -> bool {
        self.syntax.as_ref().is_some_and(|syntax| syntax.has_indent_rules())
    }

    /// What the grammar says this line's indentation should be, in steps.
    /// `None` when there is no indent query for the language.
    pub fn indent_level(&self, line: usize) -> Option<usize> {
        let syntax = self.syntax.as_ref()?;
        let blank = self.doc.line_indent_len(line);
        let base = self.doc.line_to_char(line);
        let at = self.doc.text.char_to_byte(base + blank);

        let start = self.doc.line_to_byte(line);
        let end = match line + 1 < self.doc.len_lines() {
            true => self.doc.line_to_byte(line + 1),
            false => self.doc.len_bytes(),
        };
        syntax.indent_level(&self.doc.text, at, start..end)
    }

    /// Put each of `targets` - (line, display column) - at that column, as one
    /// transaction. Blank lines are left alone, as vim leaves them: indenting
    /// a paragraph should not leave trailing whitespace in the gaps between
    /// its lines. Returns how many lines actually moved.
    pub fn set_indents(&mut self, targets: &[(usize, usize)], indent: Indent) -> usize {
        let mut changes = Vec::new();
        for &(line, column) in targets {
            let text = self.doc.line_str(line);
            if text.trim().is_empty() {
                continue;
            }
            let leading = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            let wanted = indent.make(column);
            if wanted == text[..leading_bytes(&text, leading)] {
                continue;
            }
            let base = self.doc.line_to_char(line);
            changes.push(Change {
                pos: base,
                removed: text.chars().take(leading).collect(),
                inserted: wanted,
            });
        }
        if changes.is_empty() {
            return 0;
        }

        let count = changes.len();
        let (line, _) = self.cursor_coords();
        let tx = Transaction::new(changes, self.sel, self.sel);
        self.apply_tx(&tx);
        // The cursor lands on the first non-blank of the line it was on, which
        // is where vim leaves it and the only column that still means the same
        // thing after the line has moved sideways.
        let base = self.doc.line_to_char(line);
        let blank = self.doc.line_indent_len(line);
        self.sel = Selection::point(base + blank.min(self.doc.line_len_chars(line)));
        self.goal_col = None;
        self.history.push(Transaction { sel_after: self.sel, ..tx });
        count
    }

    /// Indent or dedent lines `first..=last` by `levels` steps.
    pub fn shift_lines(
        &mut self,
        first: usize,
        last: usize,
        out: bool,
        levels: usize,
        indent: Indent,
    ) -> usize {
        let step = indent.width * levels;
        let targets: Vec<(usize, usize)> = (first..=last.min(self.last_line()))
            .map(|line| {
                let text = self.doc.line_str(line);
                let leading = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
                let column = display_col(&text, leading);
                let target = match out {
                    true => column + step,
                    false => column.saturating_sub(step),
                };
                (line, target)
            })
            .collect();
        self.set_indents(&targets, indent)
    }

    /// `gc`: comment out lines `first..=last`, or back in, as one transaction.
    /// The cursor goes to the first non-blank of the first line, where vim
    /// leaves it - for `gcc` that is the line it was already on.
    pub fn toggle_comments(&mut self, first: usize, last: usize, marker: Marker) -> Toggled {
        let last = last.min(self.last_line());
        let lines: Vec<String> = (first..=last).map(|line| self.doc.line_str(line).into_owned()).collect();
        let (edits, toggled) = comment::toggle(&lines, first, marker);
        if edits.is_empty() {
            return toggled;
        }

        let changes = edits
            .into_iter()
            .map(|edit| Change {
                pos: self.doc.line_to_char(edit.line) + edit.column,
                removed: edit.removed,
                inserted: edit.inserted,
            })
            .collect();
        let tx = Transaction::new(changes, self.sel, self.sel);
        self.apply_tx(&tx);
        let base = self.doc.line_to_char(first);
        let blank = self.doc.line_indent_len(first);
        self.sel = Selection::point(base + blank.min(self.doc.line_len_chars(first)));
        self.goal_col = None;
        self.history.push(Transaction { sel_after: self.sel, ..tx });
        toggled
    }

    /// Put one line at `column`, keeping the cursor where it is in the text
    /// rather than where it is on the screen. `merge` folds the edit into the
    /// transaction before it, for the auto-indent that follows `enter`: one
    /// keypress should be one undo.
    pub fn set_line_indent(&mut self, line: usize, column: usize, indent: Indent, merge: bool) {
        let text = self.doc.line_str(line);
        let leading = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        let wanted = indent.make(column);
        if wanted == text[..leading_bytes(&text, leading)] {
            return;
        }

        let base = self.doc.line_to_char(line);
        let cursor = self.sel.head;
        let moved = (cursor + wanted.chars().count()).saturating_sub(leading);
        let change = Change {
            pos: base,
            removed: text.chars().take(leading).collect(),
            inserted: wanted,
        };
        let tx = Transaction::new(vec![change], self.sel, Selection::point(moved.max(base)));

        self.apply_tx(&tx);
        self.sel = tx.sel_after;
        self.goal_col = None;
        match merge {
            true => self.history.amend(tx),
            false => self.history.push(tx),
        }
    }

    /// Strip trailing whitespace from every line, as one undoable transaction,
    /// and say how many lines changed. The cursor comes back to where it was,
    /// or to the end of its line if it was sitting in the spaces that went.
    pub fn trim_trailing_whitespace(&mut self) -> usize {
        let mut changes = Vec::new();
        for line in 0..self.doc.len_lines() {
            let text = self.doc.line_str(line);
            let trimmed = text.trim_end_matches([' ', '\t']);
            if trimmed.len() == text.len() {
                continue;
            }
            let base = self.doc.line_to_char(line);
            let start = base + trimmed.chars().count();
            let removed: String = text.chars().skip(trimmed.chars().count()).collect();
            // Ascending, in pre-transaction coordinates, which is what a
            // transaction expects of its changes.
            changes.push(Change { pos: start, removed, inserted: String::new() });
        }
        if changes.is_empty() {
            return 0;
        }

        let head = self.sel.head;
        let mut moved = head;
        for change in &changes {
            let end = change.pos + change.removed.chars().count();
            if head >= end {
                moved -= change.removed.chars().count();
            } else if head > change.pos {
                moved -= head - change.pos;
            }
        }

        let count = changes.len();
        let tx = Transaction::new(changes, self.sel, Selection::point(moved));
        self.apply_tx(&tx);
        self.sel = tx.sel_after;
        self.goal_col = None;
        self.history.push(tx);
        count
    }

    pub fn save(&mut self) -> Result<()> {
        self.doc.save()?;
        self.history.mark_saved();
        Ok(())
    }

    /// True if there was anything to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.undo() {
            Some(inverse) => {
                self.apply_history(inverse);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.redo() {
            Some(tx) => {
                self.apply_history(tx);
                true
            }
            None => false,
        }
    }

    /// Normal mode's cursor sits on a character, so it may rest neither after
    /// the last one on a line nor on the empty line that a trailing newline
    /// leaves at the end of the rope. The caller decides when this applies;
    /// a view does not know about modes.
    pub fn clamp_cursor(&mut self) {
        let last = self.last_line();
        if self.doc.char_to_line(self.sel.head) > last {
            self.sel = Selection::point(self.doc.line_to_char(last));
        }

        let (line, column) = self.cursor_coords();
        let len = self.doc.line_len_chars(line);
        if column >= len && len > 0 {
            self.sel.head = self.grapheme_left(self.doc.line_to_char(line) + len);
        }
    }

    /// Leaving insert mode steps back onto the last character typed.
    pub fn step_back_from_insert(&mut self) {
        let (_, column) = self.cursor_coords();
        if column > 0 {
            self.sel = Selection::point(self.grapheme_left(self.sel.head));
        }
    }

    /// Remove `start..end` and hand back what was there, for a register.
    pub fn cut(&mut self, start: usize, end: usize) -> String {
        let text = self.doc.slice_str(start, end);
        self.edit(start, end - start, "");
        text
    }

    pub fn move_cursor(&mut self, m: Move, extend: bool, height: usize) {
        let head = match m {
            Move::Left => self.grapheme_left(self.sel.head),
            Move::Right => self.grapheme_right(self.sel.head),
            Move::Up => self.vertical(-1),
            Move::Down => self.vertical(1),
            Move::LineStart => {
                let (line, _) = self.cursor_coords();
                self.doc.line_to_char(line)
            }
            Move::FirstNonBlank => {
                let (line, _) = self.cursor_coords();
                let start = self.doc.line_to_char(line);
                let blanks = self.doc.line_indent_len(line);
                start + blanks.min(self.doc.line_len_chars(line))
            }
            Move::LineEnd => {
                let (line, _) = self.cursor_coords();
                self.doc.line_to_char(line) + self.doc.line_len_chars(line)
            }
            Move::WordForward => self.word_forward(self.sel.head),
            Move::WordBack => self.word_back(self.sel.head),
            Move::WordEnd => self.word_end(self.sel.head),
            Move::HalfPageUp => self.vertical(-((height / 2).max(1) as isize)),
            Move::HalfPageDown => self.vertical((height / 2).max(1) as isize),
            Move::PageUp => self.vertical(-(height as isize)),
            Move::PageDown => self.vertical(height as isize),
            Move::ParagraphBack => self.paragraph(false),
            Move::ParagraphForward => self.paragraph(true),
            Move::FileStart => 0,
            Move::FileEnd => self.doc.len_chars(),
        };

        // Vertical moves preserve the goal column; everything else resets it.
        if !matches!(
            m,
            Move::Up
                | Move::Down
                | Move::PageUp
                | Move::PageDown
                | Move::HalfPageUp
                | Move::HalfPageDown
        ) {
            self.goal_col = None;
        }

        self.sel.head = head;
        if !extend {
            self.sel.anchor = head;
        }
    }
    /// `{` and `}`: the next blank line either way, or the end of the buffer.
    /// A run of blank lines counts once, so `}` on the line before a gap goes
    /// past it rather than sitting down in the middle of it.
    fn paragraph(&self, forward: bool) -> usize {
        let (line, _) = self.cursor_coords();
        // The last line with content: the empty one a trailing newline leaves
        // behind is not somewhere `}` should stop, it is the end of the file.
        let last = self.last_line();
        let blank = |line: usize| self.doc.line_str(line).trim().is_empty();
        let edge = |at: usize| match forward {
            true => at >= last,
            false => at == 0,
        };
        let step = |at: usize| match forward {
            true => at + 1,
            false => at - 1,
        };

        // One line, whatever it is - a motion that cannot move is no motion.
        if edge(line) {
            return match forward {
                true => self.doc.len_chars(),
                false => 0,
            };
        }
        let mut at = step(line);
        // Out of the gap this paragraph sits in, if the cursor is in one, and
        // then along it until the next gap. A run of blank lines is one gap.
        while blank(at) && !edge(at) {
            at = step(at);
        }
        while !blank(at) && !edge(at) {
            at = step(at);
        }
        match edge(at) && !blank(at) {
            true => match forward {
                true => self.doc.len_chars(),
                false => 0,
            },
            false => self.doc.line_to_char(at),
        }
    }

    /// `zt`, `zz`, `zb`: the view moved so the cursor's line sits where asked.
    /// The cursor does not move; only what is around it does.
    pub fn reveal(&mut self, where_to: Reveal, height: usize) {
        let (line, _) = self.cursor_coords();
        let pad = SCROLLOFF.min(height.saturating_sub(1) / 2);
        // The padding is honoured, as vim honours `scrolloff` here: `zt` with
        // three lines of margin leaves three lines above, not none, or the
        // next redraw would scroll it back anyway.
        self.scroll_top = match where_to {
            Reveal::Top => line.saturating_sub(pad),
            Reveal::Middle => line.saturating_sub(height / 2),
            Reveal::Bottom => (line + pad + 1).saturating_sub(height),
        };
    }

    /// `^e` and `^y`: the view moved by lines, the cursor following only when
    /// it would otherwise be scrolled off the screen.
    pub fn scroll_lines(&mut self, down: bool, count: usize, height: usize) {
        let last = self.doc.len_lines().saturating_sub(1);
        self.scroll_top = match down {
            true => (self.scroll_top + count).min(last),
            false => self.scroll_top.saturating_sub(count),
        };
        let pad = SCROLLOFF.min(height.saturating_sub(1) / 2);
        let (line, _) = self.cursor_coords();
        let top = self.scroll_top + pad;
        let bottom = (self.scroll_top + height).saturating_sub(pad + 1).min(last);
        let wanted = line.clamp(top.min(bottom), bottom);
        if wanted != line {
            self.goto_line(wanted);
        }
    }

    /// `H`, `M`, `L`: which line of the screen that is, given where the view
    /// is and how tall it is. `count` is how far in from the edge, as vim
    /// counts it - `3H` is the third line from the top.
    pub fn screen_line(&self, which: Screen, count: usize, height: usize) -> usize {
        let last = self.doc.len_lines().saturating_sub(1);
        let bottom = (self.scroll_top + height).saturating_sub(1).min(last);
        let pad = SCROLLOFF.min(height.saturating_sub(1) / 2);
        // No padding at the ends of the file: there is nothing there to keep
        // in view, and `H` on the first screen should reach line one.
        let top_pad = match self.scroll_top == 0 {
            true => 0,
            false => pad,
        };
        let bottom_pad = match bottom == last {
            true => 0,
            false => pad,
        };
        let step = count.saturating_sub(1);
        match which {
            Screen::Top => (self.scroll_top + top_pad + step).min(bottom),
            Screen::Middle => (self.scroll_top + bottom) / 2,
            Screen::Bottom => bottom.saturating_sub(bottom_pad + step).max(self.scroll_top),
        }
    }

    /// Scroll the viewport so the cursor is visible, keeping SCROLLOFF rows of
    /// context where the buffer allows it.
    pub fn scroll_to_cursor(&mut self, width: usize, height: usize) {
        let (line, _) = self.cursor_coords();
        let col = self.cursor_display_col();

        let pad = SCROLLOFF.min(height.saturating_sub(1) / 2);
        if line < self.scroll_top + pad {
            self.scroll_top = line.saturating_sub(pad);
        }
        let bottom = self.scroll_top + height;
        if line + pad >= bottom {
            self.scroll_top = (line + pad + 1).saturating_sub(height);
        }
        // Never scroll past the last line.
        let max_top = self.doc.len_lines().saturating_sub(1);
        self.scroll_top = self.scroll_top.min(max_top);

        if col < self.scroll_left {
            self.scroll_left = col;
        } else if col >= self.scroll_left + width {
            self.scroll_left = col + 1 - width;
        }
    }
    /// (line, char offset in line) of the cursor.
    pub fn cursor_coords(&self) -> (usize, usize) {
        self.doc.coords(self.sel.head)
    }
    /// Where to leave the terminal cursor, in screen coordinates.
    pub fn cursor_screen(&self) -> (u16, u16) {
        let (line, _) = self.cursor_coords();
        (
            self.cursor_display_col().saturating_sub(self.scroll_left) as u16,
            line.saturating_sub(self.scroll_top) as u16,
        )
    }
    /// Screen column of the cursor, with tabs expanded.
    pub fn cursor_display_col(&self) -> usize {
        let (line, col) = self.cursor_coords();
        display_col(&self.doc.line_str(line), col)
    }
    /// Move `delta` lines, landing as close as possible to the goal column.
    fn vertical(&mut self, delta: isize) -> usize {
        let (line, col) = self.cursor_coords();
        let goal = *self
            .goal_col
            .get_or_insert_with(|| display_col(&self.doc.line_str(line), col));

        let last = self.doc.len_lines().saturating_sub(1);
        let target = (line as isize + delta).clamp(0, last as isize) as usize;

        let text = self.doc.line_str(target);
        self.doc.line_to_char(target) + char_col_at_display(&text, goal)
    }
    pub fn grapheme_left(&self, pos: usize) -> usize {
        let (line, col) = self.doc.coords(pos);
        if col == 0 {
            if line == 0 {
                return 0;
            }
            let prev = line - 1;
            return self.doc.line_to_char(prev) + self.doc.line_len_chars(prev);
        }
        let text = self.doc.line_str(line);
        let byte = byte_of_char(&text, col);
        let prev_byte = prev_boundary(&text, byte);
        pos - (char_of_byte(&text, byte) - char_of_byte(&text, prev_byte))
    }
    pub fn grapheme_right(&self, pos: usize) -> usize {
        let (line, col) = self.doc.coords(pos);
        let line_len = self.doc.line_len_chars(line);
        if col >= line_len {
            if line + 1 >= self.doc.len_lines() {
                return pos;
            }
            return self.doc.line_to_char(line + 1);
        }
        let text = self.doc.line_str(line);
        let byte = byte_of_char(&text, col);
        let next_byte = next_boundary(&text, byte);
        pos + (char_of_byte(&text, next_byte) - char_of_byte(&text, byte))
    }
    fn char_at(&self, pos: usize) -> Option<char> {
        (pos < self.doc.len_chars()).then(|| self.doc.text.char(pos))
    }
    /// Start of the next word.
    fn word_forward(&self, mut pos: usize) -> usize {
        let len = self.doc.len_chars();
        if let Some(class) = self.char_at(pos).map(class_of)
            && class != CharClass::Space
        {
            while self.char_at(pos).map(class_of) == Some(class) {
                pos += 1;
            }
        }
        while self.char_at(pos).map(class_of) == Some(CharClass::Space) {
            pos += 1;
        }
        pos.min(len)
    }
    /// Start of the word before the cursor.
    fn word_back(&self, mut pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        pos -= 1;
        while pos > 0 && self.char_at(pos).map(class_of) == Some(CharClass::Space) {
            pos -= 1;
        }
        let Some(class) = self.char_at(pos).map(class_of) else {
            return pos;
        };
        while pos > 0 && self.char_at(pos - 1).map(class_of) == Some(class) {
            pos -= 1;
        }
        pos
    }
    /// Last character of the current or next word.
    fn word_end(&self, mut pos: usize) -> usize {
        let len = self.doc.len_chars();
        pos += 1;
        while self.char_at(pos).map(class_of) == Some(CharClass::Space) {
            pos += 1;
        }
        let Some(class) = self.char_at(pos).map(class_of) else {
            return len.saturating_sub(1);
        };
        while self.char_at(pos + 1).map(class_of) == Some(class) {
            pos += 1;
        }
        pos
    }
    /// The single funnel for every edit: builds a transaction, applies it,
    /// moves the cursor, and records it for undo.
    fn edit(&mut self, pos: usize, remove_chars: usize, insert: &str) {
        self.edit_at(pos, remove_chars, insert, None);
    }
    /// `cursor` overrides where the caret lands; by default it follows the
    /// inserted text, which is what typing wants but `O` and `cc` do not.
    pub fn edit_at(&mut self, pos: usize, remove_chars: usize, insert: &str, cursor: Option<usize>) {
        let removed = self.doc.slice_str(pos, pos + remove_chars);
        if removed.is_empty() && insert.is_empty() {
            return;
        }

        let sel_before = self.sel;
        let sel_after = Selection::point(cursor.unwrap_or(pos + insert.chars().count()));
        let tx = Transaction::new(
            vec![Change { pos, removed, inserted: insert.to_string() }],
            sel_before,
            sel_after,
        );

        self.apply_tx(&tx);
        self.sel = sel_after;
        self.goal_col = None;
        self.history.push(tx);
    }
    /// Insert text, replacing the selection if there is one.
    pub fn insert(&mut self, text: &str) {
        let (start, end) = self.sel.range();
        self.edit(start, end - start, text);
    }
    /// Enter, carrying the current line's leading whitespace onto the new line.
    pub fn insert_newline(&mut self) {
        let (start, end) = self.sel.range();
        let (line, col) = self.doc.coords(start);
        // Only as far as the cursor: splitting a line in the middle of its
        // indentation carries across what is behind the cursor, not all of it.
        let indent: String = self.doc.line_indent(line).chars().take(col).collect();
        self.edit(start, end - start, &format!("\n{indent}"));
    }
    pub fn delete_backward(&mut self) {
        if !self.sel.is_empty() {
            let (start, end) = self.sel.range();
            self.edit(start, end - start, "");
            return;
        }
        let head = self.sel.head;
        let prev = self.grapheme_left(head);
        if prev < head {
            self.edit(prev, head - prev, "");
        }
    }
    pub fn delete_forward(&mut self) {
        if !self.sel.is_empty() {
            let (start, end) = self.sel.range();
            self.edit(start, end - start, "");
            return;
        }
        let head = self.sel.head;
        let next = self.grapheme_right(head);
        if next > head {
            self.edit(head, next - head, "");
        }
    }
    /// The char range covering `count` whole lines from the cursor.
    pub fn line_range(&self, count: usize) -> (usize, usize) {
        let (line, _) = self.cursor_coords();
        let total = self.doc.len_lines();
        let end_line = (line + count).min(total);
        let start = self.doc.line_to_char(line);
        let end = if end_line >= total {
            self.doc.len_chars()
        } else {
            self.doc.line_to_char(end_line)
        };
        (start, end)
    }
    /// The last line with content. A buffer ending in a newline has a trailing
    /// empty line in the rope that is not a line as far as the user is
    /// concerned, and `G` should not land on it.
    pub fn last_line(&self) -> usize {
        let total = self.doc.len_lines();
        if total > 1 && self.doc.line_len_chars(total - 1) == 0 {
            total - 2
        } else {
            total - 1
        }
    }
    /// Jump to a line's first non-blank character, as `gg` and `G` do.
    pub fn goto_line(&mut self, line: usize) {
        let line = line.min(self.last_line());
        self.sel = Selection::point(self.doc.line_to_char(line));
        self.move_cursor(Move::FirstNonBlank, false, 0);
    }
    /// The bracket matching the one at `at`, if there is a bracket there.
    ///
    /// A plain nesting count over the rope: it does not know that a brace
    /// inside a string or a comment is not structure. Tree-sitter could tell
    /// it, which is worth doing when `%` starts being wrong often enough to
    /// notice.
    pub fn matching_bracket(&self, at: usize) -> Option<usize> {
        if at >= self.doc.len_chars() {
            return None;
        }
        let ch = self.doc.text.char(at);
        let (mate, forward) = match ch {
            '(' => (')', true),
            '[' => (']', true),
            '{' => ('}', true),
            ')' => ('(', false),
            ']' => ('[', false),
            '}' => ('{', false),
            _ => return None,
        };

        let mut depth = 0usize;
        let total = self.doc.len_chars();
        let mut i = at;
        loop {
            let c = self.doc.text.char(i);
            if c == ch {
                depth += 1;
            } else if c == mate {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            match forward {
                true if i + 1 < total => i += 1,
                false if i > 0 => i -= 1,
                _ => return None,
            }
        }
    }

    /// The word the cursor is on, if it is on one.
    pub fn word_under_cursor(&self) -> Option<String> {
        let (line, column) = self.cursor_coords();
        let text = self.doc.line_str(line);
        let chars: Vec<char> = text.chars().collect();
        if column >= chars.len() || !is_word(chars[column]) {
            return None;
        }
        let start = (0..column).rev().take_while(|&i| is_word(chars[i])).last().unwrap_or(column);
        let end = (column..chars.len()).take_while(|&i| is_word(chars[i])).last().unwrap_or(column);
        Some(chars[start..=end].iter().collect())
    }

    /// Cheap fingerprint of the document's state: how many edits deep it is
    /// and how long it is. Equal fingerprints mean nothing worth re-diffing.
    pub fn revision(&self) -> (usize, usize) {
        (self.history.depth(), self.doc.len_chars())
    }

    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Whether this is the empty buffer the editor starts with: no file, and
    /// nothing typed into it. Opening a file replaces one of these rather than
    /// leaving a dead tab beside it.
    pub fn is_empty_scratch(&self) -> bool {
        self.doc.path.is_none() && self.doc.len_chars() == 0 && !self.is_modified()
    }

    /// Apply a transaction to the text and the syntax tree, and log it for any
    /// other window looking at this buffer. Every edit comes through here.
    fn apply_tx(&mut self, tx: &Transaction) {
        let edits = tx.apply(&mut self.doc);
        if let Some(syntax) = self.syntax.as_mut() {
            syntax.edit(&edits, &self.doc.text);
        }
        self.edits += 1;
        if !self.watched && self.diagnostics.is_empty() {
            return;
        }
        // In the coordinates of the text as each change found it, which is how
        // a position is walked through them one at a time.
        let mut shift: isize = 0;
        for change in &tx.changes {
            let pos = (change.pos as isize + shift) as usize;
            let (removed, inserted) = (change.removed.chars().count(), change.inserted.chars().count());
            if self.watched {
                self.log.push((pos, removed, inserted));
            }
            for diagnostic in &mut self.diagnostics {
                diagnostic.start = carry_one(diagnostic.start, pos, removed, inserted);
                diagnostic.end = carry_one(diagnostic.end, pos, removed, inserted);
            }
            shift += inserted as isize - removed as isize;
        }
    }

    /// How many edits the text has had, for knowing a server is behind.
    pub fn edits(&self) -> u64 {
        self.edits
    }

    /// The text changed without an edit - a reload from disk.
    pub fn touch(&mut self) {
        self.edits += 1;
    }

    /// Whether another window is showing this buffer. Turning it off lets the
    /// log go.
    pub fn set_watched(&mut self, watched: bool) {
        if !watched {
            self.log_base += self.log.len();
            self.log.clear();
        }
        self.watched = watched;
    }

    /// Where the log has got to: a window takes this when it lets go of focus.
    pub fn log_mark(&self) -> usize {
        self.log_base + self.log.len()
    }

    /// A position from when the log stood at `mark`, where it is now. Text
    /// that was deleted around it leaves it at the start of the deletion;
    /// anything the log no longer covers is only clamped.
    pub fn carry(&self, mark: usize, pos: usize) -> usize {
        let mut pos = pos;
        if mark >= self.log_base {
            for &(at, removed, inserted) in &self.log[(mark - self.log_base).min(self.log.len())..] {
                pos = carry_one(pos, at, removed, inserted);
            }
        }
        pos.min(self.doc.len_chars())
    }

    /// One undo step, which is one command's worth of transactions. The
    /// cursor ends where the last of them says, so a step that typed a word
    /// leaves the caret where the typing began.
    fn apply_history(&mut self, txs: Vec<Transaction>) {
        for tx in &txs {
            self.apply_tx(tx);
            self.sel = tx.sel_after;
        }
        self.goal_col = None;
    }

    /// A command is starting: its edits are one undo step, however many
    /// transactions it takes.
    pub fn begin_undo_group(&mut self) {
        self.history.begin_group();
    }

    pub fn end_undo_group(&mut self) {
        self.history.end_group();
    }
    pub fn put_lines(&mut self, text: &str, after: bool) {
        let (line, _) = self.cursor_coords();
        let total = self.doc.len_lines();

        // Putting after the last line of a buffer with no trailing newline has
        // no line to start at, so the newline goes in front instead.
        let (at, text, first_line) = if after && line + 1 >= total {
            let at = self.doc.len_chars();
            let trimmed = text.trim_end_matches('\n').to_string();
            (at, format!("\n{trimmed}"), at + 1)
        } else {
            let at = match after {
                true => self.doc.line_to_char(line + 1),
                false => self.doc.line_to_char(line),
            };
            (at, text.to_string(), at)
        };

        // Land on the first non-blank of what was put, as vim does.
        let indent = crate::buffer::indent_of(text.trim_start_matches('\n')).chars().count();
        self.edit_at(at, 0, &text, Some(first_line + indent));
    }
    /// Insert lines at an exact position, for putting over a selection that
    /// has just been cut away.
    pub fn put_lines_at(&mut self, at: usize, text: &str) {
        let at = at.min(self.doc.len_chars());
        let indent = crate::buffer::indent_of(text).chars().count();
        self.edit_at(at, 0, text, Some(at + indent));
    }

    /// Insert text at an exact position, leaving the cursor on its last
    /// character rather than after it.
    pub fn put_inline_at(&mut self, at: usize, text: &str) {
        let at = at.min(self.doc.len_chars());
        let len = text.chars().count();
        self.edit_at(at, 0, text, Some(at + len.saturating_sub(1)));
    }

    pub fn put_inline(&mut self, text: &str, after: bool) {
        let (line, column) = self.cursor_coords();
        let line_end = self.doc.line_to_char(line) + self.doc.line_len_chars(line);
        let at = match after && column < self.doc.line_len_chars(line) {
            true => self.grapheme_right(self.sel.head).min(line_end),
            false => self.sel.head,
        };

        let len = text.chars().count();
        // The cursor ends on the last character put, not after it.
        self.edit_at(at, 0, text, Some(at + len.saturating_sub(1)));
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn char_width(ch: char, at: usize) -> usize {
    if ch == '\t' {
        TAB_WIDTH - (at % TAB_WIDTH)
    } else {
        // TODO: width should be measured per grapheme cluster, not per char;
        // this is wrong for emoji ZWJ sequences and combining marks.
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

/// Screen column of char offset `char_col` within `line`.
/// A position through one change at `at`: after it, moved by what the change
/// added or took away; inside what was removed, to where the removal was.
fn carry_one(pos: usize, at: usize, removed: usize, inserted: usize) -> usize {
    if pos >= at + removed {
        pos + inserted - removed
    } else if pos > at {
        at
    } else {
        pos
    }
}

pub fn display_col(line: &str, char_col: usize) -> usize {
    let mut w = 0;
    for ch in line.chars().take(char_col) {
        w += char_width(ch, w);
    }
    w
}

/// Char offset in `line` whose screen column is at or just past `target`.
pub fn char_col_at_display(line: &str, target: usize) -> usize {
    let mut w = 0;
    let mut col = 0;
    for ch in line.chars() {
        if w >= target {
            break;
        }
        w += char_width(ch, w);
        col += 1;
    }
    col
}

fn byte_of_char(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map_or(s.len(), |(b, _)| b)
}

fn char_of_byte(s: &str, byte_idx: usize) -> usize {
    s[..byte_idx].chars().count()
}

fn next_boundary(s: &str, byte: usize) -> usize {
    let mut cursor = GraphemeCursor::new(byte, s.len(), true);
    cursor.next_boundary(s, 0).ok().flatten().unwrap_or(s.len())
}

fn prev_boundary(s: &str, byte: usize) -> usize {
    let mut cursor = GraphemeCursor::new(byte, s.len(), true);
    cursor.prev_boundary(s, 0).ok().flatten().unwrap_or(0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Punct,
}

/// Word motions step between runs of one class, so `foo.bar` is three words.
fn class_of(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Space
    } else if ch.is_alphanumeric() || ch == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}
