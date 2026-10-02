use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct ScratchDir(PathBuf);

impl ScratchDir {
    pub fn new() -> ScratchDir {
        let path = env::temp_dir().join(format!(
            "clawlog-test-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        ScratchDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::ScratchDir;

    #[test]
    fn dirs_are_unique_and_removed_on_drop() {
        let first = ScratchDir::new();
        let second = ScratchDir::new();
        assert_ne!(first.path(), second.path());
        fs::write(first.path().join("f"), "x").unwrap();
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists());
        assert!(second.path().is_dir());
    }
}
