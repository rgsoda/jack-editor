use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

/// Everything the run loop waits on. Keys and background results arrive on the
/// same channel, so the loop blocks in one place and never polls.
pub enum Message {
    Key(KeyEvent),
    Resize,
    /// Text the terminal delivered in one piece, between the markers that
    /// bracketed paste wraps it in. It is text and never keystrokes: the
    /// editor is spared interpreting a pasted `dd` as a command.
    Paste(String),
    /// The terminal window has the focus again - the moment another program
    /// is likeliest to have changed a file.
    Focus,
    /// How the current buffer differs from what git has, as one sign per
    /// changed line. `token` identifies the view that asked.
    Signs { token: u64, signs: Vec<(usize, Sign)>, hunks: Vec<Hunk> },
    /// A background job could not run - a search pattern that is not a valid
    /// regex, most often.
    Failed { token: u64, error: String },
    /// A batch of candidates for an open picker. For a search the strings are
    /// `path:line:text`, which is what the editor splits back apart.
    ///
    /// A batch of candidates for an open picker. `token` says which picker
    /// asked for them, so results from a picker that has since been closed can
    /// be dropped instead of appearing in the next one.
    Items {
        token: u64,
        items: Vec<String>,
        done: bool,
    },
    /// A message from language server `server`, or `None` when it has gone.
    Lsp { server: usize, message: Option<serde_json::Value> },
}

/// A run of lines that differ from what git has, and what git has there
/// instead: enough to show what changed, to put it back, and to stage it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The buffer's lines, counted from zero, end excluded. Empty for lines
    /// that were only deleted, and then `start` is the line they came before.
    pub lines: std::ops::Range<usize>,
    /// Where git's version of this starts, counted from zero.
    pub old_start: usize,
    /// Git's lines, each with its line break where it had one.
    pub old: Vec<String>,
}

/// What happened to one line since the last commit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sign {
    Added,
    Modified,
    /// Something was deleted *above* this line, which is the only way to show
    /// a deletion in a gutter that has no row for it.
    Deleted,
}

/// A search stops here. Past a few thousand hits the answer is "refine the
/// pattern", not "scroll".
const MATCH_LIMIT: usize = 5_000;
/// A matching line longer than this is almost certainly minified or generated,
/// and would push everything else off the row anyway.
const MAX_LINE: usize = 300;

/// Walking stops here. A tree this big is a mistake - a home directory, `/` -
/// and the picker would be useless long before the memory mattered.
const LIMIT: usize = 100_000;
const BATCH: usize = 512;

pub fn channels() -> (Sender<Message>, Receiver<Message>) {
    channel()
}

/// How long the reader waits for a key before looking up to see whether it is
/// still the one reading the terminal, and how long it sleeps between looks
/// once it is not.
const WAITING: std::time::Duration = std::time::Duration::from_millis(200);
const RESTING: std::time::Duration = std::time::Duration::from_millis(10);
/// How long the run loop will wait for the reader to let go of the keyboard.
/// Past this something is wrong, and running the program with jack still
/// reading is better than not running it at all.
const HANDOVER: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether the editor is the one reading the keyboard. It is not, while
/// another program has the terminal: `:!lazygit` and the like.
#[derive(Default)]
pub struct Input {
    paused: std::sync::atomic::AtomicBool,
    /// Set by the reader once it has actually stopped, which is what makes
    /// `pause` something you can wait on rather than something you hope about.
    resting: std::sync::atomic::AtomicBool,
}

impl Input {
    pub fn new() -> Arc<Input> {
        Arc::new(Input::default())
    }

    /// Stop reading the keyboard, and wait until that has taken effect.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
        let until = std::time::Instant::now() + HANDOVER;
        while !self.resting.load(Ordering::Relaxed) && std::time::Instant::now() < until {
            thread::sleep(RESTING);
        }
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }
}

