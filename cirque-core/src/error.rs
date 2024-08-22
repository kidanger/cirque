use crate::client_to_server::MessageDecodingError;
use crate::message_writer::OnGoingMessage;

#[derive(thiserror::Error, Debug, Clone)]
pub(crate) enum ServerStateError {
    // NOTE: for this one, we cannot use string interpolation since the command is not a string
    // (it might not be valid utf8)
    #[error("400 {client} ____ :{info}")]
    UnknownError {
        client: String,
        command: Vec<u8>,
        info: String,
    },
    #[error("401 {client} {target} :No such nick/channel")]
    NoSuchNick { client: String, target: String },
    #[error("403 {client} {channel} :No such channel")]
    NoSuchChannel { client: String, channel: String },
    #[error("404 {client} {channel} :Cannot send to channel")]
    CannotSendToChan { client: String, channel: String },
    #[error("411 {client} :No recipient given ({command})")]
    NoRecipient { client: String, command: String },
    #[error("412 {client} :No text to send")]
    NoTextToSend { client: String },
    #[error("421 {client} {command} :Unknown command")]
    UnknownCommand { client: String, command: String },
    #[error("431 {client} :No nickname given")]
    NoNicknameGiven { client: String },
    #[error("432 {client} {nickname} :Erroneous nickname")]
    ErroneousNickname { client: String, nickname: String },
    #[error("433 {client} {nickname} :Nickname is already in use")]
    NicknameInUse { client: String, nickname: String },
    #[error("441 {client} {nickname} {channel} :They aren't on that channel")]
    UserNotInChannel {
        client: String,
        nickname: String,
        channel: String,
    },
    #[error("442 {client} {channel} :You're not on that channel")]
    NotOnChannel { client: String, channel: String },
    #[error("451 {client} :You have not registered")]
    NotRegistered { client: String },
    #[error("461 {client} {command} :Not enough parameters")]
    NeedMoreParams { client: String, command: String },
    #[error("464 {client} :Password incorrect")]
    PasswdMismatch { client: String },
    #[error("472 {client} {modechar} :is unknown mode char to me")]
    UnknownMode { client: String, modechar: String },
    #[error("476 {client} {channel} :Bad Channel Mask")]
    BadChanMask { client: String, channel: String },
    #[error("482 {client} {channel} :You're not channel operator")]
    ChanOpPrivsNeeded { client: String, channel: String },
}

impl ServerStateError {
    pub(crate) fn write_to<'b, 'c>(&self, mut m: OnGoingMessage<'b, 'c>) -> OnGoingMessage<'b, 'c> {
        match self {
            ServerStateError::UnknownError {
                client,
                command,
                info,
            } => {
                message_push!(
                    m,
                    b"400 {client} ____ :{info}",
                    &client,
                    b" ",
                    command,
                    b" :",
                    &info
                );
                m
            }
            err => {
                // NOTE: later we can optimize to avoid the to_string call
                // currently it prevents us from using Vec<u8> in ServerStateError
                m.write(&err.to_string())
            }
        }
    }

    pub(crate) fn from_decoding_error_with_client(
        err: MessageDecodingError,
        client: String,
    ) -> Option<ServerStateError> {
        let err = match err {
            MessageDecodingError::CannotDecodeUtf8 { command } => ServerStateError::UnknownError {
                client,
                command,
                info: "Cannot decode utf8".to_string(),
            },
            MessageDecodingError::NotEnoughParameters { command } => {
                ServerStateError::NeedMoreParams { client, command }
            }
            MessageDecodingError::CannotParseInteger { command } => {
                ServerStateError::UnknownError {
                    client,
                    command,
                    info: "Cannot parse integer".to_string(),
                }
            }
            MessageDecodingError::NoNicknameGiven {} => {
                ServerStateError::NoNicknameGiven { client }
            }
            MessageDecodingError::NoTextToSend {} => ServerStateError::NoTextToSend { client },
            MessageDecodingError::NoRecipient { command } => {
                ServerStateError::NoRecipient { client, command }
            }
            MessageDecodingError::SilentError {} => return None,
        };
        Some(err)
    }
}
