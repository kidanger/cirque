use crate::types::ChannelID;

#[derive(Debug)]
pub(crate) enum Message {
    Cap,
    Nick(String),
    User(String),
    Ping(Option<Vec<u8>>),
    Pong(Option<Vec<u8>>),
    Join(Vec<ChannelID>),
    GetTopic(ChannelID),
    SetTopic(ChannelID, Vec<u8>),
    AskModeChannel(ChannelID),
    PrivMsg(String, Vec<u8>),
    Notice(String, Vec<u8>),
    Part(Vec<ChannelID>, Option<Vec<u8>>),
    WhoWas(String, Option<usize>),
    #[allow(clippy::upper_case_acronyms)]
    MOTD(),
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

    fn try_from(message: &cirque_parser::Message) -> Result<Self, Self::Error> {
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
                Message::Nick(nick)
            }
            b"USER" => {
                let user = str(opt(message.first_parameter_as_vec())?)?;
                if params.len() < 4 || user.is_empty() {
                    return Err(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    });
                }
                Message::User(user)
            }
            b"PONG" => Message::Pong(message.first_parameter_as_vec()),
            b"JOIN" => {
                let channels = message
                    .first_parameter()
                    .ok_or(MessageDecodingError::NotEnoughParameters {
                        command: str(message.command().to_vec())?,
                    })?
                    .split(|&c| c == b',')
                    .flat_map(|s| String::from_utf8(s.to_owned()))
                    .collect::<Vec<_>>();
                Message::Join(channels)
            }
            b"TOPIC" => {
                let target = str(opt(message.first_parameter_as_vec())?)?;
                match params.get(1) {
                    Some(content) => {
                        let content = content.to_vec();
                        Message::SetTopic(target, content)
                    }
                    None => Message::GetTopic(target),
                }
            }
            b"PING" => Message::Ping(message.first_parameter_as_vec()),
            b"MODE" => Message::AskModeChannel(str(opt(message.first_parameter_as_vec())?)?),
            b"PRIVMSG" => {
                let target =
                    message
                        .first_parameter_as_vec()
                        .ok_or(MessageDecodingError::NoRecipient {
                            command: str(message.command().to_vec())?,
                        })?;
                let target = str(target)?;
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
                let target = str(target)?;
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
                    .collect::<Vec<_>>();
                let reason = params.get(1).map(|e| e.to_vec());
                Message::Part(channels, reason)
            }
            b"WHOWAS" => {
                let target = str(opt(message.first_parameter_as_vec())?)?;
                let count = if let Some(count) = params.get(1) {
                    let count = str(count.to_vec())?;
                    let count = count.parse::<usize>().map_err(|_| {
                        MessageDecodingError::CannotParseInteger {
                            command: message.command().to_vec(),
                        }
                    })?;
                    Some(count)
                } else {
                    None
                };
                Message::WhoWas(target, count)
            }
            b"MOTD" => {
                // don't parse the "server" argument, we don't support multi-server setups
                Message::MOTD()
            }
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
