//! Copying to the system clipboard over OSC 52.
//!
//! https://jvns.ca/til/vim-osc52/

use std::env;
use std::io::{self, Write};

/// Terminals cap the length of an OSC 52 payload; xterm's default is a little
/// under 75k of base64. Past that we quietly stay internal rather than emit a
/// sequence that gets truncated into garbage.
const MAX_ENCODED: usize = 74_000;

/// Requests the terminal to put `text` on the system clipboard.
/// The text may fail to send if it exceeds the terminal's maximum payload size, but will always be copied to the internal clipboard.
///
/// Returns `Ok(false)` when the text is too large to send.
pub fn set(text: &str) -> io::Result<bool> {
    let encoded = base64(text.as_bytes());
    if encoded.len() > MAX_ENCODED {
        return Ok(false);
    }
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    let mut out = io::stdout().lock();
    out.write_all(wrap(&sequence).as_bytes())?;
    out.flush()?;
    Ok(true)
}

/// tmux and screen swallow escape sequences they don't recognise unless the
/// sequence is wrapped in their passthrough envelope.
fn wrap(sequence: &str) -> String {
    if env::var_os("TMUX").is_some() {
        // Inner ESCs have to be doubled or tmux ends the passthrough early.
        format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
    } else if env::var("TERM").is_ok_and(|t| t.starts_with("screen")) {
        format!("\x1bP{sequence}\x1b\\")
    } else {
        sequence.to_string()
    }
}

/// Standard base64 with padding. Small enough to write than to depend on.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_high_bytes() {
        assert_eq!(base64("é→".as_bytes()), "w6nihpI=");
        assert_eq!(base64(&[0xff, 0xff, 0xff]), "////");
    }

    #[test]
    fn oversized_payloads_are_refused_not_truncated() {
        let huge = "x".repeat(MAX_ENCODED);
        assert_eq!(set(&huge).unwrap(), false);
    }
}
