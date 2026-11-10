//! The words of the program: the English key table (ADR 0057, coding.md 2a).
//!
//! Pattern: Registry (key to text) behind the `Say` strategy of `fs-components-fields`. All English text of this
//! program is in this file; translation replaces the table, not its users. Code never decides by comparing these words.
//!
//! The keys come from three places: the frontend (`FRONTEND`), the descriptions of `fs-messenger-core`
//! (`messenger.type.*`, `messenger.field.*`, `messenger.command.<type>.<command>`, and `<field label>.<choice>`), and the
//! problem codes of `fs-types`. A command label is looked up by its command (the same word in every type, `COMMANDS`);
//! a choice by its last part (`CHOICES`). A missing key says itself, as `Say` demands; a missing `hint.` is empty (a hint
//! is a placeholder: the key would only show as noise).

use fs_components_fields::Say;

/// Keys the generated frontend asks for.
const FRONTEND: &[(&str, &str)] = &[
    ("action.add", "Add"),
    ("action.back", "Back"),
    ("action.cancel", "Cancel"),
    ("action.close", "Close"),
    ("action.remove", "Remove"),
    ("action.save", "Save"),
    ("menu.program", "Program"),
    ("menu.view", "View"),
    ("menu.help", "Help"),
    ("menu.quit", "Quit"),
    ("menu.about", "About"),
    ("message.done", "Done."),
    ("message.rejected", "Please check the marked values."),
    ("message.not-found", "That does not exist any more."),
    ("message.unavailable", "The messenger could not do that."),
    (
        "message.unsaved",
        "There is text that is not sent yet. Send or cancel it first.",
    ),
    ("sort.ascending", "ascending"),
    ("sort.descending", "descending"),
    ("finding.slot", "Value {n}"),
    ("list.empty", "Nothing here yet."),
    ("list.separator", ", "),
    ("list.stale", "Out of date: the messenger did not answer."),
    ("detail.not-loaded", "Not loaded."),
    ("confirm.sure", "Are you sure?"),
    ("value.none", "none"),
    ("command.soon", "coming soon"),
    ("about.version", "Version"),
    ("about.origin", "Source"),
    ("about.licence", "Licence"),
    ("about.repository", "Repository"),
    ("about.unoffered", "Not offered here"),
    ("problem.unknown-field", "This field does not exist."),
    ("problem.missing", "A value is needed."),
    ("problem.not-multiple", "Only one value is allowed."),
    ("problem.empty", "Nothing typed."),
    ("problem.too-long", "That is too long."),
    (
        "problem.character",
        "That holds a character that is not allowed.",
    ),
    ("problem.language.bad-tag", "That is not a language tag."),
    ("problem.uri.bad", "That is not a valid address."),
    (
        "problem.uri.not-absolute",
        "Type the whole address, with https://.",
    ),
    (
        "problem.uri.unsafe-scheme",
        "That kind of address is not allowed.",
    ),
    ("problem.date.bad", "That is not a date (YYYY-MM-DD)."),
    (
        "problem.date-time.bad",
        "That is not a date and time with a time zone.",
    ),
    ("problem.place.bad", "That is not a place."),
    ("problem.place.range", "That place is outside the earth."),
    (
        "problem.key.bad-fingerprint",
        "That is not a key fingerprint.",
    ),
    (
        "problem.choice.not-in-list",
        "Pick one of the offered values.",
    ),
    ("problem.image.not-image", "That is not an image."),
    ("problem.keywords.bad", "A keyword is a short word."),
    ("problem.no-parts", "Fill in at least one part."),
    (
        "problem.address.bad-country-code",
        "That is not a country code.",
    ),
    (
        "problem.email.missing-at",
        "An e-mail address needs an @, like name@example.org.",
    ),
    ("problem.email.many-at", "An e-mail address has only one @."),
    ("problem.email.empty-local", "Type the part before the @."),
    ("problem.email.empty-domain", "Type the part after the @."),
    (
        "problem.email.local-character",
        "The part before the @ has a character that is not allowed.",
    ),
    (
        "problem.email.local-too-long",
        "The part before the @ is too long.",
    ),
    (
        "problem.email.bad-domain",
        "The part after the @ is not a valid domain.",
    ),
    (
        "problem.email.domain-without-dot",
        "The domain needs a dot, like example.org.",
    ),
    ("problem.email.too-long", "That address is too long."),
    (
        "problem.phone.no-country-code",
        "Start with + and the country code.",
    ),
    (
        "problem.phone.character",
        "A phone number has digits, spaces and hyphens only.",
    ),
    ("problem.phone.length", "A phone number has 7 to 15 digits."),
    ("problem.name.none", "Type a name."),
    ("problem.incomplete", "That is not complete."),
    ("problem.not-a-number", "That is not a number."),
    ("problem.date.no-such-date", "There is no such date."),
];

