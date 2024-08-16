use std::sync::{Arc, Mutex};

use crate::server_to_client;
use crate::types::Channel;
use crate::types::ChannelID;
use crate::types::ConnectingUser;
use crate::types::User;
use crate::types::UserID;
use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::HashMap;

use anyhow::Ok;
use thiserror::Error;

pub type SharedServerState = Arc<Mutex<ServerState>>;

#[derive(Error, Debug, Clone)]
pub enum ServerStateError {
    #[error("442 {client} {channel} :You're not on that channel")]
    ErrNotonchannel { client: String, channel: String },
    #[error("403 {client} {channel} :No such channel")]
    ErrNosuchchannel { client: String, channel: String },
    #[error("433 {client} {nickname} :Nickname is already in use")]
    ErrNicknameinuse { client: String, nickname: String },
    #[error("451 {client} :You have not registered")]
    ErrNotRegistered { client: String },
    #[error("421 {client} {command} :Unknown command")]
    ErrUnknownCommand { client: String, command: String },
    #[error("unknown error")]
    Unknown,
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
    ) -> Result<(), anyhow::Error> {
        let look = self.lookup_target(nickname);
        if look.is_none() {
            Ok(())
        } else {
            let mut nick = "*";
            if let Some(user_id) = user_id {
                let user: &User = &self.users[&user_id];
                nick = &user.nickname;
            }

            Err(ServerStateError::ErrNicknameinuse {
                client: nick.to_string(),
                nickname: nickname.into(),
            }
            .into())
        }
    }

    pub(crate) fn send_error(&self, user_id: UserID, error: anyhow::Error) {
        let user = &self.users[&user_id];
        user.send(&server_to_client::Message::Err(error.to_string()));
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
        let message = server_to_client::Message::Join(server_to_client::JoinMessage {
            channel: channel_name.to_owned(),
            user_fullspec: joiner_spec,
        });
        for user_id in &channel.users {
            let user: &User = &self.users[user_id];
            nicknames.push(user.nickname.clone());
            user.send(&message);
        }

        // send topic and names to the joiner
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Topic(server_to_client::TopicMessage {
            nickname: user.nickname.to_owned(),
            channel: channel_name.to_owned(),
            topic: if channel.topic.is_valid() {
                Some(channel.topic.clone())
            } else {
                None
            },
        });
        user.send(&message);
        let message = server_to_client::Message::Names(server_to_client::NamesMessage {
            nickname: user.nickname.clone(),
            names: vec![(channel_name.to_owned(), nicknames)],
        });
        user.send(&message);
    }

    pub(crate) fn user_leaves_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        reason: &Option<Vec<u8>>,
    ) {
        let Some(channel) = self.channels.get_mut(channel_name) else {
            return;
        };

        if !channel.users.contains(&user_id) {
            return;
        }

        let user = &self.users[&user_id];
        let message = server_to_client::Message::Part(server_to_client::PartMessage {
            user_fullspec: user.fullspec(),
            channel: channel_name.to_string(),
            reason: reason.clone(),
        });
        for user_id in &channel.users {
            let user = &self.users[user_id];
            user.send(&message);
        }

        channel.users.remove(&user_id);
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
            let message = server_to_client::Message::ErrNoTextToSend();
            user.send(&message);
            return;
        }

        let Some(obj) = self.lookup_target(target) else {
            let message = server_to_client::Message::ErrNoSuchNick(
                user.nickname.to_string(),
                target.to_string(),
            );
            user.send(&message);
            return;
        };

        let message = server_to_client::Message::PrivMsg(server_to_client::PrivMsgMessage {
            from_user: user.fullspec(),
            target: target.to_string(),
            content: content.to_vec(),
        });

        match obj {
            LookupResult::Channel(channel) => {
                if !channel.users.contains(&user_id) {
                    let message =
                        server_to_client::Message::ErrCannotSendToChan(target.to_string());
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

        let message = server_to_client::Message::Notice(server_to_client::NoticeMessage {
            from_user: user.fullspec(),
            target: target.to_string(),
            content: content.to_vec(),
        });

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
        let message =
            server_to_client::Message::ChannelMode(server_to_client::ChannelModeMessage {
                nickname: user.nickname.clone(),
                channel: channel.to_owned(),
                mode: "+n".to_string(),
            });
        user.send(&message);
    }

    pub(crate) fn user_topic(
        &mut self,
        user_id: UserID,
        target: &str,
        content: &Option<Vec<u8>>,
    ) -> Result<(), anyhow::Error> {
        if self.users.values().any(|u| u.nickname == target) {
            let user = &self.users[&user_id];

            return Err(ServerStateError::ErrNotonchannel {
                client: user.nickname.clone(),
                channel: target.into(),
            }
            .into());
        }

        if let Some(channel) = self.channels.get_mut(target) {
            let user = &self.users[&user_id];

            //Set a new topic
            if let Some(content) = content {
                channel.topic.content.clone_from(content);
                channel.topic.ts = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                channel.topic.from_nickname.clone_from(&user.nickname);

                channel
                    .users
                    .iter()
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| {
                        u.send(&server_to_client::Message::Topic(
                            server_to_client::TopicMessage {
                                nickname: user.nickname.clone(),
                                channel: target.into(),
                                topic: Some(channel.topic.clone()),
                            },
                        ))
                    });
                Ok(())
            } else {
                //view a current topic
                let message = server_to_client::Message::Topic(server_to_client::TopicMessage {
                    nickname: user.nickname.clone(),
                    channel: target.into(),
                    topic: Some(channel.topic.clone()),
                });
                user.send(&message);
                Ok(())
            }
        } else {
            let user = &self.users[&user_id];
            Err(ServerStateError::ErrNosuchchannel {
                client: user.nickname.clone(),
                channel: target.into(),
            }
            .into())
        }
    }

    pub(crate) fn add_user(&mut self, user: User) {
        self.users.insert(user.id, user);
    }

    pub(crate) fn user_pings(&mut self, user_id: UserID, token: &[u8]) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Pong(server_to_client::PongMessage {
            token: token.to_vec(),
        });
        user.send(&message);
    }

    pub(crate) fn user_sends_unknown_command(&mut self, user_id: UserID, command: &str) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::ErrState(ServerStateError::ErrUnknownCommand {
            client: user.nickname.clone(),
            command: command.to_owned(),
        });
        user.send(&message);
    }
}
