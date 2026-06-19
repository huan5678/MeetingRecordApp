//! Filesystem management for media files (paths, sizes, cleanup).
//!
//! The recordings root holds one subdirectory per meeting (`<root>/<meeting_id>/`)
//! into which the audio module writes its WAV file(s). This module owns the
//! directory layout, the "total storage used" calculation behind the settings
//! screen (PRD §3.5 story 30), and safe deletion of a meeting's files when the
//! user removes a recording (story 29).
//!
//! Everything here is plain `std::fs`; no platform-specific code, so it builds
//! and runs the same on the dev Mac and the Windows target.

use std::path::{Path, PathBuf};

use crate::storage::Result;

/// Owns the on-disk recordings directory.
#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    /// Create a store rooted at `root`, creating the directory if missing.
    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// The recordings root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/<meeting_id>` — the directory holding a meeting's media. Does not
    /// touch the filesystem; pair with [`FileStore::ensure_meeting_dir`].
    pub fn meeting_dir(&self, meeting_id: &str) -> PathBuf {
        self.root.join(meeting_id)
    }

    /// Create (if needed) and return the meeting's media directory.
    pub fn ensure_meeting_dir(&self, meeting_id: &str) -> Result<PathBuf> {
        let dir = self.meeting_dir(meeting_id);
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// Path for a named file inside a meeting's directory (e.g. `mix.wav`).
    /// Creates the meeting directory so callers can open the file for writing.
    pub fn media_path(&self, meeting_id: &str, file_name: &str) -> Result<PathBuf> {
        let dir = self.ensure_meeting_dir(meeting_id)?;
        Ok(dir.join(file_name))
    }

    /// Total bytes used by everything under the recordings root. Walks the tree;
    /// missing files are skipped. Used for the storage-usage indicator.
    pub fn total_storage_bytes(&self) -> Result<u64> {
        dir_size(&self.root)
    }

    /// Bytes used by a single meeting's directory. `0` if it doesn't exist.
    pub fn meeting_storage_bytes(&self, meeting_id: &str) -> Result<u64> {
        let dir = self.meeting_dir(meeting_id);
        if dir.exists() {
            dir_size(&dir)
        } else {
            Ok(0)
        }
    }

    /// Delete a meeting's entire media directory. Idempotent: a non-existent
    /// directory is a no-op (returns `false`); deletion returns `true`.
    pub fn delete_meeting_files(&self, meeting_id: &str) -> Result<bool> {
        let dir = self.meeting_dir(meeting_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete a single file by absolute path, but only if it lives under the
    /// recordings root — a guard against a stale DB row pointing somewhere it
    /// shouldn't. Returns `true` if a file was removed.
    pub fn delete_file<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let path = path.as_ref();
        if !is_within(&self.root, path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "refusing to delete {path:?}: outside recordings root {:?}",
                    self.root
                ),
            )
            .into());
        }
        if path.exists() {
            std::fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Recursively sum the byte size of every regular file under `dir`.
fn dir_size(dir: &Path) -> Result<u64> {
    let mut total = 0u64;
    if !dir.exists() {
        return Ok(0);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total = total.saturating_add(dir_size(&entry.path())?);
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
        // Symlinks etc. are ignored.
    }
    Ok(total)
}

/// True if `path` is the same as, or nested under, `root`. Uses lexical
/// comparison of the (best-effort) absolute forms so it works for paths that
/// don't exist yet.
fn is_within(root: &Path, path: &Path) -> bool {
    let abs_root = absolutize(root);
    let abs_path = absolutize(path);
    abs_path.starts_with(&abs_root)
}

/// Best-effort absolute, normalized path without hitting the filesystem
/// (`canonicalize` requires the path to exist). Resolves `.`/`..` lexically.
fn absolutize(path: &Path) -> PathBuf {
    let base = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    let mut out = base;
    for comp in path.components() {
        use std::path::Component::*;
        match comp {
            Prefix(p) => out.push(p.as_os_str()),
            RootDir => {
                // Keep any drive prefix already pushed, then reset to root.
                let prefix = out
                    .components()
                    .next()
                    .and_then(|c| match c {
                        Prefix(p) => Some(PathBuf::from(p.as_os_str())),
                        _ => None,
                    })
                    .unwrap_or_default();
                out = prefix;
                out.push(std::path::MAIN_SEPARATOR.to_string());
            }
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            Normal(seg) => out.push(seg),
        }
    }
    out
}

/// Format a byte count as a human-readable string (e.g. "1.5 MB"). Handy for
/// the settings UI; binary (1024) units.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A throwaway temp directory that cleans itself up on drop. Avoids pulling
    /// in the `tempfile` crate just for tests.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            let unique = format!(
                "mra_files_test_{tag}_{}_{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            p.push(unique);
            std::fs::create_dir_all(&p).unwrap();
            Self { path: p }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    #[test]
    fn new_creates_root() {
        let tmp = TempDir::new("root");
        let root = tmp.path.join("recordings");
        assert!(!root.exists());
        let store = FileStore::new(&root).unwrap();
        assert!(root.exists());
        assert_eq!(store.root(), root.as_path());
    }

    #[test]
    fn meeting_dir_and_media_path_layout() {
        let tmp = TempDir::new("layout");
        let store = FileStore::new(tmp.path.join("recordings")).unwrap();
        let dir = store.ensure_meeting_dir("m1").unwrap();
        assert!(dir.ends_with("m1"));
        assert!(dir.exists());

        let p = store.media_path("m1", "mix.wav").unwrap();
        assert!(p.ends_with("mix.wav"));
        assert_eq!(p.parent().unwrap(), dir);
    }

    #[test]
    fn total_and_per_meeting_storage_bytes() {
        let tmp = TempDir::new("size");
        let store = FileStore::new(tmp.path.join("recordings")).unwrap();
        assert_eq!(store.total_storage_bytes().unwrap(), 0);

        let d1 = store.ensure_meeting_dir("m1").unwrap();
        write_file(&d1.join("a.wav"), &[0u8; 1000]);
        write_file(&d1.join("b.wav"), &[0u8; 500]);
        let d2 = store.ensure_meeting_dir("m2").unwrap();
        write_file(&d2.join("c.wav"), &[0u8; 250]);

        assert_eq!(store.meeting_storage_bytes("m1").unwrap(), 1500);
        assert_eq!(store.meeting_storage_bytes("m2").unwrap(), 250);
        assert_eq!(store.meeting_storage_bytes("missing").unwrap(), 0);
        assert_eq!(store.total_storage_bytes().unwrap(), 1750);
    }

    #[test]
    fn delete_meeting_files_removes_directory() {
        let tmp = TempDir::new("del");
        let store = FileStore::new(tmp.path.join("recordings")).unwrap();
        let d1 = store.ensure_meeting_dir("m1").unwrap();
        write_file(&d1.join("a.wav"), &[0u8; 10]);

        assert!(store.delete_meeting_files("m1").unwrap());
        assert!(!d1.exists());
        // Idempotent: deleting again is a no-op.
        assert!(!store.delete_meeting_files("m1").unwrap());
    }

    #[test]
    fn delete_file_within_root_ok() {
        let tmp = TempDir::new("delfile");
        let store = FileStore::new(tmp.path.join("recordings")).unwrap();
        let p = store.media_path("m1", "x.wav").unwrap();
        write_file(&p, &[0u8; 4]);
        assert!(store.delete_file(&p).unwrap());
        assert!(!p.exists());
        // Deleting a non-existent (but in-root) file returns false, not error.
        assert!(!store.delete_file(&p).unwrap());
    }

    #[test]
    fn delete_file_outside_root_is_rejected() {
        let tmp = TempDir::new("guard");
        let store = FileStore::new(tmp.path.join("recordings")).unwrap();
        // A path outside the recordings root.
        let outside = tmp.path.join("not_recordings.txt");
        write_file(&outside, &[0u8; 4]);
        let err = store.delete_file(&outside).unwrap_err();
        // The guard surfaces as an Io(PermissionDenied) error and the file lives.
        assert!(matches!(err, crate::storage::StorageError::Io(_)));
        assert!(outside.exists());
    }

    #[test]
    fn format_bytes_human_readable() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }
}