/// Read the terminal forever on its own thread. Blocking here rather than in
/// the run loop is what lets the loop wait on background work too.
pub fn spawn_input(tx: Sender<Message>, input: Arc<Input>) {
    thread::spawn(move || {
        loop {
            // Handing the terminal to another program means jack has to stop
            // reading it: two processes reading one tty share the keystrokes
            // out between them, which for the other program looks like a
            // keyboard that drops every other key.
            if input.paused.load(Ordering::Relaxed) {
                input.resting.store(true, Ordering::Relaxed);
                thread::sleep(RESTING);
                continue;
            }
            input.resting.store(false, Ordering::Relaxed);
            // And that is why this polls rather than blocking in `read`: a
            // thread asleep inside `event::read` cannot be told anything. The
            // wait is long enough that an idle editor is still an idle
            // process, and the cost of it is only ever paid once, when the
            // terminal is being handed over.
            match event::poll(WAITING) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(_) => return,
            }
            let message = match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => Message::Key(key),
                Ok(Event::Resize(..)) => Message::Resize,
                Ok(Event::Paste(text)) => Message::Paste(text),
                Ok(Event::FocusGained) => Message::Focus,
                Ok(_) => continue,
                // The terminal is gone; the main thread will find out too.
                Err(_) => return,
            };
            if tx.send(message).is_err() {
                return;
            }
        }
    });
}

/// Walk `root` for files, sending them in batches so the picker can be used
/// before the walk finishes. Honours `.gitignore` and skips hidden files,
/// which is the difference between listing a project and listing a disk.
/// `active` is the editor's current picker token: when it no longer matches
/// this walk's, the picker is gone and there is no reason to keep reading the
/// disk for it.
pub fn spawn_walk(root: PathBuf, token: u64, active: Arc<AtomicU64>, tx: Sender<Message>) {
    thread::spawn(move || {
        let walk = ignore::WalkBuilder::new(&root)
            // A `.gitignore` says what is not worth looking at whether or not
            // the directory has been `git init`ed yet.
            .require_git(false)
            .build();
        let mut batch = Vec::with_capacity(BATCH);
        let mut total = 0;

        for entry in walk.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            batch.push(path.to_string_lossy().into_owned());
            total += 1;

            if batch.len() == BATCH && active.load(Ordering::Relaxed) != token {
                return;
            }
            if batch.len() == BATCH || total == LIMIT {
                let items = std::mem::take(&mut batch);
                let done = total == LIMIT;
                if tx.send(Message::Items { token, items, done }).is_err() || done {
                    return;
                }
                batch = Vec::with_capacity(BATCH);
            }
        }

        let _ = tx.send(Message::Items { token, items: batch, done: true });
    });
}

/// Search every file under `root` for `pattern`, streaming matches as they are
/// found. Like the walk, it gives up as soon as the picker it feeds is gone -
/// which matters more here, because every keystroke starts another one.
pub fn spawn_grep(root: PathBuf, pattern: String, token: u64, active: Arc<AtomicU64>, tx: Sender<Message>) {
    thread::spawn(move || {
        // Smart case, as the fuzzy matcher does: an all-lower-case pattern
        // matches either case, one with a capital in it means it.
        let built = grep_regex::RegexMatcherBuilder::new()
            .case_smart(true)
            .build(&pattern);
        let matcher = match built {
            Ok(matcher) => matcher,
            Err(err) => {
                let error = format!("bad pattern: {}", brief(&err.to_string()));
                let _ = tx.send(Message::Failed { token, error });
                return;
            }
        };
        let mut searcher = grep_searcher::SearcherBuilder::new()
            .line_number(true)
            // A binary file's "lines" are noise; ripgrep skips them and so do we.
            .binary_detection(grep_searcher::BinaryDetection::quit(0))
            .build();

        let walk = ignore::WalkBuilder::new(&root).require_git(false).build();
        let mut batch: Vec<String> = Vec::with_capacity(BATCH);
        let mut total = 0;
        let mut stopped = false;

        for entry in walk.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            // Checked per file rather than per batch: a search that is already
            // out of date should die on the next keystroke, not the next batch.
            if active.load(Ordering::Relaxed) != token {
                return;
            }

            let path = entry.path();
            let display = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().into_owned();
            let sink = grep_searcher::sinks::UTF8(|line, text| {
                let text = text.trim_end_matches(['\n', '\r']);
                let text: String = text.chars().take(MAX_LINE).collect();
                batch.push(format!("{display}:{line}:{text}"));
                total += 1;
                if batch.len() >= BATCH {
                    let items = std::mem::take(&mut batch);
                    if tx.send(Message::Items { token, items, done: false }).is_err() {
                        return Ok(false);
                    }
                }
                Ok(total < MATCH_LIMIT)
            });

            if searcher.search_path(&matcher, path, sink).is_err() {
                // An unreadable or non-UTF-8 file is not worth reporting; the
                // next one may well match.
                continue;
            }
            if total >= MATCH_LIMIT {
                stopped = true;
                break;
            }
        }

        let _ = tx.send(Message::Items { token, items: batch, done: true });
        if stopped {
            let error = format!("stopped at {MATCH_LIMIT} matches");
            let _ = tx.send(Message::Failed { token, error });
        }
    });
}

