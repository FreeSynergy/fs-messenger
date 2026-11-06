//! [`MailFields`]: what the index and the display need, with the names and meaning of JMAP `Email`
//! (RFC 8621): `messageId`, `inReplyTo`, `references`, `from`, `to`, `cc`, `subject`, `sentAt`,
//! `textBody`, `attachments`.
//!
//! Pattern: Adapter from `mail-parser` to owned value objects. The original stays apart, untouched
//! (ADR 0030); everything here can be derived again from it. HTML stays out: the core's `bodyHtml` is
//! empty until there is a sanitiser, and an HTML-only mail shows `mail-parser`'s text form.

use chrono::{DateTime, TimeZone, Utc};
use mail_parser::{MessageParser, MessageRef};

/// A file in a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentPart {
    /// File name; a neutral one if the sender gave none.
    pub name: String,
    /// Media type.
    pub media_type: String,
    /// The decoded bytes.
    pub data: Vec<u8>,
}

/// The extracted fields of one message or payload.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailFields {
    /// `Message-ID`, without angle brackets.
    pub message_id: Option<String>,
    /// `In-Reply-To`.
    pub in_reply_to: Vec<String>,
    /// `References`.
    pub references: Vec<String>,
    /// First address in `From`, without its name.
    pub from: Option<String>,
    /// `To`.
    pub to: Vec<String>,
    /// `Cc`.
    pub cc: Vec<String>,
    /// `Subject`.
    pub subject: Option<String>,
    /// `Date`.
    pub sent_at: Option<DateTime<Utc>>,
    /// Text of all `textBody` parts, line ends normalised to `\n`.
    pub text_body: String,
    /// `text/plain`, or `text/markdown` if that is what the sender says.
    pub text_media_type: String,
    /// Files.
    pub attachments: Vec<AttachmentPart>,
    /// Raw values of the `Autocrypt` headers.
    pub autocrypt: Vec<Vec<u8>>,
    /// Whether the root part declares Header Protection (RFC 9788): `hp` is `cipher` or `clear`.
    pub header_protected: bool,
}

fn addresses(list: Option<mail_parser::AddressList<'_>>) -> Vec<String> {
    list.map_or_else(Vec::new, |l| {
        l.mailboxes()
            .filter_map(|m| m.address().map(str::to_string))
            .collect()
    })
}

fn ids(list: Option<mail_parser::TextList<'_>>) -> Vec<String> {
    list.map_or_else(Vec::new, |l| l.iter().map(str::to_string).collect())
}

fn media_type(part: mail_parser::MessagePart<'_>) -> String {
    part.content_type().map_or_else(
        || "application/octet-stream".to_string(),
        |c| match c.subtype() {
            Some(sub) => format!("{}/{sub}", c.ctype()).to_lowercase(),
            None => c.ctype().to_lowercase(),
        },
    )
}

/// The detached signature of a `multipart/signed` is no file of the person's.
fn is_signature(part: mail_parser::MessagePart<'_>) -> bool {
    part.is_content_type("application", "pgp-signature")
}

fn attachments(message: MessageRef<'_>) -> Vec<AttachmentPart> {
    message
        .attachments()
        .filter(|p| !is_signature(*p))
        .map(|p| AttachmentPart {
            name: p.attachment_name().unwrap_or("attachment").to_string(),
            media_type: media_type(p),
            data: p.decoded().into_owned(),
        })
        .collect()
}

impl MailFields {
    /// Read `raw`. `None` if `mail-parser` finds neither a header nor a blank line.
    pub fn read(raw: &[u8]) -> Option<Self> {
        let parsed = MessageParser::new().parse(raw)?;
        let m = parsed.root();
        let text: Vec<String> = m.text_bodies().map(|t| t.replace("\r\n", "\n")).collect();
        let markdown = m.text_body().len() == 1
            && m.text_body()
                .next()
                .is_some_and(|p| p.is_content_type("text", "markdown"));
        Some(Self {
            message_id: m.message_id().map(str::to_string),
            in_reply_to: ids(m.in_reply_to()),
            references: ids(m.references()),
            from: addresses(m.from()).into_iter().next(),
            to: addresses(m.to()),
            cc: addresses(m.cc()),
            subject: m.subject().map(str::to_string),
            sent_at: m
                .date()
                .and_then(|d| Utc.timestamp_opt(d.to_timestamp(), 0).single()),
            text_body: text.join("\n"),
            text_media_type: if markdown {
                "text/markdown"
            } else {
                "text/plain"
            }
            .to_string(),
            attachments: attachments(m),
            autocrypt: m
                .headers()
                .all("Autocrypt")
                .map(|h| h.raw_value().to_vec())
                .collect(),
            // RFC 9788 section 4.1 knows these two values; any other declares nothing.
            header_protected: m
                .content_type()
                .and_then(|c| c.attribute("hp"))
                .is_some_and(|hp| hp == "cipher" || hp == "clear"),
        })
    }
}