/// Types and fields (also the inputs and results of commands).
const LABELS: &[(&str, &str)] = &[
    ("messenger.type.conversation", "Conversations"),
    ("messenger.type.message", "Messages"),
    ("messenger.type.draft", "Drafts"),
    ("messenger.type.account", "Accounts"),
    ("messenger.field.name", "Name"),
    ("messenger.field.attributed-to", "From"),
    ("messenger.field.to", "To"),
    ("messenger.field.cc", "Cc"),
    ("messenger.field.bcc", "Bcc"),
    ("messenger.field.published", "Date"),
    ("messenger.field.context", "Conversation"),
    ("messenger.field.in-reply-to", "In reply to"),
    ("messenger.field.account", "Account"),
    ("messenger.field.content", "Text"),
    ("messenger.field.preview", "Preview"),
    ("messenger.field.kind", "Kind"),
    ("messenger.field.encryption", "Encryption"),
    ("messenger.field.state", "State"),
    ("messenger.field.unread", "Unread"),
    ("messenger.field.flagged", "Flagged"),
    ("messenger.field.sender-kind", "Sender"),
    ("messenger.field.signature", "Signature"),
    ("messenger.field.media-type", "Format"),
    ("messenger.field.attachments", "Attachments"),
    ("messenger.field.reactions", "Reactions"),
    ("messenger.field.tool-calls", "What an AI did"),
    ("messenger.field.job-ref", "Job"),
    ("messenger.field.task-refs", "Tasks"),
    ("messenger.field.unread-count", "Unread messages"),
    ("messenger.field.last-message", "Last message"),
    ("messenger.field.pinned", "Pinned"),
    ("messenger.field.folders", "Folders"),
    ("messenger.field.address", "Address"),
    ("messenger.field.encryption-ready", "Can encrypt"),
    ("messenger.field.text", "Text"),
    ("messenger.field.subject", "Subject"),
    ("messenger.field.reply-to", "Reply to"),
    ("messenger.field.draft", "Draft"),
    ("messenger.field.message", "Message"),
    ("messenger.field.conversation", "Conversation"),
    ("messenger.field.up-to", "Up to message"),
    ("messenger.field.folder", "Folder"),
    ("messenger.field.only-unread", "Only unread"),
    ("messenger.field.from", "From"),
    ("messenger.field.has-attachment", "Has attachment"),
    ("messenger.field.since", "Since"),
    ("messenger.field.alias", "Alias"),
    ("messenger.field.fingerprint", "Fingerprint"),
    ("messenger.field.note", "Note"),
    ("messenger.field.days", "Days"),
    ("messenger.field.rule", "Rule"),
    ("messenger.field.sieve", "Rules (Sieve)"),
    ("messenger.field.choice", "Choice"),
    ("messenger.field.theirs", "Their choice"),
    ("messenger.field.shown", "Shown"),
    ("messenger.field.level", "Level"),
    ("messenger.field.until", "Until"),
    ("messenger.field.who", "Who"),
    ("messenger.field.role", "Role"),
    ("messenger.field.topic", "Topic"),
    ("messenger.field.query", "Search"),
    ("messenger.field.channel", "Channel"),
    ("messenger.field.question", "Question"),
    ("messenger.field.answers", "Answers"),
    ("messenger.field.after", "After message"),
    ("messenger.field.next", "Next unread"),
    ("messenger.field.held", "Held message"),
    ("messenger.field.wait-seconds", "Wait (seconds)"),
    ("messenger.field.stage", "Stage"),
    ("messenger.field.release-at", "Goes out at"),
    ("messenger.field.sent", "Sent message"),
    ("messenger.field.released", "Gone out"),
    ("messenger.field.unsure", "Not sure if sent"),
    ("messenger.field.reason", "Why not encrypted"),
    ("messenger.field.remedy", "What to do"),
    ("messenger.field.export", "Export"),
    ("messenger.field.bridge", "Bridge"),
    ("messenger.field.status", "Status"),
    ("messenger.field.app", "App"),
    ("messenger.field.source", "Source"),
    ("messenger.field.priority", "Priority"),
    ("messenger.field.warning", "Warning"),
    ("messenger.field.alt", "Description"),
    ("messenger.field.at", "At"),
    ("messenger.field.tool-call", "Tool call"),
    ("messenger.field.option", "Option"),
    ("messenger.field.input-tokens", "Tokens read"),
    ("messenger.field.output-tokens", "Tokens written"),
    ("messenger.field.context-used", "Context used"),
    ("messenger.field.context-size", "Context size"),
    ("messenger.field.cost-micros", "Cost (millionths)"),
    ("messenger.field.currency", "Currency"),
    ("messenger.field.label", "Label"),
    ("messenger.field.reaction", "Reaction"),
];