/// A regex error is several lines of caret diagram with the actual complaint
/// at the bottom. The status line has room for the complaint.
fn brief(text: &str) -> String {
    for line in text.lines().rev() {
        if let Some(reason) = line.trim().strip_prefix("error: ") {
            return reason.to_string();
        }
    }
    text.lines().next().unwrap_or(text).trim().to_string()
}

/// Diff a buffer against the version git has committed, off the main thread.
/// Shells out to git rather than taking a git library: this needs one file's
/// worth of bytes, and `git show` is the whole of the API for that.
pub fn spawn_git_diff(path: PathBuf, text: String, token: u64, tx: Sender<Message>) {
    thread::spawn(move || {
        let Some(staged) = git_show_index(&path) else {
            // Not a repository, not tracked, or no commits yet: no signs, and
            // nothing worth complaining about.
            let _ = tx.send(Message::Signs { token, signs: Vec::new(), hunks: Vec::new() });
            return;
        };
        let signs = diff_lines(&staged, &text);
        let hunks = diff_hunks(&staged, &text);
        let _ = tx.send(Message::Signs { token, signs, hunks });
    });
}

/// The file as the index has it - what is staged, which is the last commit's
/// version until something is staged. Against the index rather than `HEAD`
/// so that staging a hunk is something the gutter can see: the lines it
/// staged stop being marked.
fn git_show_index(path: &Path) -> Option<String> {
    // A bare file name has an empty parent, which is not a directory git can
    // be run in. `HEAD:./name` is then resolved relative to that directory,
    // so this works from a subdirectory of the repository too.
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = path.file_name()?;
    let output = Command::new("git")
        .current_dir(directory)
        .arg("show")
        .arg(format!(":./{}", name.to_string_lossy()))
        .output()
        .ok()?;
    match output.status.success() {
        true => String::from_utf8(output.stdout).ok(),
        false => None,
    }
}

/// The hunks of a diff of `before` against `after`: every run of lines that is
/// not the same in both, whether it was added, removed or replaced.
pub fn diff_hunks(before: &str, after: &str) -> Vec<Hunk> {
    use similar::{DiffOp, TextDiff};

    let diff = TextDiff::from_lines(before, after);
    // Split the way the diff splits: each line keeps its line break.
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let mut hunks: Vec<Hunk> = Vec::new();
    for op in diff.ops() {
        if matches!(op, DiffOp::Equal { .. }) {
            continue;
        }
        let (old_range, new_range) = (op.old_range(), op.new_range());
        // A deletion straight after an insertion is one change, not two: the
        // diff reports them as neighbours, and they are one hunk to look at.
        match hunks.last_mut() {
            Some(last) if last.lines.end == new_range.start && last.old_start + last.old.len() == old_range.start => {
                last.lines.end = new_range.end;
                last.old.extend(old[old_range].iter().map(|line| line.to_string()));
            }
            _ => hunks.push(Hunk {
                lines: new_range,
                old_start: old_range.start,
                old: old[old_range].iter().map(|line| line.to_string()).collect(),
            }),
        }
    }
    hunks
}