/// The id that names the thread: the first in `References`, else the parent, else the message itself.
/// Works the same over IMAP and JMAP, unlike a server's `threadId`. Provisional: a stranger who cites
/// an id joins that thread; threading is display, and says nothing about the sender.
pub fn thread_root(fields: &MailFields) -> Option<&str> {
    fields
        .references
        .first()
        .or(fields.in_reply_to.first())
        .or(fields.message_id.as_ref())
        .map(String::as_str)
}

/// Whether `raw` begins with a clean header block: every line up to the first empty one is a field or the
/// continuation of one. `mail-parser` is lenient, as it should be for mail from the wild: it skips a line
/// that is no header and reads on, which turns `From:` lines in prose into headers.
fn clean_header_block(raw: &[u8]) -> bool {
    let mut seen_field = false;
    for line in raw.split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            return true;
        }
        if line[0] == b' ' || line[0] == b'\t' {
            if !seen_field {
                return false;
            }
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            return false;
        };
        let name = &line[..colon];
        if name.is_empty() || !name.iter().all(|b| b.is_ascii_graphic()) {
            return false;
        }
        seen_field = true;
    }
    true
}

impl MailFields {
    /// Read a part a signature covers, where a header means something. Without a clean header block it
    /// has no headers at all: it is text, all of it, and what looks like a `From:` in there is prose.
    pub fn read_covered(raw: &[u8]) -> Option<Self> {
        if clean_header_block(raw) {
            return Self::read(raw);
        }
        Some(Self {
            text_body: String::from_utf8_lossy(raw).replace("\r\n", "\n"),
            text_media_type: "text/plain".to_string(),
            ..Self::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &[u8] = b"From: Ann <Ann@example.org>\r\nTo: b@example.org, C <c@example.org>\r\n\
Cc: d@example.org\r\nSubject: Hi\r\nDate: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
Message-ID: <own@example.org>\r\nIn-Reply-To: <parent@example.org>\r\n\
References: <root@example.org> <parent@example.org>\r\nContent-Type: text/plain\r\n\r\nline1\r\nline2\r\n";

    #[test]
    fn jmap_fields_and_threading() {
        let f = MailFields::read(RAW).unwrap();
        assert_eq!(f.message_id.as_deref(), Some("own@example.org"));
        assert_eq!(f.in_reply_to, ["parent@example.org"]);
        assert_eq!(f.references, ["root@example.org", "parent@example.org"]);
        assert_eq!(f.from.as_deref(), Some("Ann@example.org"));
        assert_eq!(f.to, ["b@example.org", "c@example.org"]);
        assert_eq!(f.cc, ["d@example.org"]);
        assert_eq!(f.subject.as_deref(), Some("Hi"));
        assert_eq!(f.text_body, "line1\nline2\n");
        assert_eq!(f.sent_at.unwrap().timestamp(), 1_637_446_921);
        assert_eq!(thread_root(&f), Some("root@example.org"));
    }

    #[test]
    fn prose_with_header_lines_has_no_headers() {
        let prose =
            b"Dear all\nFrom: a@b\nSubject: quoted\nAutocrypt: addr=a@b; keydata=QUJD\n\nbye\n";
        let f = MailFields::read_covered(prose).unwrap();
        assert_eq!(f.from, None);
        assert_eq!(f.subject, None);
        assert!(f.autocrypt.is_empty());
        assert!(
            f.text_body.starts_with("Dear all\nFrom: a@b"),
            "nothing is lost"
        );
        for junk in [
            &b"From: a@b\nnot a header\nSubject: s\n\nx"[..],
            b" folded first\n\nx",
            b"Two words: x\n\ny",
        ] {
            assert_eq!(
                MailFields::read_covered(junk).unwrap().subject,
                None,
                "{junk:?}"
            );
            assert_eq!(MailFields::read_covered(junk).unwrap().from, None);
        }
        let clean = b"From: a@b\r\nSubject: s\r\n continued\r\nX-Y: z\r\n\r\nbody";
        assert_eq!(
            MailFields::read_covered(clean).unwrap().from.as_deref(),
            Some("a@b")
        );
    }

    #[test]
    fn the_root_is_the_parent_then_the_message_itself() {
        let mut f = MailFields {
            message_id: Some("own".into()),
            ..MailFields::default()
        };
        assert_eq!(thread_root(&f), Some("own"));
        f.in_reply_to = vec!["parent".into()];
        assert_eq!(thread_root(&f), Some("parent"));
    }
}
