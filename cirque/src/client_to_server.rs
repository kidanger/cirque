use crate::ChannelID;

#[derive(Debug)]
pub enum Message {
    Cap,
    Nick(String),
    User(String),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Join(Vec<ChannelID>),
    AskModeChannel(ChannelID),
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
                    .map(|s| String::from_utf8(s.to_owned()))
                    .flatten()
                    .collect::<Vec<_>>();
                Message::Join(channels)
            }
            b"PING" => Message::Ping(value.parameters()[0].clone()),
            b"MODE" => Message::AskModeChannel(String::from_utf8(value.parameters()[0].clone())?),
            b"QUIT" => Message::Quit,
            _ => Message::Unknown,
        };
        Ok(message)
    }
}
