//! A directory, opened as a buffer.
//!
//! The file picker answers "where is the file called something like this".
//! This answers the other question - what is in here - and it answers it
//! without a pane or a keymap of its own: a directory is text in a buffer, so
//! `j` and `k` walk it, `/` searches it, `^w s` shows two of them at once and
//! `^o` walks back out of wherever you ended up. Nothing below this layer
//! knows it is looking at a directory; the buffer is marked so that the
//! things a listing has no business doing - highlighting it as source,
//! writing it back, reloading it from disk - are left undone.
//!
//! What is deliberately missing is oil's editable listing: renaming a file by
//! editing its line and deleting one by deleting it. That is a good idea and
//! a separate change - looking around should land first.

use std::path::Path;

use anyhow::{Context, Result};

use super::Editor;
use crate::buffer::Document;
use crate::picker::Open;
use crate::view::{Selection, View};

/// One line of a listing: what it is called, and whether it can be gone into.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
}

impl Entry {
    /// The line as it is written. A directory wears a trailing slash, the way
    /// `ls -F` gives it one, so the two kinds are told apart without colour -
    /// which matters on a terminal that has none, and for `/` and `*`.
    pub fn line(&self) -> String {
        match self.directory {
            true => format!("{}/", self.name),
            false => self.name.clone(),
        }
    }
}

/// The name a listing line refers to: its text without the slash a directory
/// is written with.
pub fn name_on(line: &str) -> &str {
    line.trim_end_matches('/').trim_end()
}

/// What a directory holds, in the order it is read in rather than the order
/// the filesystem hands it over in: the way up first, then the directories,
/// then the files, each sorted without regard to case. Dotfiles are in -
/// a directory listing that hides half the directory is a worse answer than
/// a long one, and `/` narrows it down anyway.
pub fn entries(dir: &Path) -> Result<Vec<Entry>> {
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?;
    let mut entries: Vec<Entry> = read
        .flatten()
        .map(|entry| Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            // Asked of the path rather than the entry so that a symlink to a
            // directory is one: what it points at is what going in would do.
            directory: entry.path().is_dir(),
        })
        .collect();
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    if dir.parent().is_some() {
        entries.insert(0, Entry { name: "..".into(), directory: true });
    }
    Ok(entries)
}

/// The listing as the buffer holds it: one entry per line, no trailing
/// newline - there is no empty last line to put the cursor on.
pub fn text(entries: &[Entry]) -> String {
    entries.iter().map(Entry::line).collect::<Vec<_>>().join("\n")
}

impl Editor {
    /// Whether the focused buffer is a listing, which is what the keys that
    /// only mean something in one ask before doing anything.
    pub fn in_listing(&self) -> bool {
        self.view().listing
    }

    /// The directory the focused listing is showing.
    pub fn listing_dir(&self) -> Option<&Path> {
        match self.view().listing {
            true => self.view().doc.path.as_deref(),
            false => None,
        }
    }

