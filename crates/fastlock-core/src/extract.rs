//! Writing decrypted contents to disk, safely.
//!
//! Everything is written into a temporary folder next to the final place. It
//! is moved into place only after the whole file has been authenticated, so a
//! damaged or interrupted file never leaves a partial result behind.

use std::fs::{self, File, OpenOptions};
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, ErrorKind, Result};
use crate::pipeline::Sink;

/// The name of the file that holds data that is not an archive, inside the
/// temporary folder.
const SINGLE_NAME: &str = "data";

/// A temporary folder that receives the contents.
///
/// The folder is removed when this value is dropped, unless
/// [`Staging::keep`] was called.
pub(crate) struct Staging {
    root: PathBuf,
    keep: bool,
    pub(crate) skipped: Vec<String>,
}

impl Staging {
    /// Creates the temporary folder next to `output`.
    pub(crate) fn create(output: &Path) -> Result<Self> {
        let parent = match output.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let name = output
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_string());
        let root = parent.join(format!(".{name}.fastlock-partial-{}", std::process::id()));

        create_private_dir(&root, false).map_err(|e| Error::io("create", &root, e))?;
        Ok(Staging {
            root,
            keep: false,
            skipped: Vec::new(),
        })
    }

    /// The temporary folder.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// The file that holds data that was not an archive.
    pub(crate) fn single_file(&self) -> PathBuf {
        self.root.join(SINGLE_NAME)
    }

    /// Stops the folder from being removed on drop.
    pub(crate) fn keep(&mut self) {
        self.keep = true;
    }

    /// Turns a path from inside an archive into a path inside the temporary
    /// folder. Refuses anything that could land outside it.
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let unsafe_path = || {
            Error::new(
                ErrorKind::Refused,
                format!("the archive holds an unsafe path and was not unpacked: {path}"),
            )
        };
        if path.is_empty() || path.contains('\0') || path.contains('\\') {
            return Err(unsafe_path());
        }
        let mut resolved = self.root.clone();
        let mut parts = 0usize;
        for component in Path::new(path).components() {
            match component {
                Component::Normal(part) => {
                    resolved.push(part);
                    parts += 1;
                }
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(unsafe_path());
                }
            }
        }
        if parts == 0 {
            return Err(unsafe_path());
        }
        Ok(resolved)
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

impl Sink for Staging {
    fn directory(&mut self, path: &str) -> Result<()> {
        let target = self.resolve(path)?;
        create_private_dir(&target, true).map_err(|e| Error::io("create", &target, e))
    }

    fn start_file(&mut self, path: &str) -> Result<Option<File>> {
        let target = self.resolve(path)?;
        if let Some(parent) = target.parent() {
            create_private_dir(parent, true).map_err(|e| Error::io("create", parent, e))?;
        }
        let file = create_private_file(&target).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new(
                    ErrorKind::Refused,
                    format!("the archive holds the same path twice and was not unpacked: {path}"),
                )
            } else {
                Error::io("create", &target, e)
            }
        })?;
        Ok(Some(file))
    }

    fn start_single(&mut self) -> Result<Option<File>> {
        let target = self.single_file();
        let file = create_private_file(&target).map_err(|e| Error::io("create", &target, e))?;
        Ok(Some(file))
    }

    fn skipped(&mut self, path: &str, reason: &str) {
        self.skipped.push(format!("{path}: {reason}"));
    }
}

/// Creates a folder that only its owner can use.
fn create_private_dir(path: &Path, recursive: bool) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Creates a new file that only its owner can read and write. Fails when the
/// file exists.
fn create_private_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Flushes a folder entry to disk, so that a finished rename survives a
/// crash. Does nothing where folders cannot be opened.
pub(crate) fn sync_dir(path: &Path) {
    if let Ok(dir) = File::open(path) {
        let _ = dir.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn writes_files_into_a_private_temporary_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = dir.path().join("out");
        let mut staging = Staging::create(&output).expect("staging");
        assert!(staging.root().starts_with(dir.path()));
        assert!(staging.root().is_dir());

        staging.directory("sub").expect("directory");
        let mut file = staging
            .start_file("sub/deep/a.txt")
            .expect("file")
            .expect("a file handle");
        file.write_all(b"abc").expect("write");
        drop(file);
        assert_eq!(
            fs::read(staging.root().join("sub/deep/a.txt")).expect("read back"),
            b"abc"
        );
    }

    #[test]
    fn refuses_paths_that_leave_the_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let staging = Staging::create(&dir.path().join("out")).expect("staging");
        for bad in ["../evil", "a/../../evil", "/etc/passwd", "", ".", "a\\b"] {
            let err = staging.resolve(bad).expect_err("must refuse");
            assert_eq!(err.kind(), ErrorKind::Refused, "path {bad:?}");
        }
        assert!(staging.resolve("a/b.txt").is_ok());
        assert!(staging.resolve("./a").is_ok());
    }

    #[test]
    fn refuses_the_same_path_twice() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut staging = Staging::create(&dir.path().join("out")).expect("staging");
        staging.start_file("a.txt").expect("first time");
        let err = staging.start_file("a.txt").expect_err("second time");
        assert_eq!(err.kind(), ErrorKind::Refused);
    }

    #[test]
    fn the_temporary_folder_is_removed_unless_kept() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = {
            let staging = Staging::create(&dir.path().join("out")).expect("staging");
            staging.root().to_path_buf()
        };
        assert!(!root.exists());

        let root = {
            let mut staging = Staging::create(&dir.path().join("other")).expect("staging");
            staging.keep();
            staging.root().to_path_buf()
        };
        assert!(root.exists());
    }
}
