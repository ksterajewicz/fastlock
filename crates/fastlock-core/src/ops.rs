//! The operations: verify and decrypt.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::checksums::{self, HashWriter};
use crate::error::{Error, ErrorKind, Result};
use crate::extract::{self, Staging};
use crate::inspect::{inspect, Format, Inspection};
use crate::key::Credentials;
use crate::pipeline::{self, NullSink, Walk};
use crate::progress::{Control, Stage};

/// One thing that was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckId {
    /// The checksum file next to the encrypted file.
    ChecksumFile,
    /// Decryption and authentication of the whole file.
    Decryption,
    /// Decompression.
    Compression,
    /// The archive inside.
    Archive,
    /// The checksum lists inside the archive.
    InnerChecksums,
}

impl CheckId {
    /// A short title for people.
    pub fn title(self) -> &'static str {
        match self {
            CheckId::ChecksumFile => "checksum file",
            CheckId::Decryption => "decryption",
            CheckId::Compression => "compression",
            CheckId::Archive => "archive",
            CheckId::InnerChecksums => "checksums inside",
        }
    }
}

/// How a check ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// The check passed.
    Passed,
    /// The check failed.
    Failed,
    /// There was nothing to check.
    Skipped,
}

/// The result of one check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// What was checked.
    pub id: CheckId,
    /// How it ended.
    pub status: CheckStatus,
    /// One line for people that says what was found.
    pub detail: String,
}

impl Check {
    fn new(id: CheckId, status: CheckStatus, detail: impl Into<String>) -> Self {
        Check {
            id,
            status,
            detail: detail.into(),
        }
    }
}

/// How the decrypted data was compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    /// Not compressed.
    None,
    /// Zstandard.
    Zstd,
}

/// The kind of an entry inside an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A folder.
    Directory,
    /// A link or a special file. These are listed but never written.
    Other,
}

/// One entry inside an archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The path inside the archive, with `/` between its parts.
    pub path: String,
    /// The size in bytes. Zero for anything that is not a file.
    pub size: u64,
    /// The kind of entry.
    pub kind: EntryKind,
    /// The SHA-256 checksum of a file, as lowercase hexadecimal digits.
    pub sha256: Option<String>,
}

/// The result of [`verify`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerifyReport {
    /// Whether every check that could run has passed.
    pub ok: bool,
    /// The checks, in the order they ran.
    pub checks: Vec<Check>,
    /// How the data was compressed.
    pub compression: Compression,
    /// Whether the data is an archive.
    pub archive: bool,
    /// The entries of the archive. Empty when the data is not an archive.
    pub entries: Vec<Entry>,
    /// The number of decrypted bytes.
    pub plaintext_size: u64,
}

/// The result of [`decrypt`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecryptReport {
    /// Whether every check that could run has passed.
    pub ok: bool,
    /// Where the result was written: a folder for an archive, a file
    /// otherwise.
    pub output: PathBuf,
    /// The checks, in the order they ran.
    pub checks: Vec<Check>,
    /// How the data was compressed.
    pub compression: Compression,
    /// Whether the data was an archive.
    pub archive: bool,
    /// The entries of the archive. Empty when the data was not an archive.
    pub entries: Vec<Entry>,
    /// Entries that were not written, each with the reason.
    pub skipped: Vec<String>,
    /// The number of decrypted bytes.
    pub plaintext_size: u64,
}

/// Decrypts a file in memory and checks it. Writes nothing.
///
/// Returns a report when the whole file was read and authenticated. The
/// report says `ok: false` when a checksum does not match. A wrong key, a
/// damaged file and a cancelled run are errors.
pub fn verify(
    path: &Path,
    credentials: &Credentials,
    control: Control<'_>,
) -> Result<VerifyReport> {
    let inspection = require_supported(path)?;
    let mut checks = vec![check_checksum_file(&inspection, control)?];

    control.progress.stage(Stage::Decrypt);
    let plain = pipeline::open(path, &inspection, credentials)?;
    let walk = plain.walk(inspection.expected_plaintext_size(), control, &mut NullSink)?;

    control.progress.stage(Stage::Finish);
    checks.extend(content_checks(&walk));
    let ok = checks.iter().all(|c| c.status != CheckStatus::Failed);
    Ok(VerifyReport {
        ok,
        checks,
        compression: walk.compression,
        archive: walk.archive,
        entries: walk.entries,
        plaintext_size: walk.plaintext_size,
    })
}

