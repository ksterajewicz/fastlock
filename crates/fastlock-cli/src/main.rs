//! The fastlock command-line program.
//!
//! This file only parses arguments, asks for what is missing, calls the core,
//! and prints the result. It contains no encryption logic.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use clap::{Args, Parser, Subcommand};
use serde::Serialize;

use fastlock_core::{
    default_output, human_size, inspect, CancelToken, Check, CheckStatus, Control, Credentials,
    Entry, EntryKind, Error, ErrorKind, Format, IdentityFile, Inspection, Needs, Progress,
    SecretString, Stage,
};

/// Prints a line to standard output.
///
/// A closed pipe, as in `fastlock inspect file | head`, is not an error: the
/// line is dropped instead of stopping the program with a panic.
macro_rules! say {
    ($($arg:tt)*) => {{
        let _ = writeln!(io::stdout(), $($arg)*);
    }};
}

/// Exit code for a run that finished but found a failed check.
const EXIT_DAMAGED: u8 = 4;

#[derive(Parser)]
#[command(
    name = "fastlock",
    version,
    about = "Fast, open-source file encryption and key management.",
    arg_required_else_help = true
)]
struct Cli {
    #[command(flatten)]
    global: Global,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone)]
struct Global {
    /// Print machine-readable JSON
    #[arg(long, global = true)]
    json: bool,
    /// Print only errors
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Print more detail
    #[arg(short, long, global = true)]
    verbose: bool,
    /// Do not use colors
    #[arg(long, global = true)]
    no_color: bool,
    /// Never prompt; fail instead and name the missing input
    #[arg(long, global = true)]
    no_input: bool,
    /// Read the passphrase from a file instead of a prompt
    #[arg(long, global = true, value_name = "PATH")]
    passphrase_file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Show what a file is, without needing a key
    Inspect {
        /// The file to look at
        file: PathBuf,
    },
    /// Check a file in memory, writing nothing
    Verify {
        /// The encrypted file
        file: PathBuf,
        /// The key file that opens it
        #[arg(long, value_name = "PATH")]
        key: Option<PathBuf>,
    },
    /// Decrypt a file to disk and unpack it
    Decrypt {
        /// The encrypted file
        file: PathBuf,
        /// Where to write the result
        #[arg(short, long, value_name = "PATH")]
        output: Option<PathBuf>,
        /// The key file that opens it
        #[arg(long, value_name = "PATH")]
        key: Option<PathBuf>,
        /// Allow an existing file to be replaced
        #[arg(long)]
        force: bool,
    },
    /// Open the terminal interface
    #[cfg(feature = "tui")]
    Tui {
        /// The folder to start in
        path: Option<PathBuf>,
    },
    /// Print the version
    Version,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let json = cli.global.json;
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            report_error(&err, json);
            ExitCode::from(err.exit_code())
        }
    }
}

