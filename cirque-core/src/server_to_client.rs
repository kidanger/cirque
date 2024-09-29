use crate::{
    message_writer::MessageWriter,
    types::{ChannelMode, ChannelUserMode, Topic},
    WelcomeConfig,
};

#[derive(Debug, Clone)]
pub(crate) struct ChannelInfo<'a> {
    pub name: &'a str,
    pub count: usize,
    pub topic: &'a [u8],
}

#[derive(Debug, Clone)]
pub(crate) struct UserhostReply<'a> {
    pub(crate) nickname: &'a str,
    pub(crate) is_op: bool,
    pub(crate) is_away: bool,
    pub(crate) hostname: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct WhoReply<'a> {
    pub(crate) channel: Option<&'a str>,
    pub(crate) channel_user_mode: Option<&'a ChannelUserMode>,
    pub(crate) nickname: &'a str,
    pub(crate) is_op: bool,
    pub(crate) is_away: bool,
    pub(crate) hostname: &'a str,
    pub(crate) username: &'a str,
    pub(crate) realname: &'a [u8],
}

#[derive(Debug, Clone)]
pub(crate) struct NamesReply<'a> {
    pub(crate) channel_name: &'a str,
    pub(crate) channel_mode: &'a ChannelMode,
    pub(crate) nicknames: &'a [(&'a String, &'a ChannelUserMode)],
}

#[derive(Debug, Clone)]
pub(crate) enum Message<'a> {
    Welcome {
        nickname: &'a str,
        user_fullspec: &'a str,
        welcome_config: &'a WelcomeConfig,
    },
    Join {
        channel: &'a str,
        user_fullspec: &'a str,
    },
    Nick {
        previous_user_fullspec: &'a str,
        nickname: &'a str,
    },
    Names {
        client: &'a str,
        //names: Vec<(&'a str, &'a ChannelMode, Vec<(String, ChannelUserMode)>)>,
        names: &'a [NamesReply<'a>],
    },
    /// only used on NAMES command when the channel is invalid or does not exist
    EndOfNames {
        client: &'a str,
        channel: &'a str,
    },
    /// reply to a GetTopic command or Join command
    RplTopic {
        client: &'a str,
        channel: &'a str,
        topic: Option<&'a Topic>,
    },
    /// reply to SetTopic by the user or another user
    Topic {
        user_fullspec: &'a str,
        channel: &'a str,
        topic: &'a Topic,
    },
    Ping {
        token: &'a [u8],
    },
    Pong {
        token: &'a [u8],
    },
    Mode {
        user_fullspec: &'a str,
        target: &'a str,
        modechar: &'a str,
        param: Option<&'a str>,
    },
    /// only as a reply to AskChannelMode
    ChannelMode {
        client: &'a str,
        channel: &'a str,
        mode: &'a ChannelMode,
    },
    PrivMsg {
        from_user: &'a str,
        target: &'a str,
        content: &'a [u8],
    },
    Notice {
        from_user: &'a str,
        target: &'a str,
        content: &'a [u8],
    },
    #[allow(clippy::upper_case_acronyms)]
    MOTD {
        client: &'a str,
        motd: Option<&'a [Vec<u8>]>,
    },
    LUsers {
        client: &'a str,
        n_operators: usize,
        n_unknown_connections: usize,
        n_channels: usize,
        n_clients: usize,
        n_other_servers: usize,
        // this is mostly because some clients don't like extended lusers info (chirc testsuite)
        extra_info: bool,
    },
    Part {
        user_fullspec: &'a str,
        channel: &'a str,
        reason: Option<&'a [u8]>,
    },
    List {
        client: &'a str,
        infos: &'a [ChannelInfo<'a>],
    },
    NowAway {
        client: &'a str,
    },
    UnAway {
        client: &'a str,
    },
    /// When someone sends a message to an away user or on WHOIS
    RplAway {
        client: &'a str,
        target_nickname: &'a str,
        away_message: &'a [u8],
    },
    RplUserhost {
        client: &'a str,
        info: &'a [UserhostReply<'a>],
    },
    RplWhois {
        client: &'a str,
        target_nickname: &'a str,
        away_message: Option<&'a [u8]>,
        hostname: &'a str,
        username: &'a str,
        realname: &'a [u8],
    },
    /// when the WHOIS resulted in an error, we still need to write the RPL_ENDOFWHOIS
    RplEndOfWhois {
        client: &'a str,
        target_nickname: &'a str,
    },
    Who {
        client: &'a str,
        mask: &'a str,
        replies: &'a [WhoReply<'a>],
    },
    Quit {
        user_fullspec: &'a str,
        reason: &'a [u8],
    },
    FatalError {
        reason: &'a [u8],
    },
    Err(crate::error::ServerStateError),
}

pub(crate) struct MessageContext {
    pub(crate) server_name: String,
}

