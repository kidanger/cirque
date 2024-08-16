use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::client_to_server::MessageDecodingError;
use crate::server_to_client;
use crate::transport;
use crate::types::Channel;
use crate::types::ChannelID;
use crate::types::ConnectingUser;
use crate::types::User;
use crate::types::UserID;

pub type SharedServerState = Arc<Mutex<ServerState>>;

#[derive(Error, Debug, Clone)]
pub enum ServerStateError {
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
    #[error("412 {client} :No text to send")]
    NoTextToSend { client: String },
    #[error("421 {client} {command} :Unknown command")]
    UnknownCommand { client: String, command: String },
    #[error("433 {client} {nickname} :Nickname is already in use")]
    NicknameInUse { client: String, nickname: String },
    #[error("442 {client} {channel} :You're not on that channel")]
    NotOnChannel { client: String, channel: String },
    #[error("451 {client} :You have not registered")]
    NotRegistered { client: String },
    #[error("461 {client} {command} :Not enough parameters")]
    NeedMoreParams { client: String, command: String },
}

impl ServerStateError {
    pub(crate) async fn write_to(
        &self,
        stream: &mut impl transport::Stream,
    ) -> std::io::Result<()> {
        match self {
            ServerStateError::UnknownError {
                client,
                command,
                info,
            } => {
                stream.write_all(b"400 {client} ____ :{info}").await?;
                stream.write_all(client.as_bytes()).await?;
                stream.write_all(b" ").await?;
                stream.write_all(command).await?;
                stream.write_all(b" :").await?;
                stream.write_all(info.as_bytes()).await?;
            }
            m => {
                // NOTE: later we can optimize to avoid the to_string call
                // currently it prevents us from using Vec<u8> in ServerStateError
                stream.write_all(m.to_string().as_bytes()).await?;
            }
        }
        Ok(())
    }
}

