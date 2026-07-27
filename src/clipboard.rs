//! Yanking via OSC 52.
//!
//! An escape sequence rather than a clipboard library: no X11 or Wayland
//! dependency, and it works over SSH. In tmux this needs
//! `set -g set-clipboard on`.

use std::io::{self, Write};

/// Above this many bytes the sequence is truncated. Terminals vary in what
/// they accept and several choke well before a megabyte.
pub const MAX_BYTES: usize = 64 * 1024;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let indices = [
            (bits >> 18) & 0x3f,
            (bits >> 12) & 0x3f,
            (bits >> 6) & 0x3f,
            bits & 0x3f,
        ];
        for (position, index) in indices.iter().enumerate() {
            if position <= chunk.len() {
                out.push(char::from(ALPHABET[*index as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Truncate on a character boundary so the clipboard never holds broken UTF-8.
fn clip(text: &str) -> (&str, bool) {
    if text.len() <= MAX_BYTES {
        return (text, false);
    }
    let mut end = MAX_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// Build the OSC 52 sequence, reporting whether the text was truncated.
pub fn sequence(text: &str) -> (String, bool) {
    let (text, truncated) = clip(text);
    (
        format!("\u{1b}]52;c;{}\u{7}", base64(text.as_bytes())),
        truncated,
    )
}

/// Emit the sequence to the terminal.
pub fn copy(text: &str) -> io::Result<bool> {
    let (sequence, truncated) = sequence(text);
    let mut out = io::stdout();
    out.write_all(sequence.as_bytes())?;
    out.flush()?;
    Ok(truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_non_ascii() {
        assert_eq!(base64("é".as_bytes()), "w6k=");
    }

    #[test]
    fn sequence_is_well_formed() {
        let (sequence, truncated) = sequence("foo");
        assert_eq!(sequence, "\u{1b}]52;c;Zm9v\u{7}");
        assert!(!truncated);
    }

    #[test]
    fn oversized_text_is_truncated() {
        let text = "a".repeat(MAX_BYTES + 100);
        let (_, truncated) = sequence(&text);
        assert!(truncated);
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        // A multi-byte character straddling the limit must not be split.
        let text = "é".repeat(MAX_BYTES);
        let (clipped, truncated) = clip(&text);
        assert!(truncated);
        assert!(clipped.len() <= MAX_BYTES);
        assert!(clipped.chars().all(|c| c == 'é'), "no broken UTF-8");
    }

    #[test]
    fn short_text_is_untouched() {
        let (clipped, truncated) = clip("hello");
        assert_eq!(clipped, "hello");
        assert!(!truncated);
    }
}
