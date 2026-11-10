//! Rows of `Conversation`.
//!
//! Pattern: Registry part of the capability table (`table/all.rs` chains the parts).

use fs_types::CommandKind::{Create, Find, Other};

use super::super::entry::{Entry, In, done, missing};
use super::common::*;

#[rustfmt::skip]
pub(super) static ROWS: &[Entry] = &[
    done("conversation.list", "Conversation", "find", Find, &[In::text("folder"), In::pick("onlyUnread", YN)], &[]),
    done("conversation.markRead", "Conversation", "markRead", Other, &[CONVERSATION, In::to("upTo", "Message").req()], &[]),
    done("conversation.sticky", "Conversation", "pin", Other, &[CONVERSATION, In::pick("pinned", YN).req()], &[]),
    done("conversation.pin", "Conversation", "pin", Other, &[CONVERSATION, In::pick("pinned", YN).req()], &[]),
    done("conversation.move", "Conversation", "move", Other, &[CONVERSATION, In::text("folder")], &[]),
    done("conversation.usage", "Conversation", "usage", Other, &[CONVERSATION], &[In::text("inputTokens"), In::text("outputTokens"), In::text("contextUsed"), In::text("contextSize"), In::text("costMicros"), In::text("currency")]),
    done("conversation.export", "Conversation", "export", Other, &[CONVERSATION], &[In::long("export")]),
    done("conversation.whyPlain", "Conversation", "whyPlain", Other, &[CONVERSATION], &[In::pick("reason", WHY), In::pick("remedy", REMEDY)]),
    missing("turn.stop", "Conversation", "stop", Other, &[CONVERSATION], NO_GUARD),
    missing("conversation.notify", "Conversation", "notify", Other, &[CONVERSATION, In::text("level")], NOT_BUILT),
    missing("conversation.snooze", "Conversation", "snooze", Other, &[CONVERSATION, In::new("until", fs_types::BasicType::DateTime)], NOT_BUILT),
    missing("conversation.ignore", "Conversation", "ignore", Other, &[CONVERSATION], NOT_BUILT),
    missing("mailbox.create", "Conversation", "createFolder", Other, &[In::text("folder").req()], "a folder is only a name on conversations; one alone cannot be created"),
    missing("group.create", "Conversation", "create", Create, &[In::text("name").req()], NO_PROVIDER),
    missing("group.invite", "Conversation", "invite", Other, &[CONVERSATION, In::text("who").req().many()], NO_PROVIDER),
    missing("group.leave", "Conversation", "leave", Other, &[CONVERSATION], NO_PROVIDER),
    missing("group.role", "Conversation", "setRole", Other, &[CONVERSATION, In::text("who").req(), In::text("role").req()], NO_PROVIDER),
    missing("member.remove", "Conversation", "removeMember", Other, &[CONVERSATION, In::text("who").req()], NO_PROVIDER),
    missing("actor.block", "Conversation", "block", Other, &[In::text("who").req()], NOT_BUILT),
    missing("thread.open", "Conversation", "openThread", Other, &[MESSAGE], NOT_BUILT),
    missing("topic.set", "Conversation", "setTopic", Other, &[CONVERSATION, In::text("topic").req()], NOT_BUILT),
    missing("topic.move", "Conversation", "moveTopic", Other, &[CONVERSATION, In::text("topic").req()], NOT_BUILT),
    missing("topic.resolve", "Conversation", "resolveTopic", Other, &[CONVERSATION, In::text("topic").req()], NOT_BUILT),
    missing("topic.mute", "Conversation", "muteTopic", Other, &[CONVERSATION, In::text("topic").req()], NOT_BUILT),
    missing("view.save", "Conversation", "saveView", Other, &[In::text("name").req(), In::text("query")], NOT_BUILT),
    missing("collection.create", "Conversation", "createCollection", Other, &[In::text("name").req()], NOT_BUILT),
    missing("collection.share", "Conversation", "shareCollection", Other, &[In::text("name").req()], NOT_BUILT),
    missing("inbox.share", "Conversation", "shareInbox", Other, &[In::text("who").req().many()], NOT_BUILT),
    missing("channel.follow", "Conversation", "followChannel", Other, &[In::new("channel", fs_types::BasicType::Link).req()], NOT_BUILT),
    missing("poll.create", "Conversation", "createPoll", Other, &[CONVERSATION, In::text("question").req(), In::text("answers").req().many()], "polls belong to the surveys program; not wired"),
];
