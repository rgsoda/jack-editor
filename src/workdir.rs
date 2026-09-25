//! The one directory jack works from.
//!
//! The file picker walks it, `:grep` searches it, and a relative path typed
//! after `:e` is counted from it. That is the process's own working
//! directory, deliberately: one place, which every job and every subprocess
//! already agrees on, rather than a root threaded through each of them.
//!
//! A terminal hands jack somewhere sensible - wherever you were standing. A
//! launcher does not: the Dock, Finder and a desktop entry all start a
//! program at the root of the filesystem, where a file picker has the whole
//! machine to walk and nothing you wanted in it. So the window frontend
//! settles somewhere itself, and `:cd` moves.

use std::path::{Path, PathBuf};

/// `$HOME`, if it is set and is a directory.
pub fn home() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.is_dir().then_some(home)
}

/// True when the directory jack was started in is not one anyone chose: the
/// root of the filesystem, which is what a launcher hands a program, or a
/// directory that has since been removed.
pub fn adrift() -> bool {
    match std::env::current_dir() {
        Ok(dir) => dir == Path::new("/"),
        Err(_) => true,
    }
}

/// The project a path belongs to: the nearest directory at or above it
/// holding a `.git`, else the directory it is in. The same walk a language
/// server's root is found by, and for the same reason - a repository is what
/// people mean by "the project" nearly every time.
pub fn project_root(path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    let start = match path.is_dir() {
        true => path.as_path(),
        false => path.parent()?,
    };
    let repo = start.ancestors().find(|dir| dir.join(".git").exists());
    Some(repo.unwrap_or(start).to_path_buf())
}

/// `~` and `~/src` as they are typed, which is not something the shell has
/// expanded when the path came from jack's own command line.
pub fn expanded(path: &str) -> PathBuf {
    let Some(home) = home() else {
        return PathBuf::from(path);
    };
    match path {
        "~" => home,
        path => match path.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path),
        },
    }
}

/// A directory as it is worth reading in a status line: under home, with a
/// `~` standing in for it.
pub fn shortened(dir: &Path) -> String {
    match home().and_then(|home| dir.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Go there, and say where that turned out to be. Canonical, because it is
/// shown and compared as well as worked from.
pub fn enter(dir: &Path) -> std::io::Result<PathBuf> {
    std::env::set_current_dir(dir)?;
    Ok(std::env::current_dir().unwrap_or_else(|_| dir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_under_a_repository_belongs_to_the_repository() {
        // A repository of its own rather than jack's, since a test that reads
        // the process's working directory is a test that another test can
        // move the ground under.
        let repo = std::env::temp_dir()
            .join(format!("jack_workdir_{}", std::process::id()))
            .canonicalize()
            .unwrap_or_else(|_| std::env::temp_dir().join(format!("jack_workdir_{}", std::process::id())));
        let deep = repo.join("src/inner");
        std::fs::create_dir_all(repo.join(".git")).expect("a repository");
        std::fs::create_dir_all(&deep).expect("somewhere under it");
        std::fs::write(deep.join("file.rs"), "").expect("a file in it");
        let repo = repo.canonicalize().expect("a real path");

        assert_eq!(project_root(&deep.join("file.rs")).as_deref(), Some(repo.as_path()));
        // A directory is its own starting point rather than its parent's.
        assert_eq!(project_root(&deep).as_deref(), Some(repo.as_path()));
        assert_eq!(project_root(Path::new("/definitely/not/here")), None);
        std::fs::remove_dir_all(&repo).ok();
    }

    #[test]
    fn a_path_outside_any_repository_is_the_directory_it_is_in() {
        let temp = std::env::temp_dir().canonicalize().expect("a temp directory");
        // Only true when the temp directory is not itself in a repository,
        // which on every machine this runs on it is not.
        assert_eq!(project_root(&temp).as_deref(), Some(temp.as_path()));
    }

    #[test]
    fn a_tilde_is_home_and_nothing_else_is() {
        let Some(home) = home() else {
            return;
        };
        assert_eq!(expanded("~"), home);
        assert_eq!(expanded("~/src"), home.join("src"));
        // Not a prefix to be found in the middle, and not part of a name.
        assert_eq!(expanded("~work"), PathBuf::from("~work"));
        assert_eq!(expanded("/etc/~"), PathBuf::from("/etc/~"));
    }

    #[test]
    fn home_is_written_as_a_tilde_and_the_rest_in_full() {
        let Some(home) = home() else {
            return;
        };
        assert_eq!(shortened(&home), "~");
        assert_eq!(shortened(&home.join("code/jack")), "~/code/jack");
        assert_eq!(shortened(Path::new("/usr/share")), "/usr/share");
    }
}
