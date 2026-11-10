//! The data types of the messenger as `DescribesType`: `Conversation`, `Message`, `Account` and `Draft`.
//!
//! Pattern: Registry entry (trait first, ADR 0043) — each type implements `fs_types::DescribesType`; the commands are
//! not written here but read from the capability table (`table.rs`), so there is one source of truth. The fields have basic
//! types of `fs-types`. Translation keys: `messenger.type.<type>`, `messenger.field.<field>`, `messenger.command.<type>.<command>`
//! (kebab case); choices are `<field label>.<key>`.

use fs_types::{
    BasicType, CommandDescription, DescribesType, FieldDescription, TypeDescription, TypeRegistry,
};

use super::entry::{Delivery, Entry, In};
use super::support::apply_support;
use super::table::table;
use crate::model::{Account, ConversationSummary, Message};

/// The owner program (ADR 0050).
pub const PROGRAM: &str = "fs-messenger";

const YN: &[&str] = &["yes", "no"];
const KINDS: &[&str] = &["mail", "chat", "comment", "unknown"];
const ENCRYPTION: &[&str] = &[
    "mls",
    "openpgp",
    "megolm",
    "transport-only",
    "none",
    "unknown",
];
const STATES: &[&str] = &["sending", "sent", "delivered", "read", "failed", "unknown"];
const SENDERS: &[&str] = &["person", "software", "ai", "unknown"];
const SIGNATURES: &[&str] = &["not-verified", "not-signed", "valid"];
const ACCOUNT_KINDS: &[&str] = &["mail", "matrix", "mimi"];
const FORMATS: &[&str] = &["plain", "markdown"];

/// camelCase to kebab-case, for translation keys.
fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            if !out.is_empty() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The label key of a field or input.
#[must_use]
pub fn field_label(name: &str) -> String {
    format!("messenger.field.{}", kebab(name))
}

/// The label key of a type.
#[must_use]
pub fn type_label(data_type: &str) -> String {
    format!("messenger.type.{}", kebab(data_type))
}

/// The label key of a command.
#[must_use]
pub fn command_label(data_type: &str, command: &str) -> String {
    format!("messenger.command.{}.{}", kebab(data_type), kebab(command))
}

fn field_of(i: &In) -> FieldDescription {
    let mut f = FieldDescription::new(i.name, i.basic).label(&field_label(i.name));
    f.required = i.required;
    f.multiple = i.multiple;
    i.target.clone_into(&mut f.target);
    f.choices = i.choices.iter().map(|c| (*c).to_owned()).collect();
    f
}

fn command_of(data_type: &str, entry: &Entry) -> Option<CommandDescription> {
    let Delivery::Command {
        name,
        kind,
        input,
        result,
    } = entry.delivery
    else {
        return None;
    };
    let mut c = CommandDescription::new(name, kind, &command_label(data_type, name));
    for i in input {
        c = c.input(field_of(i));
    }
    for r in result {
        c = c.result(field_of(r));
    }
    Some(apply_support(c, entry))
}

/// ALL commands of a type, from the table: the first row of a name defines the command, later rows only add capabilities.
#[must_use]
pub fn commands_of(data_type: &str) -> Vec<CommandDescription> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for e in table().filter(|e| e.data_type == data_type) {
        if let Some(name) = e.command_name()
            && !seen.contains(&name)
        {
            seen.push(name);
            out.extend(command_of(data_type, e));
        }
    }
    out
}

fn build(name: &str, standard: &str, fields: &[In]) -> TypeDescription {
    let mut d = TypeDescription::new(name, PROGRAM, standard, &type_label(name));
    for f in fields {
        d = d.field(field_of(f));
    }
    d.commands = commands_of(name);
    d
}

const MESSAGE_FIELDS: &[In] = &[
    In::text("name"),
    In::text("attributedTo"),
    In::text("to").many(),
    In::text("cc").many(),
    In::new("published", BasicType::DateTime),
    In::to("context", "Conversation"),
    In::to("inReplyTo", "Message"),
    In::to("account", "Account"),
    In::long("content"),
    In::text("preview"),
    In::pick("kind", KINDS),
    In::pick("encryption", ENCRYPTION),
    In::pick("state", STATES),
    In::pick("unread", YN),
    In::pick("flagged", YN),
    In::pick("senderKind", SENDERS),
    In::pick("signature", SIGNATURES),
    In::pick("mediaType", FORMATS),
    In::text("attachments").many(),
    In::text("reactions").many(),
    In::long("toolCalls").many(),
    In::text("jobRef"),
    In::text("taskRefs").many(),
];

const CONVERSATION_FIELDS: &[In] = &[
    In::text("name"),
    In::pick("kind", KINDS),
    In::pick("encryption", ENCRYPTION),
    In::text("unreadCount"),
    In::to("lastMessage", "Message"),
    In::pick("pinned", YN),
    In::text("folders").many(),
];

const ACCOUNT_FIELDS: &[In] = &[
    In::text("address").req(),
    In::pick("kind", ACCOUNT_KINDS),
    In::pick("encryptionReady", YN),
];

const DRAFT_FIELDS: &[In] = &[In::text("to").many(), In::long("text")];

impl DescribesType for ConversationSummary {
    fn describe() -> TypeDescription {
        build(
            "Conversation",
            "AS2 Collection (context of Note), JMAP Thread",
            CONVERSATION_FIELDS,
        )
    }
}

impl DescribesType for Message {
    fn describe() -> TypeDescription {
        build(
            "Message",
            "ActivityStreams 2.0 Note (W3C), RFC 5322 for mail",
            MESSAGE_FIELDS,
        )
    }
}

impl DescribesType for Account {
    fn describe() -> TypeDescription {
        build(
            "Account",
            "ActivityStreams 2.0 Actor, JMAP Identity (RFC 8621)",
            ACCOUNT_FIELDS,
        )
    }
}

/// A draft: written, not sent (`freesynergy.net:Draft`).
pub struct DraftRecord;

impl DescribesType for DraftRecord {
    fn describe() -> TypeDescription {
        build(
            "Draft",
            "ActivityStreams 2.0 Note (draft), JMAP keyword $draft",
            DRAFT_FIELDS,
        )
    }
}

/// The registry with all four types of this program.
///
/// # Errors
/// A defect of a description (a bug of this crate, caught by its tests).
pub fn registry() -> Result<TypeRegistry, fs_types::DescriptionProblem> {
    let mut r = TypeRegistry::new();
    r.register::<ConversationSummary>()?
        .register::<Message>()?
        .register::<Account>()?
        .register::<DraftRecord>()?;
    Ok(r)
}
