use ::lending_iterator::prelude::*;
pub use lending_iterator::LendingIterator;
use slice_ring_buffer::SliceRingBuffer;

use crate::parser::parse_message;
use crate::Message;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsingError(String);

impl std::fmt::Display for ParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParsingError {}

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
    pub fn feed_from_slice(&mut self, buf: &[u8]) {
        self.buffer.extend_from_slice(buf);
    }

    pub fn consume_iter(&mut self) -> MessageIterator<'_> {
        MessageIterator {
            stream_parser: self,
            buffer_is_full: false,
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

unsafe impl bytes::BufMut for StreamParser {
    fn remaining_mut(&self) -> usize {
        self.buffer.capacity() - self.buffer.len()
    }

    unsafe fn advance_mut(&mut self, count: usize) {
        // SAFETY: we can move the tail up to remaining_mut
        // also elements were initialized by the caller of chunk_mut
        unsafe {
            self.buffer.move_tail(count as isize);
        }
    }

    fn chunk_mut(&mut self) -> &mut bytes::buf::UninitSlice {
        // SAFETY:
        // - the data won't be mutated as it is used only by MessageIterator which hold the mutable
        //   reference to StreamParser, hence the owned buffer can't be mutated
        // - unclear what else could be needed from tail_head_slice
        unsafe { self.buffer.tail_head_slice() }.into()
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
    // SAFETY: cur is less than the size of the buffer
    // and we don't care about dropping u8
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
    // SAFETY: cur is less than the size of the buffer
    // and we don't care about dropping u8
    unsafe {
        buf.move_head(cur);
    }

    // restrict the message length to 512 characters
    // this is not strictly necessary but might prevent some abuse
    // the head of the buffer was still moved by the correct amont (till the end of line),
    // so this truncates the line but keep the next message clean
    let cur = cur.min(512);

    // retrieve the line from the pointer
    // SAFETY:
    // - the data is aligned because it is u8, and of size `cur` since it was part of the buffer
    // - the data is initialized since it is part of the buffer
    // - the data won't be mutated as it is used only by MessageIterator which hold the mutable
    //   reference to StreamParser, hence the owned buffer can't be mutated
    let line = unsafe { std::slice::from_raw_parts(line_start_ptr, cur as usize) };
    Some(line)
}

pub struct MessageIterator<'a> {
    stream_parser: &'a mut StreamParser,
    buffer_is_full: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageIteratorError {
    ParsingError(ParsingError),
    BufferFullWithoutMessageError,
}

#[gat]
impl LendingIterator for MessageIterator<'_> {
    type Item<'next>
    where
        Self: 'next,
    = Result<Message<'next>, MessageIteratorError>;

    fn next(&mut self) -> Option<Result<Message<'_>, MessageIteratorError>> {
        if self.buffer_is_full {
            return None;
        }

        let is_full = self.stream_parser.buffer.is_full();
        let line = consume_line(&mut self.stream_parser.buffer);
        if line.is_none() && is_full {
            // next iteration should stop
            self.buffer_is_full = true;
            return Some(Err(MessageIteratorError::BufferFullWithoutMessageError {}));
        }

        let line = line?;
        let result = parse_message(line);
        let result = result
            .map(|(_, msg)| msg)
            .map_err(|err| MessageIteratorError::ParsingError(ParsingError(err.to_string())));

        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use bytes::BufMut;
    use lending_iterator::LendingIterator;

    use crate::MessageIteratorError;

    use super::StreamParser;

    #[test]
    fn test_empty() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn test_one() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"CMD\r\n");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_one_and_half() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"CMD\r\nCA");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_two() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"CMD\nCAP\r");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 2);
    }

    #[test]
    fn test_one_plus_one() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"CMD\n");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);
        sp.feed_from_slice(b"CAP\r");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_one_plus_one_2() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"CMD\nCAP");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);
        sp.feed_from_slice(b"\n");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);
    }

    #[test]
    fn test_fill_100m_10b() {
        use bytes::BufMut;
        use std::io::Write;
        let mut sp = StreamParser::default();
        let mut writer = sp.writer();
        for _ in 0..100 {
            let c = writer.write(b"012345678\n").unwrap();
            assert_eq!(c, 10);
        }

        sp = writer.into_inner();
        let iter = sp.consume_iter();
        let m = iter.count();
        assert_eq!(m, 100);
    }

    #[test]
    fn test_fill_0m() {
        let mut sp = StreamParser::default();
        let mut writer = sp.writer();
        for _ in 0..100 {
            writer.write_all(b"012345678\n").unwrap();
        }

        sp = writer.into_inner();
        let iter = sp.consume_iter();
        let m = iter.count();
        assert_eq!(m, 100);
    }

    #[test]
    fn test_fill2() {
        let sp = StreamParser::default();
        let mut writer = sp.writer();

        // write 4096 bytes without end of line
        for _ in 0..4096 {
            writer.write_all(b"0").unwrap();
        }
        // cannot push another byte
        writer.write_all(b"0").unwrap_err();

        // make sure it only produces a single item
        let mut sp = writer.into_inner();
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 1);

        // make sure the item is the "BufferFullWithoutMessageError"
        let mut iter = sp.consume_iter();
        let err = iter.next().unwrap().unwrap_err();
        assert_eq!(err, MessageIteratorError::BufferFullWithoutMessageError);
    }
}