fn run(cli: Cli) -> Result<u8, Error> {
    let global = cli.global;
    let out = Output::new(&global);

    match cli.command {
        Command::Inspect { file } => {
            let inspection = inspect(&file)?;
            if global.json {
                print_json(&inspection)?;
            } else if !global.quiet {
                out.inspection(&inspection);
            }
            Ok(0)
        }
        Command::Verify { file, key } => {
            let inspection = require_age(&file)?;
            let credentials = credentials(&inspection, key.as_deref(), &global)?;
            let cancel = cancel_on_interrupt();
            let progress = TermProgress::new(&global);
            let report =
                fastlock_core::verify(&file, &credentials, Control::new(&progress, &cancel));
            progress.clear();
            let report = report?;

            if global.json {
                print_json(&report)?;
            } else if !global.quiet {
                out.checks(&report.checks);
                out.entries(&report.entries, global.verbose);
                if report.ok {
                    out.verdict(
                        true,
                        &format!("{} is intact. Nothing was written.", name_of(&file)),
                    );
                } else {
                    out.verdict(
                        false,
                        &format!("{} did not pass every check.", name_of(&file)),
                    );
                }
            }
            quiet_failure(&global, report.ok, &report.checks, &name_of(&file));
            Ok(if report.ok { 0 } else { EXIT_DAMAGED })
        }
        Command::Decrypt {
            file,
            output,
            key,
            force,
        } => {
            let inspection = require_age(&file)?;
            let output = output.unwrap_or_else(|| default_output(&file));
            let credentials = credentials(&inspection, key.as_deref(), &global)?;
            let cancel = cancel_on_interrupt();
            let progress = TermProgress::new(&global);
            let report = fastlock_core::decrypt(
                &file,
                &credentials,
                &output,
                force,
                Control::new(&progress, &cancel),
            );
            progress.clear();
            let report = report?;

            if global.json {
                print_json(&report)?;
            } else if !global.quiet {
                out.checks(&report.checks);
                out.entries(&report.entries, global.verbose);
                for note in &report.skipped {
                    out.note(&format!("not written: {note}"));
                }
                let place = report.output.display();
                if report.ok {
                    out.verdict(true, &format!("written to {place}"));
                } else {
                    out.verdict(false, &format!("written to {place}, but a check failed"));
                }
            }
            quiet_failure(&global, report.ok, &report.checks, &name_of(&file));
            Ok(if report.ok { 0 } else { EXIT_DAMAGED })
        }
        #[cfg(feature = "tui")]
        Command::Tui { path } => {
            fastlock_tui::run(path).map_err(|e| {
                Error::new(
                    ErrorKind::Other,
                    format!("the terminal interface stopped with an error: {e}"),
                )
            })?;
            Ok(0)
        }
        Command::Version => {
            say!("fastlock {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
    }
}

/// With `--quiet` nothing is printed on success. A failed check is an error,
/// so it is still reported, on standard error.
fn quiet_failure(global: &Global, ok: bool, checks: &[Check], name: &str) {
    if ok || !global.quiet || global.json {
        return;
    }
    let detail = checks
        .iter()
        .find(|check| check.status == CheckStatus::Failed)
        .map(|check| format!("{}: {}", check.id.title(), check.detail))
        .unwrap_or_else(|| "a check failed".to_string());
    eprintln!("fastlock: {name} did not pass every check ({detail})");
}

/// Inspects a file and refuses anything that is not an age file.
fn require_age(file: &Path) -> Result<Inspection, Error> {
    let inspection = inspect(file)?;
    if inspection.format != Format::Age {
        return Err(Error::new(
            ErrorKind::Unsupported,
            format!("{} is not a file fastlock can open", file.display()),
        ));
    }
    Ok(inspection)
}

/// Collects what is needed to open the file: a passphrase, or a key file and
/// the passphrase that protects it.
fn credentials(
    inspection: &Inspection,
    key: Option<&Path>,
    global: &Global,
) -> Result<Credentials, Error> {
    match inspection.needs {
        Needs::Passphrase => {
            let prompt = format!("Passphrase for {}: ", name_of(&inspection.path));
            Ok(Credentials::passphrase(passphrase(global, &prompt)?))
        }
        Needs::Key => {
            let Some(key_path) = key else {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "this file opens with a key. Add --key <path to the key file>.",
                ));
            };
            let identity = IdentityFile::read(key_path)?;
            if identity.needs_passphrase() {
                let prompt = format!("Passphrase for the key {}: ", name_of(key_path));
                let secret = passphrase(global, &prompt)?;
                identity.unlock(Some(&secret))
            } else {
                identity.unlock(None)
            }
        }
        Needs::Unknown => Err(Error::new(
            ErrorKind::Unsupported,
            format!(
                "{} is not a file fastlock can open",
                inspection.path.display()
            ),
        )),
    }
}

/// Gets a passphrase from `--passphrase-file`, or asks for it on the
/// terminal. A passphrase is never taken from an argument.
fn passphrase(global: &Global, prompt: &str) -> Result<SecretString, Error> {
    let mut text = if let Some(path) = &global.passphrase_file {
        let mut text = std::fs::read_to_string(path).map_err(|e| {
            Error::new(
                ErrorKind::NotFound,
                format!("cannot read the passphrase file {}: {e}", path.display()),
            )
        })?;
        while text.ends_with('\n') || text.ends_with('\r') {
            text.pop();
        }
        text
    } else if global.no_input {
        return Err(Error::new(
            ErrorKind::Usage,
            "a passphrase is needed. Use --passphrase-file <path>, or run without --no-input.",
        ));
    } else {
        rpassword::prompt_password(prompt).map_err(|e| {
            Error::new(
                ErrorKind::Usage,
                format!("cannot ask for a passphrase here ({e}). Use --passphrase-file <path>."),
            )
        })?
    };

    if text.is_empty() {
        return Err(Error::new(ErrorKind::Usage, "the passphrase is empty"));
    }
    Ok(SecretString::from(std::mem::take(&mut text)))
}

/// Returns a token that is cancelled when the person presses Ctrl+C, so that
/// a running operation stops cleanly and removes its temporary files.
fn cancel_on_interrupt() -> CancelToken {
    let cancel = CancelToken::new();
    let handle = cancel.clone();
    // If the handler cannot be installed, Ctrl+C keeps its default meaning.
    let _ = ctrlc::set_handler(move || handle.cancel());
    cancel
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn print_json<T: Serialize>(value: &T) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| Error::new(ErrorKind::Other, format!("cannot write JSON: {e}")))?;
    say!("{text}");
    Ok(())
}

#[derive(Serialize)]
struct JsonError<'a> {
    error: JsonErrorBody<'a>,
}

#[derive(Serialize)]
struct JsonErrorBody<'a> {
    kind: ErrorKind,
    message: &'a str,
    exit_code: u8,
}

fn report_error(err: &Error, json: bool) {
    if json {
        let body = JsonError {
            error: JsonErrorBody {
                kind: err.kind(),
                message: err.message(),
                exit_code: err.exit_code(),
            },
        };
        if let Ok(text) = serde_json::to_string_pretty(&body) {
            say!("{text}");
            return;
        }
    }
    eprintln!("fastlock: {err}");
}

