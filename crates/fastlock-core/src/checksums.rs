//! SHA-256 checksums: computing them, and reading checksum lists.
//!
//! A checksum list is a text file in the format written by `sha256sum`: one
//! line per file, made of 64 hexadecimal digits, a space, a second space or a
//! `*`, and the file name.

use std::io::{self, Write};

use sha2::{Digest, Sha256};

/// One line of a checksum list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListedChecksum {
    /// The checksum as lowercase hexadecimal digits.
    pub hex: String,
    /// The file name as written in the list.
    pub name: String,
}

/// A writer that hashes everything written to it.
pub(crate) struct HashWriter {
    hasher: Sha256,
    written: u64,
}

impl HashWriter {
    pub(crate) fn new() -> Self {
        HashWriter {
            hasher: Sha256::new(),
            written: 0,
        }
    }

    /// Finishes and returns the checksum as lowercase hexadecimal digits, and
    /// the number of bytes hashed.
    pub(crate) fn finish(self) -> (String, u64) {
        let digest = self.hasher.finalize();
        (to_hex(digest.as_ref()), self.written)
    }
}

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.hasher.update(buf);
        self.written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Formats bytes as lowercase hexadecimal digits.
pub(crate) fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Whether a file name looks like a checksum list.
pub(crate) fn is_list_name(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name);
    let lower = base.to_ascii_lowercase();
    lower.ends_with(".sha256") || lower == "sha256sums" || lower == "sha256sums.txt"
}

/// Parses a checksum list.
///
/// Returns `None` when the text is not a checksum list: when a line that is
/// not empty does not have the expected shape, or when there is no line at
/// all.
pub(crate) fn parse_list(text: &[u8]) -> Option<Vec<ListedChecksum>> {
    let text = std::str::from_utf8(text).ok()?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() {
            continue;
        }
        let (hex, rest) = line.split_at_checked(64)?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let name = rest
            .strip_prefix("  ")
            .or_else(|| rest.strip_prefix(" *"))?;
        if name.is_empty() {
            return None;
        }
        out.push(ListedChecksum {
            hex: hex.to_ascii_lowercase(),
            name: name.to_string(),
        });
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Reads the checksum of one file from a checksum file that sits next to it.
///
/// Accepts a checksum list that names the file, a list with a single line, or
/// a file that holds only the 64 digits.
pub(crate) fn sidecar_checksum(text: &[u8], file_name: &str) -> Option<String> {
    if let Some(list) = parse_list(text) {
        if let Some(entry) = list
            .iter()
            .find(|entry| base_name(&entry.name) == file_name)
        {
            return Some(entry.hex.clone());
        }
        if list.len() == 1 {
            return Some(list[0].hex.clone());
        }
        return None;
    }
    let text = std::str::from_utf8(text).ok()?.trim();
    (text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| text.to_ascii_lowercase())
}

fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// Removes a leading `./` from a path inside an archive or a checksum list.
pub(crate) fn normalize_name(name: &str) -> &str {
    let mut name = name;
    while let Some(rest) = name.strip_prefix("./") {
        name = rest;
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn hashes_known_inputs() {
        let (hex, written) = HashWriter::new().finish();
        assert_eq!(hex, EMPTY_SHA256);
        assert_eq!(written, 0);

        let mut writer = HashWriter::new();
        writer.write_all(b"a").expect("write");
        writer.write_all(b"bc").expect("write");
        let (hex, written) = writer.finish();
        assert_eq!(hex, ABC_SHA256);
        assert_eq!(written, 3);
    }

    #[test]
    fn parses_a_checksum_list() {
        let text = format!("{ABC_SHA256}  first.dump\n{EMPTY_SHA256} *second.db\n\n");
        let list = parse_list(text.as_bytes()).expect("is a list");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "first.dump");
        assert_eq!(list[0].hex, ABC_SHA256);
        assert_eq!(list[1].name, "second.db");
    }

    #[test]
    fn text_that_is_not_a_list_is_rejected() {
        assert!(parse_list(b"hello world\n").is_none());
        assert!(parse_list(b"").is_none());
        let short = format!("{}  name\n", &ABC_SHA256[..60]);
        assert!(parse_list(short.as_bytes()).is_none());
        let one_space = format!("{ABC_SHA256} name\n");
        assert!(parse_list(one_space.as_bytes()).is_none());
    }

    #[test]
    fn reads_a_checksum_file_that_sits_next_to_a_file() {
        let listed = format!("{ABC_SHA256}  backup.tar.zst.age\n");
        assert_eq!(
            sidecar_checksum(listed.as_bytes(), "backup.tar.zst.age").as_deref(),
            Some(ABC_SHA256)
        );
        let with_path = format!("{ABC_SHA256}  /some/dir/backup.tar.zst.age\n");
        assert_eq!(
            sidecar_checksum(with_path.as_bytes(), "backup.tar.zst.age").as_deref(),
            Some(ABC_SHA256)
        );
        let bare = format!("{}\n", ABC_SHA256.to_uppercase());
        assert_eq!(
            sidecar_checksum(bare.as_bytes(), "anything").as_deref(),
            Some(ABC_SHA256)
        );
        let two_others = format!("{ABC_SHA256}  a\n{EMPTY_SHA256}  b\n");
        assert_eq!(sidecar_checksum(two_others.as_bytes(), "c"), None);
    }

    #[test]
    fn recognizes_checksum_list_names() {
        assert!(is_list_name("CHECKSUMS.sha256"));
        assert!(is_list_name("dir/SHA256SUMS"));
        assert!(is_list_name("sha256sums.txt"));
        assert!(!is_list_name("notes.txt"));
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize_name("./a/b"), "a/b");
        assert_eq!(normalize_name("././a"), "a");
        assert_eq!(normalize_name("a"), "a");
    }
}
