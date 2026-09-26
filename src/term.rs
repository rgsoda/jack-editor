//! A terminal, as a grid of characters.
//!
//! `:term` needs something to interpret what a program writes to a pty, and
//! this is it: a fixed grid, a cursor, a scrollback, and the escape sequences
//! a shell and the things it runs actually use. Nothing here opens a pty or
//! reads a file descriptor - it is handed bytes and asked what the screen
//! looks like, which is what makes it testable with `feed` and a string.
//!
//! The state machine itself is vte's, alacritty's parser. The awkward part of
//! reading escape sequences is not the sequences themselves, it is the states
//! around them: UTF-8 arriving a byte at a time, device control strings,
//! intermediates. Half of a parser is worse than none. What each sequence
//! *means* is here.
//!
//! Colour is dropped on purpose. A terminal buffer in jack is text in a
//! buffer, the way a directory listing is: `j`, `/`, `y` and `^w s` all work
//! on it because it is nothing special. Keeping the colours would mean a
//! second kind of window with a second way of drawing, and that is a bigger
//! thing than a shell you can scroll back through.

use std::collections::VecDeque;

use vte::{Params, Perform};

/// How many lines that have scrolled off the top are kept. Enough for the
/// output of a build, which is what anybody scrolls back for.
const SCROLLBACK: usize = 5000;

/// A cell. The continuation of a double-width character is `None`, so the
/// column arithmetic stays honest and the text comes out with one character
/// where the screen has two columns.
type Cell = Option<char>;

/// One screen of cells, and where the cursor is in it.
#[derive(Clone)]
struct Grid {
    rows: Vec<Vec<Cell>>,
    row: usize,
    column: usize,
    /// The lines `\n` scrolls between, from `DECSTBM`. Whole screen unless a
    /// program asked otherwise.
    top: usize,
    bottom: usize,
}

impl Grid {
    fn new(rows: usize, cols: usize) -> Grid {
        Grid {
            rows: vec![vec![None; cols]; rows],
            row: 0,
            column: 0,
            top: 0,
            bottom: rows.saturating_sub(1),
        }
    }

    fn blank(&self) -> Vec<Cell> {
        vec![None; self.rows.first().map_or(0, Vec::len)]
    }
}

/// The screen a program is writing to.
pub struct Term {
    parser: vte::Parser,
    screen: Screen,
}

impl Term {
    pub fn new(rows: usize, cols: usize) -> Term {
        Term { parser: vte::Parser::new(), screen: Screen::new(rows.max(1), cols.max(1)) }
    }

    /// Bytes from the program. Whatever it wrote, in the order it wrote it;
    /// a sequence split across two reads is picked up where it left off,
    /// which is the parser's whole job.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.screen, bytes);
    }

    /// The screen as lines of text: what has scrolled off the top first, then
    /// the grid, with the blank rows at the bottom left off. This is what goes
    /// into the buffer.
    pub fn lines(&self) -> Vec<String> {
        self.screen.lines()
    }

    /// Where the cursor is, as a line in what `lines` returns and a column.
    pub fn cursor(&self) -> (usize, usize) {
        let grid = self.screen.grid();
        (self.screen.scrollback.len() + grid.row, grid.column)
    }

    /// What the program called itself, from the escape sequence that sets a
    /// window title. Empty until one does.
    pub fn title(&self) -> &str {
        &self.screen.title
    }

    /// A new size. The rows and columns a program can see change under it,
    /// which is why this is followed by a `TIOCSWINSZ` and a `SIGWINCH`.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.screen.resize(rows.max(1), cols.max(1));
    }
}

struct Screen {
    grid: Grid,
    /// The primary screen, while a full-screen program has the alternate one.
    /// A terminal has two screens so that `less` can have the whole window and
    /// give it back untouched.
    primary: Option<Grid>,
    scrollback: VecDeque<String>,
    saved: Option<(usize, usize)>,
    title: String,
    /// Whether the next printed character wraps first. A character printed in
    /// the last column stays there until another one arrives, which is what
    /// keeps a line exactly as wide as the screen from leaving a blank row.
    pending_wrap: bool,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Screen {
        Screen {
            grid: Grid::new(rows, cols),
            primary: None,
            scrollback: VecDeque::new(),
            saved: None,
            title: String::new(),
            pending_wrap: false,
        }
    }

    fn grid(&self) -> &Grid {
        &self.grid
    }

