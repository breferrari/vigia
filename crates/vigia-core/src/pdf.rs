//! How long a PDF's streams decode to, measured without holding them.
//! `hayro` holds every stream it decodes whole, and a megabyte of a PDF can
//! decode to a gigabyte, so nothing reaches it before it is measured.

use std::io::Read;

/// How many bytes of decoded stream the frame path lets one PDF hold.
#[doc(hidden)]
pub const FRAME_BUDGET: u64 = 16 * 1024 * 1024;

/// How long `raw`, zlib-compressed, decodes to, or `None` past `cap`. Read
/// through a sink, so nothing is held. A stream that stops decoding counts
/// what it gave.
#[doc(hidden)]
pub fn inflated_length(raw: &[u8], cap: u64) -> Option<u64> {
    let mut decoder = flate2::read::ZlibDecoder::new(raw);
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    while let Ok(read @ 1..) = decoder.read(&mut buffer) {
        total += read as u64;
        if total > cap {
            return None;
        }
    }
    Some(total)
}

/// Whether every stream in `bytes` decodes within `budget` together. Each
/// stream is measured but an image, which the frame path never decodes; one
/// whose dictionary does not read as one is measured, and one with a filter
/// other than Flate that is not an image fails.
#[doc(hidden)]
pub fn streams_fit(bytes: &[u8], budget: u64) -> bool {
    let mut left = budget;
    for at in memchr::memmem::find_iter(bytes, b"stream") {
        let Some(data) = stream_start(bytes, at) else {
            continue;
        };
        let tokens = tokens(&normalised(dictionary_before(bytes, at)));
        if is_image(&tokens) {
            continue;
        }
        let filters = filters_of(&tokens);
        let decoded = match filters.as_slice() {
            [] => Some(0),
            [only] if only == "FlateDecode" || only == "Fl" => {
                inflated_length(&bytes[data..], left)
            }
            _ => None,
        };
        match decoded {
            Some(length) => left -= length.min(left),
            None => return false,
        }
    }
    true
}

/// Where a stream's data starts, when `stream` at `at` is the keyword: after
/// `>>`, and followed by an end of line.
fn stream_start(bytes: &[u8], at: usize) -> Option<usize> {
    let before = bytes[..at]
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())?;
    if *before != b'>' {
        return None;
    }
    match bytes.get(at + 6)? {
        b'\r' if bytes.get(at + 7) == Some(&b'\n') => Some(at + 8),
        b'\r' | b'\n' => Some(at + 7),
        _ => None,
    }
}

/// The text of the dictionary a stream keyword at `at` closes: back to the
/// object's `obj`, or as far as 64 KiB.
fn dictionary_before(bytes: &[u8], at: usize) -> &[u8] {
    let floor = at.saturating_sub(64 * 1024);
    let window = &bytes[floor..at];
    let start = window
        .windows(3)
        .rposition(|word| word == b"obj")
        .map_or(0, |found| found + 3);
    &window[start..]
}

/// `dict` with its comments gone and every `#xx` name escape read, as one
/// string of tokens.
fn normalised(dict: &[u8]) -> String {
    let mut out = Vec::with_capacity(dict.len());
    let mut bytes = dict.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'%' => {
                while bytes
                    .peek()
                    .is_some_and(|next| !matches!(next, b'\n' | b'\r'))
                {
                    bytes.next();
                }
                out.push(b' ');
            }
            b'#' => {
                let high = bytes.next();
                let low = bytes.next();
                let digit = |byte: Option<u8>| (byte? as char).to_digit(16);
                match (digit(high), digit(low)) {
                    (Some(high), Some(low)) => out.push((high * 16 + low) as u8),
                    _ => out.push(b'#'),
                }
            }
            // A name ends at a delimiter, so one is spaced from the next.
            b'/' | b'[' | b']' | b'<' | b'>' | b'(' | b')' => {
                out.extend_from_slice(&[b' ', byte, b' ']);
            }
            _ => out.push(byte),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The tokens of a normalised dictionary, names without their slash joined
/// back to it.
fn tokens(dict: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut words = dict.split_ascii_whitespace().peekable();
    while let Some(word) = words.next() {
        if word == "/" {
            if let Some(name) = words.next() {
                tokens.push(format!("/{name}"));
            }
        } else {
            tokens.push(word.to_owned());
        }
    }
    tokens
}

fn is_image(tokens: &[String]) -> bool {
    tokens
        .windows(2)
        .any(|pair| pair[0] == "/Subtype" && pair[1] == "/Image")
}

/// The filters a dictionary names, without their slashes.
fn filters_of(tokens: &[String]) -> Vec<String> {
    let Some(at) = tokens.iter().position(|token| token == "/Filter") else {
        return Vec::new();
    };
    match tokens.get(at + 1).map(String::as_str) {
        Some("[") => tokens[at + 2..]
            .iter()
            .take_while(|token| *token != "]")
            .map(|token| token.trim_start_matches('/').to_owned())
            .collect(),
        Some(name) if name.starts_with('/') => vec![name[1..].to_owned()],
        // A reference or anything else: not a filter this reads.
        Some(other) => vec![other.to_owned()],
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_names_read() {
        let dict = normalised(b"<< /Type/Obj#53tm % a comment\n /Filter /Fl#61teDecode >>");
        assert!(tokens(&dict).contains(&"/ObjStm".to_owned()), "{dict}");
        assert_eq!(filters_of(&tokens(&dict)), ["FlateDecode"]);
        assert!(is_image(&tokens(&normalised(b"<</Subtype/Im#61ge>>"))));
        assert!(!is_image(&tokens(&normalised(b"<< /Subtype /Form >>"))));
    }
}
