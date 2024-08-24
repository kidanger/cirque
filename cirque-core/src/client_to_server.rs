use crate::types::ChannelID;

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
pub(crate) enum Message {
    Cap,
    Nick(String),
    User(String, Vec<u8>),
    Pass(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Join(Vec<ChannelID>),
    Names(Vec<ChannelID>),
    GetTopic(ChannelID),
    SetTopic(ChannelID, Vec<u8>),
    AskModeChannel(ChannelID),
    ChangeModeChannel(ChannelID, String, Option<String>),
    PrivMsg(String, Vec<u8>),
    Notice(String, Vec<u8>),
    Part(Vec<ChannelID>, Option<Vec<u8>>),
    List(Option<Vec<String>>, Option<Vec<ListOption>>),
    #[allow(clippy::upper_case_acronyms)]
    MOTD(),
    Away(Option<Vec<u8>>),
    Userhost(Vec<String>),
    Whois(String),
    Who(String),
    Lusers(),
    Quit(Option<Vec<u8>>),
    Unknown(String),
}

pub(crate) enum MessageDecodingError {
    CannotDecodeUtf8 { command: Vec<u8> },
    NotEnoughParameters { command: String },
    CannotParseInteger { command: Vec<u8> },
    NoNicknameGiven {},
    NoTextToSend {},
    NoRecipient { command: String },
    SilentError {},
}

impl TryFrom<&cirque_parser::Message<'_>> for Message {
    type Error = MessageDecodingError;