enum LookupResult<'r> {
    Channel(&'r Channel),
    User(&'r User),
}

#[derive(Debug, Default)]
pub struct ServerState {
    users: HashMap<UserID, User>,
    connecting_users: Vec<ConnectingUser>,
    channels: HashMap<ChannelID, Channel>,
}

impl ServerState {
    pub fn new() -> Self {
        Default::default()
    }

    pub(crate) fn check_nickname(
        &self,
        nickname: &str,
        user_id: Option<UserID>,
    ) -> Result<(), ServerStateError> {
        let look = self.lookup_target(nickname);
        if look.is_none() {
            return Ok(());
        }

        let mut nick = "*";
        if let Some(user_id) = user_id {
            let user: &User = &self.users[&user_id];
            nick = &user.nickname;
        }

        Err(ServerStateError::NicknameInUse {
            client: nick.to_string(),
            nickname: nickname.into(),
        })
    }

    pub(crate) fn send_error(&self, user_id: UserID, error: ServerStateError) {
        let user = &self.users[&user_id];
        user.send(&server_to_client::Message::Err(error));
    }

    pub(crate) fn user_joins_channel(&mut self, user_id: UserID, channel_name: &str) {
        let channel = self.channels.entry(channel_name.to_owned()).or_default();

        if channel.users.contains(&user_id) {
            return;
        }

        channel.users.insert(user_id);

        // notify everyone, including the joiner
        let mut nicknames = vec![];
        let joiner_spec = self.users[&user_id].fullspec();
        let message = server_to_client::Message::Join {
            channel: channel_name.to_owned(),
            user_fullspec: joiner_spec,
        };
        for user_id in &channel.users {
            let user: &User = &self.users[user_id];
            nicknames.push(user.nickname.clone());
            user.send(&message);
        }

        // send topic and names to the joiner
        let user = &self.users[&user_id];
        if channel.topic.is_valid() {
            let message = server_to_client::Message::Topic {
                nickname: user.nickname.to_owned(),
                channel: channel_name.to_owned(),
                topic: Some(channel.topic.clone()),
            };
            user.send(&message);
        }

        let message = server_to_client::Message::Names {
            nickname: user.nickname.clone(),
            names: vec![(channel_name.to_owned(), nicknames)],
        };
        user.send(&message);
    }

    pub(crate) fn user_leaves_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        reason: &Option<Vec<u8>>,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];

        let Some(channel) = self.channels.get_mut(channel_name) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        };

        if !channel.users.contains(&user_id) {
            return Err(ServerStateError::NotOnChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        }

        let message = server_to_client::Message::Part {
            user_fullspec: user.fullspec(),
            channel: channel_name.to_string(),
            reason: reason.clone(),
        };
        for user_id in &channel.users {
            let user = &self.users[user_id];
            user.send(&message);
        }

        channel.users.remove(&user_id);

        if channel.users.is_empty() {
            self.channels.remove(channel_name);
        }

        Ok(())
    }

    pub(crate) fn user_disconnects(&mut self, _user_id: UserID) {}

    fn lookup_target<'r>(&'r self, target: &str) -> Option<LookupResult<'r>> {
        if let Some(channel) = self.channels.get(target) {
            Some(LookupResult::Channel(channel))
        } else if let Some(user) = self.users.values().find(|&u| u.nickname == target) {
            Some(LookupResult::User(user))
        } else {
            None
        }
    }

    pub(crate) fn user_messages_target(&mut self, user_id: UserID, target: &str, content: &[u8]) {
        let user = &self.users[&user_id];

        if content.is_empty() {
            let message = server_to_client::Message::Err(ServerStateError::NoTextToSend {
                client: user.nickname.clone(),
            });
            user.send(&message);
            return;
        }

        let Some(obj) = self.lookup_target(target) else {
            let message = server_to_client::Message::Err(ServerStateError::NoSuchNick {
                client: user.nickname.to_string(),
                target: target.to_string(),
            });
            user.send(&message);
            return;
        };

        let message = server_to_client::Message::PrivMsg {
            from_user: user.fullspec(),
            target: target.to_string(),
            content: content.to_vec(),
        };

        match obj {
            LookupResult::Channel(channel) => {
                if !channel.users.contains(&user_id) {
                    let message =
                        server_to_client::Message::Err(ServerStateError::CannotSendToChan {
                            client: user.nickname.to_string(),
                            channel: target.to_string(),
                        });
                    user.send(&message);
                    return;
                }

                channel
                    .users
                    .iter()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message));
            }
            LookupResult::User(target_user) => {
                target_user.send(&message);
            }
        }
    }

    pub(crate) fn user_notices_target(&mut self, user_id: UserID, target: &str, content: &[u8]) {
        let user = &self.users[&user_id];

        if content.is_empty() {
            // NOTICE shouldn't receive an error
            return;
        }

        let Some(obj) = self.lookup_target(target) else {
            // NOTICE shouldn't receive an error
            return;
        };

        let message = server_to_client::Message::Notice {
            from_user: user.fullspec(),
            target: target.to_string(),
            content: content.to_vec(),
        };

        match obj {
            LookupResult::Channel(channel) => {
                if !channel.users.contains(&user_id) {
                    // NOTICE shouldn't receive an error
                    return;
                }

                channel
                    .users
                    .iter()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message));
            }
            LookupResult::User(target_user) => {
                target_user.send(&message);
            }
        }
    }

    pub(crate) fn user_asks_channel_mode(&mut self, user_id: UserID, channel: &str) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::ChannelMode {
            nickname: user.nickname.clone(),
            channel: channel.to_owned(),
            mode: "+n".to_string(),
        };
        user.send(&message);
    }

    pub(crate) fn user_topic(
        &mut self,
        user_id: UserID,
        target: &str,
        content: &Option<Vec<u8>>,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];

        if self.users.values().any(|u| u.nickname == target) {
            return Err(ServerStateError::NotOnChannel {
                client: user.nickname.clone(),
                channel: target.into(),
            });
        }

        if let Some(channel) = self.channels.get_mut(target) {
            //Set a new topic
            if let Some(content) = content {
                channel.topic.content.clone_from(content);
                channel.topic.ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                channel.topic.from_nickname.clone_from(&user.nickname);

                let message = &server_to_client::Message::Topic {
                    nickname: user.nickname.clone(),
                    channel: target.into(),
                    topic: Some(channel.topic.clone()),
                };
                channel
                    .users
                    .iter()
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message));
                Ok(())
            } else {
                //view a current topic
                let message = server_to_client::Message::Topic {
                    nickname: user.nickname.clone(),
                    channel: target.into(),
                    topic: Some(channel.topic.clone()),
                };
                user.send(&message);
                Ok(())
            }
        } else {
            Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: target.into(),
            })
        }
    }

    pub(crate) fn add_user(&mut self, user: User) {
        self.users.insert(user.id, user);
    }

    pub(crate) fn user_pings(&mut self, user_id: UserID, token: &[u8]) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Pong {
            token: token.to_vec(),
        };
        user.send(&message);
    }

    pub(crate) fn user_sends_unknown_command(&mut self, user_id: UserID, command: &str) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Err(ServerStateError::UnknownCommand {
            client: user.nickname.clone(),
            command: command.to_owned(),
        });
        user.send(&message);
    }

    pub(crate) fn user_sends_invalid_message(
        &mut self,
        user_id: UserID,
        error: MessageDecodingError,
    ) {
        let user = &self.users[&user_id];
        match error {
            MessageDecodingError::CannotDecodeUtf8 { command } => {
                let error = crate::server_state::ServerStateError::UnknownError {
                    client: user.nickname.clone(),
                    command,
                    info: "Cannot decode utf8".to_string(),
                };
                self.send_error(user_id, error);
            }
            MessageDecodingError::NotEnoughParameters { command } => {
                let error = crate::server_state::ServerStateError::NeedMoreParams {
                    client: user.nickname.clone(),
                    command,
                };
                self.send_error(user_id, error);
            }
        }
    }
}
