#[derive(Debug, Default, PartialEq)]
pub(crate) enum ListFilter {
    ChannelCreation,
    TopicUpdate,
    #[default]
    UserNumber,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) enum ListOperation {
    #[default]
    Inf,
    Sup,
}
#[derive(Debug, Default)]
pub(crate) struct ListOption {
    pub filter: ListFilter,
    pub operation: ListOperation,
    pub number: u64,
}

#[derive(Debug)]
pub(crate) enum Message<'m> {
    Cap,
    Nick(&'m str),
    User(&'m str, &'m [u8]),
    Pass(&'m [u8]),
    Ping(&'m [u8]),
    Pong(&'m [u8]),
    Join(Vec<String>),
    Names(Vec<String>),
    GetTopic(String),
    SetTopic(String, Vec<u8>),
    AskModeChannel(String),
    ChangeModeChannel(String, &'m str, Option<&'m str>),
    PrivMsg(&'m str, &'m [u8]),
    Notice(&'m str, &'m [u8]),
    Part(Vec<String>, Option<Vec<u8>>),
    List(Option<Vec<String>>, Option<Vec<ListOption>>),
    #[allow(clippy::upper_case_acronyms)]
    MOTD(),
    Away(Option<&'m [u8]>),
    Userhost(Vec<&'m str>),
    Whois(&'m str),
    Who(&'m str),
    Lusers(),
    Quit(Option<&'m [u8]>),
    Unknown(&'m str),
}

pub(crate) enum MessageDecodingError<'m> {
    CannotDecodeUtf8 { command: &'m [u8] },
    NotEnoughParameters { command: &'m str },
    CannotParseInteger { command: &'m [u8] },
    NoNicknameGiven {},
    NoTextToSend {},
    NoRecipient { command: &'m str },
    SilentError {},
}

impl<'m> TryFrom<cirque_parser::Message<'m>> for Message<'m> {
    type Error = MessageDecodingError<'m>;

    fn try_from(message: cirque_parser::Message<'m>) -> Result<Self, Self::Error> {
        let str = |s: Vec<u8>| -> Result<String, MessageDecodingError<'m>> {
            String::from_utf8(s).map_err(|_| MessageDecodingError::CannotDecodeUtf8 {
                command: message.command(),
            })
        };
        fn str2<'a, 'm>(
            command: &'m str,
            s: &'a [u8],
        ) -> Result<&'a str, MessageDecodingError<'m>> {
            std::str::from_utf8(s).map_err(|_| MessageDecodingError::CannotDecodeUtf8 {
                command: command.as_bytes(),
            })
        }
        fn opt2<'m, 'a: 'm>(
            command: &'a str,
            opt: Option<&'a [u8]>,
        ) -> Result<&'a [u8], MessageDecodingError<'m>> {
            opt.ok_or(MessageDecodingError::NotEnoughParameters { command })
        }
        fn optstr<'m, 'a: 'm>(
            command: &'a str,
            opt: Option<&'a [u8]>,
        ) -> Result<&'a str, MessageDecodingError<'m>> {
            let otp = opt
                .map(|s| std::str::from_utf8(s))
                .transpose()
                .map_err(|_| MessageDecodingError::CannotDecodeUtf8 {
                    command: command.as_bytes(),
                })?;
            otp.ok_or(MessageDecodingError::NotEnoughParameters { command })
        }

        let params = message.parameters();
        let command = message.command();
        let command = std::str::from_utf8(command)
            .map_err(|_| MessageDecodingError::CannotDecodeUtf8 { command })?;

        let message = match command {
            "CAP" => Message::Cap,
            "NICK" => {
                let nick = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NoNicknameGiven {})?;
                let nick = str2(command, nick)?;
                if nick.is_empty() {
                    return Err(MessageDecodingError::NoNicknameGiven {});
                }
                Message::Nick(nick)
            }
            "USER" => {
                let user = optstr(command, message.first_parameter())?;
                let Some(realname) = params.get(3) else {
                    return Err(MessageDecodingError::NotEnoughParameters { command });
                };
                if user.is_empty() || realname.is_empty() {
                    return Err(MessageDecodingError::NotEnoughParameters { command });
                }
                Message::User(user, realname)
            }
            "PASS" => {
                let pass = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters { command })?;
                Message::Pass(pass)
            }
            "PING" => Message::Ping(opt2(command, message.first_parameter())?),
            "PONG" => Message::Pong(opt2(command, message.first_parameter())?),
            "JOIN" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters { command })?
                    .split(|&c| c == b',')
                    .flat_map(|s| str(s.to_owned()))
                    .map(|mut s| {
                        s.make_ascii_lowercase();
                        s
                    })
                    .collect::<Vec<_>>();
                Message::Join(channels)
            }
            "NAMES" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters { command })?
                    .split(|&c| c == b',')
                    .flat_map(|s| str(s.to_owned()))
                    .map(|mut s| {
                        s.make_ascii_lowercase();
                        s
                    })
                    .collect::<Vec<_>>();
                Message::Names(channels)
            }
            "TOPIC" => {
                let target = optstr(command, message.first_parameter())?;
                let mut target = target.to_string();
                target.make_ascii_lowercase();
                match params.get(1) {
                    Some(content) => {
                        let content = content.to_vec();
                        Message::SetTopic(target, content)
                    }
                    None => Message::GetTopic(target),
                }
            }
            "MODE" => {
                let target = optstr(command, message.first_parameter())?;
                let mut target = target.to_string();

                // for now we will assume that the target is a channel
                if !target.starts_with('#') {
                    return Err(MessageDecodingError::NoRecipient { command });
                }

                target.make_ascii_lowercase();
                if let Some(change) = params.get(1) {
                    let param = if let Some(param) = params.get(2) {
                        Some(str2(command, param)?)
                    } else {
                        None
                    };
                    let modechar = str2(command, change)?;
                    Message::ChangeModeChannel(target, modechar, param)
                } else {
                    Message::AskModeChannel(target)
                }
            }
            "PRIVMSG" => {
                let target = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NoRecipient { command })?;
                let target = str2(command, target)?;
                let content = params.get(1).ok_or(MessageDecodingError::NoTextToSend {})?;
                Message::PrivMsg(target, content)
            }
            "NOTICE" => {
                let target = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::SilentError {})?;
                let target = str2(command, target)?;
                let content = params.get(1).ok_or(MessageDecodingError::SilentError {})?;
                Message::Notice(target, content)
            }
            "PART" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters { command })?
                    .split(|&c| c == b',')
                    .flat_map(|s| String::from_utf8(s.to_owned()))
                    .map(|mut s| {
                        s.make_ascii_lowercase();
                        s
                    })
                    .collect::<Vec<_>>();
                let reason = params.get(1).map(|e| e.to_vec());
                Message::Part(channels, reason)
            }
            "LIST" => {
                let (channels, start_index) =
                    if let Some(first_parameter) = message.first_parameter() {
                        let chans = first_parameter
                            .split(|&c| c == b',')
                            .flat_map(|s| String::from_utf8(s.to_owned()))
                            .map(|mut s| {
                                s.make_ascii_lowercase();
                                s
                            })
                            .collect::<Vec<_>>();
                        let empty = chans.is_empty();
                        (Some(chans), if empty { 0 } else { 1 })
                    } else {
                        (None, 0)
                    };

                let mut list_options = Vec::new();
                for (param_index, option) in
                    params.get(start_index..).into_iter().flatten().enumerate()
                {
                    // offset because enumerate starts at 0 and not param_index
                    let param_index = param_index + start_index;

                    let mut list_option: ListOption = Default::default();

                    // TODO/kid: not sure this implementation is correct, the spec is not clear

                    let mut index = param_index;
                    let Some(option) = option.first() else {
                        return Err(MessageDecodingError::NotEnoughParameters { command });
                    };
                    if option.is_ascii() {
                        list_option.filter = match option {
                            b'C' => ListFilter::ChannelCreation,
                            b'U' => ListFilter::UserNumber,
                            b'T' => ListFilter::TopicUpdate,
                            _ => {
                                return Err(MessageDecodingError::NotEnoughParameters { command });
                            }
                        };
                        index += 1;
                    }

                    let operation = message.parameters().get(param_index + index);
                    if let Some(operation) = operation {
                        list_option.operation = match *operation {
                            b"<" => ListOperation::Inf,
                            b">" => ListOperation::Sup,
                            _ => return Err(MessageDecodingError::NotEnoughParameters { command }),
                        };
                        index += 1;
                    }

                    let number = message.parameters().get(param_index + index);
                    if let Some(number) = number {
                        let count = str2(command, number)?;
                        let count = count.parse::<u64>().map_err(|_| {
                            MessageDecodingError::CannotParseInteger {
                                command: command.as_bytes(),
                            }
                        })?;
                        list_option.number = count;
                    }
                    list_options.push(list_option);
                }
                Message::List(
                    channels,
                    if list_options.is_empty() {
                        None
                    } else {
                        Some(list_options)
                    },
                )
            }
            "MOTD" => {
                // don't parse the "server" argument, we don't support multi-server setups
                Message::MOTD()
            }
            "AWAY" => {
                let away_message =
                    message.first_parameter().and_then(
                        |m| {
                            if m.is_empty() {
                                None
                            } else {
                                Some(m)
                            }
                        },
                    );
                Message::Away(away_message)
            }
            "USERHOST" => {
                // up-to five nicknames, in separate parameters
                // the first one is mandatory
                if params.is_empty() {
                    return Err(MessageDecodingError::NotEnoughParameters { command });
                }
                let mut nicknames = vec![];
                for p in params.iter().take(5) {
                    let nick = str2(command, p)?;
                    nicknames.push(nick);
                }
                Message::Userhost(nicknames)
            }
            "WHOIS" => {
                let nickname = if let Some(p) = params.get(1) {
                    str2(command, p)?
                } else {
                    optstr(command, message.first_parameter())?
                };
                Message::Whois(nickname)
            }
            "WHO" => {
                let mask = optstr(command, message.first_parameter())?;
                Message::Who(mask)
            }
            "LUSERS" => Message::Lusers(),
            "QUIT" => {
                let reason = message.first_parameter();
                Message::Quit(reason)
            }
            command => Message::Unknown(command),
        };
        Ok(message)
    }
}
