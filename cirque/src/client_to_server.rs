use crate::types::ChannelID;

#[derive(Debug)]
pub enum Message {
    Cap,
    Nick(String),
    User(String),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Join(Vec<ChannelID>),
    Topic(ChannelID, Option<Vec<u8>>),
    AskModeChannel(ChannelID),
    PrivMsg(String, Vec<u8>),
    Notice(String, Vec<u8>),
    Part(Vec<ChannelID>, Option<Vec<u8>>),
    Quit,
    Unknown(String),
}

impl TryFrom<cirque_parser::Message<'_>> for Message {
    type Error = anyhow::Error;

    fn try_from(value: cirque_parser::Message) -> Result<Self, Self::Error> {
        let params = value.parameters();
        let message = match value.command() {
            b"CAP" => Message::Cap,
            b"NICK" => Message::Nick(String::from_utf8(params[0].to_vec())?),
            b"USER" => Message::User(String::from_utf8(params[0].to_vec())?),
            b"PONG" => Message::Pong(params[0].to_vec()),
            b"JOIN" => {
                let channels = params[0]
                    .split(|&c| c == b',')
                    .flat_map(|s| String::from_utf8(s.to_owned()))
                    .collect::<Vec<_>>();
                Message::Join(channels)
            }
            b"TOPIC" => Message::Topic(
                String::from_utf8(params[0].to_vec())?,
                params.get(1).map(|e| e.to_vec()),
            ),
            b"PING" => Message::Ping(params[0].to_vec()),
            b"MODE" => Message::AskModeChannel(String::from_utf8(params[0].to_vec())?),
            b"PRIVMSG" => {
                let target = String::from_utf8(params[0].to_vec())?;
                let content = params[1].to_vec();
                Message::PrivMsg(target, content)
            }
            b"NOTICE" => {
                let target = String::from_utf8(params[0].to_vec())?;
                let content = params[1].to_vec();
                Message::Notice(target, content)
            }
            b"PART" => {
                let channels = params[0]
                    .split(|&c| c == b',')
                    .flat_map(|s| String::from_utf8(s.to_owned()))
                    .collect::<Vec<_>>();
                let reason = params.get(1).map(|e| e.to_vec());
                Message::Part(channels, reason)
            }
            b"QUIT" => Message::Quit,
            cmd => {
                let cmd = String::from_utf8(cmd.to_vec())?;
                Message::Unknown(cmd)
            }
        };
        Ok(message)
    }
}
