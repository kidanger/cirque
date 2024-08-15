use ::lending_iterator::prelude::*;
pub use lending_iterator::LendingIterator;
use slice_ring_buffer::SliceRingBuffer;

use crate::{parse_message, Message};

#[derive(Debug)]
pub struct StreamParser {
    buffer: SliceRingBuffer<u8>,
}

impl Default for StreamParser {
    fn default() -> Self {
        Self {
            buffer: SliceRingBuffer::with_capacity(4096),
        }
    }
}

impl StreamParser {
    pub fn try_read(&mut self, buf: &[u8]) -> MessageIterator {
        self.buffer.extend_from_slice(buf);

        MessageIterator {
            stream_parser: self,
        }
    }
}

fn is_end_of_message(c: &u8) -> bool {
    *c == b'\r' || *c == b'\n'
}

fn consume_line(buf: &mut SliceRingBuffer<u8>) -> Option<&[u8]> {
    if buf.is_empty() {
        return None;
    }

    // eat the EOL characters
    let mut cur = 0;
    while cur < buf.len() as isize - 1 && is_end_of_message(&buf[cur as usize]) {
        cur += 1;
    }

    // move the buffer forward to skip the EOL characters
    unsafe {
        buf.move_head(cur);
    }

    // find the next EOL characters
    let mut cur = 0;
    while cur < buf.len() as isize - 1 && !is_end_of_message(&buf[cur as usize]) {
        cur += 1;
    }

    // if no EOL found, then the message is not finished
    if cur == 0 || !is_end_of_message(&buf[cur as usize]) {
        return None;
    }

    // keep track of the start of the line
    let line_start_ptr = buf.as_ptr();

    // move the buffer forward to skip the line (will arrive at the EOL character)
    unsafe {
        buf.move_head(cur);
    }

    // retrieve the line from the pointer
    let line = unsafe { std::slice::from_raw_parts(line_start_ptr, cur as usize) };
    Some(line)
}

pub struct MessageIterator<'a> {
    stream_parser: &'a mut StreamParser,
}

#[gat]
impl LendingIterator for MessageIterator<'_> {
    type Item<'next>
    where
        Self: 'next,
    = Result<Message<'next>, anyhow::Error>;

    fn next(&mut self) -> Option<Result<Message, anyhow::Error>> {
        let line = consume_line(&mut self.stream_parser.buffer)?;
        let result = parse_message(line);
        let result = result
            .map(|(_, msg)| msg)
            .map_err(|err| anyhow::anyhow!(err.to_string()));
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::StreamParser;
    use lending_iterator::LendingIterator;

    #[test]
    fn test_empty() {
        let mut sp = StreamParser::default();
        let iter = sp.try_read(b"");
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn test_one() {
        let mut sp = StreamParser::default();
        let iter = sp.try_read(b"CMD\r\n");
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_one_and_half() {
        let mut sp = StreamParser::default();
        let iter = sp.try_read(b"CMD\r\nCA");
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_two() {
        let mut sp = StreamParser::default();
        let iter = sp.try_read(b"CMD\nCAP\r");
        assert_eq!(iter.count(), 2);
    }

    #[test]
    fn test_one_plus_one() {
        let mut sp = StreamParser::default();
        let iter = sp.try_read(b"CMD\n");
        assert_eq!(iter.count(), 1);
        let iter = sp.try_read(b"CAP\r");
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_one_plus_one_2() {
        let mut sp = StreamParser::default();
        let iter = sp.try_read(b"CMD\nCAP");
        assert_eq!(iter.count(), 1);
        let iter = sp.try_read(b"\n");
        assert_eq!(iter.count(), 1);
    }
}
