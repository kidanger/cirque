use std::{io::Write, marker::PhantomData};

use tokio::sync::mpsc::{error::TryRecvError, Permit, Receiver, Sender};

use crate::server_to_client::{self, MessageContext};

const IRC_MESSAGE_MAX_SIZE: usize = 512;

#[derive(Debug)]
pub struct SerializedMessage {
    bytes: Vec<u8>,
    is_important: bool,
}

impl SerializedMessage {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_important(&self) -> bool {
        self.is_important
    }
}

#[derive(Debug)]
pub(crate) struct Mailbox {
    sender: Sender<SerializedMessage>,
}

impl Mailbox {
    pub(crate) fn new(capacity: usize) -> (Self, MailboxSink) {
        let (sender, receiver) = tokio::sync::mpsc::channel(capacity);
        (Self { sender }, MailboxSink { receiver })
    }

    pub(crate) fn ingest(&self, message: &server_to_client::Message<'_>, context: &MessageContext) {
        if self.sender.is_closed() {
            return;
        }

        let mut mw = self.writer(message.is_important());
        message.write_to(&mut mw, context);
    }

    fn writer(&self, messages_are_important: bool) -> MessageWriter<'_> {
        MessageWriter {
            mailbox: self,
            messages_are_important,
        }
    }
}

#[derive(Debug)]
pub struct MailboxSink {
    receiver: Receiver<SerializedMessage>,
}

impl MailboxSink {
    pub async fn recv(&mut self) -> Option<SerializedMessage> {
        self.receiver.recv().await
    }

    pub fn try_recv(&mut self) -> Result<SerializedMessage, TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn close(&mut self) {
        self.receiver.close();
    }
}

/// A single server_to_client::Message might generate multiple 512-bytes IRC messages.
/// This struct offers a safe interface to write multiple IRC message, while ensuring to respect the
/// size limit of 512 bytes per IRC message.
pub(crate) struct MessageWriter<'m> {
    mailbox: &'m Mailbox,
    messages_are_important: bool,
}

impl<'m> MessageWriter<'m> {
    /// Implementation note: it is not necessary to have a &mut self here,
    /// but it allows to ensure that there are only one OnGoingMessage at a time.
    /// It makes it harder to send IRC messages in the wrong order.
    ///
    /// If the mailbox is full, returns None. This allows to skip allocation and buffer preparation
    /// for nothing, as the message won't be sent anyway.
    pub(crate) fn new_message<'w>(&'w mut self) -> Option<OnGoingMessage<'m, 'w>> {
        let permit = self.mailbox.sender.try_reserve().ok()?;
        let buf = vec![0_u8; IRC_MESSAGE_MAX_SIZE].into();
        let buf = std::io::Cursor::new(buf);
        Some(OnGoingMessage {
            buf,
            permit,
            is_important: self.messages_are_important,
            phantom: PhantomData,
        })
    }
}

/// Owner MUST call validate() after writing in order to send the message to the mailbox.
#[must_use = "You should call OnGoingMessage::validate() to send the message."]
pub(crate) struct OnGoingMessage<'m, 'w> {
    buf: std::io::Cursor<Box<[u8]>>,
    permit: Permit<'m, SerializedMessage>,
    is_important: bool,
    phantom: PhantomData<&'w mut MessageWriter<'m>>,
}

impl OnGoingMessage<'_, '_> {
    #[inline]
    pub(crate) fn write<T>(mut self, bytes: &T) -> Self
    where
        T: AsRef<[u8]>,
    {
        // might fail if the message goes beyond IRC_MESSAGE_MAX_SIZE bytes
        // but this is OK, the write fails and validate() will overwrite
        // the last bytes by the end-of-line markers
        let _ = self.buf.write_all(bytes.as_ref());
        self
    }

    pub(crate) fn validate(self) {
        fn is_utf8_char_boundary(c: u8) -> bool {
            // see u8::is_utf8_char_boundary (private method)
            (c as i8) >= -0x40
        }

        let len = self.buf.position() as usize;
        let mut buf = self.buf.into_inner().into_vec();

        let len = len.min(
            // If the buffer is filled at least until 510 bytes,
            // we cut the message in order to leave space for appending "\r\n".
            // We find the best place to cut the message, <=510 and avoiding to split an utf8 char.
            buf.get(..=IRC_MESSAGE_MAX_SIZE - 2)
                .and_then(|slice| slice.iter().rposition(|&b| is_utf8_char_boundary(b)))
                .unwrap_or(0),
        );

        // cut the message and \r\n
        buf.truncate(len);
        buf.push(b'\r');
        buf.push(b'\n');

        // send
        self.permit.send(SerializedMessage {
            bytes: buf,
            is_important: self.is_important,
        });
    }
}

