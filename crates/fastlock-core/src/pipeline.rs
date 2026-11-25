//! The read side of fastlock: decrypt, decompress, and walk the contents.
//!
//! Everything here works on streams. Nothing is held in memory beyond one
//! buffer, apart from small checksum lists found inside an archive.

use std::fs::File;
use std::io::{self, Cursor, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::checksums::{self, HashWriter};
use crate::engine::{self, AgeProcess};
use crate::error::{Error, ErrorKind, Result};
use crate::inspect::{Inspection, Needs};
use crate::key::{Credentials, Secret};
use crate::native;
use crate::ops::{Compression, Entry, EntryKind};
use crate::progress::Control;

/// The first bytes of a Zstandard frame.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// The size of one tar block. The marker `ustar` sits at offset 257 of the
/// first block.
const TAR_BLOCK: usize = 512;

/// The largest checksum list inside an archive that is kept for checking.
const MAX_LIST: u64 = 1 << 20;

/// The size of the copy buffer.
const BUFFER: usize = 64 * 1024;

/// How often progress is reported, in bytes.
const REPORT_EVERY: u64 = 256 * 1024;

/// Where the contents go while they are being read.
pub(crate) trait Sink {
    /// A folder inside an archive.
    fn directory(&mut self, path: &str) -> Result<()>;
    /// A file inside an archive. Returns where its bytes are written, or
    /// `None` to discard them.
    fn start_file(&mut self, path: &str) -> Result<Option<File>>;
    /// Data that is not an archive. Returns where the bytes are written, or
    /// `None` to discard them.
    fn start_single(&mut self) -> Result<Option<File>>;
    /// An entry that is not written, with the reason.
    fn skipped(&mut self, path: &str, reason: &str);
}

/// A sink that keeps nothing. Used by `verify`.
pub(crate) struct NullSink;

impl Sink for NullSink {
    fn directory(&mut self, _path: &str) -> Result<()> {
        Ok(())
    }
    fn start_file(&mut self, _path: &str) -> Result<Option<File>> {
        Ok(None)
    }
    fn start_single(&mut self) -> Result<Option<File>> {
        Ok(None)
    }
    fn skipped(&mut self, _path: &str, _reason: &str) {}
}

/// What was found while reading the decrypted data to its end.
pub(crate) struct Walk {
    pub compression: Compression,
    pub archive: bool,
    pub entries: Vec<Entry>,
    /// Checksum lists found inside the archive: path and content.
    pub lists: Vec<(String, Vec<u8>)>,
    /// The number of decrypted bytes.
    pub plaintext_size: u64,
}

/// The decrypted stream of one file, and the process behind it if there is
/// one.
pub(crate) struct Plain {
    reader: Option<Box<dyn Read + Send>>,
    process: Option<AgeProcess>,
}

/// Starts decrypting `path`.
pub(crate) fn open(
    path: &Path,
    inspection: &Inspection,
    credentials: &Credentials,
) -> Result<Plain> {
    match (&credentials.0, inspection.needs) {
        (Secret::Passphrase(passphrase), Needs::Passphrase) => {
            let file = File::open(path).map_err(|e| Error::io("open", path, e))?;
            let reader = native::passphrase_reader(file, passphrase)?;
            Ok(Plain {
                reader: Some(reader),
                process: None,
            })
        }
        (Secret::Identities(identities), Needs::Key) => {
            let program = engine::locate()?;
            let mut process = AgeProcess::spawn(&program, path, identities)?;
            let Some(stdout) = process.take_stdout() else {
                return Err(Error::new(
                    ErrorKind::Other,
                    "cannot read from the age program",
                ));
            };
            Ok(Plain {
                reader: Some(Box::new(stdout)),
                process: Some(process),
            })
        }
        (Secret::Passphrase(_), Needs::Key) => Err(Error::new(
            ErrorKind::Usage,
            "this file opens with a key, not with a passphrase",
        )),
        (Secret::Identities(_), Needs::Passphrase) => Err(Error::new(
            ErrorKind::Usage,
            "this file is protected by a passphrase, not by a key",
        )),
        (_, Needs::Unknown) => Err(Error::new(
            ErrorKind::Unsupported,
            format!("{} is not a file fastlock can open", path.display()),
        )),
    }
}

impl Plain {
    /// Reads the decrypted data to its end, handing the contents to `sink`,
    /// and confirms that the whole file was authenticated.
    ///
    /// Success means: every byte was read, every layer ended cleanly, and the
    /// decryption reported no error.
    pub(crate) fn walk(
        mut self,
        expected_size: Option<u64>,
        control: Control<'_>,
        sink: &mut dyn Sink,
    ) -> Result<Walk> {
        let Some(reader) = self.reader.take() else {
            return Err(Error::new(ErrorKind::Other, "the data was already read"));
        };
        let counter = Arc::new(AtomicU64::new(0));
        let tracked = Tracked {
            inner: reader,
            done: 0,
            reported: 0,
            total: expected_size,
            counter: Arc::clone(&counter),
            control,
        };

        let outcome = walk_layers(tracked, sink);
        let plaintext_size = counter.load(Ordering::Relaxed);

        match outcome {
            Ok(mut walk) => {
                if control.cancel.is_cancelled() {
                    self.stop();
                    return Err(Error::cancelled());
                }
                if let Some(process) = self.process.as_mut() {
                    process.finish()?;
                }
                walk.plaintext_size = plaintext_size;
                Ok(walk)
            }
            Err(failure) => {
                if control.cancel.is_cancelled() {
                    self.stop();
                    return Err(Error::cancelled());
                }
                match failure {
                    WalkError::Sink(err) => {
                        self.stop();
                        Err(err)
                    }
                    WalkError::Read(io_err) => {
                        // The decryption itself knows best what went wrong.
                        // Close our end first, so that the process can exit.
                        if let Some(process) = self.process.as_mut() {
                            process.finish()?;
                        }
                        Err(Error::new(
                            ErrorKind::Damaged,
                            format!("the file is damaged or was changed: {io_err}"),
                        )
                        .with_source(io_err))
                    }
                }
            }
        }
    }

    fn stop(&mut self) {
        if let Some(process) = self.process.as_mut() {
            process.kill();
        }
    }
}

/// Why reading the contents stopped.
enum WalkError {
    /// Reading the decrypted data failed: damage, or a broken layer.
    Read(io::Error),
    /// Handing the contents on failed: an unsafe path, or a write error.
    Sink(Error),
}

/// Counts the decrypted bytes, reports progress, and stops when cancelled.
struct Tracked<'a> {
    inner: Box<dyn Read + Send>,
    done: u64,
    reported: u64,
    total: Option<u64>,
    counter: Arc<AtomicU64>,
    control: Control<'a>,
}

