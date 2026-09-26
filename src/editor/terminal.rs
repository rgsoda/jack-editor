//! `:term` - a program, running in a buffer.
//!
//! A terminal buffer is text in a buffer, the way a directory listing is.
//! What the program draws is turned into lines and put in the rope, so `j`,
//! `/`, `y`, `^w s` and everything else work on it without knowing what it
//! is. The cost is colour, which the emulator drops; the gain is that there
//! is no second kind of window, no second way of drawing and no second set of
//! keys to learn.
//!
//! Terminal mode - `i` from normal mode, `^\ ^n` back out - is the one thing
//! that is new: while it is on, keys go to the program instead of the editor.
//! That is vim's arrangement, and neovim's, and the reason for it is that a
//! terminal needs every key there is, `:` and `d` included.

use crate::term::{Term, Terminal};
use crate::view::{Selection, View};

use super::{Editor, Mode};

/// How big a terminal is before a frame has said otherwise. Replaced by the
/// window's own size the moment one is drawn.
const STARTING: (usize, usize) = (24, 80);

impl Editor {
    /// `:term`, or `:term cargo test`: a shell, or one command, in a buffer.
    pub fn open_terminal(&mut self, command: &str) {
        #[cfg(not(unix))]
        {
            let _ = command;
            self.message = "no terminal buffers on this platform".into();
        }
        #[cfg(unix)]
        {
            let (rows, cols) = self.terminal_size();
            let Some(jobs) = self.jobs.clone() else {
                self.message = "no background jobs to run one in".into();
                return;
            };
            self.terms += 1;
            let token = self.terms;
            let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let pty = match crate::pty::Pty::spawn(command, &root, rows, cols, token, jobs) {
                Ok(pty) => pty,
                Err(err) => {
                    self.message = format!("{err:#}");
                    return;
                }
            };
            let terminal = Terminal {
                token,
                command: command.trim().to_string(),
                screen: Term::new(rows, cols),
                pty: Some(pty),
                over: false,
                size: (rows, cols),
            };
            let mut view = View::new(crate::buffer::Document::scratch());
            view.terminal = Some(Box::new(terminal));
            // The same rule the answer buffer follows: an untouched empty
            // buffer is a buffer nobody wanted, and is replaced rather than
            // left behind.
            let index = match self.views.len() == 1 && self.views[0].is_empty_scratch() {
                true => {
                    self.views[0] = view;
                    0
                }
                false => {
                    self.views.push(view);
                    self.views.len() - 1
                }
            };
            self.switch_to(index);
            self.message = "terminal: i to type in it, ^\\ ^n to come back".into();
        }
    }

    /// Whether the buffer in front of you is one.
    pub fn is_terminal(&self) -> bool {
        self.view().terminal.is_some()
    }

    /// Bytes the program wrote. They go through the emulator, and the buffer
    /// is redrawn from what it says the screen looks like.
    pub fn term_output(&mut self, token: u64, bytes: Vec<u8>) {
        let Some(index) = self.terminal_at(token) else {
            return;
        };
        if let Some(terminal) = self.views[index].terminal.as_mut() {
            terminal.screen.feed(&bytes);
        }
        self.redraw_terminal(index);
    }

    /// The program has finished. The buffer stays: what a build said is worth
    /// reading after it has said it, and `:bd` is how you say you are done.
    pub fn term_gone(&mut self, token: u64) {
        let Some(index) = self.terminal_at(token) else {
            return;
        };
        if let Some(terminal) = self.views[index].terminal.as_mut() {
            terminal.over = true;
            terminal.pty = None;
        }
        self.redraw_terminal(index);
        if self.mode == Mode::Terminal && self.current == index {
            self.set_mode(Mode::Normal);
        }
        if self.current == index {
            self.message = "the program has finished - :bd closes the buffer".into();
        }
    }