macro_rules! message {
    ($s:expr, $($args:expr),*) => {{
        let mut m = $s.new_message()?;
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

#[cfg(test)]
mod tests {
    use super::Mailbox;

    macro_rules! message {
        ($s:expr, $($args:expr),*) => {{
            let mut m = $s.new_message().unwrap();
            $(
                m = m.write($args);
            )*
            m.validate();
        }}
    }

    #[test]
    fn test_empty() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let _mw = mailbox.writer(false);
        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_empty_message() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);
        message!(mw, &"");
        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg.bytes).unwrap(), "\r\n");
        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_drop_message_on_full() {
        let (mailbox, mut sink) = Mailbox::new(2);
        let mut mw = mailbox.writer(false);
        mw.new_message().unwrap().validate();
        mw.new_message().unwrap().validate();
        assert!(mw.new_message().is_none());
        sink.try_recv().unwrap();
        sink.try_recv().unwrap();
        sink.try_recv().unwrap_err();

        // the mailbox still works after reading messages
        mw.new_message().unwrap().validate();
        mw.new_message().unwrap().validate();
        assert!(mw.new_message().is_none());
        sink.try_recv().unwrap();
        sink.try_recv().unwrap();
        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_1message() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);
        message!(mw, &"test");
        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg.bytes).unwrap(), "test\r\n");
        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_2messages() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        message!(mw, b"ta", b"2");
        message!(mw, b"toto");

        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg.bytes).unwrap(), "ta2\r\n");

        let msg = sink.try_recv().unwrap();
        assert_eq!(String::from_utf8(msg.bytes).unwrap(), "toto\r\n");

        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_long_message_509() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        let mut m = mw.new_message().unwrap();
        for _ in 0..499 {
            message_push!(m, b"a");
        }
        message_push!(m, b"0123456789");
        m.validate();

        let msg = sink.try_recv().unwrap();
        let msg = String::from_utf8(msg.bytes).unwrap();
        assert_eq!(msg.len(), 511);
        assert!(msg.ends_with("9\r\n"));

        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_long_message_510() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        let mut m = mw.new_message().unwrap();
        for _ in 0..500 {
            message_push!(m, b"a");
        }
        message_push!(m, b"0123456789");
        m.validate();

        let msg = sink.try_recv().unwrap();
        let msg = String::from_utf8(msg.bytes).unwrap();
        assert_eq!(msg.len(), 512);
        assert!(msg.ends_with("9\r\n"));

        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_long_message_511() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        let mut m = mw.new_message().unwrap();
        for _ in 0..501 {
            message_push!(m, b"a");
        }
        message_push!(m, b"0123456789");
        m.validate();

        let msg = sink.try_recv().unwrap();
        let msg = String::from_utf8(msg.bytes).unwrap();
        assert_eq!(msg.len(), 512);
        assert!(msg.ends_with("8\r\n"));

        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_long_message_511_utf8_cut() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        let mut m = mw.new_message().unwrap();
        for _ in 0..499 {
            message_push!(m, b"a");
        }
        message_push!(m, b"0123456789");
        message_push!(m, b"\xc3\xa9");
        m.validate();
        // the message has size 511, but to happen \r\n the last byte has to be removed to
        // 510. Because removing this last byte would make the utf-8 invalid, instead two bytes get
        // removed.

        let msg = sink.try_recv().unwrap();
        let msg = String::from_utf8(msg.bytes).unwrap();
        assert_eq!(msg.len(), 511);
        assert!(msg.ends_with("9\r\n"));

        sink.try_recv().unwrap_err();
    }
    #[test]
    fn test_long_message_512() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        let mut m = mw.new_message().unwrap();
        for _ in 0..502 {
            message_push!(m, b"a");
        }
        message_push!(m, b"0123456789");
        m.validate();

        let msg = sink.try_recv().unwrap();
        let msg = String::from_utf8(msg.bytes).unwrap();
        assert_eq!(msg.len(), 512);
        assert!(msg.ends_with("7\r\n"));

        sink.try_recv().unwrap_err();
    }

    #[test]
    fn test_long_message_512_utf8_cut() {
        let (mailbox, mut sink) = Mailbox::new(10);
        let mut mw = mailbox.writer(false);

        let mut m = mw.new_message().unwrap();
        for _ in 0..500 {
            message_push!(m, b"a");
        }
        message_push!(m, b"0123456789");
        message_push!(m, b"\xc3\xa9");
        m.validate();
        // the message has size 511, but to happen \r\n the last byte has to be removed to
        // 511. Because removing this last byte would make the utf-8 invalid, instead two bytes get
        // removed.

        let msg = sink.try_recv().unwrap();
        let msg = String::from_utf8(msg.bytes).unwrap();
        assert_eq!(msg.len(), 512);
        assert!(msg.ends_with("9\r\n"));

        sink.try_recv().unwrap_err();
    }
}