/// A patch that stages exactly `hunk` and nothing else: no context lines, so
/// it applies whatever the rest of the file is doing, which is what
/// `git apply --unidiff-zero` is for. `new` is the buffer's lines for the
/// hunk, and `path` the file as the repository names it.
pub fn hunk_patch(path: &str, hunk: &Hunk, new: &[String]) -> String {
    // In a unified diff an empty range is written as the line *before* it,
    // and a non-empty one by its first line - both counted from one.
    let start = |at: usize, len: usize| match len {
        0 => at,
        _ => at + 1,
    };
    let mut patch = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
        start(hunk.old_start, hunk.old.len()),
        hunk.old.len(),
        start(hunk.lines.start, new.len()),
        new.len(),
    );
    let mut push = |sign: char, line: &str| {
        patch.push(sign);
        patch.push_str(line);
        if !line.ends_with('\n') {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    };
    for line in &hunk.old {
        push('-', line);
    }
    for line in new {
        push('+', line);
    }
    patch
}

/// Stage one hunk of `file`. Run from the top of the repository, where the
/// path in the patch is rooted.
pub fn git_stage(file: &Path, hunk: &Hunk, new: &[String]) -> Result<(), String> {
    use std::io::Write as _;

    let file = file.canonicalize().map_err(|err| err.to_string())?;
    let directory = file.parent().ok_or("no directory")?;
    let top = Command::new("git")
        .current_dir(directory)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|err| err.to_string())?;
    if !top.status.success() {
        return Err("not in a git repository".into());
    }
    let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim());
    let top = top.canonicalize().unwrap_or(top);
    let relative = file.strip_prefix(&top).map_err(|_| "outside the repository")?;
    let patch = hunk_patch(&relative.to_string_lossy(), hunk, new);

    let mut child = Command::new("git")
        .current_dir(&top)
        .args(["apply", "--cached", "--unidiff-zero", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    child.stdin.take().ok_or("no stdin")?.write_all(patch.as_bytes()).map_err(|err| err.to_string())?;
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    match output.status.success() {
        true => Ok(()),
        false => Err(String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("git apply failed").to_string()),
    }
}

/// Line signs from a diff of `before` against `after`.
fn diff_lines(before: &str, after: &str) -> Vec<(usize, Sign)> {
    use similar::{ChangeTag, TextDiff};

    let diff = TextDiff::from_lines(before, after);
    let mut signs = Vec::new();
    // Deletions immediately followed by insertions are lines that changed
    // rather than lines that came and went - but only as many of them as were
    // deleted. Three lines replacing one is one modification and two additions.
    let mut unmatched_deletes = 0usize;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => unmatched_deletes += 1,
            ChangeTag::Insert => {
                let line = change.new_index().unwrap_or(0);
                let sign = match unmatched_deletes > 0 {
                    true => {
                        unmatched_deletes -= 1;
                        Sign::Modified
                    }
                    false => Sign::Added,
                };
                signs.push((line, sign));
            }
            ChangeTag::Equal => {
                if unmatched_deletes > 0 {
                    // Lines went and nothing replaced them; mark where they were.
                    let line = change.new_index().unwrap_or(0);
                    signs.push((line, Sign::Deleted));
                    unmatched_deletes = 0;
                }
            }
        }
    }
    if unmatched_deletes > 0 {
        let last = after.lines().count().saturating_sub(1);
        signs.push((last, Sign::Deleted));
    }
    signs
}

/// Who last touched one line, and in which commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blame {
    /// The commit's hash, all zeros for a line nobody has committed.
    pub commit: String,
    pub author: String,
    /// Seconds since the epoch.
    pub time: u64,
    pub summary: String,
}

impl Blame {
    pub fn committed(&self) -> bool {
        self.commit.chars().any(|c| c != '0')
    }
}

