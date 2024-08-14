use tokio::io::AsyncWriteExt;

use crate::transport;
use crate::ChannelID;

#[derive(Debug, Clone)]
pub struct JoinMessage {
    pub channel: ChannelID,
    pub user_fullspec: String,
}

#[derive(Debug, Clone)]
pub struct NamesMessage {
    pub nickname: String,
    pub names: Vec<(ChannelID, Vec<String>)>,
}

#[derive(Debug, Clone)]
pub struct TopicMessage {
    pub nickname: String,
    pub channel: String,
    pub topic: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ChannelModeMessage {
    pub nickname: String,
    pub channel: ChannelID,
    pub mode: String,
}

#[derive(Debug, Clone)]
pub struct PongMessage {
    pub token: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Join(JoinMessage),
    Names(NamesMessage),
    Topic(TopicMessage),
    Pong(PongMessage),
    ChannelMode(ChannelModeMessage),
}

impl Message {
    pub(crate) async fn write_to(&self, stream: &mut impl transport::Stream) -> anyhow::Result<()> {
        match self {
            Message::Join(j) => {
                stream.write_all(b":").await?;
                stream.write_all(j.user_fullspec.as_bytes()).await?;
                stream.write_all(b" JOIN ").await?;
                stream.write_all(j.channel.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Names(n) => {
                for (channel, nicknames) in &n.names {
                    stream.write_all(b"353 ").await?;
                    stream.write_all(n.nickname.as_bytes()).await?;
                    stream.write_all(b" = ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    for nick in nicknames {
                        stream.write_all(nick.as_bytes()).await?;
                        stream.write_all(b" ").await?;
                    }
                    stream.write_all(b"\r\n").await?;
                    stream.write_all(b"366 ").await?;
                    stream.write_all(n.nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :End of NAMES list\r\n").await?;
                }
            }
            Message::Topic(TopicMessage {
                nickname,
                channel,
                topic,
            }) => {
                if let Some(topic) = topic {
                    stream.write_all(b"332 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    stream.write_all(topic).await?;
                } else {
                    stream.write_all(b"331 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :No topic is set").await?;
                }
                stream.write_all(b"\r\n").await?;
            }
            Message::Pong(PongMessage { token }) => {
                stream.write_all(b"PONG :").await?;
                stream.write_all(token).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::ChannelMode(ChannelModeMessage {
                nickname,
                channel,
                mode,
            }) => {
                stream.write_all(b"324 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(channel.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(mode.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;
            }
        }

        Ok(())
    }
}
