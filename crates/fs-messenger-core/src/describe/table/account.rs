//! Rows of `Account`: the accounts and the settings of the program.
//!
//! Pattern: Registry part of the capability table (`table/all.rs` chains the parts).

use fs_types::CommandKind::{Change, Create, Delete, Find, Other};

use super::super::entry::{Entry, In, done, missing};
use super::common::*;

#[rustfmt::skip]
pub(super) static ROWS: &[Entry] = &[
    done("account.list", "Account", "find", Find, &[], &[]),
    done("account.add", "Account", "create", Create, &[In::text("address").req(), In::pick("kind", KINDS).req()], &[]),
    done("account.remove", "Account", "delete", Delete, &[ACCOUNT], &[]),
    missing("account.edit", "Account", "change", Change, &[ACCOUNT, In::text("address"), In::pick("kind", KINDS)], NO_PROVIDER),
    missing("account.secret", "Account", "setSecret", Other, &[ACCOUNT], "there is no key vault (ADR 0024); the core stores no secret, and a secret is never an input of a type (a form would show it as plain text)"),
    missing("account.alias", "Account", "alias", Other, &[ACCOUNT, In::text("alias")], "no provider offers identities (JMAP Identity)"),
    missing("key.discover", "Account", "discoverKey", Other, &[In::text("address").req()], "key discovery is not built"),
    missing("key.verify", "Account", "verifyKey", Other, &[In::text("address").req(), In::new("fingerprint", fs_types::BasicType::Key)], "key verification is not built"),
    missing("device.list", "Account", "devices", Other, &[], "devices are not built"),
    missing("profile.edit", "Account", "editProfile", Other, &[In::text("name"), In::long("note")], OTHER_PROGRAM),
    missing("autoreply.set", "Account", "setAutoreply", Other, &[ACCOUNT, In::long("text")], NOT_BUILT),
    missing("bridge.install", "Account", "installBridge", Other, &[In::text("bridge").req()], NOT_BUILT),
    missing("sync", "Account", "sync", Other, &[], "messages are device-local for now (strategy decision, ADR 0029); the hub withholds them"),
    missing("sync.scope", "Account", "syncScope", Other, &[In::text("days")], "messages are device-local for now (strategy decision, ADR 0029)"),
    missing("privacy.proxy", "Account", "privacyProxy", Other, &[], NOT_BUILT),
    missing("rule.create", "Account", "createRule", Other, &[In::long("rule").req()], "rules are not built"),
    missing("rule.run", "Account", "runRule", Other, &[], "rules are not built"),
    missing("rule.read", "Account", "readRules", Other, &[], "rules are not built"),
    missing("rule.write", "Account", "writeRules", Other, &[In::long("sieve").req()], "rules are not built"),
    done("receipt.choice", "Account", "receiptChoice", Other, &[], &[In::pick("choice", CHOICES)]),
    done("receipt.choice", "Account", "setReceiptChoice", Other, &[In::pick("choice", CHOICES).req()], &[]),
    done("receipt.choice", "Account", "showReceipt", Other, &[In::pick("theirs", CHOICES).req()], &[In::pick("shown", YN)]),
    missing("event.subscribe", "Account", "subscribe", Other, &[], "the generated frontend has no event stream yet; the Backend has only list and run"),
    missing("media.search", "Account", "searchMedia", Other, &[In::text("text").req()], NOT_BUILT),
    missing("call.start", "Account", "startCall", Other, &[], "calls are not built; scope open (own program?)"),
    missing("presence.set", "Account", "setPresence", Other, &[In::text("status")], NOT_BUILT),
    missing("app.run", "Account", "runApp", Other, &[In::text("app").req()], NOT_BUILT),
    missing("command.palette", "Account", "palette", Other, &[], "a frontend path over the capability list (ADR 0021), not a command of the program"),
    missing("contact.lookup", "Account", "lookupContact", Other, &[In::text("text").req()], OTHER_PROGRAM),
    missing("contact.vip", "Account", "setVip", Other, &[In::text("address").req()], OTHER_PROGRAM),
    missing("recipient.warn", "Account", "warnRecipients", Other, &[In::text("address").req().many()], NOT_BUILT),
    missing("participant.consent", "Account", "consent", Other, &[CONVERSATION], "not built: nothing checks consent before an AI joins (ADR 0054)"),
];