impl Read for Tracked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.control.cancel.is_cancelled() {
            return Err(io::Error::other("cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.done += n as u64;
        self.counter.store(self.done, Ordering::Relaxed);
        if n == 0 || self.done - self.reported >= REPORT_EVERY {
            self.reported = self.done;
            self.control.progress.advance(self.done, self.total);
        }
        Ok(n)
    }
}

/// Reads until `buf` holds `want` bytes or the stream ends.
fn fill(reader: &mut dyn Read, buf: &mut Vec<u8>, want: usize) -> io::Result<()> {
    let mut chunk = [0u8; TAR_BLOCK];
    while buf.len() < want {
        let room = (want - buf.len()).min(chunk.len());
        match reader.read(&mut chunk[..room]) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn walk_layers(
    mut source: Tracked<'_>,
    sink: &mut dyn Sink,
) -> std::result::Result<Walk, WalkError> {
    // Layer 1: compression.
    let mut head = Vec::with_capacity(ZSTD_MAGIC.len());
    fill(&mut source, &mut head, ZSTD_MAGIC.len()).map_err(WalkError::Read)?;
    let compressed = head == ZSTD_MAGIC;
    let joined = Cursor::new(head).chain(source);
    let (compression, mut data): (Compression, Box<dyn Read + '_>) = if compressed {
        let decoder = zstd::stream::read::Decoder::new(joined).map_err(WalkError::Read)?;
        (Compression::Zstd, Box::new(decoder))
    } else {
        (Compression::None, Box::new(joined))
    };

    // Layer 2: archive.
    let mut block = Vec::with_capacity(TAR_BLOCK);
    fill(&mut data, &mut block, TAR_BLOCK).map_err(WalkError::Read)?;
    let archive = block.len() == TAR_BLOCK && &block[257..262] == b"ustar";
    let mut data = Cursor::new(block).chain(data);

    let mut walk = Walk {
        compression,
        archive,
        entries: Vec::new(),
        lists: Vec::new(),
        plaintext_size: 0,
    };

    if archive {
        walk_archive(&mut data, sink, &mut walk)?;
    } else {
        let mut out = sink.start_single().map_err(WalkError::Sink)?;
        copy(&mut data, out.as_mut(), None)?;
        if let Some(file) = out {
            file.sync_all()
                .map_err(|e| WalkError::Sink(write_error(e)))?;
        }
    }

    // Read whatever is left, so that every layer sees its own end.
    drain(&mut data)?;
    Ok(walk)
}

fn walk_archive(
    data: &mut dyn Read,
    sink: &mut dyn Sink,
    walk: &mut Walk,
) -> std::result::Result<(), WalkError> {
    let mut archive = tar::Archive::new(data);
    let entries = archive.entries().map_err(WalkError::Read)?;

    for entry in entries {
        let mut entry = entry.map_err(WalkError::Read)?;
        let kind = entry.header().entry_type();
        let raw_path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        let path = checksums::normalize_name(&raw_path)
            .trim_end_matches('/')
            .to_string();
        if path.is_empty() {
            // The root folder itself.
            continue;
        }

        if kind.is_dir() {
            sink.directory(&path).map_err(WalkError::Sink)?;
            walk.entries.push(Entry {
                path,
                size: 0,
                kind: EntryKind::Directory,
                sha256: None,
            });
        } else if kind.is_file() {
            let keep_list = checksums::is_list_name(&path) && entry.size() <= MAX_LIST;
            let mut list = keep_list.then(Vec::new);
            let mut out = sink.start_file(&path).map_err(WalkError::Sink)?;
            let (sha256, size) = copy(&mut entry, out.as_mut(), list.as_mut())?;
            if let Some(file) = out {
                file.sync_all()
                    .map_err(|e| WalkError::Sink(write_error(e)))?;
            }
            if let Some(content) = list {
                walk.lists.push((path.clone(), content));
            }
            walk.entries.push(Entry {
                path,
                size,
                kind: EntryKind::File,
                sha256: Some(sha256),
            });
        } else {
            sink.skipped(&path, "links and special files are not written");
            walk.entries.push(Entry {
                path,
                size: 0,
                kind: EntryKind::Other,
                sha256: None,
            });
        }
    }
    Ok(())
}

/// Copies a stream to its end: hashes it, and optionally writes it to a file
/// and keeps a copy in memory. Returns the checksum and the size.
fn copy(
    reader: &mut dyn Read,
    mut file: Option<&mut File>,
    mut keep: Option<&mut Vec<u8>>,
) -> std::result::Result<(String, u64), WalkError> {
    let mut hasher = HashWriter::new();
    let mut buf = vec![0u8; BUFFER];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(WalkError::Read(e)),
        };
        let chunk = &buf[..n];
        // Writing to the hasher cannot fail.
        let _ = hasher.write_all(chunk);
        if let Some(file) = file.as_mut() {
            file.write_all(chunk)
                .map_err(|e| WalkError::Sink(write_error(e)))?;
        }
        if let Some(keep) = keep.as_mut() {
            keep.extend_from_slice(chunk);
        }
    }
    Ok(hasher.finish())
}

/// Reads and discards the rest of a stream.
fn drain(reader: &mut dyn Read) -> std::result::Result<(), WalkError> {
    let mut buf = vec![0u8; BUFFER];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(WalkError::Read(e)),
        }
    }
}

