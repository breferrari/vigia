//! The clipboard write, as the escape the terminal answers.
//!
//! `SPEC.md` §11.1. `crossterm` ships this as `clipboard::CopyToClipboard`
//! behind an `osc52` feature that is `dep:base64`, and `crossterm` is not a
//! declared dependency of this workspace at all: it arrives as
//! `ratatui::crossterm`. Taking the feature would mean declaring one crate in
//! order to add another, to buy the formatting of thirty bytes.

/// The alphabet, which is the standard one rather than the URL-safe one because
/// OSC 52 carries the payload between delimiters that cannot appear in it.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `text`, base64 encoded.
fn encode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        // Padded to three bytes so the shifts below are one expression rather
        // than three arms; how many of the four characters are real is decided
        // after, by the chunk's own length.
        let [a, b, c] = [
            u32::from(chunk[0]),
            chunk.get(1).copied().map_or(0, u32::from),
            chunk.get(2).copied().map_or(0, u32::from),
        ];
        let packed = a << 16 | b << 8 | c;
        for i in 0..4 {
            // A group of three bytes is four characters; two bytes is three and
            // one byte is two, and the rest is `=`.
            if i <= chunk.len() {
                let at = (packed >> (18 - 6 * i)) & 0b11_1111;
                out.push(ALPHABET[at as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The OSC 52 sequence that puts `text` on the terminal's clipboard.
///
/// **There is no reply**, so nothing downstream can learn whether this worked.
/// A caller telling the reader anything may only say what was sent.
pub fn copy(text: &str) -> String {
    format!("\x1b]52;c;{}\x1b\\", encode(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The vectors are `crossterm`'s own, read out of its `clipboard.rs` rather
    /// than computed here, so this asserts agreement with the implementation
    /// that was not taken instead of agreement with itself.
    #[test]
    fn the_sequence_is_the_one_crossterm_would_have_written() {
        assert_eq!(copy("foo"), "\x1b]52;c;Zm9v\x1b\\");
    }

    /// The two remainders, which are where a hand-rolled encoder goes wrong and
    /// where a path length lands two times in three.
    #[test]
    fn both_padding_lengths_are_right() {
        for (text, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(text), want, "{text:?} encoded wrong");
        }
    }

    /// A path is not ASCII in general, and the encoder walks bytes rather than
    /// characters, which is the distinction that decides whether it is right.
    #[test]
    fn a_multibyte_path_survives_the_round_trip() {
        for text in ["src/café.rs", "日本語/ファイル.rs", "a/b\u{200b}c.rs"] {
            let encoded = encode(text);
            assert!(
                encoded.is_ascii(),
                "{text:?} encoded to something the escape cannot carry"
            );
            assert_eq!(
                decode(&encoded),
                text.as_bytes(),
                "{text:?} did not survive"
            );
        }
    }

    /// Test-only, and the reason this file has no decoder: nothing in the shell
    /// ever reads a clipboard back, which §11.1 calls a privacy escalation past
    /// the affordance.
    fn decode(text: &str) -> Vec<u8> {
        let bits: Vec<u32> = text
            .bytes()
            .filter(|b| *b != b'=')
            .map(|b| ALPHABET.iter().position(|a| *a == b).expect("in alphabet") as u32)
            .collect();
        let mut out = Vec::new();
        for chunk in bits.chunks(4) {
            let mut packed = 0u32;
            for (i, six) in chunk.iter().enumerate() {
                packed |= six << (18 - 6 * i);
            }
            for i in 0..chunk.len() - 1 {
                out.push(((packed >> (16 - 8 * i)) & 0xff) as u8);
            }
        }
        out
    }
}
