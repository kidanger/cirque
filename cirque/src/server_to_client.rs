use tokio::io::AsyncWriteExt;

use crate::{
    server_state::ServerStateError,
    transport,
    types::{ChannelID, Topic},
};

#[derive(Debug, Clone)]
pub enum Message {
    Welcome {
        nickname: String,
        user_fullspec: String,
    },
    Join {
        channel: ChannelID,
        user_fullspec: String,
    },
    Nick {
        previous_user_fullspec: String,
        nickname: String,
    },
    Names {
        nickname: String,
        names: Vec<(ChannelID, Vec<String>)>,
    },
    /// reply to a GetTopic command or Join command
    RplTopic {
        nickname: String,
        channel: String,
        topic: Option<Topic>,
    },
    /// reply to SetTopic by the user or another user
    Topic {
        user_fullspec: String,
        channel: String,
        topic: Topic,
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
    #[allow(clippy::upper_case_acronyms)]
    MOTD {
        nickname: String,
        motd: Option<Vec<Vec<u8>>>,
    },
    LUsers {
        nickname: String,
        n_operators: usize,
        n_unknown_connections: usize,
        n_channels: usize,
        n_clients: usize,
        n_other_servers: usize,
    },
    Part {
        user_fullspec: String,
        channel: String,
        reason: Option<Vec<u8>>,
    },
    Quit {
        user_fullspec: String,
        reason: Vec<u8>,
    },
    FatalError {
        reason: Vec<u8>,
    },
    Err(ServerStateError),
}

pub(crate) struct MessageContext {
    pub(crate) server_name: String,
}

impl Message {
    pub(crate) async fn write_to(
        &self,
        stream: &mut impl transport::Stream,
        context: &MessageContext,
    ) -> std::io::Result<()> {
        match self {
            Message::Welcome {
                nickname,
                user_fullspec,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 001 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream
                    .write_all(b" :Welcome to the Internet Relay Network ")
                    .await?;
                stream.write_all(user_fullspec.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 002 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" :Your host is '").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b"', running cirque.\r\n").await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 003 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream
                    .write_all(b" :This server was created <datetime>.\r\n")
                    .await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 004 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 0 + +\r\n").await?;
            }
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
            Message::Nick {
                previous_user_fullspec,
                nickname,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(previous_user_fullspec.as_bytes()).await?;
                stream.write_all(b" NICK :").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Names { names, nickname } => {
                for (channel, nicknames) in names {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 353 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" = ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    let mut iter = nicknames.iter().peekable();
                    while let Some(nick) = iter.next() {
                        stream.write_all(nick.as_bytes()).await?;
                        if iter.peek().is_some() {
                            stream.write_all(b" ").await?;
                        }
                    }
                    stream.write_all(b"\r\n").await?;
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 366 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :End of NAMES list\r\n").await?;
                }
            }
            Message::RplTopic {
                nickname,
                channel,
                topic,
            } => {
                if let Some(topic) = topic {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 332 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    stream.write_all(&topic.content).await?;
                    stream.write_all(b"\r\n").await?;

                    // for now this is disabled because chirc testsuite doesn't like it
                    if false {
                        stream.write_all(b":").await?;
                        stream.write_all(context.server_name.as_bytes()).await?;
                        stream.write_all(b" 333 ").await?;
                        stream.write_all(nickname.as_bytes()).await?;
                        stream.write_all(b" ").await?;
                        stream.write_all(channel.as_bytes()).await?;
                        stream.write_all(b" ").await?;
                        stream.write_all(topic.from_nickname.as_bytes()).await?;
                        stream.write_all(b" ").await?;
                        stream.write_all(topic.ts.to_string().as_bytes()).await?;
                        stream.write_all(b"\r\n").await?;
                    }
                } else {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 331 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :No topic is set").await?;
                    stream.write_all(b"\r\n").await?;
                }
            }
            Message::Topic {
                user_fullspec,
                channel,
                topic,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(user_fullspec.as_bytes()).await?;
                stream.write_all(b" TOPIC ").await?;
                stream.write_all(channel.as_bytes()).await?;
                stream.write_all(b" :").await?;
                stream.write_all(&topic.content).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Pong { token } => {
                stream.write_all(b"PONG ").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" :").await?;
                stream.write_all(token).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::ChannelMode {
                nickname,
                channel,
                mode,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 324 ").await?;
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
            Message::MOTD { nickname, motd } => match motd {
                Some(motd) => {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 375 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream
                        .write_all(b" :- <server> Message of the day - \r\n")
                        .await?;

                    for line in motd {
                        stream.write_all(b":").await?;
                        stream.write_all(context.server_name.as_bytes()).await?;
                        stream.write_all(b" 372 ").await?;
                        stream.write_all(nickname.as_bytes()).await?;
                        stream.write_all(b" :- ").await?;
                        stream.write_all(line).await?;
                        stream.write_all(b"\r\n").await?;
                    }

                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 376 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" :End of MOTD command\r\n").await?;
                }
                None => {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 422 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" :MOTD File is missing\r\n").await?;
                }
            },
            Message::LUsers {
                nickname,
                n_operators,
                n_unknown_connections,
                n_channels,
                n_clients,
                n_other_servers,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 251 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream
                    .write_all(b" :There are N users and 0 invisible on 1 servers\r\n")
                    .await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 252 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(n_operators.to_string().as_bytes()).await?;
                stream.write_all(b" :operator(s) online\r\n").await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 253 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream
                    .write_all(n_unknown_connections.to_string().as_bytes())
                    .await?;
                stream.write_all(b" :unknown connection(s)\r\n").await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 254 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(n_channels.to_string().as_bytes()).await?;
                stream.write_all(b" :channels formed\r\n").await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 255 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" :I have ").await?;
                stream.write_all(n_clients.to_string().as_bytes()).await?;
                stream.write_all(b" clients and ").await?;
                stream
                    .write_all(n_other_servers.to_string().as_bytes())
                    .await?;
                stream.write_all(b" servers\r\n").await?;
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
            Message::Quit {
                user_fullspec,
                reason,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(user_fullspec.as_bytes()).await?;
                stream.write_all(b" QUIT :").await?;
                stream.write_all(reason).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::FatalError { reason } => {
                stream.write_all(b"ERROR :").await?;
                stream.write_all(reason).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Err(err) => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" ").await?;
                err.write_to(stream).await?;
                stream.write_all(b"\r\n").await?;
            }
        }

        Ok(())
    }
}
