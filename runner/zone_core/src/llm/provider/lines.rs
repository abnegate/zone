//! Newline framing for a child process reading in arbitrary chunks.

/// One piece of a framed stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    Line(String),
    /// A line longer than the limit, dropped whole.
    Dropped,
}

/// Splits a byte stream into lines across chunk boundaries.
///
/// Framing happens on the raw bytes rather than on decoded text: a read can
/// land in the middle of a multi-byte character, and decoding each chunk on
/// its own would corrupt it. Byte `0x0A` cannot occur inside a UTF-8 sequence,
/// so splitting first and decoding whole lines afterwards is always safe.
///
/// A line longer than the limit is never held: its bytes are discarded as they
/// arrive, up to the newline that ends it.
#[derive(Debug)]
pub struct Lines {
    buffer: Vec<u8>,
    limit: usize,
    dropping: bool,
}

impl Lines {
    pub fn new(limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            limit,
            dropping: false,
        }
    }

    pub fn extend(&mut self, chunk: &[u8]) {
        let chunk = if self.dropping {
            let Some(end) = chunk.iter().position(|byte| *byte == b'\n') else {
                return;
            };
            self.dropping = false;
            &chunk[end + 1..]
        } else {
            chunk
        };
        self.buffer.extend_from_slice(chunk);
    }

    /// The next whole line, or `None` while one is still arriving.
    pub fn take(&mut self) -> Option<Frame> {
        match self.buffer.iter().position(|byte| *byte == b'\n') {
            Some(end) => {
                let frame = if end > self.limit {
                    Frame::Dropped
                } else {
                    Frame::Line(decode(&self.buffer[..end]))
                };
                self.buffer.drain(..=end);
                Some(frame)
            }
            None if self.buffer.len() > self.limit => {
                self.buffer = Vec::new();
                self.dropping = true;
                Some(Frame::Dropped)
            }
            None => None,
        }
    }

    /// The trailing line of a stream that ended without a final newline.
    pub fn flush(&mut self) -> Option<Frame> {
        self.dropping = false;
        let rest = std::mem::take(&mut self.buffer);
        if rest.is_empty() {
            None
        } else if rest.len() > self.limit {
            Some(Frame::Dropped)
        } else {
            Some(Frame::Line(decode(&rest)))
        }
    }
}

fn decode(line: &[u8]) -> String {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    String::from_utf8_lossy(line).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(lines: &mut Lines) -> Vec<Frame> {
        std::iter::from_fn(|| lines.take()).collect()
    }

    fn line(text: &str) -> Frame {
        Frame::Line(text.to_string())
    }

    #[test]
    fn a_line_split_across_two_reads_is_rejoined() {
        let mut lines = Lines::new(1024);
        lines.extend(b"{\"type\":\"as");
        assert!(drain(&mut lines).is_empty());

        lines.extend(b"sistant\"}\n");
        assert_eq!(drain(&mut lines), [line(r#"{"type":"assistant"}"#)]);
    }

    #[test]
    fn several_lines_in_one_read_all_come_back() {
        let mut lines = Lines::new(1024);
        lines.extend(b"one\ntwo\nthree\n");
        assert_eq!(drain(&mut lines), [line("one"), line("two"), line("three")]);
    }

    #[test]
    fn a_multibyte_character_split_across_reads_survives() {
        let mut lines = Lines::new(1024);
        let text = "café ☕".as_bytes();
        let (head, tail) = text.split_at(5);

        lines.extend(head);
        assert!(drain(&mut lines).is_empty());
        lines.extend(tail);
        lines.extend(b"\n");

        assert_eq!(drain(&mut lines), [line("café ☕")]);
    }

    #[test]
    fn carriage_returns_are_stripped_but_empty_lines_survive() {
        let mut lines = Lines::new(1024);
        lines.extend(b"one\r\n\r\ntwo\r\n");
        assert_eq!(drain(&mut lines), [line("one"), line(""), line("two")]);
    }

    #[test]
    fn a_stream_ending_without_a_newline_still_yields_its_last_line() {
        let mut lines = Lines::new(1024);
        lines.extend(b"first\nlast without newline");

        assert_eq!(drain(&mut lines), [line("first")]);
        assert_eq!(lines.flush(), Some(line("last without newline")));
        assert_eq!(lines.flush(), None);
    }

    #[test]
    fn a_terminated_line_past_the_cap_is_dropped_and_the_next_one_still_arrives() {
        let mut lines = Lines::new(8);
        lines.extend(b"way past the cap but terminated\nnext\n");

        assert_eq!(drain(&mut lines), [Frame::Dropped, line("next")]);
    }

    #[test]
    fn an_unterminated_line_past_the_cap_is_skipped_to_its_newline_across_reads() {
        let mut lines = Lines::new(8);
        lines.extend(b"way past the cap");
        assert_eq!(drain(&mut lines), [Frame::Dropped]);

        lines.extend(b" and still going");
        assert!(
            drain(&mut lines).is_empty(),
            "one dropped line was reported twice"
        );

        lines.extend(b" to its end\nnext\npart");
        assert_eq!(drain(&mut lines), [line("next")]);
        assert_eq!(lines.flush(), Some(line("part")));
    }

    #[test]
    fn a_dropped_line_is_never_held() {
        let mut lines = Lines::new(8);
        lines.extend(b"way past the cap");
        drain(&mut lines);
        lines.extend(&[b'x'; 4096]);
        drain(&mut lines);

        assert!(lines.buffer.is_empty(), "{} bytes held", lines.buffer.len());
    }

    #[test]
    fn a_stream_ending_inside_a_dropped_line_yields_nothing_more() {
        let mut lines = Lines::new(8);
        lines.extend(b"way past the cap");
        assert_eq!(drain(&mut lines), [Frame::Dropped]);
        lines.extend(b" and never ending");

        assert_eq!(lines.flush(), None);
    }

    #[test]
    fn an_unterminated_last_line_past_the_cap_is_dropped() {
        let mut lines = Lines::new(8);
        lines.extend(b"first\n12345678");
        assert_eq!(drain(&mut lines), [line("first")]);
        assert_eq!(lines.flush(), Some(line("12345678")));

        lines.extend(b"123456789");
        assert_eq!(lines.flush(), Some(Frame::Dropped));
    }

    #[test]
    fn a_line_exactly_at_the_cap_is_accepted() {
        let mut lines = Lines::new(8);
        lines.extend(b"12345678\n");

        assert_eq!(lines.take(), Some(line("12345678")));
    }
}