/// `git blame` for line `line` (from zero) of `contents`, the buffer as it is
/// now: git is given the text on stdin, so a line edited but not saved is
/// "not committed" rather than blamed on whoever wrote what used to be there.
pub fn git_blame(file: &Path, line: usize, contents: &str) -> Result<Blame, String> {
    use std::io::Write as _;

    let directory = match file.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let name = file.file_name().ok_or("no file name")?;
    let range = format!("{},{}", line + 1, line + 1);
    let mut child = Command::new("git")
        .current_dir(directory)
        .args(["blame", "--porcelain", "-L", &range, "--contents", "-", "--"])
        .arg(name)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let text = contents.to_string();
    // Written from a thread: git may answer before it has read everything,
    // and a pipe that fills both ways would leave the two waiting on each other.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(text.as_bytes());
    });
    let output = child.wait_with_output().map_err(|err| err.to_string())?;
    let _ = writer.join();
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(err.lines().next().unwrap_or("git blame failed").trim_start_matches("fatal: ").to_string());
    }
    parse_blame(&String::from_utf8_lossy(&output.stdout)).ok_or_else(|| "git blame said nothing".into())
}

/// The first entry of `git blame --porcelain`: a header line whose first word
/// is the commit, then `key value` lines up to the line's own text.
fn parse_blame(porcelain: &str) -> Option<Blame> {
    let mut lines = porcelain.lines();
    let commit = lines.next()?.split_whitespace().next()?.to_string();
    let mut blame = Blame { commit, author: String::new(), time: 0, summary: String::new() };
    for line in lines {
        if line.starts_with('\t') {
            break;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "author" => blame.author = value.to_string(),
            "author-time" => blame.time = value.parse().unwrap_or(0),
            "summary" => blame.summary = value.to_string(),
            _ => {}
        }
    }
    Some(blame)
}

