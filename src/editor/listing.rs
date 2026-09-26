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
//! The listing is editable, as oil's is: rename a file by editing its line,
//! move nothing and create nothing by accident, and `:w` to mean it. What was
//! read is kept beside the buffer, so `:w` is a diff between the two - which
//! is what makes a changed line a rename rather than a deletion and a
//! creation of an empty file with the same name.

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

/// One thing `:w` on a listing would do.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    /// A line that is not the line that was read there.
    Rename { from: String, to: String },
    /// A line that was not there before. A trailing slash makes a directory,
    /// as it is what tells the two apart everywhere else in a listing.
    Create { name: String, directory: bool },
    /// A line that is gone.
    Delete { name: String, directory: bool },
}

impl Change {
    /// How it reads in the status line, which is also how it reads in the
    /// question `:w` asks before deleting anything.
    pub fn line(&self) -> String {
        match self {
            Change::Rename { from, to } => format!("{from} -> {to}"),
            Change::Create { name, .. } => format!("new {name}"),
            Change::Delete { name, .. } => format!("delete {name}"),
        }
    }
}

/// What was done to a listing, as changes to make on the disk: `before` is
/// what was read into the buffer, `after` is what is in it now.
///
/// The pairing is a diff, which is what makes an edited line a rename: the
/// lines that are still the same anchor the rest, and inside a run that
/// differs, the first line that went matches the first that arrived. An extra
/// line in the run is a new file and a missing one is a deletion. Nobody has
/// to tell jack which is which, and nothing is hidden in the buffer to track
/// it - the text is the whole of the state, as it is everywhere else here.
pub fn plan(before: &[String], after: &[String]) -> Result<Vec<Change>, String> {
    let before: Vec<String> = before.iter().filter_map(|line| editable(line)).collect();
    let after: Vec<String> = after.iter().filter_map(|line| editable(line)).collect();

    for line in &after {
        let name = name_on(line);
        // A name, not a path. Moving a file into another directory is a
        // reasonable thing to want, and a `../` typed into a listing by
        // accident is not something to find out about afterwards.
        if name.contains('/') || name == "." || name == ".." {
            return Err(format!("a name, not a path: {name}"));
        }
    }
    for (index, line) in after.iter().enumerate() {
        if after[..index].iter().any(|earlier| name_on(earlier) == name_on(line)) {
            return Err(format!("two lines say {}", name_on(line)));
        }
    }

    let mut changes = Vec::new();
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &before, &after) {
        let (old, new) = (op.old_range(), op.new_range());
        let (gone, arrived) = (&before[old.clone()], &after[new.clone()]);
        // In the order the lines are in, so what is said about a listing
        // reads down it.
        for pair in paired(gone, arrived) {
            match pair {
                (Some(from), Some(to)) => {
                    let (from, to) = (&gone[from], &arrived[to]);
                    if name_on(from) != name_on(to) {
                        let (from, to) = (name_on(from).into(), name_on(to).into());
                        changes.push(Change::Rename { from, to });
                    }
                }
                (Some(from), None) => {
                    let line = &gone[from];
                    changes.push(Change::Delete {
                        name: name_on(line).into(),
                        directory: is_dir(line),
                    });
                }
                (None, Some(to)) => {
                    let line = &arrived[to];
                    changes.push(Change::Create {
                        name: name_on(line).into(),
                        directory: is_dir(line),
                    });
                }
                (None, None) => {}
            }
        }
    }
    Ok(changes)
}

/// One change, on the disk. Nothing recursive and nothing overwritten: a
/// rename onto a name that is taken, or a directory with anything in it, is
/// said rather than done. `:w!` is about the question, not about force.
fn apply(dir: &Path, change: &Change) -> Result<(), String> {
    let at = |name: &str| dir.join(name);
    match change {
        Change::Rename { from, to } => {
            if at(to).exists() {
                return Err("there is already one of those".into());
            }
            std::fs::rename(at(from), at(to)).map_err(|err| err.to_string())
        }
        Change::Create { name, directory: true } => {
            std::fs::create_dir(at(name)).map_err(|err| err.to_string())
        }
        Change::Create { name, directory: false } => {
            if at(name).exists() {
                return Err("there is already one of those".into());
            }
            std::fs::File::create(at(name)).map(|_| ()).map_err(|err| err.to_string())
        }
        // A directory with anything in it is not removed: a line deleted by
        // accident should cost one empty directory, never a tree. What is in
        // it can be deleted from inside it, which is the same key again.
        Change::Delete { name, directory: true } => {
            std::fs::remove_dir(at(name)).map_err(|err| match err.kind() {
                std::io::ErrorKind::DirectoryNotEmpty => {
                    "there is something in it - empty it first".into()
                }
                _ => err.to_string(),
            })
        }
        Change::Delete { name, directory: false } => {
            std::fs::remove_file(at(name)).map_err(|err| err.to_string())
        }
    }
}

