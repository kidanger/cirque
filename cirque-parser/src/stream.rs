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
    pub(crate) buffer: SliceRingBuffer<u8>,
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
        if self.buffer.is_full() && !self.buffer.contains(&b'\n') {
            log::warn!("buffer full without valid message, resetting");
            self.buffer.clear();
        }
        MessageIterator {
            stream_parser: self,
        }
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

#[inline]
fn is_end_of_message(c: u8) -> bool {
    c == b'\r' || c == b'\n'
}

fn consume_line(buf: &mut SliceRingBuffer<u8>) -> Option<&[u8]> {
    assert!(buf.capacity() > 512);

    #[allow(clippy::indexing_slicing)]
    while !buf.is_empty() && is_end_of_message(buf[0]) {
        // move the buffer forward to skip the EOL character
        // SAFETY: the buffer is not empty and we eat only one character
        // (and we don't care about dropping u8)
        unsafe {
            buf.move_head_unchecked(1);
        }
    }

    // keep track of the start of the line for later
    let line_start_ptr = buf.as_ptr();

    // compute the length of the line (until the next EOL)
    // if there is no EOL, return None
    let line_length = buf
        .iter()
        .enumerate()
        .filter_map(|(i, &c)| is_end_of_message(c).then_some(i))
        .next()?;

    // move the buffer forward to skip the line
    // SAFETY: 'line_length' is within the buffer's bound
    // (and we don't care about dropping u8)
    unsafe {
        buf.move_head_unchecked(line_length as isize);
    }

    // restrict the message length to 512 characters
    // this is not strictly necessary but might prevent some abuse
    // the head of the buffer was still moved by the correct amont (till the end of line),
    // so this truncates the line but keep the next message clean
    let line_length = line_length.min(512);

    // retrieve the line from the pointer
    // SAFETY:
    // - the data is aligned because it is u8, and of size `cur` since it was part of the buffer
    // - the data is initialized since it is part of the buffer
    // - the data won't be mutated as it is used only by MessageIterator which hold the mutable
    //   reference to StreamParser, hence the owned buffer can't be mutated
    let line = unsafe { std::slice::from_raw_parts(line_start_ptr, line_length) };
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
    = Result<Message<'next>, ParsingError>;

    fn next(&mut self) -> Option<Result<Message<'_>, ParsingError>> {
        let line = consume_line(&mut self.stream_parser.buffer)?;

        let result = parse_message(line);
        let result = result
            .map(|(_, msg)| msg)
            .map_err(|err| ParsingError(err.to_string()));

        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use bytes::BufMut;
    use lending_iterator::LendingIterator;

    use super::StreamParser;

    #[test]
    fn test_empty() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn test_zero() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"C");
        let iter = sp.consume_iter();
        assert_eq!(iter.count(), 0);
    }

    #[test]
    fn test_zero_2() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"\n");
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
    fn test_short_one() {
        let mut sp = StreamParser::default();
        sp.feed_from_slice(b"C\n");
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
    fn test_fill_100m() {
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
    fn test_fill_no_endofline() {
        let sp = StreamParser::default();
        let mut sp = {
            let mut writer = sp.writer();

            // write 4096 bytes without end of line
            for _ in 0..4096 {
                writer.write_all(b"0").unwrap();
            }
            writer.into_inner()
        };
        // buffer is full, without new lines
        assert!(sp.buffer.is_full());

        // make sure it doesnt not yield any message...
        let mut iter = sp.consume_iter();
        assert!(iter.next().is_none());
        // and that the buffer is empty
        assert!(sp.buffer.is_empty());
    }

    #[test]
    fn test_2message_but_many_endoflines() {
        let sp = StreamParser::default();

        let mut writer = sp.writer();
        writer.write_all(b"\r\nto\r\n\n\r\nta\r\n\r\n").unwrap();
        let mut sp = writer.into_inner();

        let mut iter = sp.consume_iter();
        {
            let a = iter.next().unwrap().unwrap();
            assert_eq!(a.command(), b"to");
        }
        {
            let a = iter.next().unwrap().unwrap();
            assert_eq!(a.command(), b"ta");
        }

        let mut writer = sp.writer();
        writer.write_all(b"aa\r\na").unwrap();
        let mut sp = writer.into_inner();

        let mut iter = sp.consume_iter();
        let a = iter.next().unwrap().unwrap();
        assert_eq!(a.command(), b"aa");
    }
}
