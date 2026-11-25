//! End-to-end tests against the official `age` program.
//!
//! These tests build an encrypted archive the way a backup script would:
//! tar, then Zstandard, then `age` with a post-quantum key. They then check
//! that fastlock reads it. They are skipped when `age` and `age-keygen` are
//! not installed, or when the installed `age` has no post-quantum support.
//!
//! All keys and data are invented and exist only for the duration of a test.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fastlock_core::{
    decrypt, inspect, verify, CancelToken, CheckId, CheckStatus, Compression, Control, Credentials,
    ErrorKind, Format, IdentityFile, Needs, NoProgress, SecretString,
};

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// A post-quantum key pair made with `age-keygen -pq`.
struct TestKey {
    identity_file: PathBuf,
    public: String,
}

fn tool(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Creates a post-quantum key, or returns `None` when the tools are missing.
fn new_key(dir: &Path, name: &str) -> Option<TestKey> {
    let keygen = tool("age-keygen")?;
    tool("age")?;
    let identity_file = dir.join(name);
    let made = Command::new(&keygen)
        .arg("-pq")
        .arg("-o")
        .arg(&identity_file)
        .stderr(Stdio::null())
        .status()
        .ok()?;
    if !made.success() {
        // An age without post-quantum support.
        return None;
    }
    let public = Command::new(&keygen)
        .arg("-y")
        .arg(&identity_file)
        .output()
        .ok()?;
    let public = String::from_utf8(public.stdout).ok()?.trim().to_string();
    public.starts_with("age1pq1").then_some(TestKey {
        identity_file,
        public,
    })
}

fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, content) in files {
        let mut header = tar::Header::new_ustar();
        header.set_size(content.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *content)
            .expect("append to tar");
    }
    builder.into_inner().expect("finish tar")
}

/// Encrypts `plain` to `public` with the `age` program and writes `target`.
fn age_encrypt(plain: &[u8], public: &str, target: &Path) {
    let age = tool("age").expect("age is installed");
    let mut child = Command::new(age)
        .arg("-r")
        .arg(public)
        .arg("-o")
        .arg(target)
        .stdin(Stdio::piped())
        .spawn()
        .expect("start age");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(plain)
        .expect("write plaintext");
    assert!(child.wait().expect("wait for age").success());
}

/// A sample archive: two files and a checksum list for the first one.
fn sample_archive() -> Vec<u8> {
    let list = format!("{ABC_SHA256}  ./sample.dump\n");
    let tar = tar_of(&[
        ("./sample.dump", b"abc"),
        ("./sample.db", &[42u8; 200_000]),
        ("./CHECKSUMS.sha256", list.as_bytes()),
    ]);
    zstd::stream::encode_all(Cursor::new(tar), 3).expect("compress")
}

fn write_checksum_file(file: &Path) {
    let sha256sum = tool("sha256sum").expect("sha256sum is installed");
    let output = Command::new(sha256sum)
        .arg(file.file_name().expect("file name"))
        .current_dir(file.parent().expect("parent"))
        .output()
        .expect("run sha256sum");
    let mut name = file.file_name().expect("file name").to_os_string();
    name.push(".sha256");
    fs::write(file.with_file_name(name), output.stdout).expect("write checksum file");
}

fn credentials_for(key: &TestKey) -> Credentials {
    let identity = IdentityFile::read(&key.identity_file).expect("read key");
    assert!(!identity.needs_passphrase());
    identity.unlock(None).expect("unlock key")
}

macro_rules! key_or_skip {
    ($dir:expr, $name:expr) => {
        match new_key($dir, $name) {
            Some(key) => key,
            None => {
                eprintln!("skipped: age with post-quantum support is not installed");
                return;
            }
        }
    };
}