    fn cols(&self) -> usize {
        self.grid.rows.first().map_or(0, Vec::len)
    }

    fn rows(&self) -> usize {
        self.grid.rows.len()
    }

    fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = self.scrollback.iter().cloned().collect();
        let mut screen: Vec<String> = self.grid.rows.iter().map(|row| text_of(row)).collect();
        // The bottom of a shell's screen is blank, and a buffer that is mostly
        // blank rows is a worse answer than one that stops where the text
        // does. The row the cursor is on stays, wherever it is.
        while screen.len() > self.grid.row + 1 && screen.last().is_some_and(|l| l.is_empty()) {
            screen.pop();
        }
        // The row the cursor is on keeps the blanks up to it. A shell's
        // prompt ends in a space, and a cursor drawn on the `$` instead of
        // after it would be a lie about where what you type goes.
        if let Some(row) = screen_row(&mut screen, self.grid.row) {
            while row.chars().count() < self.grid.column {
                row.push(' ');
            }
        }
        out.append(&mut screen);
        out
    }

    /// Move the whole scrolling region up by one, keeping what fell off the
    /// top when it is the real screen rather than a full-screen program's.
    fn scroll_up(&mut self) {
        let (top, bottom) = (self.grid.top, self.grid.bottom.min(self.rows().saturating_sub(1)));
        if top > bottom {
            return;
        }
        let gone = self.grid.rows.remove(top);
        if self.primary.is_none() && top == 0 {
            self.scrollback.push_back(text_of(&gone));
            while self.scrollback.len() > SCROLLBACK {
                self.scrollback.pop_front();
            }
        }
        let blank = self.grid.blank();
        self.grid.rows.insert(bottom, blank);
    }

    fn scroll_down(&mut self) {
        let (top, bottom) = (self.grid.top, self.grid.bottom.min(self.rows().saturating_sub(1)));
        if top > bottom {
            return;
        }
        self.grid.rows.remove(bottom);
        let blank = self.grid.blank();
        self.grid.rows.insert(top, blank);
    }

    fn linefeed(&mut self) {
        match self.grid.row >= self.grid.bottom {
            true => self.scroll_up(),
            false => self.grid.row += 1,
        }
        self.pending_wrap = false;
    }

    fn reverse_linefeed(&mut self) {
        match self.grid.row <= self.grid.top {
            true => self.scroll_down(),
            false => self.grid.row -= 1,
        }
    }

    fn put(&mut self, c: char, width: usize) {
        let cols = self.cols();
        if cols == 0 {
            return;
        }
        if self.pending_wrap || self.grid.column + width > cols {
            self.grid.column = 0;
            self.linefeed();
        }
        let (row, column) = (self.grid.row.min(self.rows() - 1), self.grid.column);
        self.grid.rows[row][column] = Some(c);
        for next in 1..width {
            if column + next < cols {
                self.grid.rows[row][column + next] = None;
            }
        }
        self.grid.column += width;
        if self.grid.column >= cols {
            self.grid.column = cols - 1;
            self.pending_wrap = true;
        }
    }

    fn goto(&mut self, row: usize, column: usize) {
        self.grid.row = row.min(self.rows().saturating_sub(1));
        self.grid.column = column.min(self.cols().saturating_sub(1));
        self.pending_wrap = false;
    }

    /// The alternate screen, and back. A program that asks for it gets a
    /// blank one and nothing it does reaches the scrollback; giving it back
    /// puts the shell's screen up exactly as it was left.
    fn alternate(&mut self, on: bool) {
        match (on, self.primary.take()) {
            (true, None) => {
                let rows = self.rows();
                let cols = self.cols();
                self.primary = Some(std::mem::replace(&mut self.grid, Grid::new(rows, cols)));
            }
            (true, Some(primary)) => self.primary = Some(primary),
            (false, Some(primary)) => self.grid = primary,
            (false, None) => {}
        }
        self.pending_wrap = false;
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        // A row that falls off a shorter screen is kept the way one that
        // scrolled off it would be, so making the window smaller never loses
        // what was already said. The alternate screen has no scrollback and
        // its rows go.
        let keep = self.primary.is_none();
        let mut fell: Vec<String> = Vec::new();
        for (grid, scrolls) in [(Some(&mut self.grid), keep), (self.primary.as_mut(), true)] {
            let Some(grid) = grid else {
                continue;
            };
            for row in &mut grid.rows {
                row.resize(cols, None);
            }
            while grid.rows.len() > rows {
                let gone = grid.rows.remove(0);
                if scrolls {
                    fell.push(text_of(&gone));
                }
                grid.row = grid.row.saturating_sub(1);
            }
            while grid.rows.len() < rows {
                grid.rows.push(vec![None; cols]);
            }
            grid.top = grid.top.min(rows - 1);
            grid.bottom = rows - 1;
            grid.row = grid.row.min(rows - 1);
            grid.column = grid.column.min(cols - 1);
        }
        for line in fell {
            self.scrollback.push_back(line);
        }
        while self.scrollback.len() > SCROLLBACK {
            self.scrollback.pop_front();
        }
        self.pending_wrap = false;
    }

    /// `J` - erase in display. The parameter says which way from the cursor.
    fn erase_display(&mut self, which: u16) {
        let (rows, cols) = (self.rows(), self.cols());
        let (row, column) = (self.grid.row, self.grid.column);
        match which {
            0 => {
                for c in column..cols {
                    self.grid.rows[row][c] = None;
                }
                for r in row + 1..rows {
                    self.grid.rows[r] = vec![None; cols];
                }
            }
            1 => {
                for c in 0..=column.min(cols.saturating_sub(1)) {
                    self.grid.rows[row][c] = None;
                }
                for r in 0..row {
                    self.grid.rows[r] = vec![None; cols];
                }
            }
            // 2 is the screen and 3 is the screen and the scrollback: `clear`
            // sends both, and a `clear` that leaves the scrollback behind is
            // not what anybody typing it meant.
            _ => {
                for r in 0..rows {
                    self.grid.rows[r] = vec![None; cols];
                }
                if which == 3 {
                    self.scrollback.clear();
                }
            }
        }
        self.pending_wrap = false;
    }

    /// `K` - erase in line.
    fn erase_line(&mut self, which: u16) {
        let (cols, row, column) = (self.cols(), self.grid.row, self.grid.column);
        let range = match which {
            0 => column..cols,
            1 => 0..(column + 1).min(cols),
            _ => 0..cols,
        };
        for c in range {
            self.grid.rows[row][c] = None;
        }
        self.pending_wrap = false;
    }
}