/// Which line that went is which line that arrived, inside one run the diff
/// could not match up. Position is the obvious answer and the wrong one: with
/// `.hidden` deleted and `main.rs` edited to `app.rs` in the same breath, it
/// pairs `.hidden` with `app.rs` and renames the wrong file.
///
/// So a pair is the one that looks most like it - how much of the name they
/// start and end with in common - taken best-first, and position decides only
/// when nothing looks like anything, which is when position is all there is.
/// A run big enough for that to cost anything is a whole directory rewritten,
/// where the names are no guide either.
fn paired(gone: &[String], arrived: &[String]) -> Vec<(Option<usize>, Option<usize>)> {
    const TOO_BIG: usize = 4096;
    let mut pairs = Vec::new();
    let mut old: Vec<usize> = (0..gone.len()).collect();
    let mut new: Vec<usize> = (0..arrived.len()).collect();

    while !old.is_empty() && !new.is_empty() {
        let mut best = (old[0], new[0], -1i32);
        if gone.len() * arrived.len() <= TOO_BIG {
            for &from in &old {
                for &to in &new {
                    let score = alike(name_on(&gone[from]), name_on(&arrived[to]));
                    if score > best.2 {
                        best = (from, to, score);
                    }
                }
            }
        }
        pairs.push((Some(best.0), Some(best.1)));
        old.retain(|&index| index != best.0);
        new.retain(|&index| index != best.1);
    }
    pairs.extend(old.into_iter().map(|from| (Some(from), None)));
    pairs.extend(new.into_iter().map(|to| (None, Some(to))));
    // Back into the order the lines are in, so what is said about a listing
    // reads down it.
    pairs.sort_by_key(|pair| (pair.0, pair.1));
    pairs
}

/// How much two names look like each other: what they start with and what
/// they end with, in characters. `main.rs` and `app.rs` share three; `.hidden`
/// and `app.rs` share none.
fn alike(from: &str, to: &str) -> i32 {
    let common = |a: &mut dyn Iterator<Item = char>, b: &mut dyn Iterator<Item = char>| {
        a.zip(b).take_while(|(one, two)| one == two).count()
    };
    let front = common(&mut from.chars(), &mut to.chars());
    let back = common(&mut from.chars().rev(), &mut to.chars().rev());
    // Never more than the shorter name: `foo.rs` against itself is six, not
    // twelve, and a one-character name is not worth more than a long one.
    (front + back).min(from.chars().count().min(to.chars().count())) as i32
}

/// A line that stands for something that can be renamed or removed. The way
/// up is not one of them, and neither is a blank line - deleting the text of
/// a line rather than the line is how you undo a rename half-typed.
fn editable(line: &str) -> Option<String> {
    let line = line.trim().to_string();
    (!line.is_empty() && name_on(&line) != "..").then_some(line)
}

