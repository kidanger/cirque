use tokio::sync::mpsc::{error::TryRecvError, UnboundedReceiver, UnboundedSender};

use crate::server_to_client::{Message, MessageContext};

#[derive(Debug)]
pub(crate) struct Mailbox {
    sender: UnboundedSender<Vec<u8>>,
}

impl Mailbox {
    pub(crate) fn new() -> (Self, MailboxSink) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        (Self { sender }, MailboxSink { receiver })
    }

    pub(crate) fn ingest(&self, message: &Message, context: &MessageContext) {
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        message.write_to(&mut buf, context);

        let buf = buf.into_inner();
        let _ = self.sender.send(buf);
    }
}

#[derive(Debug)]
pub(crate) struct MailboxSink {
    receiver: UnboundedReceiver<Vec<u8>>,
}

impl MailboxSink {
    pub(crate) async fn recv(&mut self) -> Option<Vec<u8>> {
        self.receiver.recv().await
    }

    pub(crate) fn try_recv(&mut self) -> Result<Vec<u8>, TryRecvError> {
        self.receiver.try_recv()
    }
}
