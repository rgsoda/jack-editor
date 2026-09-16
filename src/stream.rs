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
    /// How the current buffer differs from what git has, as one sign per
    /// changed line. `token` identifies the view that asked.
    Signs { token: u64, signs: Vec<(usize, Sign)> },
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

/// Read the terminal forever on its own thread. Blocking here rather than in
/// the run loop is what lets the loop wait on background work too.
pub fn spawn_input(tx: Sender<Message>) {
    thread::spawn(move || {
        loop {
            let message = match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => Message::Key(key),
                Ok(Event::Resize(..)) => Message::Resize,
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
        let Some(head) = git_show_head(&path) else {
            // Not a repository, not tracked, or no commits yet: no signs, and
            // nothing worth complaining about.
            let _ = tx.send(Message::Signs { token, signs: Vec::new() });
            return;
        };
        let _ = tx.send(Message::Signs { token, signs: diff_lines(&head, &text) });
    });
}

fn git_show_head(path: &Path) -> Option<String> {
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
        .arg(format!("HEAD:./{}", name.to_string_lossy()))
        .output()
        .ok()?;
    match output.status.success() {
        true => String::from_utf8(output.stdout).ok(),
        false => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

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