#[test]
fn inspects_and_verifies_a_post_quantum_archive() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = key_or_skip!(dir.path(), "key.txt");
    let file = dir.path().join("sample.tar.zst.age");
    age_encrypt(&sample_archive(), &key.public, &file);
    write_checksum_file(&file);

    let info = inspect(&file).expect("inspect");
    assert_eq!(info.format, Format::Age);
    assert!(info.post_quantum);
    assert_eq!(info.needs, Needs::Key);
    assert!(info.checksum_file.is_some());

    let progress = NoProgress;
    let cancel = CancelToken::new();
    let report = verify(
        &file,
        &credentials_for(&key),
        Control::new(&progress, &cancel),
    )
    .expect("verify");
    assert!(report.ok, "checks: {:?}", report.checks);
    assert_eq!(report.compression, Compression::Zstd);
    assert!(report.archive);
    assert_eq!(report.entries.len(), 3);
    for id in [
        CheckId::ChecksumFile,
        CheckId::Decryption,
        CheckId::Compression,
        CheckId::Archive,
        CheckId::InnerChecksums,
    ] {
        let check = report
            .checks
            .iter()
            .find(|c| c.id == id)
            .expect("check ran");
        assert_eq!(
            check.status,
            CheckStatus::Passed,
            "{id:?}: {}",
            check.detail
        );
    }

    // Nothing was written next to the file.
    let names: Vec<String> = fs::read_dir(dir.path())
        .expect("list")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 3, "unexpected files: {names:?}");
}

#[test]
fn a_changed_file_fails_and_a_wrong_key_is_told_apart() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = key_or_skip!(dir.path(), "key.txt");
    let other = key_or_skip!(dir.path(), "other.txt");
    let file = dir.path().join("sample.tar.zst.age");
    age_encrypt(&sample_archive(), &key.public, &file);

    let progress = NoProgress;
    let cancel = CancelToken::new();
    let control = Control::new(&progress, &cancel);

    let err = verify(&file, &credentials_for(&other), control).expect_err("wrong key");
    assert_eq!(err.kind(), ErrorKind::WrongKey);

    let mut bytes = fs::read(&file).expect("read");
    let last = bytes.len() - 20;
    bytes[last] ^= 0x01;
    let changed = dir.path().join("changed.tar.zst.age");
    fs::write(&changed, bytes).expect("write");
    let err = verify(&changed, &credentials_for(&key), control).expect_err("changed file");
    assert_eq!(err.kind(), ErrorKind::Damaged);
}

#[test]
fn a_checksum_file_that_does_not_match_fails_the_report() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = key_or_skip!(dir.path(), "key.txt");
    let file = dir.path().join("sample.tar.zst.age");
    age_encrypt(&sample_archive(), &key.public, &file);
    fs::write(
        dir.path().join("sample.tar.zst.age.sha256"),
        format!("{ABC_SHA256}  sample.tar.zst.age\n"),
    )
    .expect("write checksum file");

    let progress = NoProgress;
    let cancel = CancelToken::new();
    let report = verify(
        &file,
        &credentials_for(&key),
        Control::new(&progress, &cancel),
    )
    .expect("verify still reads the file");
    assert!(!report.ok);
    let check = report
        .checks
        .iter()
        .find(|c| c.id == CheckId::ChecksumFile)
        .expect("check ran");
    assert_eq!(check.status, CheckStatus::Failed);
}