/// Commands, by command name (the same in every type).
const COMMANDS: &[(&str, &str)] = &[
    ("find", "Search"),
    ("create", "Create"),
    ("delete", "Delete"),
    ("change", "Change"),
    ("send", "Send"),
    ("setSecret", "Set secret"),
    ("alias", "Add alias"),
    ("discoverKey", "Find key"),
    ("verifyKey", "Verify key"),
    ("devices", "Devices"),
    ("editProfile", "Edit profile"),
    ("setAutoreply", "Set automatic reply"),
    ("installBridge", "Install bridge"),
    ("sync", "Synchronise"),
    ("syncScope", "Choose what to keep offline"),
    ("privacyProxy", "Hide my address when opening"),
    ("createRule", "Create rule"),
    ("runRule", "Run rule"),
    ("readRules", "Show rules"),
    ("writeRules", "Save rules"),
    ("receiptChoice", "Show my receipt choice"),
    ("setReceiptChoice", "Choose about read receipts"),
    ("showReceipt", "Would a receipt be shown?"),
    ("subscribe", "Follow changes"),
    ("searchMedia", "Search GIFs and stickers"),
    ("startCall", "Call"),
    ("setPresence", "Set status"),
    ("runApp", "Run app"),
    ("palette", "Command palette"),
    ("lookupContact", "Look up contact"),
    ("setVip", "Mark as VIP"),
    ("warnRecipients", "Warn about outside recipients"),
    ("consent", "Consent for an AI"),
    ("markRead", "Mark as read"),
    ("pin", "Pin"),
    ("move", "Move to folder"),
    ("usage", "Show usage"),
    ("export", "Export"),
    ("whyPlain", "Why not encrypted?"),
    ("stop", "Stop the AI"),
    ("notify", "Notifications"),
    ("snooze", "Snooze"),
    ("ignore", "Ignore"),
    ("createFolder", "Create folder"),
    ("invite", "Invite"),
    ("leave", "Leave"),
    ("setRole", "Set role"),
    ("removeMember", "Remove member"),
    ("block", "Block"),
    ("openThread", "Open thread"),
    ("setTopic", "Set topic"),
    ("moveTopic", "Move topic"),
    ("resolveTopic", "Resolve topic"),
    ("muteTopic", "Mute topic"),
    ("saveView", "Save search as view"),
    ("createCollection", "Create collection"),
    ("shareCollection", "Share collection"),
    ("shareInbox", "Share inbox"),
    ("followChannel", "Follow channel"),
    ("createPoll", "Create poll"),
    ("reply", "Reply"),
    ("forward", "Forward"),
    ("flag", "Flag"),
    ("read", "Mark read or unread"),
    ("nextUnread", "Next unread"),
    ("hold", "Hold before sending"),
    ("heldStage", "Where is the held message?"),
    ("sendHeldNow", "Send now"),
    ("recallHeld", "Take back"),
    ("releaseDue", "Send what is due"),
    ("heldUnsure", "Held messages not sure if sent"),
    ("edit", "Edit"),
    ("react", "React"),
    ("streamText", "Show text as it is written"),
    ("decide", "Allow or refuse a tool call"),
    ("quote", "Quote"),
    ("mention", "Mention"),
    ("label", "Label"),
    ("junk", "Mark as junk"),
    ("schedule", "Send later"),
    ("bookmark", "Bookmark"),
    ("expire", "Disappear after"),
    ("announce", "Boost"),
    ("markSensitive", "Mark as sensitive"),
    ("exportMessage", "Export message"),
    ("importMessage", "Import message"),
    ("followUp", "Remind me"),
    ("setPriority", "Set priority"),
    ("classify", "Sort by kind"),
    ("setAside", "Set aside"),
    ("replyLater", "Reply later"),
    ("summarize", "Summarise"),
    ("comment", "Comment"),
    ("report", "Report"),
    ("recall", "Recall"),
    ("describeAttachment", "Describe attachment"),
    ("previewAttachment", "Preview attachment"),
    ("saveAttachment", "Save attachment"),
    ("screenSender", "Screen sender"),
    ("unsubscribe", "Unsubscribe"),
    ("saveClip", "Save clip"),
    ("attach", "Attach file"),
    ("share", "Share draft"),
];

