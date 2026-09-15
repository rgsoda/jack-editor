use anyhow::{Context, Result};
use ropey::Rope;
use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

/// A single applied replacement, described in the byte and (row, column)
/// coordinates tree-sitter needs to patch its tree incrementally. Captured
/// during the edit because the "old" positions stop existing afterwards.
#[derive(Clone, Copy, Debug)]
pub struct Edit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: (usize, usize),
    pub old_end_point: (usize, usize),
    pub new_end_point: (usize, usize),
}

/// The text of one open file. Thin wrapper over a rope: everything outside this
/// module addresses text by *char* index, never byte index.
pub struct Document {
    pub text: Rope,
    pub path: Option<PathBuf>,
    /// What the file looked like when it was last read or written, so a change
    /// made behind our back can be noticed before we overwrite it.
    disk: Option<Stamp>,
}

/// Cheap evidence that a file is the one we last saw: when it was modified and
/// how big it was. Not a hash - this runs on every save.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    modified: std::time::SystemTime,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp { modified: meta.modified().ok()?, len: meta.len() })
    }
}

impl Document {
    pub fn scratch() -> Self {
        Document { text: Rope::new(), path: None, disk: None }
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let text = if path.exists() {
            let file = File::open(path)
                .with_context(|| format!("opening {}", path.display()))?;
            Rope::from_reader(BufReader::new(file))
                .with_context(|| format!("reading {}", path.display()))?
        } else {
            // Opening a path that doesn't exist yet is a new file, not an error.
            Rope::new()
        };
        Ok(Document { text, path: Some(path.to_path_buf()), disk: Stamp::of(path) })
    }

    pub fn len_lines(&self) -> usize {
        self.text.len_lines()
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    /// The line's text with any trailing line break removed.
    pub fn line_str(&self, line: usize) -> Cow<'_, str> {
        let slice = self.text.line(line);
        match Cow::<str>::from(slice) {
            Cow::Borrowed(s) => Cow::Borrowed(s.trim_end_matches(['\n', '\r'])),
            Cow::Owned(s) => Cow::Owned(s.trim_end_matches(['\n', '\r']).to_string()),
        }
    }

    /// Chars in `line`, excluding its trailing line break.
    pub fn line_len_chars(&self, line: usize) -> usize {
        let slice = self.text.line(line);
        let mut n = slice.len_chars();
        if n > 0 && slice.char(n - 1) == '\n' {
            n -= 1;
            if n > 0 && slice.char(n - 1) == '\r' {
                n -= 1;
            }
        }
        n
    }

    pub fn len_bytes(&self) -> usize {
        self.text.len_bytes()
    }

    pub fn line_to_byte(&self, line: usize) -> usize {
        self.text.line_to_byte(line.min(self.len_lines().saturating_sub(1)))
    }

    pub fn char_to_line(&self, char_idx: usize) -> usize {
        self.text.char_to_line(char_idx.min(self.len_chars()))
    }

    pub fn line_to_char(&self, line: usize) -> usize {
        self.text.line_to_char(line.min(self.len_lines().saturating_sub(1)))
    }

    /// Split a char index into (line, char offset within that line).
    pub fn coords(&self, char_idx: usize) -> (usize, usize) {
        let line = self.char_to_line(char_idx);
        (line, char_idx - self.line_to_char(line))
    }

    /// Replace `remove_chars` chars at `pos` with `text`. The only mutation
    /// point for the rope; every edit in the editor funnels through here.
    pub fn replace(&mut self, pos: usize, remove_chars: usize, text: &str) -> Edit {
        let start_byte = self.text.char_to_byte(pos);
        let old_end_byte = self.text.char_to_byte(pos + remove_chars);
        let start_point = self.point_at(start_byte);
        let old_end_point = self.point_at(old_end_byte);

        if remove_chars > 0 {
            self.text.remove(pos..pos + remove_chars);
        }
        if !text.is_empty() {
            self.text.insert(pos, text);
        }

        let new_end_byte = start_byte + text.len();
        Edit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_point,
            old_end_point,
            new_end_point: self.point_at(new_end_byte),
        }
    }

    /// (row, byte column) of a byte offset.
    fn point_at(&self, byte: usize) -> (usize, usize) {
        let line = self.text.byte_to_line(byte);
        (line, byte - self.text.line_to_byte(line))
    }

    pub fn slice_str(&self, start: usize, end: usize) -> String {
        self.text.slice(start..end).to_string()
    }

    pub fn save(&mut self) -> Result<()> {
        let path = self.path.clone().context("no file name")?;
        let file = File::create(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        self.text
            .write_to(BufWriter::new(file))
            .with_context(|| format!("writing {}", path.display()))?;
        self.disk = Stamp::of(&path);
        Ok(())
    }

    /// Point the document at a new path, as `:w name` does. The old file is
    /// left alone.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.disk = None;
    }

    /// True when the file on disk is not the one we last read or wrote. A file
    /// that has been deleted counts: overwriting it silently is the same
    /// surprise.
    pub fn changed_on_disk(&self) -> bool {
        let Some(path) = self.path.as_deref() else {
            return false;
        };
        match (self.disk, Stamp::of(path)) {
            // Never saved and never seen on disk: a new file, not a conflict.
            (None, None) => false,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (Some(before), Some(now)) => before != now,
        }
    }

    /// Re-read from disk, throwing away what is in memory.
    pub fn reload(&mut self) -> Result<()> {
        let path = self.path.clone().context("no file name")?;
        let fresh = Document::open(&path)?;
        self.text = fresh.text;
        self.disk = fresh.disk;
        Ok(())
    }

    pub fn display_name(&self) -> &str {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|n| n.to_str())
            .unwrap_or("[scratch]")
    }
}
