//! `jack --gui`: the editor in a window of its own.
//!
//! The same editor, the same keys, the same frame - `ui::draw` fills a grid of
//! cells and something turns that into pixels instead of escape codes. What a
//! window changes is the three things that were the terminal's rather than the
//! editor's: where the keys come from, where the picture goes, and what `:!`
//! hands the screen over to when there is no screen to hand over.

mod colors;
mod input;
mod paint;

use std::num::NonZeroU32;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Window, WindowId};

use crate::editor::{Editor, Mode};
use crate::screen::Screen;
use crate::session::{Flow, Session};
use crate::stream::Message;
use crate::ui;
use paint::Painter;

/// The window's size when it has no better idea, in cells. Eighty columns
/// because that is what a line of code is, and enough rows to see a function.
const COLUMNS: u32 = 100;
const ROWS: u32 = 30;

/// The editor in a window until it is closed. Background jobs reach it the
/// same way they reach the terminal frontend - one channel - forwarded into
/// the window's own event queue by a thread that does nothing else.
pub fn run(editor: &mut Editor, rx: Receiver<Message>) -> Result<()> {
    let events = EventLoop::<Message>::with_user_event()
        .build()
        .context("opening a window (is there a display to open it on?)")?;
    let proxy = events.create_proxy();
    std::thread::spawn(move || {
        while let Ok(message) = rx.recv() {
            if proxy.send_event(message).is_err() {
                return;
            }
        }
    });

    let configured = (editor.guifont.clone(), editor.guifontsize);
    let painter = Painter::new(&configured.0, configured.1);
    let mut app = App {
        editor,
        session: Session::default(),
        screen: Screen::new(),
        painter,
        window: None,
        surface: None,
        modifiers: ModifiersState::empty(),
        title: String::new(),
        configured,
        scale: 1.0,
        resting: None,
        failed: None,
    };
    events.run_app(&mut app)?;
    match app.failed.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

struct App<'a> {
    editor: &'a mut Editor,
    session: Session,
    /// Only for the grid it hands out: a window presents its own pixels, so
    /// the escape codes and the damage tracking behind this go unused.
    screen: Screen,
    painter: Painter,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    modifiers: ModifiersState,
    title: String,
    /// The font the painter was built for, to notice a `:set guifont` while
    /// running - the size it draws at is this one scaled to the display.
    configured: (String, f32),
    scale: f64,
    /// When the editor last went quiet, for the dog: it sits down when
    /// nothing has arrived for a while, and nothing arriving is not an event
    /// anything else here would wake for.
    resting: Option<Instant>,
    /// Something that went wrong in a callback, which cannot return an error
    /// of its own. Kept until the loop is over and then raised.
    failed: Option<anyhow::Error>,
}