/// Decrypts a file to disk, and unpacks it when it is an archive.
///
/// An archive is written as a folder at `output`. Other data is written as a
/// file at `output`. Nothing appears at `output` unless the whole file was
/// authenticated and the checksums inside the archive match.
///
/// `output` must not exist, unless `force` is given and it is a file. An
/// existing folder is never replaced.
pub fn decrypt(
    path: &Path,
    credentials: &Credentials,
    output: &Path,
    force: bool,
    control: Control<'_>,
) -> Result<DecryptReport> {
    let inspection = require_supported(path)?;
    refuse_existing(output, force)?;
    let mut checks = vec![check_checksum_file(&inspection, control)?];

    control.progress.stage(Stage::Decrypt);
    let plain = pipeline::open(path, &inspection, credentials)?;
    let mut staging = Staging::create(output)?;
    let walk = plain.walk(inspection.expected_plaintext_size(), control, &mut staging)?;

    control.progress.stage(Stage::Finish);
    let content = content_checks(&walk);
    if let Some(failed) = content
        .iter()
        .find(|c| c.id == CheckId::InnerChecksums && c.status == CheckStatus::Failed)
    {
        // The staging folder is removed when it goes out of scope.
        return Err(Error::new(
            ErrorKind::Damaged,
            format!("nothing was written: {}", failed.detail),
        ));
    }
    checks.extend(content);

    refuse_existing(output, force)?;
    if walk.archive {
        fs::rename(staging.root(), output).map_err(|e| Error::io("create", output, e))?;
        staging.keep();
    } else {
        fs::rename(staging.single_file(), output).map_err(|e| Error::io("create", output, e))?;
    }
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            extract::sync_dir(parent);
        }
    }

    let skipped = std::mem::take(&mut staging.skipped);
    let ok = checks.iter().all(|c| c.status != CheckStatus::Failed);
    Ok(DecryptReport {
        ok,
        output: output.to_path_buf(),
        checks,
        compression: walk.compression,
        archive: walk.archive,
        entries: walk.entries,
        skipped,
        plaintext_size: walk.plaintext_size,
    })
}

/// Suggests where to write the result of decrypting `path`: the same place,
/// with the endings `.age`, `.zst` and `.tar` removed from the name.
pub fn default_output(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let mut stem = name.as_str();
    let mut stripped = false;
    for ending in [".age", ".zst", ".tar"] {
        if let Some(rest) = strip_suffix_ignore_case(stem, ending) {
            if !rest.is_empty() {
                stem = rest;
                stripped = true;
            }
        } else if ending == ".age" {
            // Not an .age name: only add a marker, do not guess further.
            break;
        }
    }
    let result = if stripped {
        stem.to_string()
    } else {
        format!("{name}.decrypted")
    };
    path.with_file_name(result)
}

fn strip_suffix_ignore_case<'a>(text: &'a str, ending: &str) -> Option<&'a str> {
    let split = text.len().checked_sub(ending.len())?;
    let (head, tail) = text.split_at_checked(split)?;
    tail.eq_ignore_ascii_case(ending).then_some(head)
}

fn require_supported(path: &Path) -> Result<Inspection> {
    let inspection = inspect(path)?;
    if inspection.format != Format::Age {
        return Err(Error::new(
            ErrorKind::Unsupported,
            format!("{} is not a file fastlock can open", path.display()),
        ));
    }
    Ok(inspection)
}

fn refuse_existing(output: &Path, force: bool) -> Result<()> {
    match fs::symlink_metadata(output) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Error::io("check", output, e)),
        Ok(metadata) if metadata.is_dir() => Err(Error::new(
            ErrorKind::Refused,
            format!(
                "{} is an existing folder. Remove it or choose another output.",
                output.display()
            ),
        )),
        Ok(_) if force => Ok(()),
        Ok(_) => Err(Error::new(
            ErrorKind::Refused,
            format!(
                "{} already exists. Use --force to replace it.",
                output.display()
            ),
        )),
    }
}