impl Message<'_> {
    pub(crate) fn write_to(
        &self,
        stream: &mut MessageWriter<'_>,
        context: &MessageContext,
    ) -> Option<()> {
        let sv = &context.server_name;
        match self {
            Message::Welcome {
                nickname,
                user_fullspec,
                welcome_config,
            } => {
                message!(
                    stream,
                    b":",
                    sv,
                    b" 001 ",
                    nickname,
                    b" :Welcome to the Internet Relay Network ",
                    user_fullspec
                );

                message! {
                    stream,
                    b":",
                    sv,
                    b" 002 ",
                    nickname,
                    b" :Your host is '",
                    sv,
                    b"', running cirque."
                };

                message! {
                    stream,
                    b":",
                    sv,
                    b" 003 ",
                    nickname,
                    b" :This server was created <datetime>."
                };

                message! {
                    stream,
                    b":",
                    sv,
                    b" 004 ",
                    nickname,
                    b" ",
                    sv,
                    b" 0 a a"
                };

                // chirch doesn't like 005, but it's better with it for irctest
                if welcome_config.send_isupport {
                    message! {
                        stream,
                        b":",
                        sv,
                        b" 005 ",
                        nickname,
                        b" CASEMAPPING=rfc7613 :are supported by this server"
                    };
                }
            }
            Message::Join {
                channel,
                user_fullspec,
            } => {
                message!(stream, b":", user_fullspec, b" JOIN ", &channel, b"");
            }
            Message::Nick {
                previous_user_fullspec,
                nickname,
            } => {
                message!(stream, b":", previous_user_fullspec, b" NICK :", nickname);
            }
            Message::Names { names, client } => {
                for NamesReply {
                    channel_name,
                    channel_mode,
                    nicknames,
                } in *names
                {
                    let mut m = stream.new_message()?;
                    message_push!(
                        m,
                        b":",
                        sv,
                        b" 353 ",
                        client,
                        match channel_mode.is_secret() {
                            true => b" @ ",
                            false => b" = ",
                        },
                        channel_name,
                        b" :"
                    );
                    for (i, (nick, user_mode)) in nicknames.iter().enumerate() {
                        if user_mode.is_op() {
                            m = m.write(b"@");
                        } else if user_mode.is_voice() {
                            m = m.write(b"+");
                        }
                        m = m.write(nick);
                        if i != nicknames.len() - 1 {
                            m = m.write(b" ")
                        }
                    }
                    m.validate();

                    message!(
                        stream,
                        b":",
                        sv,
                        b" 366 ",
                        client,
                        b" ",
                        channel_name,
                        b" :End of NAMES list"
                    );
                }
            }
            Message::EndOfNames { client, channel } => {
                message!(
                    stream,
                    b":",
                    sv,
                    b" 366 ",
                    client,
                    b" ",
                    channel,
                    b" :End of NAMES list"
                );
            }
            Message::RplTopic {
                client,
                channel,
                topic,
            } => {
                if let Some(topic) = topic {
                    message!(
                        stream,
                        b":",
                        sv,
                        b" 332 ",
                        client,
                        b" ",
                        channel,
                        b" :",
                        &topic.content
                    );

                    // irctest requires the RPL_TOPICWHOTIME, but chirch doesn't want it
                    if true {
                        message!(
                            stream,
                            b":",
                            sv,
                            b" 333 ",
                            client,
                            b" ",
                            channel,
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
                        sv,
                        b" 331 ",
                        client,
                        b" ",
                        channel,
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
                    user_fullspec,
                    b" TOPIC ",
                    channel,
                    b" :",
                    &topic.content
                );
            }
            Message::Ping { token } => {
                message!(stream, b":", sv, b" PING :", token);
            }
            Message::Pong { token } => {
                message!(stream, b":", sv, b" PONG ", sv, b" :", token);
            }
            Message::Mode {
                user_fullspec,
                target,
                modechar,
                param,
            } => {
                let mut m = stream.new_message()?;
                message_push!(m, b":", user_fullspec, b" MODE ", target, b" ", modechar);
                if let Some(param) = param {
                    message_push!(m, b" ", param);
                }
                m.validate();
            }
            Message::ChannelMode {
                client,
                channel,
                mode,
            } => {
                let mut m = stream.new_message()?;
                message_push!(m, b":", sv, b" 324 ", client, b" ", channel, b" +");
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
                    from_user,
                    b" PRIVMSG ",
                    target,
                    b" :",
                    content
                );
            }
            Message::Notice {
                from_user,
                target,
                content,
            } => {
                message!(stream, b":", from_user, b" NOTICE ", target, b" :", content);
            }
            Message::MOTD { client, motd } => match motd {
                Some(motd) => {
                    message!(
                        stream,
                        b":",
                        sv,
                        b" 375 ",
                        client,
                        b" :- <server> Message of the day - "
                    );

                    for line in *motd {
                        message!(stream, b":", sv, b" 372 ", client, b" :- ", line);
                    }

                    message!(stream, b":", sv, b" 376 ", client, b" :End of MOTD command");
                }
                None => {
                    message!(
                        stream,
                        b":",
                        sv,
                        b" 422 ",
                        client,
                        b" :MOTD File is missing"
                    );
                }
            },
            Message::LUsers {
                client,
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
                    sv,
                    b" 251 ",
                    client,
                    b" :There are ",
                    &n_clients.to_string(),
                    b" users and 0 invisible on 1 servers"
                );

                message!(
                    stream,
                    b":",
                    sv,
                    b" 252 ",
                    client,
                    b" ",
                    &n_operators.to_string(),
                    b" :operator(s) online"
                );

                message!(
                    stream,
                    b":",
                    sv,
                    b" 253 ",
                    client,
                    b" ",
                    &n_unknown_connections.to_string(),
                    b" :unknown connection(s)"
                );

                message!(
                    stream,
                    b":",
                    sv,
                    b" 254 ",
                    client,
                    b" ",
                    &n_channels.to_string(),
                    b" :channels formed"
                );

                message!(
                    stream,
                    b":",
                    sv,
                    b" 255 ",
                    client,
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
                        sv,
                        b" 265 ",
                        client,
                        b" :Current local users  ",
                        &n_clients.to_string(),
                        b" , max ",
                        &n_clients.to_string()
                    );

                    message!(
                        stream,
                        b":",
                        sv,
                        b" 266 ",
                        client,
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
                let mut m = stream.new_message()?;
                message_push!(m, b":", user_fullspec, b" PART ", channel);
                if let Some(reason) = reason {
                    message_push!(m, b" :", reason);
                }
                m.validate();
            }
            Message::List { client, infos } => {
                // chirc test suite doesn't like 321
                if false {
                    message!(stream, b":", sv, b" 321 ", client, b" Channel :Users  Name");
                }

                for info in *infos {
                    message!(
                        stream,
                        b":",
                        sv,
                        b" 322 ",
                        client,
                        b" ",
                        &info.name,
                        b" ",
                        &info.count.to_string(),
                        b" :",
                        &info.topic
                    );
                }
                message!(stream, b":", sv, b" 323 ", &client, b" :End of LIST");
            }
            Message::NowAway { client } => {
                message!(
                    stream,
                    b":",
                    sv,
                    b" 306 ",
                    client,
                    b" :You have been marked as being away"
                );
            }
            Message::UnAway { client } => {
                message!(
                    stream,
                    b":",
                    sv,
                    b" 305 ",
                    client,
                    b" :You are no longer marked as being away"
                );
            }
            Message::RplAway {
                client,
                target_nickname,
                away_message,
            } => {
                message!(
                    stream,
                    b":",
                    sv,
                    b" 301 ",
                    client,
                    b" ",
                    target_nickname,
                    b" :",
                    away_message
                );
            }
            Message::RplUserhost { client, info } => {
                let mut m = stream.new_message()?;
                message_push!(m, b":", sv, b" 302 ", client, b" :");
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
                    message_push!(m, nickname);
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
                        hostname
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
                        sv,
                        b" 301 ",
                        client,
                        b" ",
                        target_nickname,
                        b" :",
                        away_message
                    );
                }

                message!(
                    stream,
                    b":",
                    sv,
                    b" 311 ",
                    client,
                    b" ",
                    target_nickname,
                    b" ",
                    username,
                    b" ",
                    hostname,
                    b" * :",
                    realname
                );

                // don't send RPL_WHOISCHANNELS, for privacy reasons
                // (also because the implementation is not done)
                if false {
                    message!(
                        stream,
                        b":",
                        sv,
                        b" 319 ",
                        client,
                        b" ",
                        target_nickname,
                        b" :" // list of channels would go here
                    );
                }

                message!(
                    stream,
                    b":",
                    sv,
                    b" 318 ",
                    client,
                    b" ",
                    target_nickname,
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
                    sv,
                    b" 318 ",
                    client,
                    b" ",
                    target_nickname,
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
                } in *replies
                {
                    let mut m = stream.new_message()?;
                    message_push!(m, b":", sv, b" 352 ", client, b" ");
                    if let Some(channel) = channel {
                        message_push!(m, channel);
                    } else {
                        message_push!(m, b"*");
                    }
                    message_push!(
                        m,
                        b" ",
                        username,
                        b" ",
                        hostname,
                        b" ",
                        sv,
                        b" ",
                        nickname,
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
                    message_push!(m, b" :0 ", realname);
                    m.validate();
                }
                message!(
                    stream,
                    b":",
                    sv,
                    b" 315 ",
                    client,
                    b" ",
                    mask,
                    b" :End of WHO list"
                );
            }
            Message::Quit {
                user_fullspec,
                reason,
            } => {
                message!(stream, b":", user_fullspec, b" QUIT :", reason);
            }
            Message::FatalError { reason } => {
                message!(stream, b":", sv, b" ERROR :", reason);
            }
            Message::Err(err) => {
                let mut m = stream.new_message()?;
                message_push!(m, b":", sv, b" ");
                err.write_to(m).validate();
            }
        }

        Some(())
    }
}
