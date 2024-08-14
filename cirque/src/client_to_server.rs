use crate::ChannelID;

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
    Part(Vec<ChannelID>, Option<Vec<u8>>),
    Quit,
    Unknown,
}

impl TryFrom<cirque_parser::Message> for Message {
    type Error = anyhow::Error;

    fn try_from(value: cirque_parser::Message) -> Result<Self, Self::Error> {
        let message = match value.command().as_slice() {
            b"CAP" => Message::Cap,
            b"NICK" => Message::Nick(String::from_utf8(value.parameters()[0].clone())?),
            b"USER" => Message::User(String::from_utf8(value.parameters()[0].clone())?),
            b"PONG" => Message::Pong(value.parameters()[0].clone()),
            b"JOIN" => {
                let channels = value.parameters()[0]
                    .split(|&c| c == b',')
                    .flat_map(|s| String::from_utf8(s.to_owned()))
                    .collect::<Vec<_>>();
                Message::Join(channels)
            }
            b"TOPIC" => Message::Topic(
                String::from_utf8(value.parameters()[0].clone())?,
                value.parameters().get(1).cloned(),
            ),
            b"PING" => Message::Ping(value.parameters()[0].clone()),
            b"MODE" => Message::AskModeChannel(String::from_utf8(value.parameters()[0].clone())?),
            b"PRIVMSG" => {
                let target = String::from_utf8(value.parameters()[0].clone())?;
                let content = value.parameters()[1].clone();
                Message::PrivMsg(target, content)
            }
            b"PART" => {
                let channels = value.parameters()[0]
                    .split(|&c| c == b',')
                    .flat_map(|s| String::from_utf8(s.to_owned()))
                    .collect::<Vec<_>>();
                let reason = value.parameters().get(1).cloned();
                Message::Part(channels, reason)
            }
            b"QUIT" => Message::Quit,
            _ => Message::Unknown,
        };
        Ok(message)
    }
}
