mod buffer;
mod complete;
mod editor;
mod history;
mod jump;
mod keys;
mod object;
mod picker;
mod register;
mod screen;
mod search;
mod status;
mod stream;
mod syntax;
mod theme;
mod ui;
mod view;

use anyhow::Result;
use crossterm::cursor::{SetCursorStyle, Show};
use crossterm::{execute, queue};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use std::io::{self, Write};

use editor::{Editor, Mode};
use keys::{Action, Keys};
use std::sync::mpsc::Receiver;
use stream::Message;

/// Owns the raw-mode / alternate-screen state so it is undone on every exit
/// path, including `?` returning an error out of `run`.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape,
        Show
    );
    let _ = terminal::disable_raw_mode();
}

fn main() -> Result<()> {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let mut editor = Editor::open(&paths)?;
    editor.load_config();

    // Without this a panic leaves the user's shell in raw mode on the alternate
    // screen, with no echo and no visible prompt.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default_hook(info);
    }));

    let (tx, rx) = stream::channels();
    editor.set_jobs(tx.clone());

    let _guard = TerminalGuard::enter()?;
    stream::spawn_input(tx);
    run(&mut editor, rx)
}

fn run(editor: &mut Editor, rx: Receiver<Message>) -> Result<()> {
    let mut out = io::stdout();
    let mut screen = screen::Screen::new();
    let mut keys = Keys::default();
    // Set once a quit is attempted with unsaved changes; any other key clears it.
    let mut quit_armed = false;
    let mut shown_mode = None;

    loop {
        let (cols, rows) = terminal::size()?;
        let (cols, rows) = (cols.max(1) as usize, rows.max(2) as usize);
        // One row goes to the status line, and one to the buffer list when it
        // is showing. The tabline depends on how many buffers are open, not on
        // the size, so this is safe to ask before setting the size.
        let chrome = 1 + editor.top();
        editor.set_viewport(cols, rows.saturating_sub(chrome));
        editor.scroll_to_cursor();
        // Cheap when nothing has changed: it compares a revision first.
        editor.refresh_signs();

        ui::draw(editor, &keys, screen.begin(cols, rows));
        screen.present(&mut out, editor.cursor_screen())?;

        // A block cursor in normal mode, a bar in insert, as the mode changes.
        // A bar while typing in the picker, otherwise the mode's cursor.
        let wanted = match editor.picker.is_some() || editor.prompt.is_some() {
            true => Mode::Insert,
            false => editor.mode,
        };
        if shown_mode != Some(wanted) {
            queue!(out, cursor_style(wanted))?;
            shown_mode = Some(wanted);
        }
        out.flush()?;

        // Block until something happens - a key, a resize, or a batch from a
        // background job - then take everything else that is already waiting
        // before drawing again. A walk sending 512 paths at a time must not
        // cost 512 frames.
        let mut message = rx.recv()?;
        // Batches waiting behind each other are merged, so a fast walk costs
        // one update rather than one per 512 paths.
        let mut streamed: Vec<String> = Vec::new();
        let mut streamed_token = 0;
        let mut streamed_done = false;
        loop {
            if let Message::Key(key) = message {
                editor.message.clear();
                let was_armed = std::mem::take(&mut quit_armed);

                match keys.handle(editor, key) {
                    Action::Continue => {}
                    Action::Quit { force } => {
                        if force || was_armed || !editor.any_modified() {
                            return Ok(());
                        }
                        editor.message = "unsaved changes - press ^Q again to quit".into();
                        quit_armed = true;
                    }
                }
            } else if let Message::Items { token, items, done } = message {
                if token != streamed_token {
                    editor.stream_items(streamed_token, std::mem::take(&mut streamed), streamed_done);
                    streamed_token = token;
                }
                streamed.extend(items);
                streamed_done = done;
            } else if let Message::Failed { token, error } = message {
                editor.job_failed(token, error);
            } else if let Message::Signs { token, signs } = message {
                editor.set_signs(token, signs);
            }
            // Resize needs nothing: the next frame re-reads the terminal size.

            match rx.try_recv() {
                Ok(next) => message = next,
                Err(_) => break,
            }
        }
        if !streamed.is_empty() || streamed_done {
            editor.stream_items(streamed_token, streamed, streamed_done);
        }
    }
}

fn cursor_style(mode: Mode) -> SetCursorStyle {
    match mode {
        Mode::Insert => SetCursorStyle::SteadyBar,
        // Visual mode's cursor sits on a character, like normal mode's.
        _ => SetCursorStyle::SteadyBlock,
    }
}
