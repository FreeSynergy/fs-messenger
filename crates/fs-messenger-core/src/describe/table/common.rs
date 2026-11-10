//! Shared pieces of the capability table: choice lists, standard reasons, the reference inputs.
//!
//! Pattern: none (constants).

use super::super::entry::In;

pub(super) const YN: &[&str] = &["yes", "no"];
pub(super) const KINDS: &[&str] = &["mail", "matrix", "mimi"];
// Choice keys are lower-case words (fs-types); the Backend maps them to the media types.
pub(super) const FORMATS: &[&str] = &["plain", "markdown"];
pub(super) const WHY: &[&str] = &[
    "no-key-of-the-other-side",
    "account-cannot",
    "on-the-wire-only",
    "not-known",
];
pub(super) const REMEDY: &[&str] = &["exchange-keys", "choose-other-channel", "turn-on", "wait"];
pub(super) const STAGES: &[&str] = &["held", "sent-unread", "sent-read", "recalled"];
pub(super) const CHOICES: &[&str] = &["shown", "hidden"];

pub(super) const NO_PROVIDER: &str = "no provider can carry it yet (Provider has no method for it)";
pub(super) const NOT_BUILT: &str = "not built in this step (DESIGN.md, Not built)";
pub(super) const OTHER_PROGRAM: &str = "belongs to another program (ADR 0017, 0020); the messenger borrows it by live reference, not wired yet";
pub(super) const NO_GUARD: &str =
    "no guard is installed in this app (fs-messenger-guard is not wired in)";
pub(super) const NO_BYTES: &str =
    "the generated frontend has no basic type for file contents, so it cannot give or show bytes";

pub(super) const ACCOUNT: In = In::to("account", "Account").req();
pub(super) const CONVERSATION: In = In::to("conversation", "Conversation").req();
pub(super) const MESSAGE: In = In::to("message", "Message").req();
pub(super) const DRAFT: In = In::to("draft", "Draft").req();