/// How long ago `then` was, the way a person says it: "3 days ago".
pub fn ago(then: u64, now: u64) -> String {
    let seconds = now.saturating_sub(then);
    let (n, unit) = match seconds {
        0..60 => return "just now".into(),
        60..3_600 => (seconds / 60, "minute"),
        3_600..86_400 => (seconds / 3_600, "hour"),
        86_400..2_592_000 => (seconds / 86_400, "day"),
        2_592_000..31_536_000 => (seconds / 2_592_000, "month"),
        _ => (seconds / 31_536_000, "year"),
    };
    match n {
        1 => format!("1 {unit} ago"),
        n => format!("{n} {unit}s ago"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    #[test]
    fn blame_reads_the_first_entry_of_the_porcelain() {
        let porcelain = "abc123 3 3 1\nauthor Ann\nauthor-mail <a@b>\nauthor-time 1000\nsummary Fix it\nfilename f\n\tthe line\n";
        let blame = parse_blame(porcelain).unwrap();
        assert_eq!(blame, Blame { commit: "abc123".into(), author: "Ann".into(), time: 1000, summary: "Fix it".into() });
        assert!(blame.committed());
        assert!(!Blame { commit: "0000000".into(), ..blame }.committed());
    }

    #[test]
    fn ago_says_it_the_way_a_person_would() {
        assert_eq!(ago(100, 110), "just now");
        assert_eq!(ago(0, 60), "1 minute ago");
        assert_eq!(ago(0, 7_300), "2 hours ago");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
        assert_eq!(ago(0, 800 * 86_400), "2 years ago");
        assert_eq!(ago(200, 100), "just now");
    }

    #[test]
    fn a_walk_finds_files_and_honours_gitignore() {
        let dir = std::env::temp_dir().join(format!("jack_walk_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        std::fs::write(dir.join("target/huge.o"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();

        let (tx, rx) = channels();
        spawn_walk(dir.clone(), 7, Arc::new(AtomicU64::new(7)), tx);

        let mut found = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Message::Items { token, items, done }) => {
                    assert_eq!(token, 7);
                    found.extend(items);
                    if done {
                        break;
                    }
                }
                other => panic!("expected a batch, got something else: {}", other.is_ok()),
            }
        }
        found.sort();

        // Paths are relative to the root, build output is ignored, and so are
        // hidden files - including the `.gitignore` that did the ignoring.
        assert_eq!(found, ["src/main.rs"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_walk_stops_when_its_picker_is_gone() {
        // The token the walk was started with no longer matches, so it should
        // give up rather than read a whole disk for nobody.
        let (tx, rx) = channels();
        spawn_walk(PathBuf::from("/usr"), 7, Arc::new(AtomicU64::new(8)), tx);

        let mut batches = 0;
        while let Ok(Message::Items { done, .. }) = rx.recv_timeout(Duration::from_secs(10)) {
            batches += 1;
            if done {
                break;
            }
        }
        // At most the first batch, which is sent before the check.
        assert!(batches <= 1, "kept walking: {batches} batches");
    }

    fn sample_tree(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("jack_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join(".gitignore"), "/target\n").unwrap();
        std::fs::write(dir.join("src/one.rs"), "fn one() {}\nlet x = 1;\n").unwrap();
        std::fs::write(dir.join("src/two.rs"), "fn two() {}\n").unwrap();
        std::fs::write(dir.join("target/built.rs"), "fn built() {}\n").unwrap();
        std::fs::write(dir.join("src/blob.bin"), b"fn binary\0() {}\n").unwrap();
        dir
    }

    fn collect(rx: &Receiver<Message>) -> (Vec<String>, Vec<String>) {
        let (mut items, mut errors) = (Vec::new(), Vec::new());
        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Message::Items { items: batch, done, .. }) => {
                    items.extend(batch);
                    if done {
                        // Any complaint follows the final batch.
                        while let Ok(Message::Failed { error, .. }) =
                            rx.recv_timeout(Duration::from_millis(50))
                        {
                            errors.push(error);
                        }
                        return (items, errors);
                    }
                }
                Ok(Message::Failed { error, .. }) => {
                    errors.push(error);
                    return (items, errors);
                }
                _ => return (items, errors),
            }
        }
    }

    #[test]
    fn a_search_reports_where_each_hit_is() {
        let dir = sample_tree("grep");
        let (tx, rx) = channels();
        spawn_grep(dir.clone(), "fn ".into(), 1, Arc::new(AtomicU64::new(1)), tx);

        let (mut items, errors) = collect(&rx);
        items.sort();
        assert!(errors.is_empty(), "{errors:?}");
        // Ignored and binary files are not searched.
        assert_eq!(items, ["src/one.rs:1:fn one() {}", "src/two.rs:1:fn two() {}"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_search_pattern_is_a_regex() {
        let dir = sample_tree("grep_regex");
        let (tx, rx) = channels();
        spawn_grep(dir.clone(), r"let \w+ =".into(), 1, Arc::new(AtomicU64::new(1)), tx);

        let (items, errors) = collect(&rx);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(items, ["src/one.rs:2:let x = 1;"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pattern_that_is_not_a_regex_is_reported_not_run() {
        let dir = sample_tree("grep_bad");
        let (tx, rx) = channels();
        spawn_grep(dir.clone(), "fn (".into(), 1, Arc::new(AtomicU64::new(1)), tx);

        let (items, errors) = collect(&rx);
        assert!(items.is_empty());
        assert_eq!(errors.len(), 1);
        // The complaint itself, not the caret diagram around it.
        assert!(!errors[0].contains('\n'), "{}", errors[0]);
        assert!(errors[0].contains("unclosed group"), "{}", errors[0]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_search_stops_when_its_picker_is_gone() {
        let dir = sample_tree("grep_stale");
        let (tx, rx) = channels();
        spawn_grep(dir.clone(), "fn ".into(), 1, Arc::new(AtomicU64::new(2)), tx);

        let (items, _) = collect(&rx);
        assert!(items.is_empty(), "kept searching: {items:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn a_lower_case_pattern_matches_either_case() {
        let dir = sample_tree("grep_case");
        std::fs::write(dir.join("src/three.rs"), "FN THREE\n").unwrap();
        let (tx, rx) = channels();
        spawn_grep(dir.clone(), "fn t".into(), 1, Arc::new(AtomicU64::new(1)), tx);

        let (mut items, _) = collect(&rx);
        items.sort();
        assert_eq!(items, ["src/three.rs:1:FN THREE", "src/two.rs:1:fn two() {}"]);

        // A capital in the pattern means it.
        let (tx, rx) = channels();
        spawn_grep(dir.clone(), "FN".into(), 1, Arc::new(AtomicU64::new(1)), tx);
        let (items, _) = collect(&rx);
        assert_eq!(items, ["src/three.rs:1:FN THREE"]);
        let _ = std::fs::remove_dir_all(&dir);
    }


    #[test]
    fn a_diff_marks_added_modified_and_deleted_lines() {
        let before = "one\ntwo\nthree\n";

        // A line changed in place is modified, not added-and-deleted.
        assert_eq!(diff_lines(before, "one\nTWO\nthree\n"), [(1, Sign::Modified)]);
        // A new line is added.
        assert_eq!(diff_lines(before, "one\ntwo\nextra\nthree\n"), [(2, Sign::Added)]);
        // A line that went leaves a mark where it was.
        assert_eq!(diff_lines(before, "one\nthree\n"), [(1, Sign::Deleted)]);
        // Nothing changed, nothing marked.
        assert_eq!(diff_lines(before, before), []);

        // Two lines changed in a row are two modifications, not a
        // modification and an addition.
        assert_eq!(
            diff_lines(before, "ONE\nTWO\nthree\n"),
            [(0, Sign::Modified), (1, Sign::Modified)]
        );
        // More lines than were replaced: the extras are additions.
        assert_eq!(
            diff_lines(before, "one\nTWO\nEXTRA\nthree\n"),
            [(1, Sign::Modified), (2, Sign::Added)]
        );
    }

    #[test]
    fn hunks_are_the_runs_that_differ_with_what_git_had() {
        let hunks = diff_hunks("a\nb\nc\nd\n", "a\nB\nnew\nc\n");
        assert_eq!(
            hunks,
            [
                // A replacement and an addition beside it are one hunk.
                Hunk { lines: 1..3, old_start: 1, old: vec!["b\n".into()] },
                // A deletion has no lines of its own; it sits before line 4.
                Hunk { lines: 4..4, old_start: 3, old: vec!["d\n".into()] },
            ]
        );
    }

    #[test]
    fn a_hunk_patch_counts_an_empty_side_from_the_line_before() {
        let added = Hunk { lines: 3..4, old_start: 3, old: vec![] };
        let patch = hunk_patch("sub/f.txt", &added, &["d\n".into()]);
        assert_eq!(patch, "--- a/sub/f.txt\n+++ b/sub/f.txt\n@@ -3,0 +4,1 @@\n+d\n");

        let unterminated = Hunk { lines: 0..1, old_start: 0, old: vec!["x\n".into()] };
        let patch = hunk_patch("f", &unterminated, &["y".into()]);
        assert!(patch.ends_with("+y\n\\ No newline at end of file\n"), "{patch}");
    }

    #[test]
    fn a_deletion_at_the_end_still_gets_a_mark() {
        // The lines are gone, so the mark goes on the last line that is left -
        // there is no row of its own to put it on.
        let signs = diff_lines("one\ntwo\nthree\n", "one\n");
        assert_eq!(signs, [(0, Sign::Deleted)]);
    }

    #[test]
    fn a_file_git_does_not_know_about_gets_no_signs() {
        let dir = std::env::temp_dir().join(format!("jack_nogit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("loose.txt");
        std::fs::write(&path, "hello\n").unwrap();

        let (tx, rx) = channels();
        spawn_git_diff(path, "hello\nworld\n".into(), 1, tx);
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Message::Signs { signs, .. }) => assert!(signs.is_empty()),
            _ => panic!("expected an empty set of signs"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

}