/// Compares the encrypted file with the checksum file next to it.
fn check_checksum_file(inspection: &Inspection, control: Control<'_>) -> Result<Check> {
    let Some(list_path) = inspection.checksum_file.as_deref() else {
        return Ok(Check::new(
            CheckId::ChecksumFile,
            CheckStatus::Skipped,
            "no .sha256 file next to the file",
        ));
    };
    let file_name = inspection
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let list_name = list_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let text = fs::read(list_path).map_err(|e| Error::io("read", list_path, e))?;
    let Some(expected) = checksums::sidecar_checksum(&text, &file_name) else {
        return Ok(Check::new(
            CheckId::ChecksumFile,
            CheckStatus::Failed,
            format!("{list_name} does not hold a checksum for this file"),
        ));
    };

    control.progress.stage(Stage::Checksum);
    let actual = hash_file(&inspection.path, inspection.size, control)?;
    if actual == expected {
        Ok(Check::new(
            CheckId::ChecksumFile,
            CheckStatus::Passed,
            format!("matches {list_name}"),
        ))
    } else {
        Ok(Check::new(
            CheckId::ChecksumFile,
            CheckStatus::Failed,
            format!("does not match {list_name}"),
        ))
    }
}

fn hash_file(path: &Path, size: u64, control: Control<'_>) -> Result<String> {
    use std::io::Write;

    let mut file = File::open(path).map_err(|e| Error::io("open", path, e))?;
    let mut hasher = HashWriter::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        if control.cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(Error::io("read", path, e)),
        };
        let _ = hasher.write_all(&buf[..n]);
        done += n as u64;
        control.progress.advance(done, Some(size));
    }
    Ok(hasher.finish().0)
}

/// The checks that follow from reading the contents to their end.
fn content_checks(walk: &Walk) -> Vec<Check> {
    let mut checks = vec![Check::new(
        CheckId::Decryption,
        CheckStatus::Passed,
        format!(
            "authenticated to the last byte, {}",
            human_size(walk.plaintext_size)
        ),
    )];

    checks.push(match walk.compression {
        Compression::Zstd => Check::new(
            CheckId::Compression,
            CheckStatus::Passed,
            "Zstandard, decompressed without errors",
        ),
        Compression::None => {
            Check::new(CheckId::Compression, CheckStatus::Skipped, "not compressed")
        }
    });

    if walk.archive {
        let files = walk
            .entries
            .iter()
            .filter(|e| matches!(e.kind, crate::ops::EntryKind::File))
            .count();
        checks.push(Check::new(
            CheckId::Archive,
            CheckStatus::Passed,
            format!("{files} {} read to the end", plural(files, "file", "files")),
        ));
        checks.push(inner_checksums(walk));
    } else {
        checks.push(Check::new(
            CheckId::Archive,
            CheckStatus::Skipped,
            "not an archive",
        ));
        checks.push(Check::new(
            CheckId::InnerChecksums,
            CheckStatus::Skipped,
            "no checksum list inside",
        ));
    }
    checks
}

/// Compares the files of an archive with the checksum lists found inside it.
fn inner_checksums(walk: &Walk) -> Check {
    let mut compared = 0usize;
    let mut lists = 0usize;

    for (list_path, content) in &walk.lists {
        let Some(list) = checksums::parse_list(content) else {
            continue;
        };
        lists += 1;
        let folder = match list_path.rfind('/') {
            Some(index) => &list_path[..=index],
            None => "",
        };
        for listed in &list {
            let target = format!("{folder}{}", checksums::normalize_name(&listed.name));
            let Some(entry) = walk.entries.iter().find(|e| e.path == target) else {
                return Check::new(
                    CheckId::InnerChecksums,
                    CheckStatus::Failed,
                    format!("{list_path} lists {target}, which is not in the archive"),
                );
            };
            if entry.sha256.as_deref() != Some(listed.hex.as_str()) {
                return Check::new(
                    CheckId::InnerChecksums,
                    CheckStatus::Failed,
                    format!("{target} does not match its checksum in {list_path}"),
                );
            }
            compared += 1;
        }
    }

    if lists == 0 {
        Check::new(
            CheckId::InnerChecksums,
            CheckStatus::Skipped,
            "no checksum list inside",
        )
    } else {
        Check::new(
            CheckId::InnerChecksums,
            CheckStatus::Passed,
            format!(
                "{compared} {} match the list inside",
                plural(compared, "file", "files")
            ),
        )
    }
}

fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 {
        one
    } else {
        many
    }
}

