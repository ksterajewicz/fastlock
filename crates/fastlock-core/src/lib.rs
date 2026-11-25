//! The core of fastlock.
//!
//! This crate reads, checks and decrypts encrypted files. It contains no
//! interface code: the command-line program and the terminal interface are
//! thin layers that call the functions below.
//!
//! What this stage covers:
//!
//! - [`inspect`]: describe a file without a key.
//! - [`verify`]: decrypt in memory, check everything, write nothing.
//! - [`decrypt`]: decrypt to disk, and unpack an archive.
//!
//! How a caller opens a file:
//!
//! 1. Call [`inspect`] and look at [`Inspection::needs`].
//! 2. Build [`Credentials`]. For a passphrase-protected file, use
//!    [`Credentials::passphrase`]. For a file that opens with a key, read the
//!    key with [`IdentityFile::read`], ask for its passphrase when
//!    [`IdentityFile::needs_passphrase`] says so, and call
//!    [`IdentityFile::unlock`].
//! 3. Call [`verify`] or [`decrypt`] with a [`Control`].
//!
//! fastlock contains no cryptography of its own. Passphrase-protected age
//! files are decrypted with the `age` crate. Files that open with a key are
//! decrypted by the official `age` program, which receives the key over a
//! pipe, never as an argument or an environment variable.

#![forbid(unsafe_code)]

mod age_header;
mod checksums;
mod engine;
mod error;
mod extract;
mod inspect;
mod key;
mod native;
mod ops;
mod pipeline;
mod progress;

pub use age::secrecy::{ExposeSecret, SecretString};

pub use error::{Error, ErrorKind, Result};
pub use inspect::{inspect, Format, Inspection, Needs, Recipient, RecipientKind};
pub use key::{Credentials, IdentityFile};
pub use ops::{
    decrypt, default_output, human_size, verify, Check, CheckId, CheckStatus, Compression,
    DecryptReport, Entry, EntryKind, VerifyReport,
};
pub use progress::{CancelToken, Control, NoProgress, Progress, Stage};
