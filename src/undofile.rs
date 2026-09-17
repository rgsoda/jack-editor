//! Undo that outlives the editor: every write puts the buffer's history in a
//! file of its own under the state directory, and opening the file again
//! reads it back - so `u` after a restart undoes what you did yesterday.
//!
//! A history is a list of edits against one particular text, and applied to
//! any other it would scramble the file. So the undo file carries a hash of
//! the text it belongs to, and it is only used when the file on disk still
//! hashes the same: edited by something else since, and the history is
//! quietly not there, which is the only safe thing for it to be.

use std::io::Write;
use std::path::{Path, PathBuf};

use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::history::{History, Step};

/// The most undo steps kept on disk. The oldest go first; a step is one
/// command, so this is a long way back.
const LIMIT: usize = 1000;

/// `$XDG_STATE_HOME/jack/undo`, or `~/.local/state/jack/undo`.
pub fn store_dir() -> Option<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(state.join("jack").join("undo"))
}

#[derive(Serialize, Deserialize)]
struct Stored {
    /// The file this is the history of - the name of the undo file is only a
    /// hash of it, and two paths can share one.
    path: PathBuf,
    /// The text the history ends at, as a length and a hash.
    chars: usize,
    hash: u64,
    undo: Vec<Step>,
    redo: Vec<Step>,
}

/// Write the history of `file`, whose text is now `text`.
pub fn write(dir: &Path, file: &Path, text: &Rope, history: &History) -> std::io::Result<()> {
    let file = absolute(file);
    let (undo, redo) = history.steps();
    let undo = &undo[undo.len().saturating_sub(LIMIT)..];
    let stored = Stored {
        path: file.clone(),
        chars: text.len_chars(),
        hash: hash(text),
        undo: undo.to_vec(),
        redo: redo.to_vec(),
    };
    std::fs::create_dir_all(dir)?;
    let target = dir.join(name(&file));
    let temporary = target.with_extension("tmp");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&temporary)?);
    serde_json::to_writer(&mut out, &stored).map_err(std::io::Error::other)?;
    out.flush()?;
    drop(out);
    std::fs::rename(temporary, target)
}

/// The history written for `file`, if there is one and it is about `text`.
pub fn read(dir: &Path, file: &Path, text: &Rope) -> Option<History> {
    let file = absolute(file);
    let bytes = std::fs::read(dir.join(name(&file))).ok()?;
    let stored: Stored = serde_json::from_slice(&bytes).ok()?;
    if stored.path != file || stored.chars != text.len_chars() || stored.hash != hash(text) {
        return None;
    }
    Some(History::restored(stored.undo, stored.redo))
}

/// The undo file's name: a hash of the path, so any path makes a file name.
fn name(file: &Path) -> String {
    format!("{:016x}", fnv(file.as_os_str().as_encoded_bytes(), FNV_OFFSET))
}

fn absolute(file: &Path) -> PathBuf {
    file.canonicalize().unwrap_or_else(|_| std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()))
}

/// FNV-1a over the text's bytes. Written out rather than taken from std,
/// whose hasher is free to change between releases - and a changed hash
/// would throw away everyone's history on the next upgrade.
fn hash(text: &Rope) -> u64 {
    text.chunks().fold(FNV_OFFSET, |hash, chunk| fnv(chunk.as_bytes(), hash))
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn fnv(bytes: &[u8], mut hash: u64) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_does_not_care_how_the_rope_is_chunked() {
        let whole = Rope::from_str(&"line of text\n".repeat(5000));
        let mut built = Rope::new();
        for _ in 0..5000 {
            let end = built.len_chars();
            built.insert(end, "line of text\n");
        }
        assert_eq!(hash(&whole), hash(&built));
        assert_ne!(hash(&whole), hash(&Rope::from_str("other")));
    }
}
