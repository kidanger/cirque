use tokio::io::AsyncWriteExt;

use crate::{
    server_state::ServerStateError,
    transport,
    types::{ChannelID, Topic},
};

#[derive(Debug, Clone)]
pub enum Message {
    Join {
        channel: ChannelID,
        user_fullspec: String,
    },
    Names {
        nickname: String,
        names: Vec<(ChannelID, Vec<String>)>,
    },
    Topic {
        nickname: String,
        channel: String,
        topic: Option<Topic>,
    },
    Pong {
        token: Vec<u8>,
    },
    ChannelMode {
        nickname: String,
        channel: ChannelID,
        mode: String,
    },
    PrivMsg {
        from_user: String,
        target: ChannelID,
        content: Vec<u8>,
    },
    Notice {
        from_user: String,
        target: ChannelID,
        content: Vec<u8>,
    },
    Part {
        user_fullspec: String,
        channel: String,
        reason: Option<Vec<u8>>,
    },
    Err(ServerStateError),
}

impl Message {
    pub(crate) async fn write_to(
        &self,
        stream: &mut impl transport::Stream,
    ) -> std::io::Result<()> {
        match self {
            Message::Join {
                channel,
                user_fullspec,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(user_fullspec.as_bytes()).await?;
                stream.write_all(b" JOIN ").await?;
                stream.write_all(channel.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Names { names, nickname } => {
                for (channel, nicknames) in names {
                    stream.write_all(b":srv 353 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" = ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    for nick in nicknames {
                        stream.write_all(nick.as_bytes()).await?;
                        stream.write_all(b" ").await?;
                    }
                    stream.write_all(b"\r\n").await?;
                    stream.write_all(b":srv 366 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :End of NAMES list\r\n").await?;
                }
            }
            Message::Topic {
                nickname,
                channel,
                topic,
            } => {
                if let Some(topic) = topic {
                    stream.write_all(b":srv 332 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    stream.write_all(&topic.content).await?;
                    stream.write_all(b"\r\n").await?;

                    stream.write_all(b":srv 333 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(topic.from_nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(topic.ts.to_string().as_bytes()).await?;
                    stream.write_all(b"\r\n").await?;
                } else {
                    stream.write_all(b":srv 331 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :No topic is set").await?;
                    stream.write_all(b"\r\n").await?;
                }
            }
            Message::Pong { token } => {
                stream.write_all(b"PONG :").await?;
                stream.write_all(token).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::ChannelMode {
                nickname,
                channel,
                mode,
            } => {
                stream.write_all(b":srv 324 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(channel.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(mode.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::PrivMsg {
                from_user,
                target,
                content,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(from_user.as_bytes()).await?;
                stream.write_all(b" PRIVMSG ").await?;
                stream.write_all(target.as_bytes()).await?;
                stream.write_all(b" :").await?;
                stream.write_all(content).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Notice {
                from_user,
                target,
                content,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(from_user.as_bytes()).await?;
                stream.write_all(b" NOTICE ").await?;
                stream.write_all(target.as_bytes()).await?;
                stream.write_all(b" :").await?;
                stream.write_all(content).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Part {
                user_fullspec,
                channel,
                reason,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(user_fullspec.as_bytes()).await?;
                stream.write_all(b" PART ").await?;
                stream.write_all(channel.as_bytes()).await?;
                if let Some(reason) = reason {
                    stream.write_all(b" :").await?;
                    stream.write_all(reason).await?;
                }
                stream.write_all(b"\r\n").await?;
            }
            Message::Err(err) => {
                stream.write_all(b":srv ").await?;
                err.write_to(stream).await?;
                stream.write_all(b"\r\n").await?;
            }
        }

        Ok(())
    }
}
