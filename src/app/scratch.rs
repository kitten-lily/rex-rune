//! Private folders for tests: each is created exclusively by the test that asks for it and
//! removed with what it holds when its owner drops, including when that test unwinds.
//!
//! A folder derived from a process id, a counter or a label is not proof of ownership. Being
//! created by this call is: a name that already exists is refused, never adopted or removed.
//! Folders are made under the caller's temporary directory, so a run that sets `TMPDIR` to a
//! private place keeps every file in it, and nothing here falls back to a shared one.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// How many names to try before reporting that the temporary directory is unusable.
const ATTEMPTS: usize = 64;

/// `name` under `root`, made new by this call, or `None` when that name is already taken.
pub fn claim(root: &Path, name: &str) -> Option<PathBuf> {
    let path = root.join(name);
    match std::fs::create_dir(&path) {
        Ok(()) => Some(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
        Err(error) => panic!("a private test folder {path:?}: {error}"),
    }
}

/// A folder one test owns. Its contents go when it does.
pub struct Folder {
    path: PathBuf,
}

impl Folder {
    /// A new, empty folder described by `label` for the person reading a failure.
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir();
        for _ in 0..ATTEMPTS {
            let name = format!(
                "rex-test-{}-{}-{label}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            if let Some(path) = claim(&root, &name) {
                return Self { path };
            }
        }
        panic!(
            "no unused name for a private test folder under {}",
            root.display()
        );
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `name` inside this folder, which need not exist yet.
    pub fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for Folder {
    /// Removes this test's folder and what the test put in it, and nothing else. It cannot
    /// fail a test that is already unwinding: cleanup errors are left alone.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

impl std::ops::Deref for Folder {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for Folder {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_folder_is_the_tests_own_and_goes_with_it() {
        let first = Folder::new("owner");
        let second = Folder::new("sibling");
        let (one, two) = (first.path().to_path_buf(), second.path().to_path_buf());
        assert!(
            one.starts_with(std::env::temp_dir()),
            "under the run's TMPDIR: {one:?}"
        );
        assert_ne!(one, two, "each test gets a folder of its own");
        std::fs::write(one.join("workspace.json"), b"{}").unwrap();
        drop(first);
        assert!(
            !one.exists(),
            "the owner removed its folder and the file in it"
        );
        assert!(two.is_dir(), "another test's folder is untouched");
    }

    #[test]
    fn a_name_already_on_disk_is_refused_and_never_adopted() {
        let owner = Folder::new("claim");
        let taken = owner.join("taken");
        std::fs::create_dir(&taken).unwrap();
        std::fs::write(taken.join("theirs.txt"), b"mine").unwrap();
        assert_eq!(
            claim(owner.path(), "taken"),
            None,
            "a folder this call did not make is not ours"
        );
        assert_eq!(
            std::fs::read_to_string(taken.join("theirs.txt")).unwrap(),
            "mine",
            "and what it holds stays"
        );
        assert!(claim(owner.path(), "free").unwrap().is_dir());
    }

    #[test]
    fn a_failing_test_still_removes_its_folder() {
        let seen = std::cell::RefCell::new(None);
        let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let folder = Folder::new("unwind");
            *seen.borrow_mut() = Some(folder.path().to_path_buf());
            std::fs::write(folder.join("workspace.json"), b"{}").unwrap();
            panic!("the test fails after making its folder");
        }));
        assert!(failed.is_err(), "the panic still happened");
        let path = seen.into_inner().expect("the folder's path");
        assert!(!path.exists(), "unwinding removed it: {path:?}");
    }
}
