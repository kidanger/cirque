use crate::types::ChannelID;

#[derive(Debug)]
pub(crate) enum Message {
    Cap,
    Nick(String),
    User(String),
    Ping(Option<Vec<u8>>),
    Pong(Option<Vec<u8>>),
    Join(Vec<ChannelID>),
    Topic(ChannelID, Option<Vec<u8>>),
    AskModeChannel(ChannelID),
    PrivMsg(String, Vec<u8>),
    Notice(String, Vec<u8>),
    Part(Vec<ChannelID>, Option<Vec<u8>>),
    Quit,
    Unknown(String),
}

pub(crate) enum MessageDecodingError {
    CannotDecodeUtf8 { command: Vec<u8> },
    NotEnoughParameters { command: String },
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
            b"NICK" => Message::Nick(str(opt(message.first_parameter_as_vec())?)?),
            b"USER" => Message::User(str(opt(message.first_parameter_as_vec())?)?),
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
            b"TOPIC" => Message::Topic(
                str(opt(message.first_parameter_as_vec())?)?,
                params.get(1).map(|e| e.to_vec()),
            ),
            b"PING" => Message::Ping(message.first_parameter_as_vec()),
            b"MODE" => Message::AskModeChannel(str(opt(message.first_parameter_as_vec())?)?),
            b"PRIVMSG" => {
                let target = str(opt(message.first_parameter_as_vec())?)?;
                let content = params[1].to_vec();
                Message::PrivMsg(target, content)
            }
            b"NOTICE" => {
                let target = str(opt(message.first_parameter_as_vec())?)?;
                let content = params[1].to_vec();
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
            b"QUIT" => Message::Quit,
            cmd => {
                let cmd = str(cmd.to_vec())?;
                Message::Unknown(cmd)
            }
        };
        Ok(message)
    }
}
