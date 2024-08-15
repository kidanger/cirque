use crate::server_to_client;
use crate::Channel;
use crate::ChannelID;
use crate::ConnectingUser;
use crate::User;
use crate::UserID;
use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::HashMap;

enum LookupResult<'r> {
    Channel(&'r Channel),
    User(&'r User),
}

#[derive(Debug)]
pub struct ServerState {
    users: HashMap<UserID, User>,
    connecting_users: Vec<ConnectingUser>,
    channels: HashMap<ChannelID, Channel>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            users: Default::default(),
            connecting_users: vec![],
            channels: Default::default(),
        }
    }

    pub fn user_joins_channel(&mut self, user_id: UserID, channel_name: &str) {
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
            let user = &self.users[user_id];
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

    pub fn user_leaves_channel(
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

    pub fn user_disconnects(&mut self, _user_id: UserID) {}

    fn lookup_target<'r>(&'r self, target: &str) -> Option<LookupResult<'r>> {
        if let Some(channel) = self.channels.get(target) {
            Some(LookupResult::Channel(channel))
        } else if let Some(user) = self.users.values().find(|&u| u.nickname == target) {
            Some(LookupResult::User(user))
        } else {
            None
        }
    }

    pub fn user_messages_target(&mut self, user_id: UserID, target: &str, content: &[u8]) {
        let user = &self.users[&user_id];

        if content.is_empty() {
            let message = server_to_client::Message::ErrNoTextToSend();
            user.send(&message);
            return;
        }

        let Some(obj) = self.lookup_target(target) else {
            let message = server_to_client::Message::ErrNoSuchNick(target.to_string());
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

    pub fn user_asks_channel_mode(&mut self, user_id: UserID, channel: &str) {
        let user = &self.users[&user_id];
        let message =
            server_to_client::Message::ChannelMode(server_to_client::ChannelModeMessage {
                nickname: user.nickname.clone(),
                channel: channel.to_owned(),
                mode: "+n".to_string(),
            });
        user.send(&message);
    }

    pub fn user_topic(&mut self, user_id: UserID, target: &str, content: &Option<Vec<u8>>) {
        if self.users.values().any(|u| u.nickname == target) {
            //TODO: ERR_NOTONCHANNEL
            return;
        }
        dbg!(target);

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
            } else {
                //view a current topic
                let message = server_to_client::Message::Topic(server_to_client::TopicMessage {
                    nickname: user.nickname.clone(),
                    channel: target.into(),
                    topic: Some(channel.topic.clone()),
                });
                user.send(&message);
            }
        } else {
            // TODO: ERR_NOSUCHCHANNEL
        }
    }

    pub fn add_user(&mut self, user: User) {
        self.users.insert(user.id, user);
    }

    pub fn user_pings(&mut self, user_id: UserID, token: &[u8]) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Pong(server_to_client::PongMessage {
            token: token.to_vec(),
        });
        user.send(&message);
    }
}
