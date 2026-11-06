//! The `Autocrypt` header: reading and writing it.
//!
//! Pattern: Adapter between the header text and [`AutocryptHeader`]. Rules of Autocrypt 1.1: `addr` and
//! `keydata` are required, an unknown attribute that does not start with `_` voids the header, and it
//! counts only if `addr` is the sender. Provisional: v6 certificates in a v1 header; the v2 draft
//! (`draft-autocrypt-openpgp-v2-cert-03`) is not final. `prefer-encrypt` is only carried: every mail
//! here is encrypted anyway. Autocrypt-Gossip is `Unsupported`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::address::{clean_address, normalise_address};
use crate::error::{MailError, Result};
use crate::openpgp::{MAX_CERT_BYTES, PublicCert};

const MAX_KEYDATA: usize = MAX_CERT_BYTES;
const FOLD_AT: usize = 76;

/// A parsed `Autocrypt` header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutocryptHeader {
    /// The address, normalised.
    pub address: String,
    /// The certificate, binary.
    pub keydata: Vec<u8>,
    /// `prefer-encrypt=mutual`.
    pub prefers_mutual: bool,
}

impl AutocryptHeader {
    /// Read one header value; `None` if it breaks a rule.
    pub fn parse(value: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(value).ok()?;
        let (mut address, mut keydata, mut prefers_mutual) = (None, None, false);
        for attribute in text.split(';') {
            let (name, value) = attribute.split_once('=')?;
            match name.trim() {
                "addr" => address = Some(normalise_address(value)),
                "prefer-encrypt" => prefers_mutual = value.trim() == "mutual",
                "keydata" => {
                    let compact: String = value.split_whitespace().collect();
                    if compact.len() > MAX_KEYDATA * 4 / 3 + 4 {
                        return None;
                    }
                    keydata = Some(STANDARD.decode(compact).ok()?);
                }
                other if other.starts_with('_') => {}
                _ => return None,
            }
        }
        Some(Self {
            address: address.filter(|a: &String| !a.is_empty() && clean_address(a))?,
            keydata: keydata.filter(|k: &Vec<u8>| !k.is_empty())?,
            prefers_mutual,
        })
    }
}

/// The one offer of `sender` among raw header values. Several for the sender count as none, as
/// Autocrypt 1.1 says: nobody can tell which one is meant.
pub fn offered_for<'a>(
    values: impl Iterator<Item = &'a [u8]>,
    sender: &str,
) -> Option<AutocryptHeader> {
    let sender = normalise_address(sender);
    let mut found: Vec<_> = values
        .filter_map(AutocryptHeader::parse)
        .filter(|h| h.address == sender)
        .collect();
    if found.len() == 1 { found.pop() } else { None }
}

/// The header value for our own key, folded so that it fits the line limit.
pub fn format_header(address: &str, cert: &PublicCert) -> Result<String, MailError> {
    let data = STANDARD.encode(cert.to_binary()?);
    let lines: Vec<&str> = data
        .as_bytes()
        .chunks(FOLD_AT)
        .map(|c| std::str::from_utf8(c).map_err(|e| MailError::Mime(e.to_string())))
        .collect::<Result<_>>()?;
    Ok(format!(
        "addr={}; prefer-encrypt=mutual; keydata={}",
        normalise_address(address),
        lines.join(" ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_of_autocrypt() {
        let ok = b"addr=Bob@Example.org; prefer-encrypt=mutual; keydata=QUJD RA==";
        let h = AutocryptHeader::parse(ok).unwrap();
        assert_eq!(h.address, "bob@example.org");
        assert_eq!(h.keydata, b"ABCD");
        assert!(h.prefers_mutual);
        assert!(AutocryptHeader::parse(b"addr=a@b; keydata=QUJD; _x=1").is_some());
        assert!(AutocryptHeader::parse(b"addr=a@b; keydata=QUJD; unknown=1").is_none());
        assert!(AutocryptHeader::parse(b"keydata=QUJD").is_none());
        assert!(AutocryptHeader::parse(b"addr=a@b").is_none());
        assert!(AutocryptHeader::parse(b"addr=a@b; keydata=not base64!").is_none());
        assert!(AutocryptHeader::parse(b"addr=a\x01@b; keydata=QUJD").is_none());
        assert!(AutocryptHeader::parse(b"addr=a\nb@c; keydata=QUJD").is_none());
    }

    #[test]
    fn only_a_single_offer_for_the_sender_counts() {
        let one: &[u8] = b"addr=a@b; keydata=QUJD";
        let other: &[u8] = b"addr=x@y; keydata=QUJD";
        assert!(offered_for([one, other].into_iter(), "a@b").is_some());
        assert!(offered_for([other].into_iter(), "a@b").is_none());
        assert!(offered_for([one, one].into_iter(), "a@b").is_none());
    }
}
