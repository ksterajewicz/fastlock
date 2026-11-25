//! Keys and passphrases: what a caller hands over to open a file.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use age::secrecy::SecretString;
use zeroize::Zeroizing;

use crate::age_header;
use crate::error::{Error, ErrorKind, Result};
use crate::native;

/// The largest key file fastlock reads.
const MAX_KEY_FILE: u64 = 1 << 20;

/// What opens a file: a key, or a passphrase.
///
/// The secret is wiped from memory when this value is dropped.
pub struct Credentials(pub(crate) Secret);

pub(crate) enum Secret {
    /// The text of an age identity file: one or more secret keys.
    Identities(Zeroizing<Vec<u8>>),
    /// The passphrase of a passphrase-protected file.
    Passphrase(SecretString),
}

impl Credentials {
    /// Credentials for a file that is protected by a passphrase.
    pub fn passphrase(passphrase: SecretString) -> Self {
        Credentials(Secret::Passphrase(passphrase))
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.0 {
            Secret::Identities(_) => "key",
            Secret::Passphrase(_) => "passphrase",
        };
        write!(f, "Credentials({kind}, secret hidden)")
    }
}

/// A key file read from disk.
///
/// A key file holds one or more secret keys. It can be protected by a
/// passphrase: then the file is itself an age file, and
/// [`IdentityFile::needs_passphrase`] returns `true`.
pub struct IdentityFile {
    path: PathBuf,
    data: Zeroizing<Vec<u8>>,
    encrypted: bool,
}

impl IdentityFile {
    /// Reads a key file. Nothing is decrypted yet.
    pub fn read(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| Error::io("open the key file", path, e))?;
        let metadata = file
            .metadata()
            .map_err(|e| Error::io("read the key file", path, e))?;
        if metadata.is_dir() {
            return Err(Error::new(
                ErrorKind::Usage,
                format!("{} is a folder, not a key file", path.display()),
            ));
        }
        if metadata.len() > MAX_KEY_FILE {
            return Err(Error::new(
                ErrorKind::Usage,
                format!("{} is too large to be a key file", path.display()),
            ));
        }

        let mut data = Zeroizing::new(Vec::new());
        file.take(MAX_KEY_FILE)
            .read_to_end(&mut data)
            .map_err(|e| Error::io("read the key file", path, e))?;

        let encrypted = age_header::is_binary_age(&data) || age_header::is_armored(&data);
        if !encrypted && !holds_secret_key(&data) {
            return Err(Error::new(
                ErrorKind::Usage,
                format!("{} does not look like a key file", path.display()),
            ));
        }

        Ok(IdentityFile {
            path: path.to_path_buf(),
            data,
            encrypted,
        })
    }

    /// The path the key file was read from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the key file is protected by a passphrase.
    pub fn needs_passphrase(&self) -> bool {
        self.encrypted
    }

    /// Turns the key file into [`Credentials`].
    ///
    /// Pass the passphrase when [`IdentityFile::needs_passphrase`] is `true`.
    /// A wrong passphrase gives an error of kind [`ErrorKind::WrongKey`].
    pub fn unlock(&self, passphrase: Option<&SecretString>) -> Result<Credentials> {
        if !self.encrypted {
            return Ok(Credentials(Secret::Identities(self.data.clone())));
        }

        let Some(passphrase) = passphrase else {
            return Err(Error::new(
                ErrorKind::Usage,
                format!(
                    "the key file {} is protected by a passphrase",
                    self.path.display()
                ),
            ));
        };

        let source = std::io::Cursor::new(self.data.to_vec());
        let mut reader = native::passphrase_reader(source, passphrase)?;
        let mut plain = Zeroizing::new(Vec::new());
        reader.read_to_end(&mut plain).map_err(|_| {
            Error::new(
                ErrorKind::Damaged,
                format!(
                    "the key file {} is damaged: authentication failed",
                    self.path.display()
                ),
            )
        })?;

        if !holds_secret_key(&plain) {
            return Err(Error::new(
                ErrorKind::Usage,
                format!(
                    "{} was decrypted, but it does not hold a key",
                    self.path.display()
                ),
            ));
        }
        Ok(Credentials(Secret::Identities(plain)))
    }
}

impl std::fmt::Debug for IdentityFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityFile")
            .field("path", &self.path)
            .field("encrypted", &self.encrypted)
            .finish_non_exhaustive()
    }
}

/// Whether the text holds at least one secret key: an age secret key, or an
/// OpenSSH private key.
fn holds_secret_key(data: &[u8]) -> bool {
    data.split(|&b| b == b'\n').any(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        line.starts_with(b"AGE-SECRET-KEY-")
            || line.starts_with(b"-----BEGIN OPENSSH PRIVATE KEY-----")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_a_plain_key_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("key.txt");
        fs::write(&path, b"# created: test\nAGE-SECRET-KEY-1TESTTESTTEST\n").expect("write");

        let key = IdentityFile::read(&path).expect("reads");
        assert!(!key.needs_passphrase());
        assert_eq!(key.path(), path.as_path());
        let credentials = key.unlock(None).expect("unlocks without a passphrase");
        assert!(matches!(credentials.0, Secret::Identities(_)));
    }

    #[test]
    fn a_protected_key_file_asks_for_a_passphrase() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("key.age");
        fs::write(
            &path,
            b"age-encryption.org/v1\n-> scrypt c2FsdA 18\nYm9keQ\n--- bWFj\npayload",
        )
        .expect("write");

        let key = IdentityFile::read(&path).expect("reads");
        assert!(key.needs_passphrase());
        let err = key.unlock(None).expect_err("must ask for a passphrase");
        assert_eq!(err.kind(), ErrorKind::Usage);
    }

    #[test]
    fn rejects_a_file_that_is_not_a_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.txt");
        fs::write(&path, b"just some text\n").expect("write");
        let err = IdentityFile::read(&path).expect_err("must fail");
        assert_eq!(err.kind(), ErrorKind::Usage);
    }

    #[test]
    fn debug_output_hides_the_secret() {
        let credentials = Credentials::passphrase(SecretString::from("hunter2".to_string()));
        let text = format!("{credentials:?}");
        assert!(!text.contains("hunter2"));
    }
}
