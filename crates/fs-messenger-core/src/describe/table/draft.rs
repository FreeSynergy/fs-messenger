//! Rows of `Draft`. `Draft.change` has no recipient input: the core's `Draft` sets to, cc and bcc together and cannot read cc and bcc back, so a partial change would clear them.
//!
//! Pattern: Registry part of the capability table (`table/all.rs` chains the parts).

use fs_types::CommandKind::{Change, Create, Delete, Find, Other};

use super::super::entry::{Entry, In, done, missing};
use super::common::*;

#[rustfmt::skip]
pub(super) static ROWS: &[Entry] = &[
    done("draft.list", "Draft", "find", Find, &[], &[]),
    done("draft.save", "Draft", "create", Create, &[In::text("to").many(), In::text("cc").many(), In::text("bcc").many(), In::text("subject"), In::long("text"), In::pick("mediaType", FORMATS), In::to("account", "Account"), In::to("replyTo", "Message")], &[]),
    done("message.compose", "Draft", "create", Create, &[], &[]),
    done("draft.save", "Draft", "change", Change, &[DRAFT, In::text("subject"), In::long("text"), In::pick("mediaType", FORMATS), In::to("account", "Account")], &[]),
    done("draft.list", "Draft", "delete", Delete, &[DRAFT], &[]),
    done("message.send", "Draft", "send", Other, &[DRAFT], &[In::to("sent", "Message")]),
    missing("attachment.add", "Draft", "attach", Other, &[DRAFT, In::text("name").req()], NO_BYTES),
    missing("draft.share", "Draft", "share", Other, &[DRAFT, In::text("who").req().many()], NOT_BUILT),
];
