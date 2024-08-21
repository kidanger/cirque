use tokio::io::AsyncWriteExt;

use crate::{
    server_state::ServerStateError,
    transport,
    types::{ChannelID, ChannelMode, ChannelUserMode, Topic},
    WelcomeConfig,
};

#[derive(Debug, Clone)]
pub(crate) struct ChannelInfo {
    pub name: String,
    pub count: usize,
    pub topic: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct UserhostReply {
    pub(crate) nickname: String,
    pub(crate) is_op: bool,
    pub(crate) is_away: bool,
    pub(crate) hostname: String,
}

#[derive(Debug, Clone)]
pub(crate) struct WhoReply {
    pub(crate) channel: Option<String>,
    pub(crate) channel_user_mode: Option<ChannelUserMode>,
    pub(crate) nickname: String,
    pub(crate) is_op: bool,
    pub(crate) is_away: bool,
    pub(crate) hostname: String,
    pub(crate) username: String,
    pub(crate) realname: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Welcome {
        nickname: String,
        user_fullspec: String,
        welcome_config: WelcomeConfig,
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
        names: Vec<(ChannelID, ChannelMode, Vec<(String, ChannelUserMode)>)>,
    },
    /// only used on NAMES command when the channel is invalid or does not exist
    EndOfNames {
        nickname: String,
        channel: String,
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
    Mode {
        user_fullspec: String,
        target: String,
        modechar: String,
        param: Option<String>,
    },
    /// only as a reply to AskChannelMode
    ChannelMode {
        nickname: String,
        channel: ChannelID,
        mode: ChannelMode,
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
    List {
        infos: Vec<ChannelInfo>,
    },
    NowAway {
        nickname: String,
    },
    UnAway {
        nickname: String,
    },
    /// When someone sends a message to an away user or on WHOIS
    RplAway {
        nickname: String,
        target_nickname: String,
        away_message: Vec<u8>,
    },
    RplUserhost {
        nickname: String,
        info: Vec<UserhostReply>,
    },
    RplWhois {
        client: String,
        target_nickname: String,
        away_message: Option<Vec<u8>>,
        hostname: String,
        username: String,
        realname: Vec<u8>,
    },
    /// when the WHOIS resulted in an error, we still need to write the RPL_ENDOFWHOIS
    RplEndOfWhois {
        client: String,
        target_nickname: String,
    },
    Who {
        client: String,
        mask: String,
        replies: Vec<WhoReply>,
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
        // TODO: we should make sure not to write more than 512 bytes including \r\n
        //       we could wrap the Stream into a MessageStream respecting this contraint
        //       maybe have an arena of 512-buffers, and send these to mailboxes instead of
        //       server_to_client::Message
        //       this would enable doing less copies to construct Messages (reference field)
        //          but might complicate error handling?
        match self {
            Message::Welcome {
                nickname,
                user_fullspec,
                welcome_config,
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
                stream.write_all(b" 0 a a\r\n").await?;

                // chirch doesn't like 005, but it's better with it for irctest
                if welcome_config.send_isupport {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 005 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream
                        .write_all(b" CASEMAPPING=ascii :are supported by this server\r\n")
                        .await?;
                }
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
                for (channel, channel_mode, nicknames) in names {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 353 ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    if channel_mode.is_secret() {
                        stream.write_all(b" @ ").await?;
                    } else {
                        stream.write_all(b" = ").await?;
                    }
                    stream.write_all(channel.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    for (i, (nick, user_mode)) in nicknames.iter().enumerate() {
                        if user_mode.is_op() {
                            stream.write_all(b"@").await?;
                        } else if user_mode.is_voice() {
                            stream.write_all(b"+").await?;
                        }
                        stream.write_all(nick.as_bytes()).await?;
                        if i != nicknames.len() - 1 {
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
            Message::EndOfNames { nickname, channel } => {
                stream.write_all(b"\r\n").await?;
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 366 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(channel.as_bytes()).await?;
                stream.write_all(b" :End of NAMES list\r\n").await?;
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

                    // irctest requires the RPL_TOPICWHOTIME, but chirch doesn't want it
                    if true {
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
            Message::Mode {
                user_fullspec,
                target,
                modechar,
                param,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(user_fullspec.as_bytes()).await?;
                stream.write_all(b" MODE ").await?;
                stream.write_all(target.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(modechar.as_bytes()).await?;
                if let Some(param) = param {
                    stream.write_all(b" ").await?;
                    stream.write_all(param.as_bytes()).await?;
                }
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
                stream.write_all(b" +").await?;
                if true {
                    // all channels are "no external message" for now
                    stream.write_all(b"n").await?;
                }
                if mode.is_secret() {
                    stream.write_all(b"s").await?;
                }
                if mode.is_topic_protected() {
                    stream.write_all(b"t").await?;
                }
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
            Message::List { infos } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 321 ").await?;
                stream.write_all(b" Channel :Users  Name\r\n").await?;

                for info in infos {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 322 ").await?;
                    stream.write_all(info.name.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(info.count.to_string().as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    stream.write_all(&info.topic).await?;
                    stream.write_all(b"\r\n").await?;
                }
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 323 ").await?;
                stream.write_all(b":End of /LIST\r\n").await?;
            }
            Message::NowAway { nickname } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 306 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream
                    .write_all(b" :You have been marked as being away")
                    .await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::UnAway { nickname } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 305 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream
                    .write_all(b" :You are no longer marked as being away")
                    .await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::RplAway {
                nickname,
                target_nickname,
                away_message,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 301 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(target_nickname.as_bytes()).await?;
                stream.write_all(b" :").await?;
                stream.write_all(away_message).await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::RplUserhost { nickname, info } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 302 ").await?;
                stream.write_all(nickname.as_bytes()).await?;
                stream.write_all(b" :").await?;
                for (
                    i,
                    UserhostReply {
                        nickname,
                        is_op,
                        is_away,
                        hostname,
                    },
                ) in info.iter().enumerate()
                {
                    stream.write_all(nickname.as_bytes()).await?;
                    if *is_op {
                        stream.write_all(b"*").await?;
                    }
                    stream.write_all(b"=").await?;
                    match is_away {
                        true => stream.write_all(b"-").await?,
                        false => stream.write_all(b"+").await?,
                    }
                    stream.write_all(hostname.as_bytes()).await?;
                    if i != info.len() - 1 {
                        stream.write_all(b" ").await?;
                    }
                }
                stream.write_all(b"\r\n").await?;
            }
            Message::RplWhois {
                client,
                target_nickname,
                away_message,
                hostname,
                username,
                realname,
            } => {
                if let Some(away_message) = away_message {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 301 ").await?;
                    stream.write_all(client.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(target_nickname.as_bytes()).await?;
                    stream.write_all(b" :").await?;
                    stream.write_all(away_message).await?;
                    stream.write_all(b"\r\n").await?;
                }

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 311 ").await?;
                stream.write_all(client.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(target_nickname.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(username.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(hostname.as_bytes()).await?;
                stream.write_all(b" * :").await?;
                stream.write_all(realname).await?;
                stream.write_all(b"\r\n").await?;

                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 318 ").await?;
                stream.write_all(client.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(target_nickname.as_bytes()).await?;
                stream.write_all(b" :End of /WHOIS list").await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::RplEndOfWhois {
                client,
                target_nickname,
            } => {
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 318 ").await?;
                stream.write_all(client.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(target_nickname.as_bytes()).await?;
                stream.write_all(b" :End of /WHOIS list").await?;
                stream.write_all(b"\r\n").await?;
            }
            Message::Who {
                client,
                mask,
                replies,
            } => {
                for WhoReply {
                    channel,
                    channel_user_mode,
                    nickname,
                    is_op,
                    is_away,
                    hostname,
                    username,
                    realname,
                } in replies
                {
                    stream.write_all(b":").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" 352 ").await?;
                    stream.write_all(client.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    if let Some(channel) = channel {
                        stream.write_all(channel.as_bytes()).await?;
                    } else {
                        stream.write_all(b"*").await?;
                    }
                    stream.write_all(b" ").await?;
                    stream.write_all(username.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(hostname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(context.server_name.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    stream.write_all(nickname.as_bytes()).await?;
                    stream.write_all(b" ").await?;
                    if *is_away {
                        stream.write_all(b"G").await?;
                    } else {
                        stream.write_all(b"H").await?;
                    }
                    if *is_op {
                        stream.write_all(b"*").await?;
                    }
                    if let Some(channel_user_mode) = channel_user_mode {
                        if channel_user_mode.is_op() {
                            stream.write_all(b"@").await?;
                        } else if channel_user_mode.is_voice() {
                            stream.write_all(b"v").await?;
                        }
                    }
                    stream.write_all(b" :0 ").await?;
                    stream.write_all(realname).await?;
                    stream.write_all(b"\r\n").await?;
                }
                stream.write_all(b":").await?;
                stream.write_all(context.server_name.as_bytes()).await?;
                stream.write_all(b" 315 ").await?;
                stream.write_all(client.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(mask.as_bytes()).await?;
                stream.write_all(b" :End of WHO list").await?;
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
