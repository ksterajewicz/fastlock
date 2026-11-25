//! Runs a verify or decrypt job on its own thread.
//!
//! The interface stays responsive while the core works. The job reports
//! through a channel: progress messages, throttled so they do not flood the
//! channel, and one final [`Message::Done`].

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use fastlock_core::{
    CancelToken, Check, Compression, Control, Credentials, DecryptReport, Entry, Error, ErrorKind,
    IdentityFile, Progress, SecretString, Stage, VerifyReport,
};

/// The least time between two progress messages.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// What the person chose to do with a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Decrypt in memory, check everything, write nothing.
    Verify,
    /// Decrypt to disk.
    Decrypt,
}

impl Action {
    /// The name shown in the interface.
    pub fn label(self) -> &'static str {
        match self {
            Action::Verify => "verify",
            Action::Decrypt => "decrypt",
        }
    }

    /// The name of the running job, for the status line.
    pub fn running(self) -> &'static str {
        match self {
            Action::Verify => "verifying",
            Action::Decrypt => "decrypting",
        }
    }
}

/// How the job gets its credentials. Unlocking a key file can take a moment
/// (its passphrase is stretched), so it also happens on the job thread.
pub enum Unlock {
    /// The passphrase of a passphrase-protected file.
    Passphrase(SecretString),
    /// A key file, with its passphrase when it has one.
    Key {
        identity: Arc<IdentityFile>,
        passphrase: Option<SecretString>,
    },
}

/// One verify or decrypt job.
pub struct Job {
    pub action: Action,
    pub path: PathBuf,
    /// Where to write; only for [`Action::Decrypt`].
    pub output: Option<PathBuf>,
    pub unlock: Unlock,
    pub cancel: CancelToken,
}

/// What a running job reports.
pub enum Message {
    /// The key file is being unlocked.
    Unlocking,
    /// A new stage has started.
    Stage(Stage),
    /// Bytes of the current stage are done.
    Advance { done: u64, total: Option<u64> },
    /// The job has ended.
    Done(Result<Outcome, Error>),
}

/// The result of a finished job, for both actions.
pub struct Outcome {
    pub action: Action,
    pub file: PathBuf,
    pub ok: bool,
    pub checks: Vec<Check>,
    pub compression: Compression,
    pub archive: bool,
    pub entries: Vec<Entry>,
    /// Where the data was written; only for [`Action::Decrypt`].
    pub output: Option<PathBuf>,
    /// Entries that were not written; only for [`Action::Decrypt`].
    pub skipped: Vec<String>,
    pub plaintext_size: u64,
}

impl Outcome {
    pub fn from_verify(file: PathBuf, report: VerifyReport) -> Self {
        Outcome {
            action: Action::Verify,
            file,
            ok: report.ok,
            checks: report.checks,
            compression: report.compression,
            archive: report.archive,
            entries: report.entries,
            output: None,
            skipped: Vec::new(),
            plaintext_size: report.plaintext_size,
        }
    }

    pub fn from_decrypt(file: PathBuf, report: DecryptReport) -> Self {
        Outcome {
            action: Action::Decrypt,
            file,
            ok: report.ok,
            checks: report.checks,
            compression: report.compression,
            archive: report.archive,
            entries: report.entries,
            output: Some(report.output),
            skipped: report.skipped,
            plaintext_size: report.plaintext_size,
        }
    }
}

/// Starts the job on a new thread. The thread always ends by sending
/// [`Message::Done`].
pub fn spawn(job: Job, tx: Sender<Message>) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name("fastlock-job".to_string())
        .spawn(move || {
            let result = run_job(job, &tx);
            let _ = tx.send(Message::Done(result));
        })
}

fn run_job(job: Job, tx: &Sender<Message>) -> Result<Outcome, Error> {
    let Job {
        action,
        path,
        output,
        unlock,
        cancel,
    } = job;

    // The passphrase is dropped, and wiped, as soon as the credentials exist.
    let credentials = match unlock {
        Unlock::Passphrase(passphrase) => Credentials::passphrase(passphrase),
        Unlock::Key {
            identity,
            passphrase,
        } => {
            if passphrase.is_some() {
                let _ = tx.send(Message::Unlocking);
            }
            identity.unlock(passphrase.as_ref())?
        }
    };

    if cancel.is_cancelled() {
        return Err(Error::new(ErrorKind::Cancelled, "cancelled"));
    }

    let progress = ChannelProgress::new(tx.clone());
    let control = Control::new(&progress, &cancel);
    match action {
        Action::Verify => fastlock_core::verify(&path, &credentials, control)
            .map(|report| Outcome::from_verify(path.clone(), report)),
        Action::Decrypt => {
            let output =
                output.ok_or_else(|| Error::new(ErrorKind::Usage, "no output path was given"))?;
            fastlock_core::decrypt(&path, &credentials, &output, false, control)
                .map(|report| Outcome::from_decrypt(path.clone(), report))
        }
    }
}

/// A [`Progress`] that sends messages, at most one advance per
/// [`PROGRESS_INTERVAL`] plus the last one of each stage.
pub struct ChannelProgress {
    state: Mutex<Throttle>,
}

struct Throttle {
    tx: Sender<Message>,
    last: Option<Instant>,
}

impl ChannelProgress {
    pub fn new(tx: Sender<Message>) -> Self {
        ChannelProgress {
            state: Mutex::new(Throttle { tx, last: None }),
        }
    }
}

impl Progress for ChannelProgress {
    fn stage(&self, stage: Stage) {
        if let Ok(mut state) = self.state.lock() {
            state.last = None;
            let _ = state.tx.send(Message::Stage(stage));
        }
    }

    fn advance(&self, done: u64, total: Option<u64>) {
        if let Ok(mut state) = self.state.lock() {
            let now = Instant::now();
            let finished = matches!(total, Some(t) if done >= t);
            let due = match state.last {
                None => true,
                Some(last) => now.duration_since(last) >= PROGRESS_INTERVAL,
            };
            if finished || due {
                state.last = Some(now);
                let _ = state.tx.send(Message::Advance { done, total });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn progress_is_throttled_but_the_end_is_always_sent() {
        let (tx, rx) = mpsc::channel();
        let progress = ChannelProgress::new(tx);
        progress.stage(Stage::Decrypt);
        for done in 0..10_000u64 {
            progress.advance(done, Some(10_000));
        }
        progress.advance(10_000, Some(10_000));
        drop(progress);

        let messages: Vec<Message> = rx.iter().collect();
        assert!(matches!(messages[0], Message::Stage(Stage::Decrypt)));
        let advances = messages.len() - 1;
        assert!(advances >= 2, "first and last advance are sent");
        assert!(advances < 100, "{advances} advances were sent");
        assert!(matches!(
            messages.last(),
            Some(Message::Advance {
                done: 10_000,
                total: Some(10_000)
            })
        ));
    }

    #[test]
    fn a_new_stage_sends_its_first_advance_at_once() {
        let (tx, rx) = mpsc::channel();
        let progress = ChannelProgress::new(tx);
        progress.advance(1, None);
        progress.stage(Stage::Finish);
        progress.advance(2, None);
        drop(progress);
        let advances = rx
            .iter()
            .filter(|m| matches!(m, Message::Advance { .. }))
            .count();
        assert_eq!(advances, 2);
    }
}