fn write_error(err: io::Error) -> Error {
    Error::new(ErrorKind::Other, format!("cannot write the output: {err}")).with_source(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{CancelToken, NoProgress};

    fn tracked<'a>(data: Vec<u8>, control: Control<'a>) -> Tracked<'a> {
        Tracked {
            inner: Box::new(Cursor::new(data)),
            done: 0,
            reported: 0,
            total: None,
            counter: Arc::new(AtomicU64::new(0)),
            control,
        }
    }

    fn tar_with(files: &[(&str, &[u8])]) -> Vec<u8> {
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

    #[test]
    fn walks_a_plain_tar_and_hashes_its_files() {
        let progress = NoProgress;
        let cancel = CancelToken::new();
        let control = Control::new(&progress, &cancel);
        let data = tar_with(&[("a.txt", b"abc"), ("sub/b.txt", b"")]);

        let walk = walk_layers(tracked(data, control), &mut NullSink)
            .ok()
            .expect("walk works");
        assert!(walk.archive);
        assert_eq!(walk.compression, Compression::None);
        assert_eq!(walk.entries.len(), 2);
        assert_eq!(walk.entries[0].path, "a.txt");
        assert_eq!(walk.entries[0].size, 3);
        assert_eq!(
            walk.entries[0].sha256.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(walk.entries[1].path, "sub/b.txt");
    }

    #[test]
    fn walks_a_compressed_tar_and_keeps_checksum_lists() {
        let progress = NoProgress;
        let cancel = CancelToken::new();
        let control = Control::new(&progress, &cancel);
        let list = b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  a.txt\n";
        let tar = tar_with(&[("./a.txt", b"abc"), ("./CHECKSUMS.sha256", list)]);
        let compressed = zstd::stream::encode_all(Cursor::new(tar), 3).expect("compress");

        let walk = walk_layers(tracked(compressed, control), &mut NullSink)
            .ok()
            .expect("walk works");
        assert_eq!(walk.compression, Compression::Zstd);
        assert!(walk.archive);
        assert_eq!(walk.entries[0].path, "a.txt");
        assert_eq!(walk.lists.len(), 1);
        assert_eq!(walk.lists[0].0, "CHECKSUMS.sha256");
        assert_eq!(walk.lists[0].1, list.to_vec());
    }

    #[test]
    fn data_that_is_not_an_archive_is_one_stream() {
        let progress = NoProgress;
        let cancel = CancelToken::new();
        let control = Control::new(&progress, &cancel);
        let walk = walk_layers(tracked(b"just some bytes".to_vec(), control), &mut NullSink)
            .ok()
            .expect("walk works");
        assert!(!walk.archive);
        assert!(walk.entries.is_empty());
    }

    #[test]
    fn a_cut_compressed_stream_is_a_read_error() {
        let progress = NoProgress;
        let cancel = CancelToken::new();
        let control = Control::new(&progress, &cancel);
        let tar = tar_with(&[("a.txt", &[7u8; 4096])]);
        let mut compressed = zstd::stream::encode_all(Cursor::new(tar), 3).expect("compress");
        compressed.truncate(compressed.len() / 2);

        let outcome = walk_layers(tracked(compressed, control), &mut NullSink);
        assert!(matches!(outcome, Err(WalkError::Read(_))));
    }

    #[test]
    fn a_cancelled_walk_stops() {
        let progress = NoProgress;
        let cancel = CancelToken::new();
        cancel.cancel();
        let control = Control::new(&progress, &cancel);
        let outcome = walk_layers(tracked(vec![0u8; 100], control), &mut NullSink);
        assert!(matches!(outcome, Err(WalkError::Read(_))));
    }
}
