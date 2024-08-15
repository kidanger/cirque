use ::lending_iterator::prelude::*;
pub use lending_iterator::LendingIterator;
use slice_ring_buffer::SliceRingBuffer;
use tokio::io::AsyncRead;

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
    pub fn feed_from_slice(&mut self, buf: &[u8]) {
        self.buffer.extend_from_slice(buf);
    }

    pub async fn feed_from_stream(&mut self, stream: impl AsyncRead) -> anyhow::Result<()> {
        assert!(self.buffer.len() < self.buffer.capacity());

        // SAFETY: there is still space in the buffer since it is many times larger than an IRC
        // maximum message length
        let rest = unsafe { self.buffer.tail_head_slice() };
        assert!(!rest.is_empty());

        // https://users.rust-lang.org/t/how-to-async-read-into-mut-mem-maybeuninit-u8-with-tokio/77743/2
        let mut read_buf = tokio::io::ReadBuf::uninit(rest);
        let mut pinned = std::pin::pin!(stream);
        std::future::poll_fn(|cx| pinned.as_mut().poll_read(cx, &mut read_buf)).await?;

        let count = read_buf.filled().len();
        anyhow::ensure!(count > 0, "stream ended");
        assert!(count <= rest.len());

        // SAFETY: we can move the tail up to rest.len(), as count is <= rest.len()
        // also elements were initialized by the poll_read
        unsafe {
            self.buffer.move_tail(count as isize);
        }

        Ok(())
    }

    pub fn consume_iter(&mut self) -> MessageIterator {
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
}
