mod buffer;
mod editor;
mod history;
mod screen;
mod syntax;
mod theme;
mod ui;

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use std::io::{self, Write};

use editor::{Editor, Move};

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
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    let _ = terminal::disable_raw_mode();
}

fn main() -> Result<()> {
    let mut editor = match std::env::args().nth(1) {
        Some(path) => Editor::open(path)?,
        None => Editor::scratch(),
    };

    // Without this a panic leaves the user's shell in raw mode on the alternate
    // screen, with no echo and no visible prompt.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default_hook(info);
    }));

    let _guard = TerminalGuard::enter()?;
    run(&mut editor)
}

fn run(editor: &mut Editor) -> Result<()> {
    let mut out = io::stdout();
    let mut screen = screen::Screen::new();
    // Set once a quit is attempted with unsaved changes; any other key clears it.
    let mut quit_armed = false;

    loop {
        let (cols, rows) = terminal::size()?;
        let (cols, rows) = (cols.max(1) as usize, rows.max(2) as usize);
        // One row goes to the status line.
        editor.set_viewport(cols, rows - 1);
        editor.scroll_to_cursor();

        ui::draw(editor, screen.begin(cols, rows));
        screen.present(&mut out, editor.cursor_screen())?;
        out.flush()?;

        // Resize events just fall through and redraw on the next iteration.
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            editor.message.clear();
            let was_armed = std::mem::take(&mut quit_armed);

            match handle_key(editor, key) {
                Action::Continue => {}
                Action::Quit => {
                    if was_armed || !editor.is_modified() {
                        return Ok(());
                    }
                    editor.message = "unsaved changes - press ^Q again to quit".into();
                    quit_armed = true;
                }
            }
        }
    }
}

enum Action {
    Continue,
    Quit,
}

fn handle_key(editor: &mut Editor, key: KeyEvent) -> Action {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let extend = key.modifiers.contains(KeyModifiers::SHIFT);

    if ctrl {
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('s') => {
                editor.save();
                return Action::Continue;
            }
            KeyCode::Char('z') => {
                editor.undo();
                return Action::Continue;
            }
            KeyCode::Char('y') => {
                editor.redo();
                return Action::Continue;
            }
            _ => {}
        }
    }

    let motion = match key.code {
        // Ctrl/Alt chords that got this far are unbound, not text to insert.
        KeyCode::Char(c) if !ctrl && !alt => {
            editor.insert(&c.to_string());
            return Action::Continue;
        }
        KeyCode::Enter => {
            editor.insert_newline();
            return Action::Continue;
        }
        KeyCode::Tab => {
            editor.insert("\t");
            return Action::Continue;
        }
        KeyCode::Backspace => {
            editor.delete_backward();
            return Action::Continue;
        }
        KeyCode::Delete => {
            editor.delete_forward();
            return Action::Continue;
        }
        KeyCode::Left => Move::Left,
        KeyCode::Right => Move::Right,
        KeyCode::Up => Move::Up,
        KeyCode::Down => Move::Down,
        KeyCode::Home if ctrl => Move::FileStart,
        KeyCode::End if ctrl => Move::FileEnd,
        KeyCode::Home => Move::LineStart,
        KeyCode::End => Move::LineEnd,
        KeyCode::PageUp => Move::PageUp,
        KeyCode::PageDown => Move::PageDown,
        _ => return Action::Continue,
    };

    editor.move_cursor(motion, extend);
    Action::Continue
}