/// The row the cursor is on, when it is one of the rows being shown.
fn screen_row(screen: &mut [String], row: usize) -> Option<&mut String> {
    screen.get_mut(row)
}

/// A row as text, without the blanks at the end of it. A cell nothing has
/// been written into is a space; the continuation of a wide character is
/// nothing at all, since the character itself is already there.
fn text_of(row: &[Cell]) -> String {
    let mut text = String::with_capacity(row.len());
    let last = row.iter().rposition(|cell| cell.is_some_and(|c| c != ' '));
    let Some(last) = last else {
        return String::new();
    };
    for cell in &row[..=last] {
        match cell {
            Some(c) => text.push(*c),
            None => text.push(' '),
        }
    }
    text
}

/// The first parameter, or a default. Every CSI sequence in here takes its
/// parameters this way, and a zero means "not given" for all of them.
fn param(params: &Params, at: usize, default: u16) -> u16 {
    match params.iter().nth(at).and_then(|p| p.first().copied()).unwrap_or(0) {
        0 => default,
        n => n,
    }
}

impl Perform for Screen {
    fn print(&mut self, c: char) {
        let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width == 0 {
            return;
        }
        self.put(c, width);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            b'\r' => {
                self.grid.column = 0;
                self.pending_wrap = false;
            }
            0x08 => {
                self.grid.column = self.grid.column.saturating_sub(1);
                self.pending_wrap = false;
            }
            b'\t' => {
                let cols = self.cols();
                let next = (self.grid.column / 8 + 1) * 8;
                self.grid.column = next.min(cols.saturating_sub(1));
            }
            // The bell. A terminal buffer that beeps at you from inside an
            // editor is not a feature.
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // `0` is icon and title, `2` is the title. The rest - the working
        // directory, the clipboard, colours - are not this buffer's business.
        let Some(kind) = params.first() else {
            return;
        };
        if matches!(*kind, b"0" | b"2")
            && let Some(title) = params.get(1)
        {
            self.title = String::from_utf8_lossy(title).to_string();
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let private = intermediates.first() == Some(&b'?');
        let first = param(params, 0, 1);
        match (action, private) {
            // The modes worth knowing about: the alternate screen, in its
            // three spellings. Everything else a program turns on - bracketed
            // paste, mouse reporting, the cursor's shape - changes nothing
            // about what the text says.
            ('h', true) | ('l', true) => {
                let on = action == 'h';
                for mode in params.iter().filter_map(|p| p.first().copied()) {
                    if matches!(mode, 47 | 1047 | 1049) {
                        self.alternate(on);
                    }
                }
            }
            ('A', _) => self.grid.row = self.grid.row.saturating_sub(first as usize),
            ('B', _) | ('e', _) => {
                self.grid.row = (self.grid.row + first as usize).min(self.rows() - 1);
            }
            ('C', _) | ('a', _) => {
                let cols = self.cols();
                self.grid.column = (self.grid.column + first as usize).min(cols - 1);
                self.pending_wrap = false;
            }
            ('D', _) => {
                self.grid.column = self.grid.column.saturating_sub(first as usize);
                self.pending_wrap = false;
            }
            ('E', _) => {
                self.grid.row = (self.grid.row + first as usize).min(self.rows() - 1);
                self.grid.column = 0;
            }
            ('F', _) => {
                self.grid.row = self.grid.row.saturating_sub(first as usize);
                self.grid.column = 0;
            }
            ('G', _) | ('`', _) => self.grid.column = (first as usize - 1).min(self.cols() - 1),
            ('d', _) => self.grid.row = (first as usize - 1).min(self.rows() - 1),
            ('H', _) | ('f', _) => {
                let column = param(params, 1, 1);
                self.goto(first as usize - 1, column as usize - 1);
            }
            ('J', _) => self.erase_display(param(params, 0, 0)),
            ('K', _) => self.erase_line(param(params, 0, 0)),
            ('L', _) => {
                for _ in 0..first {
                    let (row, bottom) = (self.grid.row, self.grid.bottom);
                    if row > bottom {
                        break;
                    }
                    self.grid.rows.remove(bottom);
                    let blank = self.grid.blank();
                    self.grid.rows.insert(row, blank);
                }
            }
            ('M', _) => {
                for _ in 0..first {
                    let (row, bottom) = (self.grid.row, self.grid.bottom);
                    if row > bottom {
                        break;
                    }
                    self.grid.rows.remove(row);
                    let blank = self.grid.blank();
                    self.grid.rows.insert(bottom, blank);
                }
            }
            ('P', _) => {
                let (row, cols) = (self.grid.row, self.cols());
                for _ in 0..first.min(cols as u16) {
                    self.grid.rows[row].remove(self.grid.column.min(cols - 1));
                    self.grid.rows[row].push(None);
                }
            }
            ('@', _) => {
                let (row, cols) = (self.grid.row, self.cols());
                for _ in 0..first.min(cols as u16) {
                    self.grid.rows[row].pop();
                    self.grid.rows[row].insert(self.grid.column.min(cols - 1), None);
                }
            }
            ('X', _) => {
                let (row, cols, column) = (self.grid.row, self.cols(), self.grid.column);
                for c in column..(column + first as usize).min(cols) {
                    self.grid.rows[row][c] = None;
                }
            }
            ('S', _) => {
                for _ in 0..first {
                    self.scroll_up();
                }
            }
            ('T', _) => {
                for _ in 0..first {
                    self.scroll_down();
                }
            }
            ('r', _) => {
                let bottom = param(params, 1, self.rows() as u16);
                self.grid.top = (first as usize - 1).min(self.rows() - 1);
                self.grid.bottom = (bottom as usize - 1).min(self.rows() - 1);
                self.goto(self.grid.top, 0);
            }
            ('s', _) => self.saved = Some((self.grid.row, self.grid.column)),
            ('u', _) => {
                if let Some((row, column)) = self.saved {
                    self.goto(row, column);
                }
            }
            // `m` is colour, and colour is not kept. Everything else left
            // over - status reports, cursor shapes - is answered by silence,
            // which is what a program that asked without needing an answer
            // expects anyway.
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if !intermediates.is_empty() {
            return;
        }
        match byte {
            // Reverse index: up one line, scrolling the region if it is at
            // the top. This is how `less` draws a line above the screen.
            b'M' => self.reverse_linefeed(),
            b'D' => self.linefeed(),
            b'E' => {
                self.linefeed();
                self.grid.column = 0;
            }
            b'7' => self.saved = Some((self.grid.row, self.grid.column)),
            b'8' => {
                if let Some((row, column)) = self.saved {
                    self.goto(row, column);
                }
            }
            b'c' => {
                let (rows, cols) = (self.rows(), self.cols());
                self.grid = Grid::new(rows, cols);
                self.primary = None;
                self.saved = None;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(rows: usize, cols: usize, bytes: &str) -> Term {
        let mut term = Term::new(rows, cols);
        term.feed(bytes.as_bytes());
        term
    }

    #[test]
    fn text_written_to_it_is_text_you_can_read_back() {
        let term = screen(5, 20, "hello\r\nworld");
        assert_eq!(term.lines(), ["hello", "world"]);
        assert_eq!(term.cursor(), (1, 5));
    }

    #[test]
    fn a_carriage_return_writes_over_what_is_there() {
        // Which is how every progress bar in the world works.
        let term = screen(5, 20, "50%\r100%");
        assert_eq!(term.lines(), ["100%"]);
    }

    #[test]
    fn a_line_wider_than_the_screen_goes_on_to_the_next_row() {
        let term = screen(5, 4, "abcdefgh");
        assert_eq!(term.lines(), ["abcd", "efgh"]);
        // And the character that exactly fills the last column does not
        // leave a blank row behind it.
        let term = screen(5, 4, "abcd");
        assert_eq!(term.lines(), ["abcd"]);
        assert_eq!(term.cursor(), (0, 3));
    }

    #[test]
    fn what_scrolls_off_the_top_is_kept() {
        let mut term = Term::new(3, 10);
        term.feed(b"one\r\ntwo\r\nthree\r\nfour");
        assert_eq!(term.lines(), ["one", "two", "three", "four"]);
        // The grid is still three rows: the first line is in the scrollback.
        assert_eq!(term.cursor().0, 3);
    }

    #[test]
    fn the_sequences_a_shell_actually_sends() {
        // Cursor home and erase the screen, which is what `clear` is.
        let term = screen(4, 10, "junk\r\nmore\x1b[H\x1b[2Jfresh");
        assert_eq!(term.lines(), ["fresh"]);

        // Move to a row and a column, and erase to the end of the line:
        // a prompt being redrawn.
        let term = screen(4, 10, "abcdef\x1b[1;4H\x1b[K");
        assert_eq!(term.lines(), ["abc"]);

        // Colour is parsed and dropped rather than printed.
        let term = screen(4, 20, "\x1b[31mred\x1b[0m plain");
        assert_eq!(term.lines(), ["red plain"]);
    }

    #[test]
    fn a_full_screen_program_gets_a_screen_of_its_own_and_gives_it_back() {
        let mut term = Term::new(4, 10);
        term.feed(b"shell\r\n");
        term.feed(b"\x1b[?1049h");
        term.feed(b"vim");
        assert_eq!(term.lines(), ["vim"], "the alternate screen is its own");
        term.feed(b"\x1b[?1049l");
        assert_eq!(term.lines(), ["shell", ""], "and the shell's is as it was left");
    }

    #[test]
    fn a_title_is_read_and_a_bell_is_not_rung() {
        let term = screen(4, 10, "\x1b]0;a shell\x07hi\x07");
        assert_eq!(term.title(), "a shell");
        assert_eq!(term.lines(), ["hi"]);
    }

    #[test]
    fn resizing_keeps_the_text_and_the_cursor_on_the_screen() {
        let mut term = Term::new(4, 10);
        term.feed(b"one\r\ntwo\r\nthree");
        term.resize(2, 6);
        assert_eq!(term.lines(), ["one", "two", "three"], "nothing is lost by shrinking");
        let (row, column) = term.cursor();
        assert!(row < term.lines().len() + 1, "the cursor is on a line that exists");
        assert!(column < 6);
    }

    #[test]
    fn utf8_arriving_in_pieces_is_still_one_character() {
        let mut term = Term::new(2, 10);
        let bytes = "héllo".as_bytes();
        for byte in bytes {
            term.feed(&[*byte]);
        }
        assert_eq!(term.lines(), ["héllo"]);
    }
}

/// A terminal buffer: the program, the pty it is talking through, and the
/// screen its bytes have painted.
///
/// The `Pty` is dropped with the buffer, and dropping it is what kills the
/// program - closing a terminal buffer closes the terminal, the way closing a
/// terminal window does.
pub struct Terminal {
    /// Which terminal a `Message::Term` is for. Buffers move around as others
    /// are closed; a token does not.
    pub token: u64,
    /// What was asked for, for the buffer's name. Empty is a shell.
    pub command: String,
    /// What the program has drawn.
    pub screen: Term,
    #[cfg(unix)]
    pub pty: Option<crate::pty::Pty>,
    /// Whether the program has finished. The buffer stays afterwards - what a
    /// build said is worth reading after it has said it.
    pub over: bool,
    /// The size the program was last told about, so a redraw that changes
    /// nothing does not send a `SIGWINCH`.
    pub size: (usize, usize),
}

impl Terminal {
    /// The name the buffer wears: what the program called itself, or what it
    /// was started as.
    pub fn name(&self) -> String {
        let what = match (self.screen.title().is_empty(), self.command.is_empty()) {
            (false, _) => self.screen.title().to_string(),
            (true, false) => self.command.clone(),
            (true, true) => "shell".to_string(),
        };
        match self.over {
            true => format!("[term: {what} - finished]"),
            false => format!("[term: {what}]"),
        }
    }

    /// Keys, on their way to the program.
    pub fn send(&self, bytes: &[u8]) {
        #[cfg(unix)]
        if let Some(pty) = &self.pty {
            pty.write(bytes);
        }
        #[cfg(not(unix))]
        let _ = bytes;
    }
}

/// A key as the bytes a terminal would have sent for it. `None` for a key
/// that sends nothing - a bare modifier, or a chord no terminal has an
/// encoding for.
///
/// This is the other half of what `crossterm` does when it reads a key, and
/// it has to agree with it: what jack's own terminal sent, jack sends on.
pub fn encode(key: crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let mut bytes = match key.code {
        KeyCode::Char(c) if ctrl => {
            // `^a` is 1, `^z` is 26, and the handful after them are what the
            // characters before `a` map to. Anything else with control held
            // goes as the plain character.
            let byte = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                ' ' | '@' | '2' => 0,
                '[' | '3' => 27,
                // `^\` reaches a terminal as control and `4`, which is what
                // the byte for it would be typed as: crossterm reads the
                // control bytes above `^z` that way, and this puts them back.
                '\\' | '4' => 28,
                ']' | '5' => 29,
                '^' | '6' => 30,
                '_' | '?' | '7' => 31,
                _ => return Some(c.to_string().into_bytes()),
            };
            vec![byte]
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        // What a terminal sends for backspace, and what readline expects.
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::F(n @ 1..=4) => vec![0x1b, b'O', b'P' + (n - 1)],
        KeyCode::F(n @ 5..=12) => {
            let code = [15, 17, 18, 19, 20, 21, 23, 24][n as usize - 5];
            format!("\x1b[{code}~").into_bytes()
        }
        _ => return None,
    };
    // Meta, as every terminal has sent it since the seventies: an escape in
    // front of whatever the key would have sent on its own.
    if alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

#[cfg(test)]
mod keys {
    use super::encode;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn sent(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
        encode(KeyEvent::new(code, modifiers)).expect("something to send")
    }

    #[test]
    fn a_key_goes_as_the_bytes_a_terminal_would_have_sent() {
        assert_eq!(sent(KeyCode::Char('a'), KeyModifiers::NONE), b"a");
        assert_eq!(sent(KeyCode::Enter, KeyModifiers::NONE), b"\r");
        assert_eq!(sent(KeyCode::Backspace, KeyModifiers::NONE), [0x7f]);
        assert_eq!(sent(KeyCode::Up, KeyModifiers::NONE), b"\x1b[A");
        // The one that matters most: `^c` has to reach the program.
        assert_eq!(sent(KeyCode::Char('c'), KeyModifiers::CONTROL), [3]);
        assert_eq!(sent(KeyCode::Char('d'), KeyModifiers::CONTROL), [4]);
        // `^\` arrives from crossterm as control and `4`, because that is how
        // a terminal sends the byte: it has to go back out as the byte.
        assert_eq!(sent(KeyCode::Char('4'), KeyModifiers::CONTROL), [28]);
        // Meta is an escape in front of the key, as it has always been.
        assert_eq!(sent(KeyCode::Char('f'), KeyModifiers::ALT), b"\x1bf");
        // And a key a terminal has no encoding for sends nothing at all
        // rather than something else.
        assert!(encode(KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)).is_none());
    }
}
