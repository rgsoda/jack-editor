//! Where the cursor was in each file when it was last closed, so opening the
//! file again puts you back there.
//!
//! One small text file in the state directory, a line per file:
//! `line<TAB>column<TAB>path`, most recent first. The path goes last so that
//! a tab or a space in it needs no quoting - everything after the second tab
//! is the path. Read once at startup and written once on the way out, so it
//! costs nothing while you work.

use std::io::Write;
use std::path::{Path, PathBuf};

/// How many files are remembered. Past this the oldest go: a position in a
/// file you have not opened for a thousand files is not one you are coming
/// back to.
const LIMIT: usize = 1000;

#[derive(Default)]
pub struct Positions {
    /// Where the list lives, or `None` when there is nowhere to keep it.
    store: Option<PathBuf>,
    /// Most recent first. Paths are absolute, so `jack src/a.rs` and
    /// `jack a.rs` from inside `src` are the same file.
    entries: Vec<(PathBuf, usize, usize)>,
}

/// `$XDG_STATE_HOME/jack/positions`, or `~/.local/state/jack/positions`.
/// State rather than config: it is written by jack, not by you, and losing it
/// loses nothing you made.
pub fn store_path() -> Option<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(state.join("jack").join("positions"))
}

impl Positions {
    /// Read the list at `store`. A missing or unreadable file is an empty
    /// list, and a line that does not parse is skipped: this is a convenience,
    /// and nothing about it is worth refusing to start over.
    pub fn load(store: Option<PathBuf>) -> Positions {
        let text = store.as_deref().and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
        let entries = text
            .lines()
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                let row = fields.next()?.parse().ok()?;
                let column = fields.next()?.parse().ok()?;
                let path = PathBuf::from(fields.next()?);
                Some((path, row, column))
            })
            .take(LIMIT)
            .collect();
        Positions { store, entries }
    }

    /// Where the cursor was in `file`, as a line and column.
    pub fn get(&self, file: &Path) -> Option<(usize, usize)> {
        let file = absolute(file);
        self.entries.iter().find(|(path, ..)| *path == file).map(|&(_, line, column)| (line, column))
    }

    /// Remember where the cursor is in `file`, as the most recent.
    pub fn set(&mut self, file: &Path, line: usize, column: usize) {
        let file = absolute(file);
        self.entries.retain(|(path, ..)| *path != file);
        self.entries.insert(0, (file, line, column));
        self.entries.truncate(LIMIT);
    }

    /// Write the list back. Through a temporary file and a rename, so two
    /// jacks quitting at once leave one of their lists rather than half of
    /// each.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(store) = self.store.as_deref() else {
            return Ok(());
        };
        if let Some(dir) = store.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temporary = store.with_extension(format!("{}", std::process::id()));
        let mut file = std::io::BufWriter::new(std::fs::File::create(&temporary)?);
        for (path, line, column) in &self.entries {
            // A newline in a path would split the entry; such a file is not
            // worth remembering a place in.
            let Some(path) = path.to_str().filter(|path| !path.contains('\n')) else {
                continue;
            };
            writeln!(file, "{line}\t{column}\t{path}")?;
        }
        file.into_inner().map_err(|err| err.into_error())?.sync_all()?;
        std::fs::rename(&temporary, store)
    }
}

fn absolute(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| match std::env::current_dir() {
        Ok(dir) => dir.join(path),
        Err(_) => path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jack_positions_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("state").join("positions")
    }

    #[test]
    fn a_position_survives_being_written_and_read_back() {
        let path = store("roundtrip");
        let mut positions = Positions::load(Some(path.clone()));
        positions.set(Path::new("/nowhere/a b\tc.rs"), 41, 7);
        positions.set(Path::new("/nowhere/other.rs"), 3, 0);
        positions.save().unwrap();

        let again = Positions::load(Some(path.clone()));
        // A space and a tab in the name: everything after the second tab is
        // the path.
        assert_eq!(again.get(Path::new("/nowhere/a b\tc.rs")), Some((41, 7)));
        assert_eq!(again.get(Path::new("/nowhere/other.rs")), Some((3, 0)));
        assert_eq!(again.get(Path::new("/nowhere/never.rs")), None);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn the_newest_is_first_and_the_oldest_fall_off() {
        let mut positions = Positions::load(None);
        for n in 0..LIMIT + 5 {
            positions.set(Path::new(&format!("/nowhere/{n}.rs")), n, 0);
        }
        assert_eq!(positions.entries.len(), LIMIT);
        assert_eq!(positions.get(Path::new("/nowhere/0.rs")), None, "the oldest went");
        // Setting one again moves it to the front rather than adding a copy.
        positions.set(Path::new("/nowhere/10.rs"), 99, 1);
        assert_eq!(positions.entries[0], (PathBuf::from("/nowhere/10.rs"), 99, 1));
        assert_eq!(positions.entries.iter().filter(|(p, ..)| p.ends_with("10.rs")).count(), 1);
    }

    #[test]
    fn a_damaged_list_is_what_can_be_read_of_it() {
        let path = store("damaged");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "nonsense\n4\t2\t/nowhere/fine.rs\nx\ty\t/nowhere/bad.rs\n").unwrap();
        let positions = Positions::load(Some(path.clone()));
        assert_eq!(positions.get(Path::new("/nowhere/fine.rs")), Some((4, 2)));
        assert_eq!(positions.entries.len(), 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
