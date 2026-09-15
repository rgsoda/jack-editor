//! The system clipboard, for `^c` `^x` `^v`.
//!
//! Two ways out of the terminal and one way in. A helper program - `wl-copy`,
//! `xclip`, `pbcopy` - is the way that works in both directions, so it is tried
//! first; where there is none, a copy still goes out as an OSC 52 escape, which
//! is the terminal's own clipboard protocol and the only thing that works over
//! ssh. Nothing here is required: with neither, `^c` and `^v` still move text
//! around inside the editor through the registers, which is where they put it
//! in the first place.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// A pair of programs that read and write the clipboard.
struct Helper {
    copy: &'static [&'static str],
    paste: &'static [&'static str],
}

/// In the order they are tried: Wayland, X11, macOS. The first whose copy
/// program is on `PATH` wins, so a Wayland session does not end up talking to
/// an `xclip` that has no display to talk to.
static HELPERS: &[Helper] = &[
    Helper { copy: &["wl-copy"], paste: &["wl-paste", "--no-newline"] },
    Helper {
        copy: &["xclip", "-selection", "clipboard"],
        paste: &["xclip", "-selection", "clipboard", "-o"],
    },
    Helper {
        copy: &["xsel", "--clipboard", "--input"],
        paste: &["xsel", "--clipboard", "--output"],
    },
    Helper { copy: &["pbcopy"], paste: &["pbpaste"] },
];

/// How much text OSC 52 will carry. Terminals cap what they will accept and
/// differ about where; past this it is not worth sending something that will
/// be silently half-pasted.
const OSC_LIMIT: usize = 74_994;

fn helper() -> Option<&'static Helper> {
    static FOUND: OnceLock<Option<&'static Helper>> = OnceLock::new();
    *FOUND.get_or_init(|| HELPERS.iter().find(|helper| on_path(helper.copy[0])))
}

/// Looking for the program rather than running it: spawning something to ask
/// whether it exists costs more than reading `PATH`, and a clipboard helper
/// with no `--version` would answer wrongly anyway.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// Put `text` on the system clipboard. The return is an escape sequence for
/// the terminal, to be written with the rest of the frame - the editor does
/// not own stdout, and a module that wrote to it behind the renderer's back
/// would eventually write into the middle of one.
pub fn copy(text: &str) -> Option<String> {
    if cfg!(test) {
        test_clipboard(Some(text.to_string()));
        return None;
    }
    if let Some(helper) = helper()
        && write_with(helper, text)
    {
        return None;
    }
    match text.len() <= OSC_LIMIT {
        true => Some(format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))),
        false => None,
    }
}

/// What is on the system clipboard, or `None` when there is no way to ask -
/// which is not an error, just a reason to use what the editor has itself.
pub fn paste() -> Option<String> {
    if cfg!(test) {
        return test_clipboard(None);
    }
    let output = Command::new(helper()?.paste[0])
        .args(&helper()?.paste[1..])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    match output.status.success() {
        true => String::from_utf8(output.stdout).ok(),
        false => None,
    }
}

fn write_with(helper: &Helper, text: &str) -> bool {
    let Ok(mut child) = Command::new(helper.copy[0])
        .args(&helper.copy[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    // `wl-copy` forks and keeps the selection alive, so waiting for it is
    // waiting for the fork to happen and nothing more.
    matches!(child.wait(), Ok(status) if status.success())
}

/// Base64, because OSC 52 carries its payload that way and this is the whole
/// of what the editor needs from an encoder.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut bits = 0u32;
        for (i, byte) in chunk.iter().enumerate() {
            bits |= (*byte as u32) << (16 - 8 * i);
        }
        for i in 0..4 {
            match i <= chunk.len() {
                true => out.push(ALPHABET[(bits >> (18 - 6 * i)) as usize & 0x3f] as char),
                false => out.push('='),
            }
        }
    }
    out
}

/// A clipboard for the tests, so they neither reach the session's real one nor
/// depend on there being one.
fn test_clipboard(write: Option<String>) -> Option<String> {
    use std::cell::RefCell;
    thread_local! {
        static CLIPBOARD: RefCell<Option<String>> = const { RefCell::new(None) };
    }
    CLIPBOARD.with(|clipboard| match write {
        Some(text) => {
            *clipboard.borrow_mut() = Some(text);
            None
        }
        None => clipboard.borrow().clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_pads_the_last_chunk() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Bytes above ASCII, which is where a sloppy shift goes wrong.
        assert_eq!(base64("é\n".as_bytes()), "w6kK");
    }

    #[test]
    fn a_copy_is_there_to_paste() {
        assert_eq!(copy("hello"), None, "tests never write an escape");
        assert_eq!(paste().as_deref(), Some("hello"));
    }
}
