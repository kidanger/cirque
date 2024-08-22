use crate::{
    message_writer::MessageWriter,
    server_state::ServerStateError,
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
        // this is mostly because some clients don't like extended lusers info (chirc testsuite)
        extra_info: bool,
    },
    Part {
        user_fullspec: String,
        channel: String,
        reason: Option<Vec<u8>>,
    },
    List {
        client: String,
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
    pub(crate) fn write_to(&self, stream: &mut MessageWriter, context: &MessageContext) {
        match self {
            Message::Welcome {
                nickname,
                user_fullspec,
                welcome_config,
            } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 001 ",
                    &nickname,
                    b" :Welcome to the Internet Relay Network ",
                    &user_fullspec
                );

                message! {
                    stream,
                    b":",
                    &context.server_name,
                    b" 002 ",
                    nickname,
                    b" :Your host is '",
                    &context.server_name,
                    b"', running cirque."
                };

                message! {
                    stream,
                    b":",
                    &context.server_name,
                    b" 003 ",
                    &nickname,
                    b" :This server was created <datetime>."
                };

                message! {
                    stream,
                    b":",
                    &context.server_name,
                    b" 004 ",
                    &nickname,
                    b" ",
                    &context.server_name,
                    b" 0 a a"
                };

                // chirch doesn't like 005, but it's better with it for irctest
                if welcome_config.send_isupport {
                    message! {
                        stream,
                        b":",
                        &context.server_name,
                        b" 005 ",
                        &nickname,
                        b" CASEMAPPING=rfc7613 :are supported by this server"
                    };
                }
            }
            Message::Join {
                channel,
                user_fullspec,
            } => {
                message!(stream, b":", &user_fullspec, b" JOIN ", &channel, b"");
            }
            Message::Nick {
                previous_user_fullspec,
                nickname,
            } => {
                message!(stream, b":", &previous_user_fullspec, b" NICK :", &nickname);
            }
            Message::Names { names, nickname } => {
                for (channel, channel_mode, nicknames) in names {
                    let mut m = stream
                        .new_message()
                        .write(b":")
                        .write(&context.server_name)
                        .write(b" 353 ")
                        .write(&nickname)
                        .write(match channel_mode.is_secret() {
                            true => b" @ ",
                            false => b" = ",
                        })
                        .write(&channel)
                        .write(b" :");

                    for (i, (nick, user_mode)) in nicknames.iter().enumerate() {
                        if user_mode.is_op() {
                            m = m.write(b"@");
                        } else if user_mode.is_voice() {
                            m = m.write(b"+");
                        }
                        m = m.write(&nick.as_bytes());
                        if i != nicknames.len() - 1 {
                            m = m.write(b" ")
                        }
                    }
                    m.validate();

                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 366 ",
                        &nickname,
                        b" ",
                        &channel,
                        b" :End of NAMES list"
                    );
                }
            }
            Message::EndOfNames { nickname, channel } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 366 ",
                    &nickname,
                    b" ",
                    &channel,
                    b" :End of NAMES list"
                );
            }
            Message::RplTopic {
                nickname,
                channel,
                topic,
            } => {
                if let Some(topic) = topic {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 332 ",
                        &nickname,
                        b" ",
                        &channel,
                        b" :",
                        &&topic.content
                    );

                    // irctest requires the RPL_TOPICWHOTIME, but chirch doesn't want it
                    if true {
                        message!(
                            stream,
                            b":",
                            &context.server_name,
                            b" 333 ",
                            &nickname,
                            b" ",
                            &channel,
                            b" ",
                            &topic.from_nickname,
                            b" ",
                            &topic.ts.to_string()
                        );
                    }
                } else {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 331 ",
                        &nickname,
                        b" ",
                        &channel,
                        b" :No topic is set"
                    );
                }
            }
            Message::Topic {
                user_fullspec,
                channel,
                topic,
            } => {
                message!(
                    stream,
                    b":",
                    &user_fullspec,
                    b" TOPIC ",
                    &channel,
                    b" :",
                    &&topic.content
                );
            }
            Message::Pong { token } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" PONG ",
                    &context.server_name,
                    b" :",
                    &token
                );
            }
            Message::Mode {
                user_fullspec,
                target,
                modechar,
                param,
            } => {
                let mut m = stream
                    .new_message()
                    .write(b":")
                    .write(&user_fullspec)
                    .write(b" MODE ")
                    .write(&target)
                    .write(b" ")
                    .write(&modechar.as_bytes());
                if let Some(param) = param {
                    m = m.write(b" ").write(&param.as_bytes());
                }
                m.validate();
            }
            Message::ChannelMode {
                nickname,
                channel,
                mode,
            } => {
                let mut m = stream.new_message();
                message_push!(
                    m,
                    b":",
                    &context.server_name,
                    b" 324 ",
                    &nickname,
                    b" ",
                    &channel,
                    b" +"
                );
                if mode.is_no_external() {
                    m = m.write(b"n");
                }
                if mode.is_secret() {
                    m = m.write(b"s");
                }
                if mode.is_moderated() {
                    m = m.write(b"m");
                }
                if mode.is_topic_protected() {
                    m = m.write(b"t");
                }
                m.validate();
            }
            Message::PrivMsg {
                from_user,
                target,
                content,
            } => {
                message!(
                    stream,
                    b":",
                    &from_user,
                    b" PRIVMSG ",
                    &target,
                    b" :",
                    &content
                );
            }
            Message::Notice {
                from_user,
                target,
                content,
            } => {
                message!(
                    stream,
                    b":",
                    &from_user,
                    b" NOTICE ",
                    &target,
                    b" :",
                    &content
                );
            }
            Message::MOTD { nickname, motd } => match motd {
                Some(motd) => {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 375 ",
                        &nickname,
                        b" :- <server> Message of the day - "
                    );

                    for line in motd {
                        message!(
                            stream,
                            b":",
                            &context.server_name,
                            b" 372 ",
                            &nickname,
                            b" :- ",
                            &line
                        );
                    }

                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 376 ",
                        &nickname,
                        b" :End of MOTD command"
                    );
                }
                None => {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 422 ",
                        &nickname,
                        b" :MOTD File is missing"
                    );
                }
            },
            Message::LUsers {
                nickname,
                n_operators,
                n_unknown_connections,
                n_channels,
                n_clients,
                n_other_servers,
                extra_info,
            } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 251 ",
                    &nickname,
                    b" :There are ",
                    &n_clients.to_string(),
                    b" users and 0 invisible on 1 servers"
                );

                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 252 ",
                    &nickname,
                    b" ",
                    &n_operators.to_string(),
                    b" :operator(s) online"
                );

                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 253 ",
                    &nickname,
                    b" ",
                    &n_unknown_connections.to_string(),
                    b" :unknown connection(s)"
                );

                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 254 ",
                    &nickname,
                    b" ",
                    &n_channels.to_string(),
                    b" :channels formed"
                );

                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 255 ",
                    &nickname,
                    b" :I have ",
                    &n_clients.to_string(),
                    b" clients and ",
                    &n_other_servers.to_string(),
                    b" servers"
                );

                if *extra_info {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 265 ",
                        &nickname,
                        b" :Current local users  ",
                        &n_clients.to_string(),
                        b" , max ",
                        &n_clients.to_string()
                    );

                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 266 ",
                        &nickname,
                        b" :Current global users  ",
                        &n_clients.to_string(),
                        b" , max ",
                        &n_clients.to_string()
                    );
                }
            }
            Message::Part {
                user_fullspec,
                channel,
                reason,
            } => {
                let mut m = stream
                    .new_message()
                    .write(b":")
                    .write(&user_fullspec.as_bytes())
                    .write(b" PART ")
                    .write(&channel.as_bytes());
                if let Some(reason) = reason {
                    m = m.write(b" :").write(&reason);
                }
                m.validate();
            }
            Message::List { client, infos } => {
                // chirc test suite doesn't like 321
                if false {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 321 ",
                        &client,
                        b" ",
                        b"Channel :Users  Name"
                    );
                }

                for info in infos {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 322 ",
                        &client,
                        b" ",
                        &info.name,
                        b" ",
                        &info.count.to_string(),
                        b" :",
                        &info.topic
                    );
                }
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 323 ",
                    &client,
                    b" ",
                    b":End of LIST"
                );
            }
            Message::NowAway { nickname } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 306 ",
                    &nickname,
                    b" :You have been marked as being away"
                );
            }
            Message::UnAway { nickname } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 305 ",
                    &nickname,
                    b" :You are no longer marked as being away"
                );
            }
            Message::RplAway {
                nickname,
                target_nickname,
                away_message,
            } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 301 ",
                    &nickname,
                    b" ",
                    &target_nickname,
                    b" :",
                    &away_message
                );
            }
            Message::RplUserhost { nickname, info } => {
                let mut m = stream
                    .new_message()
                    .write(b":")
                    .write(&context.server_name.as_bytes())
                    .write(b" 302 ")
                    .write(&nickname.as_bytes())
                    .write(b" :");
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
                    message_push!(m, &nickname.as_bytes());
                    if *is_op {
                        message_push!(m, b"*");
                    }
                    message_push!(
                        m,
                        b"=",
                        match is_away {
                            true => b"-",
                            false => b"+",
                        },
                        &hostname.as_bytes()
                    );
                    if i != info.len() - 1 {
                        message_push!(m, b" ");
                    }
                }
                m.validate();
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
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 301 ",
                        &client,
                        b" ",
                        &target_nickname,
                        b" :",
                        &away_message
                    );
                }

                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 311 ",
                    &client,
                    b" ",
                    &target_nickname,
                    b" ",
                    &username,
                    b" ",
                    &hostname,
                    b" * :",
                    &realname
                );

                // don't send RPL_WHOISCHANNELS, for privacy reasons
                if false {
                    message!(
                        stream,
                        b":",
                        &context.server_name,
                        b" 319 ",
                        &client,
                        b" ",
                        &target_nickname,
                        b" :" // list of channels would go here
                    );
                }

                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 318 ",
                    &client,
                    b" ",
                    &target_nickname,
                    b" :End of /WHOIS list"
                );
            }
            Message::RplEndOfWhois {
                client,
                target_nickname,
            } => {
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 318 ",
                    &client,
                    b" ",
                    &target_nickname,
                    b" :End of /WHOIS list"
                );
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
                    let mut m = stream.new_message();
                    message_push!(m, b":", &context.server_name, b" 352 ", &client, b" ");
                    if let Some(channel) = channel {
                        message_push!(m, &channel.as_bytes());
                    } else {
                        message_push!(m, &"*".to_string().as_bytes());
                    }
                    message_push!(
                        m,
                        b" ",
                        &username,
                        b" ",
                        &hostname,
                        b" ",
                        &context.server_name,
                        b" ",
                        &nickname,
                        b" ",
                        if *is_away { b"G" } else { b"H" }
                    );
                    if *is_op {
                        message_push!(m, b"*");
                    }
                    if let Some(channel_user_mode) = channel_user_mode {
                        if channel_user_mode.is_op() {
                            message_push!(m, b"@");
                        } else if channel_user_mode.is_voice() {
                            message_push!(m, b"+");
                        }
                    }
                    message_push!(m, b" :0 ", &realname);
                    m.validate();
                }
                message!(
                    stream,
                    b":",
                    &context.server_name,
                    b" 315 ",
                    &client,
                    b" ",
                    &mask,
                    b" :End of WHO list"
                );
            }
            Message::Quit {
                user_fullspec,
                reason,
            } => {
                message!(stream, b":", &user_fullspec, b" QUIT :", &reason);
            }
            Message::FatalError { reason } => {
                message!(stream, b"ERROR :", &reason);
            }
            Message::Err(err) => {
                let mut m = stream.new_message();
                message_push!(m, b":", &context.server_name, b" ");
                err.write_to(m).validate();
            }
        }
    }
}
