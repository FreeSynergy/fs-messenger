//! Which of the shapes a mail has: [`classify`].
//!
//! Pattern: Specification. Only the ROOT decides. A plain mail with an encrypted part inside is plain:
//! counting it as encrypted would let a sender add readable text around a real ciphertext and have it
//! marked `openpgp` (the "partial encryption" attack). Inline PGP stays plain text, for the same reason.

use mail_parser::{MessageParser, PartKind};

/// What a mail looks like from the outside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// No end-to-end structure at the root.
    Plain,
    /// `multipart/signed` at the root (RFC 3156 section 5).
    Signed {
        /// The signed part, as written, headers included.
        content: Vec<u8>,
        /// The armored signature.
        signature: Vec<u8>,
    },
    /// `multipart/encrypted` at the root with the two parts of RFC 3156 section 4.
    Encrypted {
        /// The armored OpenPGP message.
        armored: Vec<u8>,
    },
    /// Says it is encrypted, but has not the two parts.
    MalformedEncrypted,
    /// Not even a mail.
    Unparsable,
}

/// Find the shape of `raw`.
pub fn classify(raw: &[u8]) -> Shape {
    let Some(parsed) = MessageParser::new().parse(raw) else {
        return Shape::Unparsable;
    };
    let root = parsed.root().root_part();
    if !matches!(root.kind(), PartKind::Multipart) {
        return Shape::Plain;
    }
    let children: Vec<_> = root.children().collect();
    let protocol = root.content_type().and_then(|c| c.attribute("protocol"));
    if root.is_content_type("multipart", "encrypted") {
        return match children.as_slice() {
            [control, data]
                if protocol.is_some_and(|p| p.eq_ignore_ascii_case("application/pgp-encrypted"))
                    && control.is_content_type("application", "pgp-encrypted") =>
            {
                Shape::Encrypted {
                    armored: data.decoded().into_owned(),
                }
            }
            _ => Shape::MalformedEncrypted,
        };
    }
    if root.is_content_type("multipart", "signed")
        && protocol.is_some_and(|p| p.eq_ignore_ascii_case("application/pgp-signature"))
        && let [content, signature] = children.as_slice()
    {
        return Shape::Signed {
            content: content.raw().to_vec(),
            signature: signature.decoded().into_owned(),
        };
    }
    Shape::Plain
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCRYPTED_INSIDE_PLAIN: &[u8] = b"From: a@b\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n\
--x\r\nContent-Type: text/plain\r\n\r\nI am readable\r\n--x\r\nContent-Type: multipart/encrypted; \
protocol=\"application/pgp-encrypted\"; boundary=y\r\n\r\n--y\r\nContent-Type: application/pgp-encrypted\
\r\n\r\nVersion: 1\r\n--y\r\nContent-Type: application/octet-stream\r\n\r\nxx\r\n--y--\r\n--x--\r\n";

    #[test]
    fn only_the_root_counts() {
        assert_eq!(classify(ENCRYPTED_INSIDE_PLAIN), Shape::Plain);
        assert_eq!(classify(b"From: a@b\r\n\r\ntext\r\n"), Shape::Plain);
        let inline = b"From: a@b\r\n\r\n-----BEGIN PGP MESSAGE-----\r\nxx\r\n-----END PGP MESSAGE-----\r\n";
        assert_eq!(classify(inline), Shape::Plain);
    }

    #[test]
    fn encrypted_needs_its_two_parts() {
        let one = b"From: a@b\r\nContent-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; \
boundary=y\r\n\r\n--y\r\nContent-Type: application/octet-stream\r\n\r\nxx\r\n--y--\r\n";
        assert_eq!(classify(one), Shape::MalformedEncrypted);
    }
}