    /// A key, while terminal mode is on. `true` when it was sent, which is
    /// every key a terminal has an encoding for; the rest are dropped rather
    /// than doing something else.
    pub fn terminal_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let Some(terminal) = self.view().terminal.as_ref() else {
            return false;
        };
        if terminal.over {
            self.set_mode(Mode::Normal);
            self.message = "the program has finished".into();
            return true;
        }
        let Some(bytes) = crate::term::encode(key) else {
            return false;
        };
        terminal.send(&bytes);
        true
    }

    /// Text pasted into a terminal buffer, as the program's own input.
    pub fn terminal_paste(&mut self, text: &str) {
        if let Some(terminal) = self.view().terminal.as_ref() {
            terminal.send(text.as_bytes());
        }
    }

    /// Tell the program how big its terminal is, when that has changed. Due
    /// every frame, because a split or a resize changes it and nothing else
    /// tells us.
    pub fn resize_terminals(&mut self) {
        let (rows, cols) = self.terminal_size();
        let Some(terminal) = self.view_mut().terminal.as_mut() else {
            return;
        };
        if terminal.size == (rows, cols) {
            return;
        }
        terminal.size = (rows, cols);
        terminal.screen.resize(rows, cols);
        #[cfg(unix)]
        if let Some(pty) = &terminal.pty {
            pty.resize(rows, cols);
        }
        let index = self.current;
        self.redraw_terminal(index);
    }

    /// The rows and columns the program has to draw into: the window's text
    /// area, less the gutter, which the program cannot write in.
    fn terminal_size(&self) -> (usize, usize) {
        match self.height == 0 || self.width == 0 {
            true => STARTING,
            false => (self.height.max(1), self.text_width().max(1)),
        }
    }

    /// Which buffer that token belongs to.
    fn terminal_at(&self, token: u64) -> Option<usize> {
        self.views
            .iter()
            .position(|view| view.terminal.as_ref().is_some_and(|t| t.token == token))
    }

    /// The buffer, from the screen. The whole text every time: a screen is
    /// eighty columns by fifty rows of characters that can all have changed,
    /// and working out which ones did would cost more than writing them.
    ///
    /// The cursor goes where the program put it, so the block cursor is where
    /// anybody typing would expect it, and the view follows it down.
    fn redraw_terminal(&mut self, index: usize) {
        let Some(terminal) = self.views[index].terminal.as_ref() else {
            return;
        };
        let lines = terminal.screen.lines();
        let (row, column) = terminal.screen.cursor();
        let text = match lines.is_empty() {
            true => String::new(),
            false => lines.join("\n"),
        };
        let view = &mut self.views[index];
        view.doc.text = ropey::Rope::from_str(&text);
        // An edit log nobody can undo: a terminal's text is the program's,
        // not yours, and `u` in a terminal buffer should do nothing rather
        // than half-restore a screen from a second ago.
        view.forget_history();
        let line = row.min(view.doc.len_lines().saturating_sub(1));
        let at = view.doc.line_to_char(line) + column.min(view.doc.line_len_chars(line));
        view.sel = Selection::point(at.min(view.doc.len_chars()));
        if index == self.current {
            self.clamp_cursor();
            self.scroll_to_cursor();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    /// A terminal buffer with no program behind it: everything but the pty,
    /// which is the operating system's and not worth a unit test.
    fn with_terminal() -> Editor {
        let mut editor = Editor::scratch();
        let mut view = View::new(crate::buffer::Document::scratch());
        view.terminal = Some(Box::new(Terminal {
            token: 1,
            command: "sh".into(),
            screen: Term::new(4, 20),
            #[cfg(unix)]
            pty: None,
            over: false,
            size: (4, 20),
        }));
        editor.views[0] = view;
        editor
    }

    #[test]
    fn what_the_program_draws_is_what_the_buffer_says() {
        let mut editor = with_terminal();
        editor.term_output(1, b"$ ls\r\nfile.txt\r\n$ ".to_vec());
        assert_eq!(editor.view().doc.text.to_string(), "$ ls\nfile.txt\n$ ");
        // The cursor is on the line the program left it on. In normal mode
        // it sits on the last character, as it does in any buffer; in
        // terminal mode it sits after the prompt, where typing goes.
        assert_eq!(editor.view().cursor_coords(), (2, 1));
        editor.set_mode(Mode::Insert);
        editor.term_output(1, b"".to_vec());
        assert_eq!(editor.view().cursor_coords(), (2, 2));
        editor.set_mode(Mode::Normal);

        // A screen redrawn in place is a buffer redrawn in place, not one
        // with the old screen still in it.
        editor.term_output(1, b"\x1b[H\x1b[2Jcleared".to_vec());
        assert_eq!(editor.view().doc.text.to_string(), "cleared");

        // And `u` in a terminal does nothing: the text is the program's.
        editor.undo();
        assert_eq!(editor.view().doc.text.to_string(), "cleared");
    }

    #[test]
    fn output_for_a_buffer_nobody_is_looking_at_still_arrives() {
        let mut editor = with_terminal();
        editor.views.push(View::new(crate::buffer::Document::scratch()));
        editor.switch_to(1);
        editor.term_output(1, b"working".to_vec());
        assert_eq!(editor.views[0].doc.text.to_string(), "working");
        assert_eq!(editor.view().doc.text.to_string(), "", "and not into this one");
        // A token from a terminal that has been closed is dropped rather
        // than landing in whatever buffer is there now.
        editor.term_output(9, b"stale".to_vec());
        assert_eq!(editor.views[1].doc.text.to_string(), "");
    }

    #[test]
    fn i_in_a_terminal_buffer_types_in_the_program() {
        let mut editor = with_terminal();
        editor.set_mode(Mode::Insert);
        assert_eq!(editor.mode, Mode::Terminal, "insert in a terminal is terminal mode");
        // With no pty behind it the key goes nowhere, but it is still the
        // program's key and not the editor's: the buffer is untouched.
        editor.terminal_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(editor.view().doc.text.to_string(), "");

        // In an ordinary buffer `i` is still insert mode.
        editor.views.push(View::new(crate::buffer::Document::scratch()));
        editor.switch_to(1);
        editor.set_mode(Mode::Insert);
        assert_eq!(editor.mode, Mode::Insert);
    }

    #[test]
    fn a_program_that_has_finished_leaves_its_output_behind() {
        let mut editor = with_terminal();
        editor.set_mode(Mode::Insert);
        editor.term_output(1, b"done\r\n".to_vec());
        editor.term_gone(1);
        assert_eq!(editor.view().doc.text.to_string(), "done\n");
        assert_eq!(editor.mode, Mode::Normal, "and hands the keyboard back");
        assert!(editor.view().name().contains("finished"), "{}", editor.view().name());
        // The buffer says what it is, wherever a name is shown.
        let terminal = editor.view().terminal.as_ref().expect("a terminal");
        assert!(terminal.over);
    }

    #[test]
    fn the_buffer_is_called_after_what_is_running_in_it() {
        let mut editor = with_terminal();
        assert_eq!(editor.view().name(), "[term: sh]");
        // A program that says what it is called says it in an escape
        // sequence, the way it would to a window manager.
        editor.term_output(1, b"\x1b]0;~/code/jack\x07".to_vec());
        assert_eq!(editor.view().name(), "[term: ~/code/jack]");
    }
}
