use crate::{parse_message, Message};

#[derive(Debug)]
pub struct StreamParser {
    buffer: Vec<u8>,
}

impl Default for StreamParser {
    fn default() -> Self {
        Self {
            buffer: Vec::with_capacity(512),
        }
    }
}

impl StreamParser {
    pub fn try_read(&mut self, buf: &[u8]) -> impl Iterator<Item = anyhow::Result<Message>> {
        self.buffer.extend_from_slice(buf);

        let mut messages = vec![];

        let mut iter = self.buffer.split(|&c| c == b'\r' || c == b'\n').peekable();

        let end = loop {
            let line = iter.next().unwrap();
            let is_last = iter.peek().is_none();

            if is_last {
                break line;
            } else if !line.is_empty() {
                let result = parse_message(line);
                let result = result
                    .map(|(_, msg)| msg)
                    .map_err(|err| anyhow::anyhow!(err.to_string()));
                messages.push(result);
            }
        };

        self.buffer = end.to_vec();

        messages.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::StreamParser;

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