    /// `:e path` and the file arguments: a directory is listed, anything else
    /// is opened as a file.
    pub fn open_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        let opened = match path.is_dir() {
            true => self.open_listing(path, None),
            false => self.open_file(path),
        };
        // A window opened from a launcher has no directory anyone chose, and
        // this is the first thing it has been told about the work: take it.
        // After the first, `:cd`.
        if opened.is_ok() {
            self.settle_near(path);
        }
        opened
    }

    /// Show `dir` in the focused window, with the cursor on `on` if that is
    /// one of the names in it - which is how coming up from a file lands on
    /// the file you came from rather than at the top.
    pub fn open_listing(&mut self, dir: &Path, on: Option<&str>) -> Result<()> {
        // The path a listing is filed under is canonical, so that `..` and
        // `.` and a symlinked route to the same directory are one buffer.
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let entries = entries(&dir)?;
        let mut document = Document::scratch();
        document.text = ropey::Rope::from_str(&text(&entries));
        document.path = Some(dir.clone());

        let existing = self
            .views
            .iter()
            .position(|view| view.listing && view.doc.path.as_deref() == Some(dir.as_path()));
        let index = match existing {
            // Read again rather than shown as it was: the point of coming
            // back to a directory is usually that it has changed.
            Some(index) => {
                self.views[index] = View::listing(document);
                index
            }
            None if self.views.len() == 1 && self.views[0].is_empty_scratch() => {
                self.views[0] = View::listing(document);
                0
            }
            None => {
                self.views.push(View::listing(document));
                self.views.len() - 1
            }
        };
        self.switch_to(index);
        if let Some(name) = on {
            self.cursor_on(name);
        }
        Ok(())
    }

    /// Put the cursor on the entry called `name`, if the listing has one.
    fn cursor_on(&mut self, name: &str) {
        let view = self.view();
        let found = (0..view.doc.len_lines())
            .find(|&line| name_on(&view.doc.line_str(line)) == name);
        if let Some(line) = found {
            let at = self.view().doc.line_to_char(line);
            self.view_mut().sel = Selection::point(at);
            self.view_mut().centre = true;
        }
    }

    /// `enter` on a listing line: into the directory, or open the file.
    /// `^v` and `^s` do the same in a new window, as they do in the picker.
    pub fn listing_enter(&mut self, open: Open) {
        let Some(dir) = self.listing_dir().map(Path::to_path_buf) else {
            return;
        };
        let line = self.cursor_coords().0;
        let name = name_on(&self.view().doc.line_str(line)).to_string();
        if name.is_empty() {
            return;
        }
        // `..` is the one entry that is not a name in this directory, and
        // going up wants the same cursor placement `-` gives it.
        if name == ".." {
            self.listing_up_from(&dir, open);
            return;
        }
        let target = dir.join(&name);
        // Going in is a jump, so `^o` comes back out to the line of the
        // listing you left - the same courtesy the picker does.
        if open == Open::Here {
            self.push_jump();
        }
        if open != Open::Here {
            self.split_window(open == Open::Beside, None);
        }
        if let Err(err) = self.open_path(&target) {
            self.message = format!("{err:#}");
        }
    }

    /// `-`: the directory the buffer is in, with the cursor on the buffer -
    /// and, in a listing, one level further out each time. A buffer with no
    /// file of its own gets the working directory, which is where its
    /// relative names would be resolved anyway.
    pub fn listing_up(&mut self) {
        let here = self.view().doc.path.clone();
        let here = match here {
            Some(path) => path,
            None => match std::env::current_dir() {
                Ok(dir) => dir,
                Err(err) => {
                    self.message = format!("{err:#}");
                    return;
                }
            },
        };
        // An unsaved buffer's path is a file that may not exist yet; its
        // directory does, and is the answer either way.
        let here = here.canonicalize().unwrap_or(here);
        match here.is_dir() {
            true => self.listing_up_from(&here, Open::Here),
            false => {
                self.push_jump();
                let name = here.file_name().map(|n| n.to_string_lossy().into_owned());
                let dir = here.parent().unwrap_or(&here).to_path_buf();
                if let Err(err) = self.open_listing(&dir, name.as_deref()) {
                    self.message = format!("{err:#}");
                }
            }
        }
    }

    /// Out of `dir` into its parent, with the cursor on `dir` itself.
    fn listing_up_from(&mut self, dir: &Path, open: Open) {
        let Some(parent) = dir.parent() else {
            self.message = format!("{} has nothing above it", dir.display());
            return;
        };
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        let parent = parent.to_path_buf();
        if open == Open::Here {
            self.push_jump();
        }
        if open != Open::Here {
            self.split_window(open == Open::Beside, None);
        }
        if let Err(err) = self.open_listing(&parent, name.as_deref()) {
            self.message = format!("{err:#}");
        }
    }
}

