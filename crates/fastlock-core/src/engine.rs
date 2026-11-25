//! Decrypting with the official `age` program.
//!
//! Files that open with a key are decrypted by the `age` program, version
//! 1.3 or newer. It is the reference implementation, and it supports
//! post-quantum keys. fastlock starts it, hands it the key over its standard
//! input, and reads the plaintext from its standard output. The key is never
//! placed in an argument or an environment variable.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;

use crate::error::{Error, ErrorKind, Result};

/// The name of the program.
const PROGRAM: &str = "age";

/// An environment variable that names the `age` program to use instead of the
/// one found on `PATH`. It holds a path, not a secret.
const PROGRAM_OVERRIDE: &str = "FASTLOCK_AGE";

/// Finds the `age` program.
pub(crate) fn locate() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(PROGRAM_OVERRIDE) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(Error::new(
            ErrorKind::MissingTool,
            format!(
                "{PROGRAM_OVERRIDE} points to {}, which is not a file",
                path.display()
            ),
        ));
    }

    let names: &[&str] = if cfg!(windows) {
        &["age.exe", "age"]
    } else {
        &[PROGRAM]
    };
    let search: OsString = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&search) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(Error::new(
        ErrorKind::MissingTool,
        "fastlock needs the `age` program, version 1.3 or newer, to open files that use a key, \
         and it was not found. Install age and try again.",
    ))
}

/// A running `age` process that is decrypting one file.
pub(crate) struct AgeProcess {
    child: Child,
    stdout: Option<ChildStdout>,
    stderr: Option<JoinHandle<String>>,
    finished: bool,
}

impl AgeProcess {
    /// Starts `age` to decrypt `file` with the given identities.
    ///
    /// `identities` is the text of an age identity file. It is written to the
    /// standard input of the process, which is then closed.
    pub(crate) fn spawn(program: &Path, file: &Path, identities: &[u8]) -> Result<Self> {
        let mut child = Command::new(program)
            .arg("--decrypt")
            .arg("--identity")
            .arg("-")
            .arg("--")
            .arg(file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                Error::new(
                    ErrorKind::MissingTool,
                    format!("cannot start {}: {e}", program.display()),
                )
                .with_source(e)
            })?;

        // The identities are small, far below the size of a pipe buffer, so
        // this cannot block. If the process has already exited, the write
        // fails and the real reason is reported when the process is finished.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(identities);
            let _ = stdin.flush();
        }

        let stderr = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                text
            })
        });
        let stdout = child.stdout.take();

        Ok(AgeProcess {
            child,
            stdout,
            stderr,
            finished: false,
        })
    }

    /// The plaintext stream. Can be taken once.
    pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.stdout.take()
    }

    /// Stops the process. Used when the operation is cancelled or has failed.
    pub(crate) fn kill(&mut self) {
        if !self.finished {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.finished = true;
        }
    }

    /// Waits for the process and reports how it ended.
    ///
    /// Call this only after its output has been read to the end. Success means
    /// that `age` authenticated the whole file.
    pub(crate) fn finish(&mut self) -> Result<()> {
        let status = self
            .child
            .wait()
            .map_err(|e| Error::new(ErrorKind::Other, format!("cannot wait for age: {e}")))?;
        self.finished = true;
        let stderr = self
            .stderr
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(classify_failure(&stderr))
        }
    }
}

impl Drop for AgeProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Turns what `age` printed on failure into an error of ours.
///
/// The messages matched here were collected from age 1.3.2.
pub(crate) fn classify_failure(stderr: &str) -> Error {
    let detail = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("age failed without a message")
        .trim_start_matches("age: error: ")
        .to_string();

    let has = |needle: &str| stderr.contains(needle);

    if has("no identity matched any of the recipients") {
        Error::new(ErrorKind::WrongKey, "this key does not open the file")
    } else if has("failed to decrypt and authenticate payload chunk")
        || has("bad header MAC")
        || has("failed to read header")
        || has("unexpected EOF")
    {
        Error::new(
            ErrorKind::Damaged,
            format!("the file is damaged or was changed: {detail}"),
        )
    } else if has("unknown identity type") {
        Error::new(
            ErrorKind::MissingTool,
            "the installed age program does not understand this key. \
             Post-quantum keys need age 1.3 or newer.",
        )
    } else if has("no identities found") {
        Error::new(ErrorKind::Usage, "the key file holds no key")
    } else if has("failed to open input file") {
        Error::new(
            ErrorKind::NotFound,
            format!("age could not open the file: {detail}"),
        )
    } else {
        Error::new(ErrorKind::Other, format!("age failed: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrong_key_is_recognized() {
        let err = classify_failure(
            "age: error: no identity matched any of the recipients\n\
             age: report unexpected or unhelpful errors at https://filippo.io/age/report\n",
        );
        assert_eq!(err.kind(), ErrorKind::WrongKey);
    }

    #[test]
    fn a_damaged_payload_is_recognized() {
        let err = classify_failure(
            "age: error: failed to decrypt and authenticate payload chunk, \
             file may be corrupted or tampered with\n",
        );
        assert_eq!(err.kind(), ErrorKind::Damaged);
        assert!(!err.message().contains("age: error:"));
    }

    #[test]
    fn a_damaged_header_is_recognized() {
        assert_eq!(
            classify_failure("age: error: bad header MAC\n").kind(),
            ErrorKind::Damaged
        );
        assert_eq!(
            classify_failure(
                "age: error: failed to read header: failed to parse header: failed to read line: EOF\n"
            )
            .kind(),
            ErrorKind::Damaged
        );
    }

    #[test]
    fn an_old_age_is_reported_as_a_missing_tool() {
        let err = classify_failure(
            "age: error: reading \"-\": failed to read \"-\": error at line 1: unknown identity type\n",
        );
        assert_eq!(err.kind(), ErrorKind::MissingTool);
    }

    #[test]
    fn anything_else_keeps_the_first_line() {
        let err = classify_failure("age: error: something new\nsecond line\n");
        assert_eq!(err.kind(), ErrorKind::Other);
        assert_eq!(err.message(), "age failed: something new");
    }
}
