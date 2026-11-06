//! What a stream says, put together: text in pieces on whole code points.
//!
//! Pattern: Builder. Offsets are in UTF-8 bytes per part, as in the contract's `TextChunk`, and they count
//! what was really handed on: without the key, without NUL (the core would take that out too, and the
//! stored text must equal the pieces), and without what waits because it may be the start of the key.

use crate::redact::Redactor;

/// One text being written: the answer, or the reasoning.
pub(crate) struct PartText {
    sent: u64,
    waiting: String,
    cap: usize,
    cut: bool,
}

impl PartText {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            sent: 0,
            waiting: String::new(),
            cap,
            cut: false,
        }
    }

    /// The piece to hand on now, with its offset. `None` if nothing is ready.
    pub(crate) fn push(&mut self, delta: &str, redactor: &Redactor) -> Option<(u64, String)> {
        if self.cut {
            return None;
        }
        self.waiting.push_str(&delta.replace('\0', ""));
        self.waiting = redactor.scrubbed(&self.waiting);
        let ready = self.waiting.len() - redactor.held_back(&self.waiting);
        let rest = self.waiting.split_off(ready);
        let piece = std::mem::replace(&mut self.waiting, rest);
        self.hand_on(piece)
    }

    /// What was still waiting: the stream has ended, so it cannot become the key any more.
    pub(crate) fn flush(&mut self) -> Option<(u64, String)> {
        let piece = std::mem::take(&mut self.waiting);
        self.hand_on(piece)
    }

    fn hand_on(&mut self, mut piece: String) -> Option<(u64, String)> {
        let room = self
            .cap
            .saturating_sub(usize::try_from(self.sent).unwrap_or(usize::MAX));
        if piece.len() > room {
            let at = (0..=room)
                .rev()
                .find(|&i| piece.is_char_boundary(i))
                .unwrap_or(0);
            piece.truncate(at);
            self.cut = true;
        }
        if piece.is_empty() {
            return None;
        }
        let offset = self.sent;
        self.sent += piece.len() as u64;
        Some((offset, piece))
    }

    /// Whether the cap took some of it.
    pub(crate) fn was_cut(&self) -> bool {
        self.cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::ApiKey;

    #[test]
    fn offsets_count_bytes_and_a_key_over_two_pieces_never_shows() {
        let r = Redactor::new(Some(ApiKey::new("sk-key")));
        let mut part = PartText::new(usize::MAX);
        assert_eq!(part.push("gr\u{fc}", &r), Some((0, "gr\u{fc}".into())));
        assert_eq!(part.push("\u{df} s", &r), Some((4, "\u{df} ".into())));
        assert_eq!(part.push("k-", &r), None);
        assert_eq!(part.push("key!", &r), Some((7, "[redacted]!".into())));
        assert_eq!(part.push(" sk", &r), Some((18, " ".into())));
        assert_eq!(part.flush(), Some((19, "sk".into())));
    }

    #[test]
    fn the_cap_ends_on_a_whole_code_point_and_says_so() {
        let r = Redactor::default();
        let mut part = PartText::new(3);
        assert_eq!(part.push("a\u{1f600}", &r), Some((0, "a".into())));
        assert!(part.was_cut());
        assert_eq!(part.push("b", &r), None);
    }

    #[test]
    fn nul_never_counts() {
        let r = Redactor::default();
        let mut part = PartText::new(usize::MAX);
        assert_eq!(part.push("a\0b", &r), Some((0, "ab".into())));
    }
}
