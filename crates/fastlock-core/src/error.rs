//! Errors, and the exit codes they map to.

use std::fmt;
use std::io;
use std::path::Path;

/// What went wrong, in the terms of the documented exit codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The command was used wrongly, or an input is missing.
    Usage,
    /// No key fits, or the passphrase is wrong.
    WrongKey,
    /// The file is damaged or was changed: authentication failed.
    Damaged,
    /// A file or a key was not found.
    NotFound,
    /// Refused: the output exists, or a confirmation is needed.
    Refused,
    /// Cancelled by the person.
    Cancelled,
    /// A program fastlock needs is missing or too old.
    MissingTool,
    /// The file uses something this version cannot handle yet.
    Unsupported,
    /// Any other failure.
    Other,
}

impl ErrorKind {
    /// The process exit code for this kind of error.
    pub fn exit_code(self) -> u8 {
        match self {
            ErrorKind::Usage => 2,
            ErrorKind::WrongKey => 3,
            ErrorKind::Damaged => 4,
            ErrorKind::NotFound => 5,
            ErrorKind::Refused => 6,
            ErrorKind::Cancelled => 7,
            ErrorKind::MissingTool | ErrorKind::Unsupported | ErrorKind::Other => 1,
        }
    }
}

/// An error from the core.
///
/// The message is written for the person using the program. It never contains
/// a passphrase or key material.
#[derive(Debug)]
pub struct Error {
    kind: ErrorKind,
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl Error {
    /// Creates an error with a message for the person.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
            source: None,
        }
    }

    /// Attaches the underlying error.
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// The kind of error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The message for the person.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The process exit code for this error.
    pub fn exit_code(&self) -> u8 {
        self.kind.exit_code()
    }

    pub(crate) fn cancelled() -> Self {
        Error::new(ErrorKind::Cancelled, "cancelled")
    }

    /// Builds an error for a failed file operation, naming the path.
    pub(crate) fn io(action: &str, path: &Path, err: io::Error) -> Self {
        let kind = match err.kind() {
            io::ErrorKind::NotFound => ErrorKind::NotFound,
            io::ErrorKind::AlreadyExists => ErrorKind::Refused,
            _ => ErrorKind::Other,
        };
        Error::new(kind, format!("cannot {action} {}: {err}", path.display())).with_source(err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|s| s.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// The result type of the core.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_the_documented_table() {
        assert_eq!(ErrorKind::Usage.exit_code(), 2);
        assert_eq!(ErrorKind::WrongKey.exit_code(), 3);
        assert_eq!(ErrorKind::Damaged.exit_code(), 4);
        assert_eq!(ErrorKind::NotFound.exit_code(), 5);
        assert_eq!(ErrorKind::Refused.exit_code(), 6);
        assert_eq!(ErrorKind::Cancelled.exit_code(), 7);
        assert_eq!(ErrorKind::Other.exit_code(), 1);
    }

    #[test]
    fn a_missing_file_maps_to_not_found() {
        let err = Error::io(
            "open",
            Path::new("missing.age"),
            io::Error::from(io::ErrorKind::NotFound),
        );
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(err.message().contains("missing.age"));
    }
}
