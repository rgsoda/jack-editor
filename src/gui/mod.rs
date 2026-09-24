//! `jack --gui`: the editor in a window of its own.
//!
//! The same editor, the same keys, the same frame - `ui::draw` fills a grid of
//! cells and something turns that into pixels instead of escape codes. What a
//! window changes is the three things that were the terminal's rather than the
//! editor's: where the keys come from, where the picture goes, and what `:!`
//! hands the screen over to when there is no screen to hand over.

mod colors;
mod input;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac::{dock_icon, opened_files, opening_notes, watch_for_opened_files};
mod paint;

/// Every font family on this machine, for `:guifonts`.
pub fn families() -> Vec<String> {
    Painter::families()
}

use std::num::NonZeroU32;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
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
/// How close together two clicks are a double click. What every toolkit
/// calls the double-click time; nothing here can ask the desktop for its own.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

/// How often a drag held past the edge of its window scrolls it by a line.
const DRAG_SCROLL: std::time::Duration = std::time::Duration::from_millis(60);

const COLUMNS: u32 = 100;
const ROWS: u32 = 30;

/// The editor in a window until it is closed. Background jobs reach it the
/// same way they reach the terminal frontend - one channel - forwarded into
/// the window's own event queue by a thread that does nothing else.
pub fn run(editor: &mut Editor, rx: Receiver<Message>) -> Result<()> {
    // Before the event loop, not after the window: a file dropped on a jack
    // that is not running launches it, and the event asking for that file
    // arrives while the window is still being made.
    watch_for_opened_files();
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
        pointer: (0, 0),
        clicked: None,
        scrolled: None,
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
    /// Where the pointer is, in cells, and the click it is part of: a window
    /// reports presses one at a time, so a double click is two of them close
    /// together in time and in the same cell, counted here.
    pointer: (usize, usize),
    clicked: Option<(Instant, (usize, usize), u8)>,
    /// When a drag held past the edge last scrolled: a redraw brings the loop
    /// straight back here, so the clock has to be kept rather than waited on.
    scrolled: Option<Instant>,
    /// When the editor last went quiet, for the dog: it sits down when
    /// nothing has arrived for a while, and nothing arriving is not an event
    /// anything else here would wake for.
    resting: Option<Instant>,
    /// Something that went wrong in a callback, which cannot return an error
    /// of its own. Kept until the loop is over and then raised.
    failed: Option<anyhow::Error>,
}