/// What to paint a listing with: directories in one colour and the way up in
/// another, which is the whole of its syntax. Returned per byte, like the
/// grammar's, so the drawing code cannot tell the two apart.
pub fn highlights(
    view: &View,
    range: std::ops::Range<usize>,
    theme: &crate::theme::Theme,
) -> crate::syntax::Highlights {
    let directory = theme.style("ui.listing.directory");
    let parent = theme.style("ui.listing.parent");
    let mut styles = vec![None; range.end.saturating_sub(range.start)];
    let first = view.doc.text.byte_to_line(range.start.min(view.doc.len_bytes()));
    let last = view.doc.text.byte_to_line(range.end.min(view.doc.len_bytes()));
    for line in first..=last.min(view.doc.len_lines().saturating_sub(1)) {
        let text = view.doc.line_str(line);
        if !text.ends_with('/') {
            continue;
        }
        let style = match name_on(&text) {
            ".." => parent,
            _ => directory,
        };
        let start = view.doc.line_to_byte(line);
        for byte in start..start + text.len() {
            if let Some(slot) = byte.checked_sub(range.start).and_then(|i| styles.get_mut(i)) {
                *slot = Some(style);
            }
        }
    }
    crate::syntax::Highlights::painted(range.start, styles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A directory with something of each kind in it. Named per test, since
    /// tests run beside each other, and canonical because a listing files
    /// itself under the path the filesystem agrees on.
    fn dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("jack_listing_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::create_dir(root.join("Docs")).unwrap();
        std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join(".hidden"), "").unwrap();
        std::fs::write(root.join("README.md"), "# hi\n").unwrap();
        root
    }

    #[test]
    fn a_listing_reads_the_way_up_then_the_directories_then_the_files() {
        let root = dir("a_listing_reads_the_way_up_then_the_directories_then_the_files");
        let lines = text(&entries(&root).unwrap());
        assert_eq!(lines, "../\nDocs/\nsrc/\n.hidden\nmain.rs\nREADME.md");
    }

    #[test]
    fn the_root_of_the_filesystem_has_nothing_above_it() {
        let entries = entries(Path::new("/")).unwrap();
        assert!(entries.iter().all(|entry| entry.name != ".."));
    }

    #[test]
    fn a_name_is_the_line_without_the_slash_a_directory_wears() {
        assert_eq!(name_on("src/"), "src");
        assert_eq!(name_on("main.rs"), "main.rs");
        assert_eq!(name_on("../"), "..");
    }

    #[test]
    fn opening_a_directory_lists_it_and_opening_a_file_opens_it() {
        let root = dir("opening_a_directory_lists_it_and_opening_a_file_opens_it");
        let mut editor = Editor::scratch();
        editor.open_path(&root).unwrap();
        assert!(editor.in_listing());
        assert_eq!(editor.listing_dir(), Some(root.as_path()));

        editor.open_path(root.join("main.rs")).unwrap();
        assert!(!editor.in_listing());
        assert_eq!(editor.view().doc.text.to_string(), "fn main() {}\n");
    }

    #[test]
    fn enter_on_a_directory_goes_in_and_enter_on_a_file_opens_it() {
        let root = dir("enter_on_a_directory_goes_in_and_enter_on_a_file_opens_it");
        let mut editor = Editor::scratch();
        editor.open_listing(&root, None).unwrap();

        // "../ Docs/ src/" - the cursor starts at the top, so one down is a
        // directory and going in lists it.
        editor.move_cursor(crate::view::Move::Down, false);
        editor.listing_enter(Open::Here);
        assert_eq!(editor.listing_dir(), Some(root.join("Docs").as_path()));

        // Back out, and onto the file below the directories.
        editor.listing_up();
        assert_eq!(editor.listing_dir(), Some(root.as_path()));
        assert_eq!(editor.view().doc.line_str(editor.cursor_coords().0), "Docs/");
        editor.cursor_on("main.rs");
        editor.listing_enter(Open::Here);
        assert!(!editor.in_listing());
        assert_eq!(editor.view().doc.display_name(), "main.rs");
    }

    #[test]
    fn minus_from_a_file_lists_its_directory_with_the_cursor_on_it() {
        let root = dir("minus_from_a_file_lists_its_directory_with_the_cursor_on_it");
        let mut editor = Editor::scratch();
        editor.open_file(root.join("main.rs")).unwrap();
        editor.listing_up();
        assert_eq!(editor.listing_dir(), Some(root.as_path()));
        assert_eq!(editor.view().doc.line_str(editor.cursor_coords().0), "main.rs");

        // And again, out of the directory the file was in, onto its name.
        editor.listing_up();
        assert_eq!(editor.listing_dir(), root.parent());
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(editor.view().doc.line_str(editor.cursor_coords().0), format!("{name}/"));
    }

    #[test]
    fn a_split_lists_the_directory_beside_the_buffer_it_came_from() {
        let root = dir("a_split_lists_the_directory_beside_the_buffer_it_came_from");
        let mut editor = Editor::scratch();
        editor.set_viewport(120, 40);
        editor.open_listing(&root, None).unwrap();
        editor.cursor_on("main.rs");
        editor.listing_enter(Open::Beside);
        assert_eq!(editor.windows_open(), 2);
        assert_eq!(editor.view().doc.display_name(), "main.rs");
    }

    #[test]
    fn coming_back_to_a_directory_reads_it_again() {
        let root = dir("coming_back_to_a_directory_reads_it_again");
        let mut editor = Editor::scratch();
        editor.open_listing(&root, None).unwrap();
        let views = editor.views.len();
        std::fs::write(root.join("later.rs"), "").unwrap();
        editor.open_listing(&root, None).unwrap();
        assert!(editor.view().doc.text.to_string().contains("later.rs"));
        // The same directory is the same buffer, not another tab of it.
        assert_eq!(editor.views.len(), views);
    }

    #[test]
    fn the_keys_reach_it() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let root = dir("the_keys_reach_it");
        let mut editor = Editor::scratch();
        editor.set_viewport(120, 40);
        editor.open_file(root.join("main.rs")).unwrap();
        let mut keys = crate::keys::Keys::default();

        keys.handle(&mut editor, key('-'));
        assert!(editor.in_listing());
        // The cursor came up onto the file, so enter goes straight back in.
        keys.handle(&mut editor, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!editor.in_listing());
        assert_eq!(editor.view().doc.display_name(), "main.rs");

        // `^s` saves everywhere else; on a listing it splits, as in the picker.
        keys.handle(&mut editor, key('-'));
        keys.handle(&mut editor, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert_eq!(editor.windows_open(), 2);
        assert_eq!(editor.view().doc.display_name(), "main.rs");
    }

    #[test]
    fn a_listing_is_not_written_back() {
        let root = dir("a_listing_is_not_written_back");
        let mut editor = Editor::scratch();
        editor.open_listing(&root, None).unwrap();
        editor.write(None, false);
        assert!(editor.message.contains("directory"), "{}", editor.message);
        // And the directory is still a directory.
        assert!(root.is_dir());
    }

    #[test]
    fn the_directories_are_painted_and_the_files_are_not() {
        let root = dir("the_directories_are_painted_and_the_files_are_not");
        let mut editor = Editor::scratch();
        editor.open_listing(&root, None).unwrap();
        let view = editor.view();
        let painted = highlights(view, 0..view.doc.len_bytes(), &editor.theme);
        let style_of = |line: usize| painted.style_at(view.doc.line_to_byte(line));
        assert!(style_of(0).is_some(), "../ is painted");
        assert!(style_of(1).is_some(), "Docs/ is painted");
        assert!(style_of(3).is_none(), ".hidden is not");
        assert_ne!(style_of(0), style_of(1), "the way up is not a directory");
    }
}
