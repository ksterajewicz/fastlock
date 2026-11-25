//! Describing a file without a key.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::age_header::{self, HeaderError, MAX_HEADER};
use crate::error::{Error, ErrorKind, Result};

/// The format of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// An age file.
    Age,
    /// Not a format this version recognizes.
    Unknown,
}

impl Format {
    /// A short name for people.
    pub fn label(self) -> &'static str {
        match self {
            Format::Age => "age v1",
            Format::Unknown => "not recognized",
        }
    }
}

/// The kind of a recipient: who, or what, can open the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipientKind {
    /// A classic age key.
    X25519,
    /// A post-quantum age key.
    PostQuantum,
    /// A passphrase.
    Passphrase,
    /// An Ed25519 SSH key.
    SshEd25519,
    /// An RSA SSH key.
    SshRsa,
    /// A hardware key.
    Hardware,
    /// A post-quantum hardware key.
    HardwarePostQuantum,
    /// A recipient type this version does not know.
    Other,
}

impl RecipientKind {
    fn from_tag(tag: &str) -> Self {
        match tag {
            "X25519" => RecipientKind::X25519,
            "mlkem768x25519" => RecipientKind::PostQuantum,
            "scrypt" => RecipientKind::Passphrase,
            "ssh-ed25519" => RecipientKind::SshEd25519,
            "ssh-rsa" => RecipientKind::SshRsa,
            "p256tag" | "piv-p256" => RecipientKind::Hardware,
            "mlkem768p256tag" => RecipientKind::HardwarePostQuantum,
            _ => RecipientKind::Other,
        }
    }

    /// A short name for people.
    pub fn label(self) -> &'static str {
        match self {
            RecipientKind::X25519 => "age key",
            RecipientKind::PostQuantum => "post-quantum age key",
            RecipientKind::Passphrase => "passphrase",
            RecipientKind::SshEd25519 => "SSH key (ed25519)",
            RecipientKind::SshRsa => "SSH key (rsa)",
            RecipientKind::Hardware => "hardware key",
            RecipientKind::HardwarePostQuantum => "post-quantum hardware key",
            RecipientKind::Other => "unknown recipient type",
        }
    }

    /// Whether this kind of recipient is designed to withstand a quantum
    /// computer.
    pub fn is_post_quantum(self) -> bool {
        matches!(
            self,
            RecipientKind::PostQuantum | RecipientKind::HardwarePostQuantum
        )
    }
}

/// One recipient of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recipient {
    /// The kind of recipient.
    pub kind: RecipientKind,
    /// The recipient type as written in the file.
    pub stanza: String,
}

/// What a person must provide to open a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// A passphrase.
    Passphrase,
    /// A key.
    Key,
    /// Not known, because the file was not recognized.
    Unknown,
}

/// What can be said about a file without a key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inspection {
    /// The file that was inspected.
    pub path: PathBuf,
    /// Its format.
    pub format: Format,
    /// Its size in bytes.
    pub size: u64,
    /// Whether it is stored as text (armor) instead of binary data.
    pub armored: bool,
    /// Who can open it. Empty when the format is not recognized.
    pub recipients: Vec<Recipient>,
    /// Whether every recipient is post-quantum.
    pub post_quantum: bool,
    /// What is needed to open it.
    pub needs: Needs,
    /// The size of the header in bytes, for a binary age file.
    pub header_size: Option<u64>,
    /// The size of the encrypted payload in bytes, for a binary age file.
    pub payload_size: Option<u64>,
    /// The checksum file next to it (`<name>.sha256`), when there is one.
    pub checksum_file: Option<PathBuf>,
}

impl Inspection {
    /// The size the decrypted data is expected to have, when it can be worked
    /// out from the payload size. Used for progress only.
    pub(crate) fn expected_plaintext_size(&self) -> Option<u64> {
        const NONCE: u64 = 16;
        const CHUNK: u64 = 64 * 1024;
        const TAG: u64 = 16;
        let payload = self.payload_size?.checked_sub(NONCE)?;
        if payload == 0 {
            return Some(0);
        }
        let chunks = payload.div_ceil(CHUNK + TAG);
        payload.checked_sub(chunks * TAG)
    }
}

