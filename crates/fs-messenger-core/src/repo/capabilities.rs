//! What the store-backed program reports.
//!
//! Pattern: Builder (the report of the provider rule, ADR 0045): what is missing says so, with why.

use fs_interfaces::provider::Capabilities;

pub(super) fn report() -> Capabilities {
    let mut caps = Capabilities::new("fs-messenger");
    for id in [
        "account.list",
        "account.add",
        "account.remove",
        "conversation.list",
        "conversation.open",
        "conversation.nextUnread",
        "conversation.markRead",
        "conversation.pin",
        "conversation.move",
        "conversation.usage",
        "message.open",
        "message.act",
        "message.send",
        "message.reply",
        "message.forward",
        "message.edit",
        "message.delete",
        "message.flag",
        "message.react",
        "message.search",
        "message.encryption",
        "message.compose",
        "draft.save",
        "draft.list",
        "attachment.add",
        "attachment.save",
        "toolcall.show",
        "event.subscribe",
    ] {
        caps.supported(id);
    }
    for (id, why) in [
        ("toolcall.decide", "the guard follows"),
        ("turn.stop", "the guard follows"),
        ("text.stream", "nothing streams yet"),
        ("message.schedule", "nothing runs a scheduled send yet"),
        ("account.secret", "no key vault yet (ADR 0024)"),
        ("rule.read", "rules are not built"),
        ("rule.write", "rules are not built"),
    ] {
        caps.unsupported(id, why);
    }
    caps
}