/// Formats a size for people, in binary units: `8.4 MiB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn file(path: &str, sha256: &str) -> Entry {
        Entry {
            path: path.to_string(),
            size: 3,
            kind: EntryKind::File,
            sha256: Some(sha256.to_string()),
        }
    }

    fn walk_with(entries: Vec<Entry>, lists: Vec<(&str, String)>) -> Walk {
        Walk {
            compression: Compression::Zstd,
            archive: true,
            entries,
            lists: lists
                .into_iter()
                .map(|(path, text)| (path.to_string(), text.into_bytes()))
                .collect(),
            plaintext_size: 2048,
        }
    }

    #[test]
    fn suggests_an_output_name() {
        assert_eq!(
            default_output(Path::new("/tmp/backup.tar.zst.age")),
            PathBuf::from("/tmp/backup")
        );
        assert_eq!(
            default_output(Path::new("report.pdf.zst.age")),
            PathBuf::from("report.pdf")
        );
        assert_eq!(default_output(Path::new("note.AGE")), PathBuf::from("note"));
        assert_eq!(
            default_output(Path::new("archive.bin")),
            PathBuf::from("archive.bin.decrypted")
        );
        assert_eq!(
            default_output(Path::new(".age")),
            PathBuf::from(".age.decrypted")
        );
    }

    #[test]
    fn matching_inner_checksums_pass() {
        let walk = walk_with(
            vec![file("a.dump", ABC), file("CHECKSUMS.sha256", ABC)],
            vec![("CHECKSUMS.sha256", format!("{ABC}  a.dump\n"))],
        );
        let check = inner_checksums(&walk);
        assert_eq!(check.status, CheckStatus::Passed);
        assert!(check.detail.starts_with("1 file "));
    }

    #[test]
    fn a_mismatch_inside_fails() {
        let other = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let walk = walk_with(
            vec![file("a.dump", ABC)],
            vec![("CHECKSUMS.sha256", format!("{other}  a.dump\n"))],
        );
        assert_eq!(inner_checksums(&walk).status, CheckStatus::Failed);
    }

    #[test]
    fn a_listed_file_that_is_missing_fails() {
        let walk = walk_with(
            vec![file("a.dump", ABC)],
            vec![("CHECKSUMS.sha256", format!("{ABC}  gone.db\n"))],
        );
        let check = inner_checksums(&walk);
        assert_eq!(check.status, CheckStatus::Failed);
        assert!(check.detail.contains("gone.db"));
    }

    #[test]
    fn a_list_in_a_folder_is_relative_to_that_folder() {
        let walk = walk_with(
            vec![file("data/a.dump", ABC)],
            vec![("data/SHA256SUMS", format!("{ABC}  ./a.dump\n"))],
        );
        assert_eq!(inner_checksums(&walk).status, CheckStatus::Passed);
    }

    #[test]
    fn no_list_inside_is_skipped() {
        let walk = walk_with(vec![file("a.dump", ABC)], vec![]);
        assert_eq!(inner_checksums(&walk).status, CheckStatus::Skipped);
    }

    #[test]
    fn content_checks_cover_every_layer() {
        let walk = walk_with(vec![file("a.dump", ABC)], vec![]);
        let checks = content_checks(&walk);
        let ids: Vec<CheckId> = checks.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            vec![
                CheckId::Decryption,
                CheckId::Compression,
                CheckId::Archive,
                CheckId::InnerChecksums
            ]
        );
        assert_eq!(checks[0].detail, "authenticated to the last byte, 2.0 KiB");
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(8_808_038), "8.4 MiB");
        assert_eq!(human_size(200 * 1024 * 1024), "200 MiB");
    }

    #[test]
    fn refuses_an_existing_output() {
        let dir = tempfile::tempdir().expect("temp dir");
        let existing = dir.path().join("out.txt");
        std::fs::write(&existing, b"x").expect("write");

        assert_eq!(
            refuse_existing(&existing, false)
                .expect_err("must refuse")
                .kind(),
            ErrorKind::Refused
        );
        assert!(refuse_existing(&existing, true).is_ok());
        assert_eq!(
            refuse_existing(dir.path(), true)
                .expect_err("folder")
                .kind(),
            ErrorKind::Refused
        );
        assert!(refuse_existing(&dir.path().join("new"), false).is_ok());
    }
}
