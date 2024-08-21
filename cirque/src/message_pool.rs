use tokio::sync::mpsc::UnboundedSender;

use crate::server_to_client::{Message, MessageContext};

pub(crate) struct MessagePool {}

impl MessagePool {
    pub(crate) fn ingest_into_channel(
        message: &Message,
        channel: &UnboundedSender<Vec<u8>>,
        context: &MessageContext,
    ) {
        let mut buf = std::io::Cursor::new(Vec::<u8>::new());
        message.write_to(&mut buf, context);

        let buf = buf.into_inner();
        let _ = channel.send(buf);
    }
}