impl App<'_> {
    /// Draw a frame, having first done everything a frame is due.
    fn frame(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        // A `:set guifont` while running is a font to draw the next frame in.
        let asked = (self.editor.guifont.clone(), self.editor.guifontsize);
        if asked != self.configured {
            self.painter = Painter::new(&asked.0, asked.1 * self.scale as f32);
            self.configured = asked;
        }
        self.hand_over();

        let size = window.inner_size();
        let (width, height) = (size.width.max(1) as usize, size.height.max(1) as usize);
        let (cols, rows) = self.painter.grid(width, height);
        // One row for the status line, and one for the buffer list when it is
        // showing, exactly as the terminal frontend counts them.
        let chrome = 1 + self.editor.top();
        self.session.before_frame(self.editor, cols, rows.saturating_sub(chrome));

        let grid = self.screen.begin(cols, rows);
        ui::draw(self.editor, &self.session.keys, grid);
        // A bar while typing, a block otherwise - the shapes the terminal
        // frontend asks the terminal for.
        let bar = matches!(self.editor.mode, Mode::Insert)
            || self.editor.picker.is_some()
            || self.editor.prompt.is_some();
        let (x, y) = self.editor.cursor_screen();
        // The window's clipboard needs no escape code, and OSC 52 would go
        // nowhere: `clipboard.rs` has already done the copy.
        self.editor.escape.take();

        let Some(display) = self.surface.as_mut() else {
            return;
        };
        let (Some(w), Some(h)) = (NonZeroU32::new(width as u32), NonZeroU32::new(height as u32)) else {
            return;
        };
        if let Err(err) = display.resize(w, h) {
            self.failed = Some(anyhow::anyhow!("{err}"));
            return;
        }
        let mut pixels = match display.buffer_mut() {
            Ok(pixels) => pixels,
            Err(err) => {
                self.failed = Some(anyhow::anyhow!("{err}"));
                return;
            }
        };
        self.painter.paint(grid, &mut pixels, (width, height), (x as usize, y as usize), bar);
        if let Err(err) = pixels.present() {
            self.failed = Some(anyhow::anyhow!("{err}"));
        }

        let title = window_title(self.editor);
        if title != self.title {
            window.set_title(&title);
            self.title = title;
        }
    }

    /// `:!cmd`, `:sh` and `^z` in a window.
    ///
    /// There is no terminal here to hand over, so the command gets one of its
    /// own: a terminal emulator, started on it. `^z` has nothing to be
    /// suspended into at all, and says so rather than appearing to work.
    fn hand_over(&mut self) {
        if std::mem::take(&mut self.editor.suspend) {
            self.editor.message = "no shell to stop into: this is a window, not a terminal".into();
        }
        let Some(command) = self.editor.shell.take() else {
            return;
        };
        let Some(terminal) = terminal_program() else {
            self.editor.message = "no terminal to run it in: set $TERMINAL".into();
            return;
        };
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        // A shell that stays open after the command, so whatever it printed
        // can be read - the pause the terminal frontend gets for free.
        let line = match command.is_empty() {
            true => shell.clone(),
            false => format!("{command}; printf '\\n[enter to close] '; read _"),
        };
        match std::process::Command::new(&terminal).arg("-e").arg(&shell).arg("-c").arg(&line).spawn() {
            Ok(_) => self.editor.message = format!("running in {terminal}"),
            Err(err) => self.editor.message = format!("{terminal}: {err}"),
        }
    }

    /// `ctrl` with `+`, `-` or `0`: bigger, smaller, back to what the config
    /// file says. Handled here rather than passed on, because a window is the
    /// only thing that has a font size to change.
    fn zoom(&mut self, key: &Key) -> bool {
        if !self.modifiers.control_key() {
            return false;
        }
        let Key::Character(text) = key else {
            return false;
        };
        let size = self.painter.size();
        match text.as_str() {
            "+" | "=" => self.painter.set_size(size + 1.0),
            "-" => self.painter.set_size(size - 1.0),
            "0" => self.painter.set_size(self.editor.guifontsize * self.scale as f32),
            _ => return false,
        }
        self.editor.message = format!("{}px", self.painter.size().round());
        true
    }

    fn deliver(&mut self, event_loop: &ActiveEventLoop, message: Message) {
        self.resting = None;
        if self.session.deliver(self.editor, message) == Flow::Quit {
            event_loop.exit();
            return;
        }
        self.session.flush_stream(self.editor);
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler<Message> for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (cell_w, cell_h) = self.painter.cell;
        let size = winit::dpi::PhysicalSize::new(COLUMNS * cell_w as u32, ROWS * cell_h as u32);
        let attributes = named(Window::default_attributes().with_title("jack").with_inner_size(size));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.failed = Some(anyhow::anyhow!("{err}"));
                event_loop.exit();
                return;
            }
        };
        // Text is measured in pixels, so a screen that scales its pixels
        // scales the font with them - otherwise jack is half-size on a
        // laptop and enormous on a monitor beside it.
        self.scale = window.scale_factor();
        self.painter.set_size(self.editor.guifontsize * self.scale as f32);

        match softbuffer::Context::new(window.clone()) {
            Ok(context) => match softbuffer::Surface::new(&context, window.clone()) {
                Ok(surface) => self.surface = Some(surface),
                Err(err) => self.failed = Some(anyhow::anyhow!("{err}")),
            },
            Err(err) => self.failed = Some(anyhow::anyhow!("{err}")),
        }
        if self.failed.is_some() {
            event_loop.exit();
            return;
        }
        window.request_redraw();
        self.window = Some(window);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, message: Message) {
        self.deliver(event_loop, message);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // The window's close button is `^q`: what it does about
                // unsaved changes is the editor's business, not the window's.
                self.deliver(event_loop, Message::Key(crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('q'),
                    crossterm::event::KeyModifiers::CONTROL,
                )));
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = self.window.as_ref() {
                    let scale = window.scale_factor();
                    if scale != self.scale {
                        self.scale = scale;
                        self.painter.set_size(self.editor.guifontsize * scale as f32);
                    }
                    window.request_redraw();
                }
            }
            WindowEvent::Focused(true) => self.deliver(event_loop, Message::Focus),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if self.zoom(&event.logical_key) {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                    return;
                }
                if let Some(key) = input::key_event(&event.logical_key, self.modifiers) {
                    self.deliver(event_loop, Message::Key(key));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // The wheel is `^e` and `^y`, which is what scrolling the view
                // without moving the cursor already is.
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => position.y as f32 / 20.0,
                };
                let (key, count) = match lines < 0.0 {
                    true => ('e', (-lines).round() as usize),
                    false => ('y', lines.round() as usize),
                };
                for _ in 0..count.clamp(1, 10) {
                    self.deliver(event_loop, Message::Key(crossterm::event::KeyEvent::new(
                        crossterm::event::KeyCode::Char(key),
                        crossterm::event::KeyModifiers::CONTROL,
                    )));
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // The dog is the one thing here that needs a clock: it stops running
        // when nothing has arrived for a while, and nothing arriving is not
        // an event anything else would wake up for.
        match self.editor.dog.running {
            true => {
                // Nothing has arrived since the last frame, or this would not
                // be the wait: one rest's worth of that is the dog stopping.
                match self.resting {
                    Some(since) if since.elapsed() >= crate::DOG_REST => {
                        self.editor.dog_rests();
                        self.resting = None;
                        if let Some(window) = self.window.as_ref() {
                            window.request_redraw();
                        }
                        event_loop.set_control_flow(ControlFlow::Wait);
                    }
                    Some(since) => {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(since + crate::DOG_REST));
                    }
                    None => {
                        self.resting = Some(Instant::now());
                        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + crate::DOG_REST));
                    }
                }
            }
            false => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// The window's name to the desktop - `app_id` on Wayland, the class on X11 -
