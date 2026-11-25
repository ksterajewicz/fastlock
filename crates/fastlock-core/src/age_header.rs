//! Reading the header of an age file, without a key.
//!
//! This is parsing only. Nothing here decrypts or authenticates anything. The
//! format is described in the age specification (C2SP `age.md`): a version
//! line, one stanza per recipient, and a final line that starts with `---`.

/// The first line of every age file.
pub(crate) const VERSION_LINE: &[u8] = b"age-encryption.org/v1";

/// The first line of an armored (text) age file.
pub(crate) const ARMOR_BEGIN: &[u8] = b"-----BEGIN AGE ENCRYPTED FILE-----";

/// The largest header this code is willing to read.
pub(crate) const MAX_HEADER: usize = 1 << 20;

/// One recipient stanza: its type and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stanza {
    pub tag: String,
    pub args: Vec<String>,
}

/// A parsed header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedHeader {
    pub stanzas: Vec<Stanza>,
    /// The number of bytes the header takes, including its last newline.
    pub header_len: u64,
}

/// Why a header could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderError {
    /// The data does not start with the age version line.
    NotAge,
    /// The data ends before the header does.
    Incomplete,
    /// The header is not laid out as the format requires.
    Malformed,
}

/// Whether the data starts like an armored age file.
pub(crate) fn is_armored(data: &[u8]) -> bool {
    data.starts_with(ARMOR_BEGIN)
}

/// Whether the data starts like a binary age file.
pub(crate) fn is_binary_age(data: &[u8]) -> bool {
    data.starts_with(VERSION_LINE)
}

/// Parses a binary age header from the start of `data`.
///
/// `data` may hold more than the header. It may also be a prefix of the file,
/// in which case a header that does not fit gives [`HeaderError::Incomplete`].
pub(crate) fn parse(data: &[u8]) -> Result<ParsedHeader, HeaderError> {
    if !is_binary_age(data) {
        return Err(HeaderError::NotAge);
    }

    let mut stanzas = Vec::new();
    let mut offset = 0usize;
    let mut first = true;

    loop {
        let rest = &data[offset..];
        let Some(end) = rest.iter().position(|&b| b == b'\n') else {
            return Err(HeaderError::Incomplete);
        };
        let line = &rest[..end];
        offset += end + 1;

        if first {
            if line != VERSION_LINE {
                return Err(HeaderError::NotAge);
            }
            first = false;
            continue;
        }

        if let Some(args) = line.strip_prefix(b"-> ") {
            let text = std::str::from_utf8(args).map_err(|_| HeaderError::Malformed)?;
            let mut parts = text.split(' ');
            let tag = parts.next().unwrap_or("");
            if tag.is_empty() || !text.is_ascii() {
                return Err(HeaderError::Malformed);
            }
            stanzas.push(Stanza {
                tag: tag.to_string(),
                args: parts.map(str::to_string).collect(),
            });
        } else if line.starts_with(b"---") {
            if stanzas.is_empty() {
                return Err(HeaderError::Malformed);
            }
            return Ok(ParsedHeader {
                stanzas,
                header_len: offset as u64,
            });
        } else if stanzas.is_empty() {
            // A body line before any stanza.
            return Err(HeaderError::Malformed);
        }
        // Otherwise: a body line of the current stanza. Its content is not needed.
    }
}

/// Decodes the start of an armored age file into binary data.
///
/// Stops at the end marker, at the end of `text`, or once `limit` bytes have
/// been decoded. Returns `None` when a line is not valid base64.
pub(crate) fn decode_armor_prefix(text: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut lines = text.split(|&b| b == b'\n');
    let first = lines.next()?;
    if trim_cr(first) != ARMOR_BEGIN {
        return None;
    }
    for line in lines {
        let line = trim_cr(line);
        if line.starts_with(b"-----END") {
            break;
        }
        if line.is_empty() {
            continue;
        }
        decode_base64_line(line, &mut out)?;
        if out.len() >= limit {
            break;
        }
    }
    Some(out)
}

fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn base64_value(byte: u8) -> Option<u32> {
    match byte {
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes one line of standard, padded base64 and appends the bytes to `out`.
fn decode_base64_line(line: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let (groups, rest) = line.as_chunks::<4>();
    if !rest.is_empty() {
        return None;
    }
    for (index, group) in groups.iter().enumerate() {
        let last = index + 1 == groups.len();
        let padding = group.iter().rev().take_while(|&&b| b == b'=').count();
        if padding > 2 || (padding > 0 && !last) {
            return None;
        }
        let mut value = 0u32;
        for &byte in &group[..4 - padding] {
            value = (value << 6) | base64_value(byte)?;
        }
        value <<= 6 * padding as u32;
        let bytes = [(value >> 16) as u8, (value >> 8) as u8, value as u8];
        out.extend_from_slice(&bytes[..3 - padding]);
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"age-encryption.org/v1\n\
-> X25519 abcDEF\n\
c29tZSBib2R5\n\
-> scrypt c2FsdA 18\n\
Ym9keQ\n\
--- bWFj\n\
PAYLOAD";

    #[test]
    fn parses_stanzas_and_header_length() {
        let header = parse(SAMPLE).expect("header parses");
        assert_eq!(header.stanzas.len(), 2);
        assert_eq!(header.stanzas[0].tag, "X25519");
        assert_eq!(header.stanzas[0].args, vec!["abcDEF".to_string()]);
        assert_eq!(header.stanzas[1].tag, "scrypt");
        assert_eq!(
            header.stanzas[1].args,
            vec!["c2FsdA".to_string(), "18".to_string()]
        );
        let payload_at = SAMPLE.len() - b"PAYLOAD".len();
        assert_eq!(header.header_len as usize, payload_at);
    }

    #[test]
    fn rejects_data_that_is_not_age() {
        assert_eq!(parse(b"PK\x03\x04 not age"), Err(HeaderError::NotAge));
        assert_eq!(parse(b"age-encryption.org/v2\n"), Err(HeaderError::NotAge));
    }

    #[test]
    fn reports_a_header_that_is_cut_short() {
        let cut = &SAMPLE[..40];
        assert_eq!(parse(cut), Err(HeaderError::Incomplete));
    }

    #[test]
    fn rejects_a_header_without_recipients() {
        assert_eq!(
            parse(b"age-encryption.org/v1\n--- bWFj\n"),
            Err(HeaderError::Malformed)
        );
    }

    #[test]
    fn decodes_base64_with_and_without_padding() {
        let mut out = Vec::new();
        decode_base64_line(b"aGVsbG8=", &mut out).expect("valid base64");
        assert_eq!(out, b"hello");
        out.clear();
        decode_base64_line(b"aGVsbG8h", &mut out).expect("valid base64");
        assert_eq!(out, b"hello!");
        assert!(decode_base64_line(b"a===", &mut Vec::new()).is_none());
        assert!(decode_base64_line(b"ab!d", &mut Vec::new()).is_none());
    }

    #[test]
    fn decodes_the_start_of_an_armored_file() {
        // "age-encryption.org/v1\n" encoded as base64.
        let armored = b"-----BEGIN AGE ENCRYPTED FILE-----\n\
YWdlLWVuY3J5cHRpb24ub3JnL3YxCg==\n\
-----END AGE ENCRYPTED FILE-----\n";
        assert!(is_armored(armored));
        let decoded = decode_armor_prefix(armored, MAX_HEADER).expect("valid armor");
        assert_eq!(decoded, b"age-encryption.org/v1\n");
    }
}