    fn try_from(message: &cirque_parser::Message<'_>) -> Result<Self, Self::Error> {
        let str = |s: Vec<u8>| -> Result<String, MessageDecodingError> {
            String::from_utf8(s).map_err(|_| MessageDecodingError::CannotDecodeUtf8 {
                command: message.command().to_vec(),
            })
        };
        let opt = |opt: Option<Vec<u8>>| -> Result<Vec<u8>, MessageDecodingError> {
            opt.ok_or(MessageDecodingError::NotEnoughParameters {
                command: str(message.command().to_vec())?,
            })
        };
        let params = message.parameters();
        let message = match message.command() {
            b"CAP" => Message::Cap,
            b"NICK" => {
                let nick = message
                    .first_parameter_as_vec()
                    .ok_or(MessageDecodingError::NoNicknameGiven {})?;
                let nick = str(nick)?;
                if nick.is_empty() {
                    return Err(MessageDecodingError::NoNicknameGiven {});
                }
                Message::Nick(nick)
            }
            b"USER" => {
                let user = str(opt(message.first_parameter_as_vec())?)?;
                if user.is_empty() {
                    return Err(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    });
                }
                let Some(realname) = params.get(3).map(|p| p.to_vec()) else {
                    return Err(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    });
                };
                Message::User(user, realname)
            }
            b"PASS" => {
                let pass = message.first_parameter_as_vec().ok_or(
                    MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    },
                )?;
                Message::Pass(pass)
            }
            b"PING" => Message::Ping(opt(message.first_parameter_as_vec())?),
            b"PONG" => Message::Pong(opt(message.first_parameter_as_vec())?),
            b"JOIN" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    })?
                    .split(|&c| c == b',')
                    .flat_map(|s| str(s.to_owned()))
                    .map(|mut s| {
                        s.make_ascii_lowercase();
                        s
                    })
                    .collect::<Vec<_>>();
                Message::Join(channels)
            }
            b"NAMES" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    })?
                    .split(|&c| c == b',')
                    .flat_map(|s| str(s.to_owned()))
                    .map(|mut s| {
                        s.make_ascii_lowercase();
                        s
                    })
                    .collect::<Vec<_>>();
                Message::Names(channels)
            }
            b"TOPIC" => {
                let mut target = str(opt(message.first_parameter_as_vec())?)?;
                target.make_ascii_lowercase();
                match params.get(1) {
                    Some(content) => {
                        let content = content.to_vec();
                        Message::SetTopic(target, content)
                    }
                    None => Message::GetTopic(target),
                }
            }
            b"MODE" => {
                let mut target = str(opt(message.first_parameter_as_vec())?)?;

                // for now we will assume that the target is a channel
                if !target.starts_with('#') {
                    return Err(MessageDecodingError::NoRecipient {
                        command: str(message.command().into())?,
                    });
                }

                target.make_ascii_lowercase();
                if let Some(change) = params.get(1) {
                    let param = if let Some(param) = params.get(2) {
                        Some(str(param.to_vec())?)
                    } else {
                        None
                    };
                    let modechar = str(change.to_vec())?;
                    Message::ChangeModeChannel(target, modechar, param)
                } else {
                    Message::AskModeChannel(target)
                }
            }
            b"PRIVMSG" => {
                let target =
                    message
                        .first_parameter_as_vec()
                        .ok_or(MessageDecodingError::NoRecipient {
                            command: str(message.command().to_vec())?,
                        })?;
                let mut target = str(target)?;
                if target.starts_with('#') {
                    target.make_ascii_lowercase();
                }
                let content = params
                    .get(1)
                    .ok_or(MessageDecodingError::NoTextToSend {})?
                    .to_vec();
                Message::PrivMsg(target, content)
            }
            b"NOTICE" => {
                let target = message
                    .first_parameter_as_vec()
                    .ok_or(MessageDecodingError::SilentError {})?;
                let mut target = str(target)?;
                if target.starts_with('#') {
                    target.make_ascii_lowercase();
                }
                let content = params
                    .get(1)
                    .ok_or(MessageDecodingError::SilentError {})?
                    .to_vec();
                Message::Notice(target, content)
            }
            b"PART" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    })?
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
            b"LIST" => {
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
                        return Err(MessageDecodingError::NotEnoughParameters {
                            command: str(message.command().to_vec())?,
                        });
                    };
                    if option.is_ascii() {
                        list_option.filter = match option {
                            b'C' => ListFilter::ChannelCreation,
                            b'U' => ListFilter::UserNumber,
                            b'T' => ListFilter::TopicUpdate,
                            _ => {
                                return Err(MessageDecodingError::NotEnoughParameters {
                                    command: str(message.command().to_vec())?,
                                });
                            }
                        };
                        index += 1;
                    }

                    let operation = message.parameters().get(param_index + index);
                    if let Some(operation) = operation {
                        list_option.operation = match *operation {
                            b"<" => ListOperation::Inf,
                            b">" => ListOperation::Sup,
                            _ => {
                                return Err(MessageDecodingError::NotEnoughParameters {
                                    command: str(message.command().to_vec())?,
                                })
                            }
                        };
                        index += 1;
                    }

                    let number = message.parameters().get(param_index + index);
                    if let Some(number) = number {
                        let count = str(number.to_vec())?;
                        let count = count.parse::<u64>().map_err(|_| {
                            MessageDecodingError::CannotParseInteger {
                                command: message.command().to_vec(),
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
            b"MOTD" => {
                // don't parse the "server" argument, we don't support multi-server setups
                Message::MOTD()
            }
            b"AWAY" => {
                let away_message = message.first_parameter_as_vec().and_then(|m| {
                    if m.is_empty() {
                        None
                    } else {
                        Some(m)
                    }
                });
                Message::Away(away_message)
            }
            b"USERHOST" => {
                // up-to five nicknames, in separate parameters
                // the first one is mandatory
                if params.is_empty() {
                    return Err(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    });
                }
                let mut nicknames = vec![];
                for p in params.iter().take(5) {
                    let nick = str(p.to_vec())?;
                    nicknames.push(nick);
                }
                Message::Userhost(nicknames)
            }
            b"WHOIS" => {
                let nickname = if let Some(p) = params.get(1) {
                    str(p.to_vec())?
                } else {
                    str(opt(message.first_parameter_as_vec())?)?
                };
                Message::Whois(nickname)
            }
            b"WHO" => {
                let mask = str(opt(message.first_parameter_as_vec())?)?;
                Message::Who(mask)
            }
            b"LUSERS" => Message::Lusers(),
            b"QUIT" => {
                let reason = message.first_parameter_as_vec();
                Message::Quit(reason)
            }
            cmd => {
                let cmd = str(cmd.to_vec())?;
                Message::Unknown(cmd)
            }
        };
        Ok(message)
    }
}
