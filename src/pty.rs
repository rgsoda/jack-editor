//! A program with a terminal of its own.
//!
//! `:term` needs a pty rather than a pipe: a shell behaves differently when
//! its output is not a terminal, and the programs worth running in a terminal
//! buffer - `git log`, `top`, another editor - want a size, a controlling
//! terminal and a signal when that size changes. A pipe gives none of those.
//!
//! What is here is the file descriptor and the child, and nothing about what
//! the bytes mean: `term.rs` reads them. Unix only, because a pty is.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use anyhow::{Context, Result};

use crate::stream::Message;

/// How much is read at once. A build writes faster than a screen can be
/// drawn, so the reads are big and the redraws are per batch rather than per
/// line.
const CHUNK: usize = 8192;

/// A running program and the terminal it thinks it has.
pub struct Pty {
    master: OwnedFd,
    /// The process group, which is what gets signalled: a shell's children
    /// have to go too, or `:term` closed on a running `sleep` leaves it.
    group: libc::pid_t,
}

impl Pty {
    /// Start `command` under a new pty of this size, reading what it writes
    /// into the channel as `Message::Term`.
    pub fn spawn(
        command: &str,
        root: &Path,
        rows: usize,
        cols: usize,
        token: u64,
        tx: Sender<Message>,
    ) -> Result<Pty> {
        let (master, slave) = openpty(rows, cols)?;

        // Three handles on the same pty: what the program sees as its stdin,
        // its stdout and its stderr are one terminal, the way they are when
        // it is started from a shell.
        let (stdin, stdout, stderr) = (slave.try_clone()?, slave.try_clone()?, slave);
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut child = Command::new(&shell);
        if !command.trim().is_empty() {
            child.arg("-c").arg(command);
        }
        child
            .current_dir(root)
            // What a program looks up to find out what its terminal can do.
            // The one jack itself draws with, since what the program writes
            // comes back through the same parser.
            .env("TERM", "xterm-256color")
            .env_remove("COLUMNS")
            .env_remove("LINES")
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        // A session of its own, and the pty as its controlling terminal, so
        // that `^c` typed into the buffer reaches the program and not jack.
        unsafe {
            child.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = child.spawn().with_context(|| format!("starting {shell}"))?;
        let group = child.id() as libc::pid_t;

        let reader = master.try_clone().context("the pty")?;
        thread::spawn(move || {
            let mut file = std::fs::File::from(reader);
            let mut buffer = [0u8; CHUNK];
            loop {
                match file.read(&mut buffer) {
                    // A pty reports the program having gone as an error
                    // rather than as an end of file, and either means the
                    // same thing here.
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        let bytes = buffer[..read].to_vec();
                        if tx.send(Message::Term { token, bytes }).is_err() {
                            return;
                        }
                    }
                }
            }
            // The child is waited on here rather than in the editor: nobody
            // is going to ask what a shell exited with, and a zombie left
            // behind by a terminal that was closed is still a zombie.
            let mut child = child;
            let _ = child.wait();
            let _ = tx.send(Message::TermGone { token });
        });

        Ok(Pty { master, group })
    }

    /// Keys, as the bytes a terminal would have sent.
    pub fn write(&self, bytes: &[u8]) {
        let mut file = std::mem::ManuallyDrop::new(unsafe {
            std::fs::File::from_raw_fd(self.master.as_raw_fd())
        });
        let _ = file.write_all(bytes);
        let _ = file.flush();
    }

    /// A new size. The `SIGWINCH` that goes with it is the kernel's doing,
    /// and is what makes a full-screen program redraw itself.
    pub fn resize(&self, rows: usize, cols: usize) {
        let size = winsize(rows, cols);
        unsafe {
            libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size);
        }
    }

    /// Ask the program to go, the way closing a terminal window asks: a
    /// hangup to the whole group, and a `SIGKILL` for whatever is still there
    /// a moment later is the operating system's business, not an editor's.
    pub fn kill(&self) {
        if self.group > 0 {
            unsafe {
                libc::killpg(self.group, libc::SIGHUP);
                libc::killpg(self.group, libc::SIGCONT);
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.kill();
    }
}

fn winsize(rows: usize, cols: usize) -> libc::winsize {
    libc::winsize {
        ws_row: rows.clamp(1, u16::MAX as usize) as u16,
        ws_col: cols.clamp(1, u16::MAX as usize) as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// The two ends of a new pty: the one jack reads and writes, and the one the
/// program gets as its terminal.
fn openpty(rows: usize, cols: usize) -> Result<(OwnedFd, OwnedFd)> {
    let (mut master, mut slave): (RawFd, RawFd) = (-1, -1);
    let size = winsize(rows, cols);
    let opened = unsafe {
        libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size)
    };
    if opened < 0 {
        return Err(std::io::Error::last_os_error()).context("opening a pty");
    }
    Ok(unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) })
}