/// Prints results for people.
struct Output {
    color: bool,
}

impl Output {
    fn new(global: &Global) -> Self {
        let no_color_env = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Output {
            color: !global.no_color && !no_color_env && io::stdout().is_terminal(),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn inspection(&self, info: &Inspection) {
        say!("{}", self.paint("1", &name_of(&info.path)));
        self.row("format", info.format.label());
        if info.format == Format::Age {
            self.row("opens with", &recipients_summary(info));
            self.row("post-quantum", if info.post_quantum { "yes" } else { "no" });
            if info.armored {
                self.row("stored as", "text (armor)");
            }
        }
        self.row("size", &human_size(info.size));
        match &info.checksum_file {
            Some(path) => self.row("checksum file", &name_of(path)),
            None => self.row("checksum file", "none next to it"),
        }
    }

    fn row(&self, label: &str, value: &str) {
        say!("  {}  {value}", self.paint("2", &format!("{label:<13}")));
    }

    fn checks(&self, checks: &[Check]) {
        for check in checks {
            let mark = match check.status {
                CheckStatus::Passed => self.paint("32", "✓"),
                CheckStatus::Failed => self.paint("31", "✗"),
                CheckStatus::Skipped => self.paint("2", "–"),
            };
            say!("{mark} {:<17} {}", check.id.title(), check.detail);
        }
    }

    fn entries(&self, entries: &[Entry], with_checksums: bool) {
        if entries.is_empty() {
            return;
        }
        say!("");
        for entry in entries {
            let size = match entry.kind {
                EntryKind::File => human_size(entry.size),
                EntryKind::Directory => "folder".to_string(),
                EntryKind::Other => "not a file".to_string(),
            };
            say!("  {:<40} {}", entry.path, self.paint("2", &size));
            if with_checksums {
                if let Some(sha256) = &entry.sha256 {
                    say!("    {}", self.paint("2", sha256));
                }
            }
        }
    }

    fn note(&self, text: &str) {
        say!("{}", self.paint("33", text));
    }

    fn verdict(&self, ok: bool, text: &str) {
        say!("");
        if ok {
            say!("{} {text}", self.paint("32", "OK:"));
        } else {
            say!("{} {text}", self.paint("31", "FAILED:"));
        }
    }
}

/// Says who can open the file, grouping recipients of the same kind.
fn recipients_summary(info: &Inspection) -> String {
    let mut groups: Vec<(&'static str, usize)> = Vec::new();
    for recipient in &info.recipients {
        let label = recipient.kind.label();
        match groups.iter_mut().find(|(known, _)| *known == label) {
            Some((_, count)) => *count += 1,
            None => groups.push((label, 1)),
        }
    }
    groups
        .iter()
        .map(|(label, count)| {
            if *count == 1 {
                (*label).to_string()
            } else {
                format!("{label} ({count})")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Shows progress on one line of the terminal. Silent when the output is not
/// a terminal, with `--quiet`, and with `--json`.
struct TermProgress {
    enabled: bool,
    stage: AtomicU8,
    shown: AtomicU64,
}

impl TermProgress {
    fn new(global: &Global) -> Self {
        TermProgress {
            enabled: !global.quiet && !global.json && io::stderr().is_terminal(),
            stage: AtomicU8::new(0),
            shown: AtomicU64::new(u64::MAX),
        }
    }

    fn label(&self) -> &'static str {
        match self.stage.load(Ordering::Relaxed) {
            1 => "checking the checksum file",
            2 => "decrypting",
            3 => "finishing",
            _ => "working",
        }
    }

    /// Removes the progress line.
    fn clear(&self) {
        if self.enabled {
            let mut err = io::stderr();
            let _ = write!(err, "\r\x1b[2K");
            let _ = err.flush();
        }
    }
}

impl Progress for TermProgress {
    fn stage(&self, stage: Stage) {
        let number = match stage {
            Stage::Checksum => 1,
            Stage::Decrypt => 2,
            Stage::Finish => 3,
        };
        self.stage.store(number, Ordering::Relaxed);
        self.shown.store(u64::MAX, Ordering::Relaxed);
    }

    fn advance(&self, done: u64, total: Option<u64>) {
        if !self.enabled {
            return;
        }
        let mut err = io::stderr();
        match total {
            Some(total) if total > 0 => {
                let percent = (done.saturating_mul(100) / total).min(100);
                if self.shown.swap(percent, Ordering::Relaxed) == percent {
                    return;
                }
                let _ = write!(err, "\r\x1b[2K{} {percent}%", self.label());
            }
            _ => {
                let mebibytes = done / (1024 * 1024);
                if self.shown.swap(mebibytes, Ordering::Relaxed) == mebibytes {
                    return;
                }
                let _ = write!(err, "\r\x1b[2K{} {}", self.label(), human_size(done));
            }
        }
        let _ = err.flush();
    }
}