/// Describes a file without needing a key.
///
/// A file that is not recognized is not an error: the result then has
/// [`Format::Unknown`]. An error means the file could not be read, or that it
/// starts like an age file but its header is broken.
pub fn inspect(path: &Path) -> Result<Inspection> {
    let mut file = File::open(path).map_err(|e| Error::io("open", path, e))?;
    let metadata = file.metadata().map_err(|e| Error::io("read", path, e))?;
    if metadata.is_dir() {
        return Err(Error::new(
            ErrorKind::Usage,
            format!("{} is a folder, not a file", path.display()),
        ));
    }
    let size = metadata.len();

    // Armored data is about a third larger than what it encodes.
    let mut prefix = Vec::new();
    file.by_ref()
        .take((MAX_HEADER * 2) as u64)
        .read_to_end(&mut prefix)
        .map_err(|e| Error::io("read", path, e))?;

    let checksum_file = checksum_file_for(path);
    let unknown = |armored: bool| Inspection {
        path: path.to_path_buf(),
        format: Format::Unknown,
        size,
        armored,
        recipients: Vec::new(),
        post_quantum: false,
        needs: Needs::Unknown,
        header_size: None,
        payload_size: None,
        checksum_file: checksum_file.clone(),
    };

    let armored = age_header::is_armored(&prefix);
    let parsed = if armored {
        let Some(binary) = age_header::decode_armor_prefix(&prefix, MAX_HEADER) else {
            return Err(damaged_header(path));
        };
        age_header::parse(&binary)
    } else if age_header::is_binary_age(&prefix) {
        let end = prefix.len().min(MAX_HEADER);
        age_header::parse(&prefix[..end])
    } else {
        return Ok(unknown(false));
    };

    let header = match parsed {
        Ok(header) => header,
        Err(HeaderError::NotAge) => return Ok(unknown(armored)),
        Err(HeaderError::Incomplete) | Err(HeaderError::Malformed) => {
            return Err(damaged_header(path));
        }
    };

    let recipients: Vec<Recipient> = header
        .stanzas
        .iter()
        .map(|stanza| Recipient {
            kind: RecipientKind::from_tag(&stanza.tag),
            stanza: stanza.tag.clone(),
        })
        .collect();
    let post_quantum = recipients.iter().all(|r| r.kind.is_post_quantum());
    let needs = if recipients
        .iter()
        .any(|r| r.kind == RecipientKind::Passphrase)
    {
        Needs::Passphrase
    } else {
        Needs::Key
    };
    let (header_size, payload_size) = if armored {
        (None, None)
    } else {
        (
            Some(header.header_len),
            Some(size.saturating_sub(header.header_len)),
        )
    };

    Ok(Inspection {
        path: path.to_path_buf(),
        format: Format::Age,
        size,
        armored,
        recipients,
        post_quantum,
        needs,
        header_size,
        payload_size,
        checksum_file,
    })
}

fn damaged_header(path: &Path) -> Error {
    Error::new(
        ErrorKind::Damaged,
        format!(
            "{} starts like an age file, but its header is damaged or incomplete",
            path.display()
        ),
    )
}

/// The checksum file that belongs to `path`: the same name with `.sha256`
/// added. Returned only when it exists.
pub(crate) fn checksum_file_for(path: &Path) -> Option<PathBuf> {
    let mut name = path.file_name()?.to_os_string();
    name.push(".sha256");
    let candidate = path.with_file_name(name);
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(dir: &Path, name: &str, data: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, data).expect("write test file");
        path
    }

    #[test]
    fn describes_a_binary_age_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut data =
            b"age-encryption.org/v1\n-> mlkem768x25519 QUJD\nYm9keQ\n--- bWFj\n".to_vec();
        let header_len = data.len() as u64;
        data.extend_from_slice(&[0u8; 100]);
        let path = write(dir.path(), "sample.tar.zst.age", &data);

        let info = inspect(&path).expect("inspect works");
        assert_eq!(info.format, Format::Age);
        assert!(!info.armored);
        assert_eq!(info.recipients.len(), 1);
        assert_eq!(info.recipients[0].kind, RecipientKind::PostQuantum);
        assert!(info.post_quantum);
        assert_eq!(info.needs, Needs::Key);
        assert_eq!(info.header_size, Some(header_len));
        assert_eq!(info.payload_size, Some(100));
        assert_eq!(info.checksum_file, None);
    }

    #[test]
    fn a_passphrase_file_needs_a_passphrase() {
        let dir = tempfile::tempdir().expect("temp dir");
        let data = b"age-encryption.org/v1\n-> scrypt c2FsdA 18\nYm9keQ\n--- bWFj\npayload";
        let path = write(dir.path(), "note.age", data);
        let info = inspect(&path).expect("inspect works");
        assert_eq!(info.needs, Needs::Passphrase);
        assert!(!info.post_quantum);
    }

    #[test]
    fn an_unrecognized_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write(dir.path(), "photo.jpg", b"\xff\xd8\xff\xe0 not age");
        let info = inspect(&path).expect("inspect works");
        assert_eq!(info.format, Format::Unknown);
        assert_eq!(info.needs, Needs::Unknown);
        assert!(info.recipients.is_empty());
    }

    #[test]
    fn a_cut_header_is_reported_as_damaged() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write(
            dir.path(),
            "cut.age",
            b"age-encryption.org/v1\n-> X25519 QUJD\n",
        );
        let err = inspect(&path).expect_err("must fail");
        assert_eq!(err.kind(), ErrorKind::Damaged);
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let err = inspect(Path::new("/nonexistent/fastlock-test.age")).expect_err("must fail");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn finds_the_checksum_file_next_to_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write(
            dir.path(),
            "a.age",
            b"age-encryption.org/v1\n-> X25519 QQ\n--- bQ\n",
        );
        write(dir.path(), "a.age.sha256", b"00  a.age\n");
        let info = inspect(&path).expect("inspect works");
        assert_eq!(info.checksum_file, Some(dir.path().join("a.age.sha256")));
    }

    #[test]
    fn expected_plaintext_size_accounts_for_nonce_and_tags() {
        let mut info = Inspection {
            path: PathBuf::new(),
            format: Format::Age,
            size: 0,
            armored: false,
            recipients: Vec::new(),
            post_quantum: false,
            needs: Needs::Key,
            header_size: Some(0),
            payload_size: Some(16 + 10 + 16),
            checksum_file: None,
        };
        assert_eq!(info.expected_plaintext_size(), Some(10));
        // Two chunks: one full, one of a single byte.
        info.payload_size = Some(16 + (65536 + 16) + (1 + 16));
        assert_eq!(info.expected_plaintext_size(), Some(65537));
        info.payload_size = None;
        assert_eq!(info.expected_plaintext_size(), None);
    }
}