impl App<'_> {
    /// The font this frame is drawn in. Normally `guifont`, but while the
    /// font picker is open it is whatever is under the cursor there: choosing
    /// a font is choosing how the text looks, and a list of names is a poor
    /// way to see that. Moving off it, or escaping the picker, puts the
    /// configured font back - nothing was set, only tried on.
    fn wanted_font(&self) -> String {
        match self.trying_on() {
            true => {
                let picker = self.editor.picker.as_ref().expect("a picker to be trying one on");
                let at = picker.matches().get(picker.cursor()).expect("a match under the cursor");
                picker.item(at).target.clone()
            }
            false => self.editor.guifont.clone(),
        }
    }

    /// Whether a font is being tried on rather than used: the font picker is
    /// open and its cursor is on something.
    fn trying_on(&self) -> bool {
        self.editor.picker.as_ref().is_some_and(|picker| {
            picker.source == crate::picker::Source::Fonts && picker.matches().get(picker.cursor()).is_some()
        })
    }

    /// Draw a frame, having first done everything a frame is due.
    fn frame(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        // A `:set guifont` while running is a font to draw the next frame in,
        // and so is the font under the cursor in the font picker.
        let asked = (self.wanted_font(), self.editor.guifontsize);
        if asked != self.configured {
            // A name the machine does not have is answered by the shaper with
            // some other font entirely - often a proportional one, which in a
            // grid is worse than the font you already had. So: keep that, and
            // say what happened. A font being tried on in the picker came from
            // the machine's own list, so it needs no such complaint.
            let painter = Painter::new(&asked.0, asked.1 * self.scale as f32);
            match (painter.matched(), self.trying_on()) {
                (true, _) => self.painter = painter,
                (false, true) => {}
                (false, false) => self.editor.message = format!("no font called {} - try :guifonts", asked.0),
            }
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

    /// How many clicks this press is part of: two in the same cell within the
    /// time a double click takes is a double click, and a third a triple.
    fn clicks(&mut self) -> u8 {
        let count = match self.clicked {
            Some((when, cell, count)) if cell == self.pointer && when.elapsed() < DOUBLE_CLICK => count % 3 + 1,
            _ => 1,
        };
        self.clicked = Some((Instant::now(), self.pointer, count));
        count
    }

    /// What a mouse event owes the frame: the dog is awake, and what changed
    /// wants drawing. The view following the cursor is `before_frame`'s, as
    /// it is for a key press.
    fn after_mouse(&mut self) {
        self.resting = None;
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
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

impl App<'_> {
    /// Open what the desktop has handed over - dropped on the window, dropped
    /// on the Dock icon, or opened with jack from a file manager. A directory
    /// is listed, the same as it would be on the command line.
    fn open_files(&mut self, paths: Vec<std::path::PathBuf>) {
        if paths.is_empty() {
            return;
        }
        for path in paths {
            if let Err(err) = self.editor.open_path(&path) {
                self.editor.message = format!("{err:#}");
            }
        }
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
        let attributes = named(
            Window::default_attributes().with_title("jack").with_inner_size(size).with_window_icon(icon()),
        );
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
        dock_icon();
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
            // A file dragged onto the window, which every desktop has its
            // own way of offering and winit has one way of reporting.
            WindowEvent::DroppedFile(path) => self.open_files(vec![path]),
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
            WindowEvent::CursorMoved { position, .. } => {
                let (cell_w, cell_h) = self.painter.cell;
                let cell = (position.x as usize / cell_w.max(1), position.y as usize / cell_h.max(1));
                if cell == self.pointer {
                    return;
                }
                self.pointer = cell;
                if self.editor.drag.is_some() {
                    self.editor.mouse_drag(cell.0, cell.1);
                    self.after_mouse();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let (x, y) = self.pointer;
                match state {
                    ElementState::Pressed => {
                        let clicks = self.clicks();
                        self.editor.mouse_press(x, y, clicks);
                        self.after_mouse();
                    }
                    ElementState::Released => {
                        self.editor.mouse_release();
                        self.after_mouse();
                    }
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
        // Files the desktop has asked for since the last time round: a drop
        // on the Dock icon, or a double-click in Finder. They arrive from
        // outside the loop, so this is where they are picked up.
        self.open_files(opened_files());
        // And anything the desktop asked for that jack could not make sense
        // of, which is worth saying rather than looking like it was ignored.
        if let Some(note) = opening_notes().pop() {
            self.editor.message = note;
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
        // A drag pointing past the top or bottom of its window scrolls for as
        // long as it is held there, and a pointer held still sends nothing.
        // So the scrolling is on a clock of its own.
        if self.editor.dragging_away(self.pointer.1) {
            let due = self.scrolled.is_none_or(|last| last.elapsed() >= DRAG_SCROLL);
            if due {
                self.scrolled = Some(Instant::now());
                self.editor.mouse_drag(self.pointer.0, self.pointer.1);
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            let next = self.scrolled.unwrap_or_else(Instant::now) + DRAG_SCROLL;
            event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            return;
        }
        self.scrolled = None;
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

/// The icon the window carries, 64 square of raw RGBA - the one shape winit
/// takes, and the one that needs no decoder to read back. `packaging/icon`
/// draws it from `jack.svg`.
///
/// This is X11's and Windows' way of asking. Wayland does not take an icon
/// from the program at all: it looks up the desktop entry that matches the
/// app id, which is why `packaging/jack.desktop` exists, and macOS takes it
/// from the bundle that `packaging/macos/bundle.sh` builds.
const ICON: &[u8] = include_bytes!("icon.rgba");
const ICON_SIDE: u32 = 64;

fn icon() -> Option<winit::window::Icon> {
    winit::window::Icon::from_rgba(ICON.to_vec(), ICON_SIDE, ICON_SIDE).ok()
}

/// Every other platform takes its icon from the window or from a desktop
/// entry, both of which are settled by the time this would be called, and
/// hands a program its files on the command line like anything else.
#[cfg(not(target_os = "macos"))]
fn dock_icon() {}
#[cfg(not(target_os = "macos"))]
fn watch_for_opened_files() {}
#[cfg(not(target_os = "macos"))]
fn opened_files() -> Vec<std::path::PathBuf> {
    Vec::new()
}
#[cfg(not(target_os = "macos"))]
fn opening_notes() -> Vec<String> {
    Vec::new()
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
    fn the_icon_is_the_shape_winit_asks_for() {
        // Raw RGBA, so the size is the only thing that can be wrong with it -
        // and a redrawn icon that came out the wrong size would otherwise be
        // a window with no icon and nothing said about it.
        assert_eq!(ICON.len(), (ICON_SIDE * ICON_SIDE * 4) as usize);
        assert!(icon().is_some(), "winit took it");
    }

    #[test]
    fn the_mac_icon_holds_the_size_each_tag_promises() {
        // An icns is a header and a run of tagged PNGs, and each tag stands
        // for exactly one size: `ic13` is 128 points at two pixels each, so
        // 256. macOS does not scale a picture that came in under the wrong
        // tag - it throws the whole file out and draws the generic icon - so
        // this is a mistake that can only be seen on a mac, which is reason
        // enough to check it here.
        let icns = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/icon/jack.icns"))
            .expect("packaging/icon/jack.icns");
        let word = |at: usize| u32::from_be_bytes(icns[at..at + 4].try_into().unwrap()) as usize;
        assert_eq!(&icns[..4], b"icns");
        assert_eq!(word(4), icns.len(), "the length in the header is the file's");

        // The tags `iconutil` itself writes, and nothing else: `ic04` and
        // `ic05` hold raw ARGB rather than a PNG, and a PNG filed under one
        // of those is enough for macOS to reject the file.
        let sizes = [
            (b"icp4", 16),
            (b"icp5", 32),
            (b"icp6", 64),
            (b"ic07", 128),
            (b"ic08", 256),
            (b"ic09", 512),
            (b"ic10", 1024),
            (b"ic13", 256),
            (b"ic14", 512),
        ];
        let mut at = 8;
        let mut seen = Vec::new();
        while at < icns.len() {
            let tag: [u8; 4] = icns[at..at + 4].try_into().unwrap();
            let length = word(at + 4);
            // The picture is a PNG, and its width and height are the first
            // two words of the IHDR chunk.
            assert_eq!(&icns[at + 8..at + 12], b"\x89PNG".as_slice(), "{:?} is a PNG", tag);
            let (width, height) = (word(at + 8 + 16), word(at + 8 + 20));
            let (_, size) = sizes.iter().find(|(name, _)| *name == &tag).expect("a tag macOS knows");
            assert_eq!((width, height), (*size, *size), "{:?}", std::str::from_utf8(&tag));
            seen.push(tag);
            at += length;
        }
        assert_eq!(seen.len(), sizes.len(), "every size is in it");
        assert_eq!(at, icns.len(), "and the blocks end where the file does");
    }

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
