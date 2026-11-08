//! Words a person reads, apart from the code that decides (ADR 0057, coding.md section 2a).
//!
//! Pattern: none, allowed by section 2 (constants). Code never decides by comparing these.

/// Where the text goes, when the engine does not know.
pub(crate) const DESTINATION_UNKNOWN: &str = "unbekannt";

/// The notice a person reads before an AI enters a conversation (ADR 0054, "KIs in Unterhaltungen").
/// `{ai}` is the AI's name; `{destination}` says where its text goes, or [`DESTINATION_UNKNOWN`].
pub(crate) const CONSENT_NOTICE: &str = "{ai} möchte an dieser Unterhaltung teilnehmen. Eine KI liest mit, \
und der Text geht an {destination}. Sie liest nur, was ab ihrem Eintritt geschrieben wird. \
Alle Anwesenden müssen zustimmen; wer später kommt, muss die KI ebenfalls annehmen. \
Wer die Zustimmung zurückzieht, schickt die KI hinaus.";

/// What a room shows when the AI sits in a program we do not run (Element, for example): we cannot keep
/// it out, we can only say so and offer to leave.
pub(crate) const FOREIGN_WARNING: &str = "{ai} ist in einem fremden Matrix-Programm in diesem Raum. \
Wir können sie hier nicht fernhalten. Sie sehen, dass sie mitliest; Sie können den Raum verlassen.";
