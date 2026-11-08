//! What the store-backed program reports.
//!
//! Pattern: none, allowed by section 2 (a single function that fills a `Capabilities` report; there is no `build()`, so no
//! Builder of coding.md section 2). The report of the provider rule (ADR 0045):
//! what is missing says so, with why.

use fs_interfaces::provider::Capabilities;

pub(super) fn report(guard_installed: bool) -> Capabilities {
    let mut caps = Capabilities::new("fs-messenger");
    for id in [
        "account.list",
        "account.add",
        "account.remove",
        "conversation.list",
        "conversation.open",
        "conversation.nextUnread",
        "conversation.markRead",
        "conversation.sticky",
        "conversation.move",
        "conversation.usage",
        "message.open",
        "message.act",
        "message.send",
        "message.reply",
        "message.forward",
        "message.delete",
        "message.flag",
        "message.search",
        "message.encryption",
        "message.compose",
        "draft.save",
        "draft.list",
        "attachment.add",
        "attachment.save",
        "toolcall.show",
        "event.subscribe",
        // Accepted ideas of requirements.md; not in `messenger.capnp` yet, so these ids are ours.
        "conversation.export",
        "conversation.whyPlain",
        "receipt.choice",
        "message.hold",
    ] {
        caps.supported(id);
    }
    for (id, why) in [
        ("message.edit", "no provider can carry it yet"),
        ("message.react", "no provider can carry it yet"),
        ("text.stream", "nothing streams yet"),
        ("message.schedule", "nothing runs a scheduled send yet"),
        ("account.secret", "no key vault yet (ADR 0024)"),
        ("rule.read", "rules are not built"),
        ("rule.write", "rules are not built"),
    ] {
        caps.unsupported(id, why);
    }
    for id in ["toolcall.decide", "turn.stop"] {
        if guard_installed {
            caps.supported(id);
        } else {
            caps.unsupported(id, "no guard is installed");
        }
    }
    caps
}