#[test]
fn decrypts_an_archive_into_a_folder_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = key_or_skip!(dir.path(), "key.txt");
    let file = dir.path().join("sample.tar.zst.age");
    age_encrypt(&sample_archive(), &key.public, &file);
    let output = dir.path().join("restored");

    let progress = NoProgress;
    let cancel = CancelToken::new();
    let control = Control::new(&progress, &cancel);
    let report = decrypt(&file, &credentials_for(&key), &output, false, control).expect("decrypt");
    assert!(report.ok);
    assert_eq!(report.output, output);
    assert_eq!(fs::read(output.join("sample.dump")).expect("read"), b"abc");
    assert_eq!(
        fs::read(output.join("sample.db")).expect("read").len(),
        200_000
    );

    let err = decrypt(&file, &credentials_for(&key), &output, true, control).expect_err("exists");
    assert_eq!(err.kind(), ErrorKind::Refused);

    // No temporary folder is left behind.
    let leftovers: Vec<String> = fs::read_dir(dir.path())
        .expect("list")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("fastlock-partial"))
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[test]
fn a_damaged_archive_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = key_or_skip!(dir.path(), "key.txt");
    let file = dir.path().join("sample.tar.zst.age");
    age_encrypt(&sample_archive(), &key.public, &file);
    let mut bytes = fs::read(&file).expect("read");
    bytes.truncate(bytes.len() - 100);
    fs::write(&file, bytes).expect("write");
    let output = dir.path().join("restored");

    let progress = NoProgress;
    let cancel = CancelToken::new();
    let err = decrypt(
        &file,
        &credentials_for(&key),
        &output,
        false,
        Control::new(&progress, &cancel),
    )
    .expect_err("damaged");
    assert_eq!(err.kind(), ErrorKind::Damaged);
    assert!(!output.exists());
    let leftovers = fs::read_dir(dir.path())
        .expect("list")
        .filter(|e| {
            e.as_ref()
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .contains("fastlock-partial")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn opens_a_key_file_that_is_protected_by_a_passphrase() {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = key_or_skip!(dir.path(), "key.txt");
    let file = dir.path().join("sample.tar.zst.age");
    age_encrypt(&sample_archive(), &key.public, &file);

    // Protect the key file with a passphrase, in the age format.
    let passphrase = SecretString::from("correct horse battery staple".to_string());
    let plain_key = fs::read(&key.identity_file).expect("read key");
    let protected = dir.path().join("key.age");
    {
        let encryptor = age::Encryptor::with_user_passphrase(passphrase.clone());
        let mut encrypted = Vec::new();
        let mut writer = encryptor.wrap_output(&mut encrypted).expect("wrap");
        writer.write_all(&plain_key).expect("write key");
        writer.finish().expect("finish");
        fs::write(&protected, encrypted).expect("write protected key");
    }

    let identity = IdentityFile::read(&protected).expect("read protected key");
    assert!(identity.needs_passphrase());

    let wrong = SecretString::from("not the passphrase".to_string());
    let err = identity.unlock(Some(&wrong)).expect_err("wrong passphrase");
    assert_eq!(err.kind(), ErrorKind::WrongKey);

    let credentials = identity.unlock(Some(&passphrase)).expect("unlock");
    let progress = NoProgress;
    let cancel = CancelToken::new();
    let report = verify(&file, &credentials, Control::new(&progress, &cancel)).expect("verify");
    assert!(report.ok);
}

#[test]
fn verifies_and_decrypts_a_passphrase_protected_file_without_age() {
    let dir = tempfile::tempdir().expect("temp dir");
    let passphrase = SecretString::from("correct horse battery staple".to_string());
    let file = dir.path().join("note.txt.age");
    {
        let encryptor = age::Encryptor::with_user_passphrase(passphrase.clone());
        let mut encrypted = Vec::new();
        let mut writer = encryptor.wrap_output(&mut encrypted).expect("wrap");
        writer.write_all(b"a short note").expect("write");
        writer.finish().expect("finish");
        fs::write(&file, encrypted).expect("write file");
    }

    let info = inspect(&file).expect("inspect");
    assert_eq!(info.needs, Needs::Passphrase);

    let progress = NoProgress;
    let cancel = CancelToken::new();
    let control = Control::new(&progress, &cancel);
    let report =
        verify(&file, &Credentials::passphrase(passphrase.clone()), control).expect("verify");
    assert!(report.ok);
    assert!(!report.archive);
    assert_eq!(report.plaintext_size, 12);

    let output = dir.path().join("note.txt");
    let report = decrypt(
        &file,
        &Credentials::passphrase(passphrase),
        &output,
        false,
        control,
    )
    .expect("decrypt");
    assert!(report.ok);
    assert_eq!(fs::read(&output).expect("read"), b"a short note");
}
