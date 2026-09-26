//! What happens between one frame and the next, whatever is drawing them.
//!
//! The terminal and the window are two ways of showing the same editor and
//! feeding it the same messages. Everything they would otherwise each have a
//! copy of lives here: the work due before a frame, and what a message means.
//! A frontend is then a window, a keyboard and a painter, and nothing that
//! decides anything.

use crate::editor::Editor;
use crate::keys::{Action, Keys};
use crate::stream::Message;

/// Whether the editor wants to carry on after a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// The state a run loop keeps between messages, which is less than it looks:
/// the pending key sequence, whether a quit has been refused once, and the
/// items a background walk has sent but which have not been shown yet.
#[derive(Default)]
pub struct Session {
    pub keys: Keys,
    /// Set when a quit is attempted with unsaved changes; any other key
    /// clears it, so `^q ^q` quits and `^q j ^q` does not.
    quit_armed: bool,
    /// Items from a background job, merged until the next frame. A walk
    /// sending 512 paths at a time must cost one update, not 512.
    streamed: Vec<String>,
    streamed_token: u64,
    streamed_done: bool,
}

impl Session {
    /// Everything due before a frame is drawn, in the order it is due: what
    /// changed on disk first, so that what it reloads is scrolled to, synced
    /// and drawn in this same frame. `rows` is the text area - the caller has
    /// taken the status line and the buffer list out of it already.
    pub fn before_frame(&mut self, editor: &mut Editor, cols: usize, rows: usize) {
        editor.watch_disk();
        editor.set_viewport(cols, rows);
        editor.scroll_to_cursor();
        // Cheap when nothing has changed: it compares a revision first.
        editor.refresh_signs();
        // Cheap too: an edit count per buffer.
        editor.lsp_sync();
        // And hints for whatever was just synced, outside insert mode.
        editor.lsp_hints();
    }

    /// One message. Everything that has arrived should be delivered before a
    /// frame is drawn, so a burst costs one frame rather than one each.
    pub fn deliver(&mut self, editor: &mut Editor, message: Message) -> Flow {
        match message {
            Message::Key(key) => {
                editor.message.clear();
                // A pat lasts until the next key, the same as a message does.
                editor.dog_forgets();
                // The box from a `K` is read and gone, the same as a message.
                editor.dismiss_hover();
                let was_armed = std::mem::take(&mut self.quit_armed);
                // The dog runs on the cursor, not on the keyboard: a key that
                // moves nothing - `esc`, a `:w`, `l` at the end of a line - is
                // not a step, and neither is a leant-on key that has run out
                // of line to move along.
                let was_at = editor.cursor_mark();

                match self.keys.handle(editor, key) {
                    Action::Continue => {}
                    Action::Quit { force } => {
                        if force || was_armed || !editor.any_modified() {
                            return Flow::Quit;
                        }
                        editor.message = "unsaved changes - press ^Q again to quit".into();
                        // The one thing the editor ever refuses, and a line of
                        // text in the middle of a status line is easy to miss.
                        editor.dog_barks();
                        self.quit_armed = true;
                    }
                }
                if editor.cursor_mark() != was_at {
                    editor.dog_runs();
                }
            }
            Message::Paste(text) => {
                editor.message.clear();
                editor.dismiss_hover();
                editor.paste(&text);
                editor.dog_runs();
            }
            Message::Focus => editor.focus_gained(),
            Message::Items { token, items, done } => {
                if token != self.streamed_token {
                    self.flush_stream(editor);
                    self.streamed_token = token;
                }
                self.streamed.extend(items);
                self.streamed_done = done;
            }
            Message::Failed { token, error } => editor.job_failed(token, error),
            Message::Signs { token, signs, hunks } => editor.set_signs(token, signs, hunks),
            Message::Lsp { server, message } => editor.lsp_message(server, message),
            Message::Built { token, output, ok } => editor.build_finished(token, output, ok),
            Message::Said { token, said, ok } => editor.ai_answered(token, said, ok),
            // A resize needs nothing: the next frame asks how big the screen
            // is, whichever screen it is.
            Message::Resize => {}
        }
        Flow::Continue
    }

    /// Hand the editor whatever a background job has sent since the last
    /// frame. Due once a burst of messages has been delivered and before the
    /// frame that shows them.
    pub fn flush_stream(&mut self, editor: &mut Editor) {
        if self.streamed.is_empty() && !self.streamed_done {
            return;
        }
        let items = std::mem::take(&mut self.streamed);
        let done = std::mem::take(&mut self.streamed_done);
        editor.stream_items(self.streamed_token, items, done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> Message {
        Message::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn a_key_goes_to_the_editor_and_the_editor_carries_on() {
        let mut editor = Editor::scratch();
        let mut session = Session::default();
        assert_eq!(session.deliver(&mut editor, key('i')), Flow::Continue);
        assert_eq!(session.deliver(&mut editor, key('x')), Flow::Continue);
        assert_eq!(editor.view().doc.line_str(0).trim_end(), "x");
    }

    #[test]
    fn quitting_with_unsaved_changes_takes_two_goes() {
        let mut editor = Editor::scratch();
        let mut session = Session::default();
        session.deliver(&mut editor, key('i'));
        session.deliver(&mut editor, key('x'));
        let quit = || Message::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));

        // In insert mode `^q` is not a quit at all; back to normal first.
        session.deliver(&mut editor, Message::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert_eq!(session.deliver(&mut editor, quit()), Flow::Continue);
        assert!(editor.message.contains("unsaved changes"), "{}", editor.message);
        assert_eq!(session.deliver(&mut editor, quit()), Flow::Quit);

        // And a key in between disarms it, so a stray second `^q` is not a
        // quit you did not ask for.
        let mut session = Session::default();
        assert_eq!(session.deliver(&mut editor, quit()), Flow::Continue);
        session.deliver(&mut editor, key('j'));
        assert_eq!(session.deliver(&mut editor, quit()), Flow::Continue);
    }

    #[test]
    fn streamed_items_are_merged_until_the_frame_that_shows_them() {
        let mut editor = Editor::scratch();
        let mut session = Session::default();
        editor.open_file_picker();
        let token = editor.token();
        for batch in 0..3 {
            let items = vec![format!("file{batch}.rs")];
            session.deliver(&mut editor, Message::Items { token, items, done: batch == 2 });
        }
        // Nothing until the flush: three batches are one update.
        assert_eq!(editor.picker.as_ref().map_or(0, |p| p.matches().len()), 0);
        session.flush_stream(&mut editor);
        assert_eq!(editor.picker.as_ref().map_or(0, |p| p.matches().len()), 3);
    }
}
