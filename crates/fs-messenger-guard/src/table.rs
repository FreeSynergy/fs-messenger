//! The risk of every capability, from the engine and never from a caller.
//!
//! Pattern: Registry: capability id to rule. **Provisional form:** this table becomes annotations in
//! the schema. Whatever is not in it is `unknown`. `Conversation.act` has one entry per union member,
//! because one method has mixed risk; `action_capability` is exhaustive, so a new member will not
//! compile until it has a name here.

use fs_messenger_core::{MessageAction, ToolCallRisk as R};

use crate::secret_name::is_secret_name;

/// What the table says about a capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityRule {
    /// Goes through release and brake with this risk.
    Risk(R),
    /// Only a person's device may do this. `check` refuses it for every AI, without asking anyone.
    NeverForAi,
}

use CapabilityRule::{NeverForAi, Risk};

const TABLE: &[(&str, CapabilityRule)] = &[
    ("account.list", Risk(R::ReadsOnly)),
    ("account.add", Risk(R::ActsForPerson)),
    ("account.remove", Risk(R::Destroys)),
    ("account.secret", NeverForAi),
    ("conversation.list", Risk(R::ReadsOnly)),
    ("conversation.open", Risk(R::ReadsOnly)),
    ("conversation.nextUnread", Risk(R::ReadsOnly)),
    ("conversation.usage", Risk(R::ReadsOnly)),
    // A read mark can reach the other side as a receipt.
    ("conversation.markRead", Risk(R::ActsForPerson)),
    ("conversation.pin", Risk(R::Changes)),
    // A folder can be a distribution list, or the Trash.
    ("conversation.move", Risk(R::ActsForPerson)),
    ("turn.stop", Risk(R::Changes)),
    ("message.open", Risk(R::ReadsOnly)),
    ("message.search", Risk(R::ReadsOnly)),
    ("message.encryption", Risk(R::ReadsOnly)),
    ("toolcall.show", Risk(R::ReadsOnly)),
    ("event.subscribe", Risk(R::ReadsOnly)),
    ("text.stream", Risk(R::ReadsOnly)),
    ("attachment.save", Risk(R::ReadsOnly)),
    ("draft.list", Risk(R::ReadsOnly)),
    ("rule.read", Risk(R::ReadsOnly)),
    ("message.compose", Risk(R::Changes)),
    // A new draft. Overwriting one that exists loses what a person wrote, so it asks every time.
    ("draft.save", Risk(R::Changes)),
    ("draft.change", Risk(R::Destroys)),
    ("attachment.add", Risk(R::Changes)),
    ("draft.discard", Risk(R::Destroys)),
    // A rule keeps acting for the person when nobody is looking: it can forward or delete mail.
    ("rule.write", Risk(R::ActsForPerson)),
    ("message.send", Risk(R::ActsForPerson)),
    ("message.reply", Risk(R::ActsForPerson)),
    ("message.schedule", Risk(R::ActsForPerson)),
    // One method, mixed risk: the bare id says nothing, so it is `unknown` and offers no release.
    ("message.act", Risk(R::Unknown)),
    // `Conversation.act`, one entry per union member.
    ("message.none", Risk(R::Unknown)),
    ("message.delete", Risk(R::Destroys)),
    ("message.flag", Risk(R::Changes)),
    ("message.unread", Risk(R::Changes)),
    ("message.react", Risk(R::ActsForPerson)),
    ("message.forward", Risk(R::ActsForPerson)),
    ("message.edit", Risk(R::ActsForPerson)),
    ("toolcall.decide", NeverForAi),
    // The `Guard` interface, and the way to it.
    ("messenger.guard", NeverForAi),
    ("guard.state", NeverForAi),
    ("guard.invite", NeverForAi),
    ("guard.release", NeverForAi),
    ("guard.revoke", NeverForAi),
    ("guard.stop", NeverForAi),
    ("guard.reopen", NeverForAi),
    ("guard.uninvite", NeverForAi),
];

/// The rule for a capability id. Exact match; what is not in the table is `unknown`. Anything naming
/// the guard, and anything `is_secret_name` catches, is refused whatever its spelling, so a variant
/// cannot be asked about and allowed.
pub fn capability_rule(capability: &str) -> CapabilityRule {
    // The rule comes first: no entry of the table can make a secret's name plain.
    if is_secret_name(capability) {
        return NeverForAi;
    }
    if let Some((_, rule)) = TABLE.iter().find(|(id, _)| *id == capability) {
        return *rule;
    }
    let plain = capability.trim().to_ascii_lowercase();
    if plain.starts_with("guard.") || plain == "toolcall.decide" || plain == "messenger.guard" {
        return NeverForAi;
    }
    Risk(R::Unknown)
}

/// The whole table. **Provisional:** it will become schema annotations; do not build on this form.
pub fn provisional_table() -> &'static [(&'static str, CapabilityRule)] {
    TABLE
}

/// The capability id of one `MessageAction`.
pub fn action_capability(action: &MessageAction) -> &'static str {
    match action {
        MessageAction::None => "message.none",
        MessageAction::Delete => "message.delete",
        MessageAction::Flag(_) => "message.flag",
        MessageAction::Unread(_) => "message.unread",
        MessageAction::React(_) => "message.react",
        MessageAction::Forward(_) => "message.forward",
        MessageAction::Edit(_) => "message.edit",
        MessageAction::Decide(_) => "toolcall.decide",
    }
}
