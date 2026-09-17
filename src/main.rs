mod buffer;
#[cfg(test)]
mod budget;
mod clipboard;
mod comment;
mod command;
mod complete;
mod editor;
mod info;
mod history;
mod jump;
mod keys;
mod lsp;
mod indent;
mod object;
mod picker;
mod positions;
mod register;
mod screen;
mod search;
mod status;
mod stream;
mod substitute;
mod syntax;
mod theme;
mod ui;
mod view;
mod window;

use anyhow::{Context, Result};
use crossterm::cursor::{SetCursorStyle, Show};
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::{execute, queue};
use crossterm::event::{DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use std::io::{self, Write};

use editor::{Editor, Mode};
use keys::{Action, Keys};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;
use stream::Message;

/// Owns the raw-mode / alternate-screen state so it is undone on every exit
/// path, including `?` returning an error out of `run`.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        // Bracketed paste: the terminal wraps pasted text in markers, so it
        // arrives as one event rather than as a burst of keystrokes that
        // normal mode would read as commands and insert mode would auto-indent
        // line by line.
        // Focus reports: coming back to the terminal is when to look for
        // files another program changed while you were away.
        execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste, EnableFocusChange)?;
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
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
        SetCursorStyle::DefaultUserShape,
        Show
    );
    let _ = terminal::disable_raw_mode();
}

/// The directory to start in, when that is what the arguments name. One
/// directory and nothing else: `jack .` is a way of saying "this project",
/// and mixing it with file names would be asking for two things at once.
fn start_directory(paths: &[String]) -> Option<&String> {
    match paths {
        [only] if std::path::Path::new(only).is_dir() => Some(only),
        _ => None,
    }
}

/// What `--version` and `--help` say. Every packaging recipe reaches for one
/// of these to check the thing it just installed actually runs, and a text
/// editor that has to be opened to answer is no use to a build script.
const USAGE: &str = "\
jack - a terminal text editor

Usage:
  jack [file ...]   open files
  jack <dir>        start in that directory with the file picker open
  jack              an empty buffer

Options:
  -h, --help        this
  -V, --version     the version

Inside: :help is <space>?, and :config writes a config file of every default.
";

/// `--help` and `--version` before anything else opens a terminal. `Some` is
/// what to print and leave.
fn flag(paths: &[String]) -> Option<String> {
    match paths.iter().find(|arg| arg.starts_with('-')).map(String::as_str) {
        Some("-h" | "--help") => Some(USAGE.to_string()),
        Some("-V" | "--version") => {
            Some(format!("jack {}\n", env!("CARGO_PKG_VERSION")))
        }
        // Anything else starting with a dash is a mistake worth saying so
        // about, rather than a file to create called `--colour`.
        Some(other) => Some(format!("jack: not an option: {other}\n\n{USAGE}")),
        None => None,
    }
}

fn main() -> Result<()> {
    let paths: Vec<String> = std::env::args().skip(1).collect();

    if let Some(text) = flag(&paths) {
        print!("{text}");
        return Ok(());
    }

    // A directory is not a buffer: it means start in that project with the
    // file picker open. Nothing here is a mode - the picker already walks the
    // working directory, so this is a `cd` and an empty editor.
    let directory = start_directory(&paths).cloned();
    if let Some(directory) = &directory {
        std::env::set_current_dir(directory)
            .with_context(|| format!("entering {directory}"))?;
    }

    let files: &[String] = match directory {
        Some(_) => &[],
        None => &paths,
    };
    let mut editor = Editor::open(files)?;
    editor.load_config();
    editor.set_positions(positions::Positions::load(positions::store_path()));

    // Without this a panic leaves the user's shell in raw mode on the alternate
    // screen, with no echo and no visible prompt.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default_hook(info);
    }));

    let (tx, rx) = stream::channels();
    editor.set_jobs(tx.clone());
    // After `set_jobs`, because the picker needs somewhere to send the walk.
    if directory.is_some() {
        editor.open_file_picker();
    }

    let guard = TerminalGuard::enter()?;
    let input = stream::Input::new();
    stream::spawn_input(tx, input.clone());
    let result = run(&mut editor, rx, &input);
    // The terminal back first, so that a complaint about the list is printed
    // somewhere it can be read.
    drop(guard);
    editor.save_positions();
    result
}

/// How long a pause in typing means the dog has stopped running. Long enough
/// that it keeps going between words, short enough that it sits down while you
/// are thinking.
const DOG_REST: Duration = Duration::from_millis(700);