/// Whether a line asks for a directory, which is the slash a listing writes
/// one with.
fn is_dir(line: &str) -> bool {
    line.trim_end().ends_with('/')
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

    /// `:w` on a listing: do to the directory what was done to its lines.
    ///
    /// Renames and new files go ahead; nothing is deleted without `:w!`,
    /// which is the one rule worth remembering here. A deletion is the only
    /// change that cannot be undone by typing the old line back, so it is the
    /// only one that asks - and it asks by saying exactly what it would
    /// remove rather than by counting.
    pub(crate) fn write_listing(&mut self, force: bool) {
        let Some(dir) = self.listing_dir().map(Path::to_path_buf) else {
            return;
        };
        let after: Vec<String> = (0..self.view().doc.len_lines())
            .map(|line| self.view().doc.line_str(line).trim_end().to_string())
            .collect();
        let changes = match plan(&self.view().listing_was, &after) {
            Ok(changes) => changes,
            Err(complaint) => {
                self.message = complaint;
                return;
            }
        };
        if changes.is_empty() {
            self.message = format!("{} is as it was", dir.display());
            return;
        }
        let deletions: Vec<&Change> =
            changes.iter().filter(|change| matches!(change, Change::Delete { .. })).collect();
        if !deletions.is_empty() && !force {
            let what: Vec<String> = deletions.iter().map(|change| change.line()).collect();
            self.message = format!("{} - :w! to go ahead", what.join(", "));
            return;
        }

        // The cursor is put back on the line it was on by name, so a rename
        // leaves you on the file you renamed rather than wherever that line
        // has moved to.
        let line = self.cursor_coords().0;
        let on = name_on(&self.view().doc.line_str(line)).to_string();
        let (mut done, mut failed) = (0, Vec::new());
        for change in &changes {
            match apply(&dir, change) {
                Ok(()) => done += 1,
                Err(complaint) => failed.push(format!("{}: {complaint}", change.line())),
            }
        }
        let landed = changes
            .iter()
            .find(|change| matches!(change, Change::Rename { from, .. } if *from == on))
            .map(|change| match change {
                Change::Rename { to, .. } => to.clone(),
                _ => on.clone(),
            })
            .unwrap_or(on);
        if let Err(err) = self.open_listing(&dir, Some(&landed)) {
            self.message = format!("{err:#}");
            return;
        }
        let s = match done {
            1 => "",
            _ => "s",
        };
        self.message = match failed.is_empty() {
            true => format!("{done} change{s}"),
            false => format!("{done} change{s}, and {}", failed.join("; ")),
        };
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

    /// The listing as it was read, and the listing with `edit` done to it.
    fn edited(root: &Path, edit: impl Fn(&mut Vec<String>)) -> (Vec<String>, Vec<String>) {
        let before: Vec<String> =
            text(&entries(root).unwrap()).lines().map(str::to_string).collect();
        let mut after = before.clone();
        edit(&mut after);
        (before, after)
    }

    #[test]
    fn an_edited_line_is_a_rename_and_a_line_that_is_gone_is_a_deletion() {
        let root = dir("an_edited_line_is_a_rename");
        let (before, after) = edited(&root, |lines| {
            // ../ Docs/ src/ .hidden main.rs README.md
            lines[4] = "app.rs".into();
            lines.remove(3);
            lines.push("notes.md".into());
            lines.push("build/".into());
        });
        assert_eq!(
            plan(&before, &after).unwrap(),
            vec![
                Change::Delete { name: ".hidden".into(), directory: false },
                Change::Rename { from: "main.rs".into(), to: "app.rs".into() },
                Change::Create { name: "notes.md".into(), directory: false },
                Change::Create { name: "build".into(), directory: true },
            ]
        );
    }

    #[test]
    fn the_way_up_and_a_blank_line_are_not_changes() {
        let root = dir("the_way_up_and_a_blank_line_are_not_changes");
        let (before, after) = edited(&root, |lines| {
            lines.remove(0);
            lines.push(String::new());
            lines.push("   ".into());
        });
        assert_eq!(plan(&before, &after).unwrap(), vec![]);
    }

    #[test]
    fn a_path_or_a_name_said_twice_is_refused_before_anything_happens() {
        let root = dir("a_path_or_a_name_said_twice_is_refused");
        let (before, after) = edited(&root, |lines| lines[4] = "../elsewhere/main.rs".into());
        assert_eq!(plan(&before, &after), Err("a name, not a path: ../elsewhere/main.rs".into()));

        let (before, after) = edited(&root, |lines| lines[4] = "README.md".into());
        assert_eq!(plan(&before, &after), Err("two lines say README.md".into()));
    }

    #[test]
    fn writing_a_listing_renames_creates_and_asks_before_deleting() {
        let root = dir("writing_a_listing_renames_creates_and_asks_before_deleting");
        let mut editor = Editor::scratch();
        editor.open_listing(&root, None).unwrap();

        // Rename a file and make a directory: neither loses anything, so
        // neither asks.
        let text = editor.view().doc.text.to_string();
        let edited = text.replace("main.rs", "app.rs") + "\nbuild/";
        editor.view_mut().doc.text = ropey::Rope::from_str(&edited);
        editor.write(None, false);
        assert_eq!(editor.message, "2 changes", "{}", editor.message);
        assert!(root.join("app.rs").is_file() && !root.join("main.rs").exists());
        assert!(root.join("build").is_dir());
        assert_eq!(std::fs::read_to_string(root.join("app.rs")).unwrap(), "fn main() {}\n");
        // The listing was read again, and the cursor followed the rename.
        assert!(editor.view().doc.text.to_string().contains("app.rs"));

        // A deletion says what it would delete and does nothing.
        let text = editor.view().doc.text.to_string();
        let without = text.lines().filter(|line| *line != "README.md").collect::<Vec<_>>().join("\n");
        editor.view_mut().doc.text = ropey::Rope::from_str(&without);
        editor.write(None, false);
        assert_eq!(editor.message, "delete README.md - :w! to go ahead");
        assert!(root.join("README.md").is_file(), "still there");

        // And `:w!` means it. The buffer still holds the same edit.
        editor.write(None, true);
        assert_eq!(editor.message, "1 change", "{}", editor.message);
        assert!(!root.join("README.md").exists());
    }

    #[test]
    fn a_directory_with_something_in_it_is_not_emptied_by_accident() {
        let root = dir("a_directory_with_something_in_it_is_not_emptied");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let mut editor = Editor::scratch();
        editor.open_listing(&root, None).unwrap();

        let text = editor.view().doc.text.to_string();
        let without = text.lines().filter(|line| *line != "src/").collect::<Vec<_>>().join("\n");
        editor.view_mut().doc.text = ropey::Rope::from_str(&without);
        editor.write(None, true);
        assert!(editor.message.contains("empty it first"), "{}", editor.message);
        assert!(root.join("src/main.rs").is_file(), "the file in it is untouched");
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
