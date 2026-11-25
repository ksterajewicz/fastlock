//! Decrypting passphrase-protected age data with the `age` crate.
//!
//! This is the only place where fastlock links cryptographic code, and it is
//! the reviewed `age` crate, not code of our own.

use std::io::Read;

use age::secrecy::SecretString;

use crate::error::{Error, ErrorKind, Result};

/// The highest scrypt work factor accepted when decrypting, as a power of two.
/// The official `age` program uses the same limit.
const MAX_WORK_FACTOR: u8 = 22;

/// Opens passphrase-protected age data and returns a reader over the
/// plaintext. Both binary and armored input work.
///
/// The returned reader fails with an I/O error when the data is damaged, and
/// it only reports the end of the data after the last chunk has been
/// authenticated.
pub(crate) fn passphrase_reader<R>(
    input: R,
    passphrase: &SecretString,
) -> Result<Box<dyn Read + Send>>
where
    R: Read + Send + 'static,
{
    let armored = age::armor::ArmoredReader::new(input);
    let decryptor = age::Decryptor::new(armored).map_err(map_decrypt_error)?;
    if !decryptor.is_scrypt() {
        return Err(Error::new(
            ErrorKind::Usage,
            "this file is not protected by a passphrase; it opens with a key",
        ));
    }
    let mut identity = age::scrypt::Identity::new(passphrase.clone());
    identity.set_max_work_factor(MAX_WORK_FACTOR);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|err| match err {
            // At this point only the passphrase has been tried. The `age`
            // crate reports a passphrase that does not fit as a failed
            // decryption, so that is what this means here.
            age::DecryptError::DecryptionFailed => wrong_passphrase(),
            other => map_decrypt_error(other),
        })?;
    Ok(Box::new(reader))
}

fn wrong_passphrase() -> Error {
    Error::new(ErrorKind::WrongKey, "the passphrase is wrong")
}

/// Turns an error of the `age` crate into one of ours.
pub(crate) fn map_decrypt_error(err: age::DecryptError) -> Error {
    use age::DecryptError as E;
    match err {
        E::KeyDecryptionFailed | E::NoMatchingKeys => wrong_passphrase(),
        E::ExcessiveWork { .. } => Error::new(
            ErrorKind::Refused,
            "this file asks for far more passphrase work than fastlock accepts",
        ),
        E::DecryptionFailed | E::InvalidHeader | E::InvalidMac => Error::new(
            ErrorKind::Damaged,
            "the file is damaged or was changed: authentication failed",
        ),
        E::UnknownFormat => Error::new(
            ErrorKind::Unsupported,
            "this age file uses a format version that is not supported",
        ),
        E::Io(io) => {
            Error::new(ErrorKind::Other, format!("cannot read the file: {io}")).with_source(io)
        }
        #[allow(unreachable_patterns)]
        other => Error::new(ErrorKind::Other, format!("cannot decrypt: {other}")),
    }
}
