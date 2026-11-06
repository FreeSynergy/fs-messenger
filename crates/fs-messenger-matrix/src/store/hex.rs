//! Bytes as text, for the few binary values in a journal.
//!
//! Pattern: Strategy (pure functions): small values, so hex is enough and needs no dependency.

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// As lower-case hex.
pub(crate) fn encode_bytes(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        text.push(char::from(DIGITS[usize::from(b >> 4)]));
        text.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    text
}

/// Back from hex. `None` if it is not hex.
pub(crate) fn decode_bytes(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let pairs = (0..text.len()).step_by(2);
    pairs
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_and_back() {
        assert_eq!(encode_bytes(&[0, 15, 255]), "000fff");
        assert_eq!(decode_bytes("000fff"), Some(vec![0, 15, 255]));
        assert_eq!(decode_bytes("0"), None);
        assert_eq!(decode_bytes("zz"), None);
    }
}