fn run(editor: &mut Editor, rx: Receiver<Message>, input: &stream::Input) -> Result<()> {
    let mut out = io::stdout();
    let mut screen = screen::Screen::new();
    let mut keys = Keys::default();
    // Set once a quit is attempted with unsaved changes; any other key clears it.
    let mut quit_armed = false;
    let mut shown_mode = None;

    loop {
        // `:!cmd` - the terminal goes to another program, and comes back when
        // it is done. Before the frame, because there is no sense drawing one
        // onto a screen that is about to be handed away.
        if let Some(command) = editor.shell.take() {
            hand_over(editor, &command, input)?;
            // The other program drew all over the terminal, and the screen's
            // record of what is on it is now fiction.
            screen = screen::Screen::new();
            shown_mode = None;
        }

        // A stat per buffer, at most once a second - and first, so that what
        // it reloads is scrolled to, synced and drawn in this same frame.
        editor.watch_disk();

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
        // Cheap too: an edit count per buffer.
        editor.lsp_sync();

        ui::draw(editor, &keys, screen.begin(cols, rows));
        screen.present(&mut out, editor.cursor_screen())?;

        // Anything the editor wants said to the terminal itself rather than
        // drawn - an OSC 52 copy - goes out with the frame, never behind it.
        if let Some(escape) = editor.escape.take() {
            out.write_all(escape.as_bytes())?;
        }

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
        // While the dog is running, wait with a timeout rather than for ever:
        // the moment nothing arrives is the moment typing has stopped, which
        // is the one thing the dog needs a clock for.
        let mut message = match editor.dog.running {
            false => rx.recv()?,
            true => match rx.recv_timeout(DOG_REST) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    editor.dog_rests();
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            },
        };
        // Batches waiting behind each other are merged, so a fast walk costs
        // one update rather than one per 512 paths.
        let mut streamed: Vec<String> = Vec::new();
        let mut streamed_token = 0;
        let mut streamed_done = false;
        loop {
            if let Message::Key(key) = message {
                editor.message.clear();
                // The box from a `K` is read and gone, the same as a message.
                editor.dismiss_hover();
                let was_armed = std::mem::take(&mut quit_armed);
                // The dog runs on the cursor, not on the keyboard: a key that
                // moves nothing - `esc`, a `:w`, `l` at the end of a line -
                // is not a step, and neither is a leant-on key that has run
                // out of line to move along.
                let was_at = editor.cursor_mark();

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
                if editor.cursor_mark() != was_at {
                    editor.dog_runs();
                }
            } else if let Message::Paste(text) = message {
                editor.message.clear();
                editor.dismiss_hover();
                editor.paste(&text);
                editor.dog_runs();
            } else if let Message::Focus = message {
                editor.focus_gained();
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
            } else if let Message::Lsp { server, message } = message {
                editor.lsp_message(server, message);
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

/// Give the terminal to another program, wait for it, and take it back.
///
/// The whole terminal: raw mode off, off the alternate screen, and the reader
/// thread told to stop reading so the program gets every key it is sent.
/// Anything less and a full-screen program - which is what this is for - gets
/// half a keyboard and a screen jack is still drawing on.
fn hand_over(editor: &mut Editor, command: &str, input: &stream::Input) -> Result<()> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    input.pause();
    restore();

    let mut out = io::stdout();
    let status = match command.is_empty() {
        // `:sh`: a shell of your own, and `exit` comes back here.
        true => {
            let _ = writeln!(out, "{shell}  (exit to come back)\r");
            std::process::Command::new(&shell).status()
        }
        false => {
            let _ = writeln!(out, ":!{command}\r");
            std::process::Command::new(&shell).arg("-c").arg(command).status()
        }
    };
    let said = match status {
        Ok(status) if status.success() => String::new(),
        Ok(status) => match status.code() {
            Some(code) => format!("[exit {code}] "),
            None => "[killed] ".to_string(),
        },
        Err(err) => format!("[{err}] "),
    };

    // The pause before coming back, so whatever the program printed can be
    // read. A full-screen program leaves nothing to read, but it is not worth
    // guessing which sort this was.
    let _ = write!(out, "\n{said}[any key to return to jack] ");
    let _ = out.flush();
    // In raw mode, and a key rather than a line: a line would need the
    // terminal's own line editing, which is exactly what raw mode is not, and
    // whether `enter` even arrives as a newline depends on how the terminal
    // was set up before jack started.
    terminal::enable_raw_mode()?;
    while let Ok(event) = event::read() {
        if matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press) {
            break;
        }
    }
    execute!(out, EnterAlternateScreen, EnableBracketedPaste, EnableFocusChange)?;
    input.resume();
    // What it did to the files that are open here.
    editor.reload_changed_files();
    Ok(())
}

fn cursor_style(mode: Mode) -> SetCursorStyle {
    match mode {
        Mode::Insert => SetCursorStyle::SteadyBar,
        // Visual mode's cursor sits on a character, like normal mode's.
        _ => SetCursorStyle::SteadyBlock,
    }
}

#[cfg(test)]
mod tests {
    use super::{flag, start_directory};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_flags_answer_without_opening_a_terminal() {
        let version = flag(&args(&["--version"])).expect("a version");
        assert_eq!(version, format!("jack {}\n", env!("CARGO_PKG_VERSION")));
        assert_eq!(flag(&args(&["-V"])), Some(version));

        assert!(flag(&args(&["--help"])).unwrap().starts_with("jack - "));
        assert_eq!(flag(&args(&["-h"])), flag(&args(&["--help"])));
    }

    #[test]
    fn an_unknown_flag_says_so_rather_than_being_a_file_name() {
        let message = flag(&args(&["--colour"])).expect("a complaint");
        assert!(message.starts_with("jack: not an option: --colour"));
        // Files are not flags, whatever they are called.
        assert_eq!(flag(&args(&["src/main.rs", "README.md"])), None);
        assert_eq!(flag(&[]), None);
    }

    #[test]
    fn one_directory_argument_names_where_to_start() {
        assert_eq!(start_directory(&["src".to_string()]), Some(&"src".to_string()));
        assert_eq!(start_directory(&[".".to_string()]), Some(&".".to_string()));
    }

    #[test]
    fn a_file_or_a_list_is_not_a_directory_to_start_in() {
        assert_eq!(start_directory(&["src/main.rs".to_string()]), None);
        assert_eq!(start_directory(&[]), None);
        // Asking for a directory *and* a file is asking for two things.
        assert_eq!(start_directory(&["src".to_string(), "src/main.rs".to_string()]), None);
    }
}