/// which is what a window rule, a taskbar and an icon theme all look for. Not
/// the title: that changes with the file, and a rule that follows the file is
/// no rule at all.
#[cfg(all(unix, not(target_os = "macos")))]
fn named(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::wayland::WindowAttributesExtWayland;
    use winit::platform::x11::WindowAttributesExtX11;
    WindowAttributesExtX11::with_name(
        WindowAttributesExtWayland::with_name(attributes, "jack", "jack"),
        "jack",
        "jack",
    )
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn named(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attributes
}

/// What the window is called: the file, and whether it has been changed.
fn window_title(editor: &Editor) -> String {
    let view = editor.view();
    let modified = match view.is_modified() {
        true => " +",
        false => "",
    };
    format!("{}{modified} - jack", view.doc.display_name())
}

/// A terminal emulator to run `:!cmd` in: what the session says it uses, and
/// failing that whatever is installed, in the order a desktop is likely to
/// have them.
fn terminal_program() -> Option<String> {
    if let Ok(terminal) = std::env::var("TERMINAL")
        && !terminal.is_empty()
    {
        return Some(terminal);
    }
    let known = ["ghostty", "alacritty", "kitty", "foot", "wezterm", "konsole", "gnome-terminal", "xterm"];
    known.iter().find(|program| which(program)).map(|program| program.to_string())
}

fn which(program: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(program).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_says_the_file_and_whether_it_has_been_changed() {
        let mut editor = Editor::scratch();
        assert!(window_title(&editor).ends_with(" - jack"), "{}", window_title(&editor));
        assert!(!window_title(&editor).contains('+'));

        editor.insert("x");
        assert!(window_title(&editor).contains("+ - jack"), "{}", window_title(&editor));
    }

    #[test]
    fn a_terminal_for_the_shell_commands_is_whatever_the_session_says() {
        // SAFETY: single-threaded test, and the variable is read back here.
        unsafe { std::env::set_var("TERMINAL", "my-terminal") };
        assert_eq!(terminal_program().as_deref(), Some("my-terminal"));
        unsafe { std::env::set_var("TERMINAL", "") };
        // Empty is not an answer; fall through to what is installed, which on
        // a machine with none is nothing at all.
        assert_ne!(terminal_program().as_deref(), Some(""));
        unsafe { std::env::remove_var("TERMINAL") };
    }
}