/// Choices, by their last part.
const CHOICES: &[(&str, &str)] = &[
    ("yes", "Yes"),
    ("no", "No"),
    ("mail", "E-mail"),
    ("chat", "Chat"),
    ("comment", "Comment"),
    ("unknown", "Not known"),
    ("matrix", "Matrix"),
    ("mimi", "MIMI"),
    ("mls", "End-to-end (MLS)"),
    ("openpgp", "End-to-end (OpenPGP)"),
    ("megolm", "End-to-end (Megolm)"),
    ("transport-only", "Protected on the way only"),
    ("none", "Not encrypted"),
    ("sending", "Sending"),
    ("sent", "Sent"),
    ("delivered", "Delivered"),
    ("read", "Read"),
    ("failed", "Failed"),
    ("person", "A person"),
    ("software", "Software"),
    ("ai", "An AI"),
    ("not-verified", "Not verified"),
    ("not-signed", "Not signed"),
    ("valid", "Valid"),
    ("plain", "Plain text"),
    ("markdown", "Markdown"),
    ("shown", "Shown"),
    ("hidden", "Hidden"),
    (
        "no-key-of-the-other-side",
        "The key of the other side is not known yet.",
    ),
    ("account-cannot", "This account cannot encrypt end to end."),
    (
        "on-the-wire-only",
        "Only the way is protected; the servers can read it.",
    ),
    ("not-known", "It is not known whether it is protected."),
    (
        "exchange-keys",
        "Exchange keys: send a signed message or ask for the key.",
    ),
    (
        "choose-other-channel",
        "Use a channel or account that can encrypt.",
    ),
    ("turn-on", "Turn encryption on in this conversation."),
    ("wait", "Nothing to do; the program is still waiting."),
    ("held", "Held"),
    ("sent-unread", "Sent, not read yet"),
    ("sent-read", "Sent and read"),
    ("recalled", "Taken back"),
];

/// The English words.
#[derive(Clone, Copy, Debug, Default)]
pub struct Words;

fn find(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

impl Words {
    /// The English table.
    #[must_use]
    pub fn english() -> Self {
        Self
    }

    /// The text of `key`, if the table has it.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<&'static str> {
        if let Some(t) = find(FRONTEND, key).or_else(|| find(LABELS, key)) {
            return Some(t);
        }
        if let Some(rest) = key.strip_prefix("messenger.command.") {
            // `<type>.<command in kebab case>`
            let (_, command) = rest.split_once('.')?;
            return COMMANDS
                .iter()
                .find(|(c, _)| kebab(c) == command)
                .map(|(_, v)| *v);
        }
        if key.starts_with("messenger.field.") {
            // `<field label>.<choice>`
            let (_, choice) = key.rsplit_once('.')?;
            return find(CHOICES, choice);
        }
        None
    }
}

fn kebab(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Say for Words {
    fn say(&self, key: &str) -> String {
        match self.lookup(key) {
            Some(t) => t.to_owned(),
            None if key.starts_with("hint.") => String::new(),
            None => key.to_owned(),
        }
    }
}

/// The keys the generated frontend asks for with a fixed name (for the test that every one has a word).
#[must_use]
pub fn frontend_keys() -> Vec<&'static str> {
    FRONTEND.iter().map(|(k, _)| *k).collect()
}
