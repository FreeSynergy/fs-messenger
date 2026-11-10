//! Rows of `Message`.
//!
//! Pattern: Registry part of the capability table (`table/all.rs` chains the parts).

use fs_types::CommandKind::{Change, Create, Delete, Find, Other};

use super::super::entry::{Entry, In, done, missing, shown};
use super::common::*;

#[rustfmt::skip]
pub(super) static ROWS: &[Entry] = &[
    done("message.search", "Message", "find", Find, &[In::to("conversation", "Conversation"), In::text("text"), In::text("from"), In::pick("unread", YN), In::pick("hasAttachment", YN), In::new("since", fs_types::BasicType::DateTime)], &[]),
    done("message.open", "Message", "find", Find, &[], &[]),
    done("conversation.open", "Message", "find", Find, &[], &[]),
    shown("message.encryption", "Message", "encryption"),
    shown("message.status", "Message", "state"),
    shown("toolcall.show", "Message", "toolCalls"),
    done("message.send", "Message", "create", Create, &[In::text("to").req().many(), In::text("cc").many(), In::text("bcc").many(), In::text("subject"), In::long("text").req(), In::pick("mediaType", FORMATS), In::to("account", "Account")], &[]),
    done("message.reply", "Message", "reply", Create, &[MESSAGE, In::long("text").req(), In::pick("mediaType", FORMATS)], &[]),
    done("message.forward", "Message", "forward", Other, &[MESSAGE, In::text("to").req().many()], &[]),
    done("message.delete", "Message", "delete", Delete, &[MESSAGE], &[]),
    done("message.act", "Message", "flag", Other, &[], &[]),
    done("message.flag", "Message", "flag", Other, &[MESSAGE, In::pick("flagged", YN).req()], &[]),
    done("message.read", "Message", "read", Other, &[MESSAGE, In::pick("unread", YN).req()], &[]),
    done("conversation.nextUnread", "Message", "nextUnread", Other, &[In::to("after", "Message")], &[In::to("next", "Message")]),
    done("message.hold", "Draft", "hold", Other, &[DRAFT, In::text("waitSeconds")], &[In::to("held", "Message")]),
    done("message.hold", "Message", "heldStage", Other, &[In::to("held", "Message").req()], &[In::pick("stage", STAGES), In::text("releaseAt")]),
    done("message.hold", "Message", "sendHeldNow", Other, &[In::to("held", "Message").req()], &[In::to("sent", "Message")]),
    done("message.hold", "Message", "recallHeld", Other, &[In::to("held", "Message").req()], &[]),
    done("message.hold", "Message", "releaseDue", Other, &[], &[In::to("released", "Message").many()]),
    done("message.hold", "Message", "heldUnsure", Other, &[], &[In::to("unsure", "Message").many()]),
    missing("message.edit", "Message", "edit", Change, &[MESSAGE, In::long("text").req()], NO_PROVIDER),
    missing("message.react", "Message", "react", Other, &[MESSAGE, In::text("reaction")], NO_PROVIDER),
    missing("text.stream", "Message", "streamText", Other, &[MESSAGE], "nothing emits live text yet"),
    missing("toolcall.decide", "Message", "decide", Other, &[MESSAGE, In::text("toolCall").req(), In::text("option").req()], "no guard is installed, and a decision needs the signature of the person's device"),
    missing("message.quote", "Message", "quote", Other, &[MESSAGE], NOT_BUILT),
    missing("message.mention", "Message", "mention", Other, &[MESSAGE, In::text("who").req()], NOT_BUILT),
    missing("message.label", "Message", "label", Other, &[MESSAGE, In::text("label").req()], NOT_BUILT),
    missing("message.junk", "Message", "junk", Other, &[MESSAGE], NOT_BUILT),
    missing("message.schedule", "Draft", "schedule", Other, &[DRAFT, In::new("at", fs_types::BasicType::DateTime).req()], "nothing runs a scheduled send yet; a future time is refused"),
    missing("message.bookmark", "Message", "bookmark", Other, &[MESSAGE], NOT_BUILT),
    missing("message.expire", "Message", "expire", Other, &[MESSAGE, In::new("at", fs_types::BasicType::DateTime).req()], NOT_BUILT),
    missing("message.announce", "Message", "announce", Other, &[MESSAGE], NOT_BUILT),
    missing("message.sensitive", "Message", "markSensitive", Other, &[MESSAGE, In::text("warning")], NOT_BUILT),
    missing("message.export", "Message", "exportMessage", Other, &[MESSAGE], "only whole conversations export (conversation.export); EML and mbox are not built"),
    missing("message.import", "Message", "importMessage", Other, &[In::long("source").req()], "EML and mbox import are not built"),
    missing("message.followup", "Message", "followUp", Other, &[MESSAGE, In::new("at", fs_types::BasicType::DateTime)], OTHER_PROGRAM),
    missing("message.priority", "Message", "setPriority", Other, &[MESSAGE, In::text("priority").req()], NOT_BUILT),
    missing("message.classify", "Message", "classify", Other, &[MESSAGE], NOT_BUILT),
    missing("message.setAside", "Message", "setAside", Other, &[MESSAGE], NOT_BUILT),
    missing("message.replyLater", "Message", "replyLater", Other, &[MESSAGE, In::new("at", fs_types::BasicType::DateTime)], NOT_BUILT),
    missing("message.summarize", "Message", "summarize", Other, &[MESSAGE], "needs a local model; not wired"),
    missing("message.comment", "Message", "comment", Other, &[MESSAGE, In::long("text").req()], NOT_BUILT),
    missing("message.report", "Message", "report", Other, &[MESSAGE, In::text("reason")], NOT_BUILT),
    missing("message.recall", "Message", "recall", Other, &[MESSAGE], "only a held message can be taken back (message.hold); one already sent cannot"),
    missing("attachment.describe", "Message", "describeAttachment", Other, &[MESSAGE, In::text("name").req(), In::text("alt").req()], NOT_BUILT),
    missing("attachment.preview", "Message", "previewAttachment", Other, &[MESSAGE, In::text("name").req()], NO_BYTES),
    missing("attachment.save", "Message", "saveAttachment", Other, &[MESSAGE, In::text("name").req()], NO_BYTES),
    missing("sender.screen", "Message", "screenSender", Other, &[MESSAGE], NOT_BUILT),
    missing("list.unsubscribe", "Message", "unsubscribe", Other, &[MESSAGE], NOT_BUILT),
    missing("clip.save", "Message", "saveClip", Other, &[MESSAGE, In::long("text").req()], NOT_BUILT),
];
