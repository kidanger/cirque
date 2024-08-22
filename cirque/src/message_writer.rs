use std::io::Write;

use tokio::sync::mpsc::{error::TryRecvError, UnboundedReceiver, UnboundedSender};

use crate::server_to_client::{Message, MessageContext};

pub(crate) type SerializedMessage = Vec<u8>;

#[derive(Debug)]
pub(crate) struct Mailbox {
    sender: UnboundedSender<SerializedMessage>,
}

impl Mailbox {
    pub(crate) fn new() -> (Self, MailboxSink) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        (Self { sender }, MailboxSink { receiver })
    }

    pub(crate) fn ingest(&self, message: &Message, context: &MessageContext) {
        let mut mw = MessageWriter { mailbox: self };
        message.write_to(&mut mw, context);
    }
}

#[derive(Debug)]
pub(crate) struct MailboxSink {
    receiver: UnboundedReceiver<SerializedMessage>,
}

impl MailboxSink {
    pub(crate) async fn recv(&mut self) -> Option<SerializedMessage> {
        self.receiver.recv().await
    }

    pub(crate) fn try_recv(&mut self) -> Result<SerializedMessage, TryRecvError> {
        self.receiver.try_recv()
    }
}

/// A single server_to_client::Message might generate multiple 512-bytes IRC messages.
/// This struct offers a safe interface to write multiple IRC message, while ensuring to respect the
/// size limit of 512 bytes per IRC message.
pub(crate) struct MessageWriter<'m> {
    mailbox: &'m Mailbox,
}

impl MessageWriter<'_> {
    pub(crate) fn new_message(&self) -> OnGoingMessage {
        let buf = vec![0_u8; 512].into_boxed_slice();
        let buf = std::io::Cursor::new(buf);
        OnGoingMessage {
            buf,
            mailbox: self.mailbox,
        }
    }
}

#[must_use]
pub(crate) struct OnGoingMessage<'m> {
    buf: std::io::Cursor<Box<[u8]>>,
    mailbox: &'m Mailbox,
}

macro_rules! message {
    ($s:expr, $($args:expr),*) => {{
        let mut m = $s.new_message();
        $(
            m = m.write($args);
        )*
        m.validate();
    }}
}

macro_rules! message_push {
    ($m:ident, $($args:expr),*) => {{
        $(
            $m = $m.write($args);
        )*
    }}
}

impl OnGoingMessage<'_> {
    #[inline]
    pub(crate) fn write<T>(mut self, bytes: &T) -> Self
    where
        T: AsRef<[u8]>,
    {
        let _ = self.buf.write_all(bytes.as_ref());
        self
    }

    pub(crate) fn validate(mut self) {
        // cut at 510 bytes and add new lines
        let pos = self.buf.position().min(510);
        self.buf.set_position(pos);
        let _ = self.buf.write_all(b"\r\n");

        // convert into a Vec<u8>
        let len = self.buf.position() as usize;
        let mut buf = self.buf.into_inner().into_vec();
        buf.truncate(len);

        // send
        let _ = self.mailbox.sender.send(buf);
    }
}

#[cfg(test)]
mod tests {
    use super::{Mailbox, MessageWriter};

    #[test]
    fn test_empty() {
        let (mailbox, mut sink) = Mailbox::new();
        let _mw = MessageWriter { mailbox: &mailbox };
        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_1message() {
        let (mailbox, mut sink) = Mailbox::new();
        let mw = MessageWriter { mailbox: &mailbox };
        message!(mw, &"test");
        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg).unwrap(), "test\r\n");
        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_2message() {
        let (mailbox, mut sink) = Mailbox::new();
        let mw = MessageWriter { mailbox: &mailbox };

        message!(mw, b"ta", b"2");
        message!(mw, b"toto");

        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg).unwrap(), "ta2\r\n");

        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg).unwrap(), "toto\r\n");

        sink.try_recv().unwrap_err();
    }
}
