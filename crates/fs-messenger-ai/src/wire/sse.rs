//! [`SseReader`]: bytes to server-sent events, per the WHATWG format.
//!
//! Pattern: Iterator (by hand, fed by chunks): a line is taken only when whole, so a multi-byte character
//! split across chunks arrives whole. A line must be UTF-8: the format says so, and a replacement
//! character would be a guess in somebody's text. `id:` and `retry:` are ignored: this reader never
//! reconnects, so a server cannot steer it.

use crate::turn::TurnFailure;

/// One event: its name, if any, and its data lines joined by `\n`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SseEvent {
    pub(crate) name: Option<String>,
    pub(crate) data: String,
}

/// Holds at most `limit` bytes of an unfinished line or event.
pub(crate) struct SseReader {
    limit: usize,
    line: Vec<u8>,
    name: Option<String>,
    data: String,
    has_data: bool,
    skip_lf: bool,
    started: bool,
}

const BOM: &str = "\u{feff}";

impl SseReader {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            line: vec![],
            name: None,
            data: String::new(),
            has_data: false,
            skip_lf: false,
            started: false,
        }
    }

    /// The events `chunk` completes.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseEvent>, TurnFailure> {
        let mut events = vec![];
        for &byte in chunk {
            let lf = byte == b'\n';
            if std::mem::take(&mut self.skip_lf) && lf {
                continue;
            }
            if lf || byte == b'\r' {
                self.skip_lf = byte == b'\r';
                let line = std::mem::take(&mut self.line);
                if let Some(event) = self.end_of_line(&line)? {
                    events.push(event);
                }
            } else {
                self.line.push(byte);
                if self.line.len() + self.data.len() > self.limit {
                    return Err(TurnFailure::Malformed("an event over the limit".into()));
                }
            }
        }
        Ok(events)
    }

    fn end_of_line(&mut self, line: &[u8]) -> Result<Option<SseEvent>, TurnFailure> {
        let text = std::str::from_utf8(line).map_err(|_| TurnFailure::InvalidUtf8)?;
        let text = if self.started {
            text
        } else {
            self.started = true;
            text.strip_prefix(BOM).unwrap_or(text)
        };
        if text.is_empty() {
            let name = self.name.take();
            if !std::mem::take(&mut self.has_data) {
                return Ok(None);
            }
            let data = std::mem::take(&mut self.data);
            return Ok(Some(SseEvent { name, data }));
        }
        let (field, value) = text.split_once(':').unwrap_or((text, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.name = Some(value.to_string()),
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            // A comment, `id`, `retry`, or a field nobody defined.
            _ => {}
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(bytes: &[u8]) -> Result<Vec<SseEvent>, TurnFailure> {
        SseReader::new(1024).feed(bytes)
    }

    fn event(name: Option<&str>, data: &str) -> SseEvent {
        SseEvent {
            name: name.map(str::to_string),
            data: data.into(),
        }
    }

    #[test]
    fn events_end_at_a_blank_line_whichever_line_end() {
        for bytes in [
            &b"event: a\ndata: b\n\n"[..],
            b"event: a\r\ndata: b\r\n\r\n",
            b"event: a\rdata: b\r\r",
        ] {
            assert_eq!(all(bytes).unwrap(), [event(Some("a"), "b")], "{bytes:?}");
        }
    }

    #[test]
    fn data_lines_join_and_comments_and_other_fields_go() {
        let bytes = b": ping\nid: 1\nretry: 9\ndata:a\ndata: b\nfoo\n\n";
        assert_eq!(all(bytes).unwrap(), [event(None, "a\nb")]);
    }

    #[test]
    fn an_event_without_data_is_none_and_its_name_does_not_stick() {
        assert_eq!(all(b"event: a\n\ndata: b\n\n").unwrap(), [event(None, "b")]);
    }

    #[test]
    fn one_byte_at_a_time_gives_the_same_and_a_split_character_arrives_whole() {
        let bytes = "\u{feff}data: gr\u{fc}\u{df}e \u{1f600}\r\n\r\n".as_bytes();
        let mut reader = SseReader::new(1024);
        let mut events = vec![];
        for byte in bytes {
            events.extend(reader.feed(std::slice::from_ref(byte)).unwrap());
        }
        assert_eq!(events, [event(None, "gr\u{fc}\u{df}e \u{1f600}")]);
    }

    #[test]
    fn invalid_utf8_and_an_endless_line_are_refused() {
        assert_eq!(all(b"data: \xff\n\n"), Err(TurnFailure::InvalidUtf8));
        let long = vec![b'a'; 2000];
        assert!(matches!(all(&long), Err(TurnFailure::Malformed(_))));
        let many = b"data: aaaaaaaaaaaaaaaaaaaa\n".repeat(100);
        assert!(matches!(all(&many), Err(TurnFailure::Malformed(_))));
    }

    #[test]
    fn an_unfinished_event_is_not_delivered() {
        assert_eq!(all(b"data: a\n").unwrap(), []);
    }
}
