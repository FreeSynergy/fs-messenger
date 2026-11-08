//! Cool-down (accepted idea 1): before a heated message goes out, "send now or in 10 minutes", and recall
//! while it is unread.
//!
//! Pattern: State machine over plain data, no clock of its own (the caller passes `now`): `Held` ->
//! `Sent` or `Recalled`. A recall works only while the message is held, or sent and still unread.

use serde::{Deserialize, Serialize};

/// The default wait, in seconds.
pub const COOL_DOWN_SECS: u64 = 600;

/// Where a message with a cool-down stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Waiting; goes out at `release_at`.
    Held {
        /// Seconds since the epoch.
        release_at: u64,
    },
    /// Out, and not read by the other side yet.
    SentUnread,
    /// Out and read, or too late: it stays.
    SentRead,
    /// Taken back before it was read; the other side never sees it, or sees it removed.
    Recalled,
}

impl Stage {
    /// Hold a message until `now + wait`.
    #[must_use]
    pub fn hold(now: u64, wait: u64) -> Self {
        Self::Held {
            release_at: now.saturating_add(wait),
        }
    }

    /// The person chose "send now", or the time is up.
    #[must_use]
    pub fn release(self, now: u64) -> Self {
        match self {
            Self::Held { release_at } if now >= release_at => Self::SentUnread,
            other => other,
        }
    }

    /// "Send now" regardless of the time.
    #[must_use]
    pub fn send_now(self) -> Self {
        match self {
            Self::Held { .. } => Self::SentUnread,
            other => other,
        }
    }

    /// The other side read it.
    #[must_use]
    pub fn read(self) -> Self {
        match self {
            Self::SentUnread => Self::SentRead,
            other => other,
        }
    }

    /// Take it back: only a held or unread message can be.
    #[must_use]
    pub fn recall(self) -> Self {
        match self {
            Self::Held { .. } | Self::SentUnread => Self::Recalled,
            other => other,
        }
    }

    /// Whether a recall would work now.
    #[must_use]
    pub fn can_recall(self) -> bool {
        matches!(self, Self::Held { .. } | Self::SentUnread)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_message_goes_out_when_the_time_is_up() {
        let s = Stage::hold(100, COOL_DOWN_SECS);
        assert_eq!(s.release(699), s, "not yet");
        assert_eq!(s.release(700), Stage::SentUnread);
    }

    #[test]
    fn send_now_skips_the_wait() {
        assert_eq!(Stage::hold(0, 600).send_now(), Stage::SentUnread);
    }

    #[test]
    fn recall_works_while_held_or_unread_only() {
        assert_eq!(Stage::hold(0, 600).recall(), Stage::Recalled);
        assert_eq!(Stage::SentUnread.recall(), Stage::Recalled);
        assert_eq!(Stage::SentUnread.read().recall(), Stage::SentRead);
        assert!(!Stage::SentRead.can_recall());
        assert!(!Stage::Recalled.can_recall());
    }

    #[test]
    fn a_recalled_message_does_not_come_back() {
        let s = Stage::hold(0, 10).recall();
        assert_eq!(s.release(999), Stage::Recalled);
        assert_eq!(s.send_now(), Stage::Recalled);
    }

    #[test]
    fn the_wait_cannot_overflow() {
        assert!(matches!(Stage::hold(u64::MAX, 600), Stage::Held { .. }));
    }
}
