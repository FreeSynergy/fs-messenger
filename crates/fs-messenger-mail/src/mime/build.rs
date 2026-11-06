//! Building an outgoing mail: the inner payload and the outer PGP/MIME wrapper.
//!
//! Pattern: Builder (over `mail-builder`). Header Protection per RFC 9788: the real headers sit in the
//! encrypted payload, with `hp="cipher"` on its root and one `HP-Outer` per field that went outside. We
//! use the baseline policy: the outer `Subject` is the placeholder, and addressing stays where servers
//! need it. No legacy display part. Bcc is in no header, inner or outer. Line ends are CRLF throughout.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fs_messenger_core::OutgoingAttachment;
use mail_builder::MessageBuilder;
use mail_builder::headers::address::Address;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::date::Date;
use mail_builder::headers::raw::Raw;
use mail_builder::mime::MimePart;

use crate::error::{MailError, Result};

/// The outer `Subject` of every encrypted mail (RFC 9788 section 4.3).
pub const OUTER_SUBJECT: &str = "[...]";

/// What goes into a mail.
pub struct PayloadInput<'a> {
    /// The sender's address.
    pub from: &'a str,
    /// `To`.
    pub to: &'a [String],
    /// `Cc`.
    pub cc: &'a [String],
    /// The real subject; empty if the person gave none.
    pub subject: &'a str,
    /// `Message-ID`, without brackets.
    pub message_id: &'a str,
    /// The parent's id.
    pub in_reply_to: Option<&'a str>,
    /// `References`.
    pub references: &'a [String],
    /// `Date`, seconds since the epoch.
    pub date: i64,
    /// The text.
    pub text: &'a str,
    /// Media type of the text.
    pub media_type: &'a str,
    /// The files.
    pub attachments: &'a [OutgoingAttachment],
    /// The value of our `Autocrypt` header.
    pub autocrypt: &'a str,
}

fn list(addresses: &[String]) -> Address<'static> {
    Address::new_list(
        addresses
            .iter()
            .map(|a| Address::new_address(None::<String>, a.clone()))
            .collect(),
    )
}

/// The fields both forms share.
fn addressed<'a>(input: &'a PayloadInput<'a>, subject: &'a str) -> MessageBuilder<'a> {
    let mut b = MessageBuilder::new()
        .from(input.from)
        .message_id(input.message_id)
        .date(input.date)
        .header("Autocrypt", Raw::new(input.autocrypt));
    if !subject.is_empty() {
        b = b.subject(subject);
    }
    if !input.to.is_empty() {
        b = b.to(list(input.to));
    }
    if !input.cc.is_empty() {
        b = b.cc(list(input.cc));
    }
    if let Some(parent) = input.in_reply_to {
        b = b.in_reply_to(parent);
    }
    if !input.references.is_empty() {
        let refs: Vec<&str> = input.references.iter().map(String::as_str).collect();
        b = b.references(refs);
    }
    b
}

/// Base64 with our own line ends, so that a file comes back byte for byte, also a text file.
fn file_part(file: &OutgoingAttachment) -> MimePart<'_> {
    let encoded = STANDARD.encode(&file.data);
    let wrapped: Vec<&str> = encoded
        .as_bytes()
        .chunks(76)
        .filter_map(|c| std::str::from_utf8(c).ok())
        .collect();
    MimePart::new(file.media_type.as_str(), wrapped.join("\r\n").into_bytes())
        .attachment(file.name.as_str())
        .transfer_encoding("base64")
}

/// `Date` as `mail-builder` writes it outside, so that the `HP-Outer` line says what is really there.
fn date_text(date: i64) -> String {
    let mut out = Vec::new();
    Date::new(date).write_rfc822(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn written(builder: MessageBuilder<'_>) -> Result<Vec<u8>> {
    builder
        .write_to_vec()
        .map_err(|e| MailError::Mime(e.to_string()))
}

/// The cryptographic payload: everything secret, in plain MIME, ready to be encrypted.
pub fn build_payload(input: &PayloadInput<'_>) -> Result<Vec<u8>, MailError> {
    let mut parts = vec![MimePart::new(input.media_type, input.text)];
    parts.extend(input.attachments.iter().map(file_part));
    let root = ContentType::new("multipart/mixed").attribute("hp", "cipher");
    let mut b = addressed(input, input.subject)
        .header(
            "HP-Outer",
            Raw::new(format!("Date: {}", date_text(input.date))),
        )
        .header("HP-Outer", Raw::new(format!("From: {}", input.from)))
        .header("HP-Outer", Raw::new(format!("Subject: {OUTER_SUBJECT}")))
        .header(
            "HP-Outer",
            Raw::new(format!("Message-ID: <{}>", input.message_id)),
        );
    if !input.to.is_empty() {
        b = b.header("HP-Outer", Raw::new(format!("To: {}", input.to.join(", "))));
    }
    if !input.cc.is_empty() {
        b = b.header("HP-Outer", Raw::new(format!("Cc: {}", input.cc.join(", "))));
    }
    if let Some(parent) = input.in_reply_to {
        b = b.header("HP-Outer", Raw::new(format!("In-Reply-To: <{parent}>")));
    }
    if !input.references.is_empty() {
        let refs: Vec<String> = input.references.iter().map(|r| format!("<{r}>")).collect();
        b = b.header(
            "HP-Outer",
            Raw::new(format!("References: {}", refs.join(" "))),
        );
    }
    written(b.body(MimePart::new(root, parts)))
}

/// The mail as servers see it: RFC 3156 section 4 around the armored OpenPGP message.
pub fn build_outer(input: &PayloadInput<'_>, armored: &str) -> Result<Vec<u8>, MailError> {
    let crlf = armored.replace("\r\n", "\n").replace('\n', "\r\n");
    let root =
        ContentType::new("multipart/encrypted").attribute("protocol", "application/pgp-encrypted");
    let control = MimePart::new(
        "application/pgp-encrypted",
        "Version: 1\r\n".as_bytes().to_vec(),
    )
    .transfer_encoding("7bit");
    let data = MimePart::new(
        ContentType::new("application/octet-stream").attribute("name", "encrypted.asc"),
        crlf.into_bytes(),
    )
    .inline()
    .transfer_encoding("7bit");
    written(addressed(input, OUTER_SUBJECT).body(MimePart::new(root, vec![control, data])))
}
