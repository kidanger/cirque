use std::collections::HashMap;
use std::ops::Div;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::client_to_server::{ListFilter, ListOperation, ListOption, MessageDecodingError};
use crate::server_to_client::{self, ChannelInfo};
use crate::transport;
use crate::types::RegisteredUser;
use crate::types::RegisteringUser;
use crate::types::UserID;
use crate::types::{Channel, ChannelUserMode};
use crate::types::{ChannelID, WelcomeConfig};

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
    #[error("411 {client} :No recipient given ({command})")]
    NoRecipient { client: String, command: String },
    #[error("412 {client} :No text to send")]
    NoTextToSend { client: String },
    #[error("421 {client} {command} :Unknown command")]
    UnknownCommand { client: String, command: String },
    #[error("431 {client} :No nickname given")]
    NoNicknameGiven { client: String },
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
    #[error("472 {client} {modechar} :is unknown mode char to me")]
    UnknownMode { client: String, modechar: String },
    #[error("476 {client} {channel} :Bad Channel Mask")]
    BadChanMask { client: String, channel: String },
    #[error("482 {client} {channel} :You're not channel operator")]
    ChanOpPrivsNeeded { client: String, channel: String },
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

    pub(crate) fn from_decoding_error_with_client(
        err: MessageDecodingError,
        client: String,
    ) -> Option<ServerStateError> {
        let err = match err {
            MessageDecodingError::CannotDecodeUtf8 { command } => {
                crate::server_state::ServerStateError::UnknownError {
                    client,
                    command,
                    info: "Cannot decode utf8".to_string(),
                }
            }
            MessageDecodingError::NotEnoughParameters { command } => {
                crate::server_state::ServerStateError::NeedMoreParams { client, command }
            }
            MessageDecodingError::CannotParseInteger { command } => {
                crate::server_state::ServerStateError::UnknownError {
                    client,
                    command,
                    info: "Cannot parse integer".to_string(),
                }
            }
            MessageDecodingError::NoNicknameGiven {} => {
                crate::server_state::ServerStateError::NoNicknameGiven { client }
            }
            MessageDecodingError::NoTextToSend {} => {
                crate::server_state::ServerStateError::NoTextToSend { client }
            }
            MessageDecodingError::NoRecipient { command } => {
                crate::server_state::ServerStateError::NoRecipient { client, command }
            }
            MessageDecodingError::SilentError {} => return None,
        };
        Some(err)
    }
}

enum LookupResult<'r> {
    Channel(&'r Channel),
    RegisteredUser(&'r RegisteredUser),
}

pub struct ServerState {
    server_name: String,
    users: HashMap<UserID, RegisteredUser>,
    connecting_users: HashMap<UserID, RegisteringUser>,
    channels: HashMap<ChannelID, Channel>,
    welcome_config: WelcomeConfig,
    motd_provider: Arc<dyn MOTDProvider + Send + Sync>,
}

impl ServerState {
    pub fn new<MP>(
        server_name: &str,
        welcome_config: &WelcomeConfig,
        motd_provider: Arc<MP>,
    ) -> Self
    where
        MP: MOTDProvider + Send + Sync + 'static,
    {
        Self {
            server_name: server_name.to_owned(),
            users: Default::default(),
            connecting_users: Default::default(),
            channels: Default::default(),
            welcome_config: welcome_config.to_owned(),
            motd_provider,
        }
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    pub(crate) fn shared(self) -> SharedServerState {
        Arc::new(Mutex::new(self))
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
            let user: &RegisteredUser = &self.users[&user_id];
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

    pub(crate) fn user_joins_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];
        validate_channel_name(user, channel_name)?;

        let channel = self.channels.entry(channel_name.to_owned()).or_default();

        if channel.users.contains_key(&user_id) {
            return Ok(());
        }

        let user_mode = if channel.users.is_empty() {
            ChannelUserMode::new_op()
        } else {
            ChannelUserMode::default()
        };

        channel.users.insert(user_id, user_mode);

        // notify everyone, including the joiner
        let mut nicknames = vec![];
        let joiner_spec = self.users[&user_id].fullspec();
        let message = server_to_client::Message::Join {
            channel: channel_name.to_owned(),
            user_fullspec: joiner_spec,
        };
        for (user_id, user_mode) in &channel.users {
            let user: &RegisteredUser = &self.users[user_id];
            nicknames.push((user.nickname.clone(), user_mode.clone()));
            user.send(&message);
        }

        // send topic and names to the joiner
        if channel.topic.is_valid() {
            let message = server_to_client::Message::RplTopic {
                nickname: user.nickname.to_owned(),
                channel: channel_name.to_owned(),
                topic: Some(channel.topic.clone()),
            };
            user.send(&message);
        }

        let message = server_to_client::Message::Names {
            nickname: user.nickname.clone(),
            names: vec![(channel_name.to_owned(), channel.mode.clone(), nicknames)],
        };
        user.send(&message);

        Ok(())
    }

    pub(crate) fn user_names_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];

        let Some(channel) = self.channels.get_mut(channel_name) else {
            // if the channel is invalid or does not exist, returns RPL_ENDOFNAMES (366)
            let message = server_to_client::Message::EndOfNames {
                nickname: user.nickname.clone(),
                channel: channel_name.to_string(),
            };
            user.send(&message);
            return Ok(());
        };

        if channel.mode.is_secret() && !channel.users.contains_key(&user_id) {
            let message = server_to_client::Message::EndOfNames {
                nickname: user.nickname.clone(),
                channel: channel_name.to_string(),
            };
            user.send(&message);
            return Ok(());
        }

        let mut nicknames = vec![];
        for (user_id, user_mode) in &channel.users {
            let user: &RegisteredUser = &self.users[user_id];
            nicknames.push((user.nickname.clone(), user_mode.clone()));
        }

        let message = server_to_client::Message::Names {
            nickname: user.nickname.clone(),
            names: vec![(channel_name.to_owned(), channel.mode.clone(), nicknames)],
        };
        user.send(&message);
        Ok(())
    }

    pub(crate) fn user_leaves_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        reason: &Option<Vec<u8>>,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];
        validate_channel_name(user, channel_name)?;

        let Some(channel) = self.channels.get_mut(channel_name) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        };

        if !channel.users.contains_key(&user_id) {
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
        for user_id in channel.users.keys() {
            let user = &self.users[user_id];
            user.send(&message);
        }

        channel.users.remove(&user_id);

        if channel.users.is_empty() {
            self.channels.remove(channel_name);
        }

        Ok(())
    }

    pub(crate) fn user_disconnects_voluntarily(&mut self, user_id: UserID, reason: Option<&[u8]>) {
        let user = &self.users[&user_id];
        let reason = reason.unwrap_or(b"Client Quit");

        let message = server_to_client::Message::Quit {
            user_fullspec: user.fullspec(),
            reason: reason.to_vec(),
        };
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                channel.users.remove(&user_id);
                for user_id in channel.users.keys() {
                    let user = &self.users[user_id];
                    user.send(&message);
                }
            }
        }

        let message = server_to_client::Message::FatalError {
            reason: (b"Closing Link: ".iter().copied())
                .chain(self.server_name.as_bytes().iter().copied())
                .chain(b" (".iter().copied())
                .chain(reason.iter().copied())
                .chain(b")".iter().copied())
                .collect::<Vec<u8>>(),
        };
        user.send(&message);

        self.channels.retain(|_, channel| !channel.users.is_empty());
        self.users.remove(&user_id);
    }

    pub(crate) fn user_disconnects_suddently(&mut self, user_id: UserID) {
        let user = &self.users[&user_id];
        let reason = b"Disconnected suddently.";

        let message = server_to_client::Message::Quit {
            user_fullspec: user.fullspec(),
            reason: reason.to_vec(),
        };
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                channel.users.remove(&user_id);
                for user_id in channel.users.keys() {
                    let user = &self.users[user_id];
                    user.send(&message);
                }
            }
        }

        let message = server_to_client::Message::FatalError {
            reason: reason.to_vec(),
        };
        user.send(&message);

        self.channels.retain(|_, channel| !channel.users.is_empty());
        self.users.remove(&user_id);
    }

    pub(crate) fn user_changes_nick(
        &mut self,
        user_id: UserID,
        new_nick: &str,
    ) -> Result<(), ServerStateError> {
        self.check_nickname(new_nick, Some(user_id))?;

        let user = self.users.get_mut(&user_id).unwrap();

        let message = server_to_client::Message::Nick {
            previous_user_fullspec: user.fullspec(),
            nickname: new_nick.to_string(),
        };
        new_nick.clone_into(&mut user.nickname);

        // TODO: maybe make sure we don't send it multiple times to the same client?
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                for user_id in channel.users.keys() {
                    let user = &self.users[user_id];
                    user.send(&message);
                }
            }
        }

        Ok(())
    }

    fn lookup_target<'r>(&'r self, target: &str) -> Option<LookupResult<'r>> {
        if let Some(channel) = self.channels.get(target) {
            Some(LookupResult::Channel(channel))
        } else if let Some(user) = self.users.values().find(|&u| u.nickname == target) {
            Some(LookupResult::RegisteredUser(user))
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
                if !channel.users.contains_key(&user_id) {
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
                    .keys()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message));
            }
            LookupResult::RegisteredUser(target_user) => {
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
                if !channel.users.contains_key(&user_id) {
                    // NOTICE shouldn't receive an error
                    return;
                }

                channel
                    .users
                    .keys()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message));
            }
            LookupResult::RegisteredUser(target_user) => {
                target_user.send(&message);
            }
        }
    }

    pub(crate) fn user_asks_channel_mode(
        &mut self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];
        validate_channel_name(user, channel_name)?;

        let Some(channel) = self.channels.get_mut(channel_name) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        };

        let message = server_to_client::Message::ChannelMode {
            nickname: user.nickname.clone(),
            channel: channel_name.to_owned(),
            mode: channel.mode.clone(),
        };

        user.send(&message);
        Ok(())
    }

    pub(crate) fn user_changes_channel_mode(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        modechar: &str,
        param: Option<&str>,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];
        validate_channel_name(user, channel_name)?;

        let Some(channel) = self.channels.get_mut(channel_name) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        };

        channel.ensure_user_can_set_channel_mode(user, channel_name)?;

        let lookup_user = |nickname: &str| -> Result<UserID, ServerStateError> {
            let Some(target_user) = self.users.values().find(|&u| u.nickname == nickname) else {
                return Err(ServerStateError::NoSuchNick {
                    client: user.nickname.clone(),
                    target: nickname.to_string(),
                });
            };

            let user_id = target_user.user_id;
            if !channel.users.contains_key(&user_id) {
                return Err(ServerStateError::UserNotInChannel {
                    client: user.nickname.clone(),
                    nickname: nickname.to_string(),
                    channel: channel_name.to_string(),
                });
            }

            Ok(user_id)
        };

        match modechar {
            "+s" => channel.mode = channel.mode.with_secret(),
            "-s" => channel.mode = channel.mode.without_secret(),
            "+t" => channel.mode = channel.mode.with_topic_protected(),
            "-t" => channel.mode = channel.mode.without_topic_protected(),
            "+o" | "+v" => {
                let target = param.unwrap();
                let target_user_id = lookup_user(target)?;
                let new_user_mode = match modechar {
                    "+o" => ChannelUserMode::new_op(),
                    "+v" => ChannelUserMode::new_voice(),
                    _ => panic!(),
                };
                if channel
                    .users
                    .insert(target_user_id, new_user_mode)
                    .is_some()
                {
                    let message = server_to_client::Message::Mode {
                        user_fullspec: user.fullspec(),
                        target: channel_name.to_string(),
                        modechar: modechar.to_string(),
                        param: Some(target.to_string()),
                    };
                    for user_id in channel.users.keys() {
                        let user = &self.users[user_id];
                        user.send(&message);
                    }
                }
            }
            "-o" | "-v" => {
                let target = param.unwrap();
                let user_id = lookup_user(target)?;
                if channel
                    .users
                    .insert(user_id, ChannelUserMode::default())
                    .is_some()
                {
                    let message = server_to_client::Message::Mode {
                        user_fullspec: user.fullspec(),
                        target: channel_name.to_string(),
                        modechar: modechar.to_string(),
                        param: Some(target.to_string()),
                    };
                    for user_id in channel.users.keys() {
                        let user = &self.users[user_id];
                        user.send(&message);
                    }
                }
            }
            _ => {
                return Err(ServerStateError::UnknownMode {
                    client: user.nickname.clone(),
                    modechar: modechar.to_string(),
                });
            }
        }

        Ok(())
    }

    pub(crate) fn user_sets_topic(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        content: &Vec<u8>,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];

        let Some(channel) = self.channels.get_mut(channel_name) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.into(),
            });
        };

        channel.ensure_user_can_set_topic(user, channel_name)?;

        channel.topic.content.clone_from(content);
        channel.topic.ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        channel.topic.from_nickname.clone_from(&user.nickname);

        let message = &server_to_client::Message::Topic {
            user_fullspec: user.fullspec(),
            channel: channel_name.into(),
            topic: channel.topic.clone(),
        };
        channel
            .users
            .keys()
            .flat_map(|u| self.users.get(u))
            .for_each(|u| u.send(message));
        Ok(())
    }

    pub(crate) fn user_wants_topic(
        &mut self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];

        let Some(channel) = self.channels.get_mut(channel_name) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.into(),
            });
        };

        if !channel.users.contains_key(&user_id) {
            return Err(ServerStateError::NotOnChannel {
                client: user.nickname.clone(),
                channel: channel_name.into(),
            });
        }

        let topic = &channel.topic;
        let message = server_to_client::Message::RplTopic {
            nickname: user.nickname.clone(),
            channel: channel_name.into(),
            topic: if topic.is_valid() {
                Some(channel.topic.clone())
            } else {
                None
            },
        };
        user.send(&message);
        Ok(())
    }

    pub(crate) fn user_connects(&mut self, user: RegisteredUser) {
        let message = server_to_client::Message::Welcome {
            nickname: user.nickname.clone(),
            user_fullspec: user.fullspec(),
            welcome_config: self.welcome_config.clone(),
        };
        user.send(&message);

        let message = server_to_client::Message::LUsers {
            nickname: user.nickname.to_string(),
            n_operators: 0,
            n_unknown_connections: 0,
            n_channels: 0,
            n_clients: 1,
            n_other_servers: 0,
        };
        user.send(&message);

        let message = server_to_client::Message::MOTD {
            nickname: user.nickname.to_string(),
            motd: self.motd_provider.motd(),
        };
        user.send(&message);

        self.users.insert(user.user_id, user);
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
        let client = user.nickname.clone();
        if let Some(err) = ServerStateError::from_decoding_error_with_client(error, client) {
            self.send_error(user_id, err);
        }
    }

    pub(crate) fn user_wants_motd(&self, user_id: UserID) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::MOTD {
            nickname: user.nickname.clone(),
            motd: self.motd_provider.motd(),
        };
        user.send(&message);
    }

    fn filter_channel(&self, list_option: &ListOption, channel: &Channel) -> bool {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .div(60);
        match list_option.filter {
            ListFilter::ChannelCreation => match list_option.operation {
                ListOperation::Inf => false,
                ListOperation::Sup => false,
                ListOperation::Unknown => false,
            },
            ListFilter::TopicUpdate => match list_option.operation {
                ListOperation::Inf => channel.topic.ts.div(60) - current_time < list_option.number,
                ListOperation::Sup => channel.topic.ts.div(60) - current_time > list_option.number,
                ListOperation::Unknown => false,
            },
            ListFilter::UserNumber => match list_option.operation {
                ListOperation::Inf => channel.users.len() > list_option.number.try_into().unwrap(),
                ListOperation::Sup => channel.users.len() < list_option.number.try_into().unwrap(),
                ListOperation::Unknown => false,
            },
            ListFilter::Unknown => false,
        }
    }

    pub(crate) fn user_sends_list_info(
        &self,
        user_id: UserID,
        list_channels: Option<Vec<String>>,
        list_options: Option<Vec<ListOption>>,
    ) {
        let mut channel_info_list: Vec<ChannelInfo> = Vec::new();

        if let Some(list_channels) = list_channels {
            list_channels
                .iter()
                .filter(|channel_name| {
                    let mut is_valid: bool = true;
                    if let Some(ref options) = list_options {
                        if let Some(channel) = self.channels.get(*channel_name) {
                            for option in options {
                                let ok = self.filter_channel(option, channel);
                                if !ok || !is_valid {
                                    is_valid = false;
                                }
                            }
                        }
                    }
                    is_valid
                })
                .for_each(|channel_name| {
                    if let Some(channel) = self.channels.get(channel_name) {
                        channel_info_list.push(ChannelInfo {
                            count: channel.users.len(),
                            name: channel_name.clone(),
                            topic: channel.topic.content.clone(),
                        });
                    }
                })
        } else {
            self.channels
                .iter()
                .filter(|&(_channel_name, channel)| {
                    let mut is_valid: bool = true;
                    if let Some(ref options) = list_options {
                        for option in options {
                            let ok = self.filter_channel(option, channel);
                            if !ok || !is_valid {
                                is_valid = false;
                            }
                        }
                    }
                    is_valid
                })
                .for_each(|(channel_name, channel)| {
                    channel_info_list.push(ChannelInfo {
                        count: channel.users.len(),
                        name: channel_name.clone(),
                        topic: channel.topic.content.clone(),
                    });
                });
        }
        let message = server_to_client::Message::List {
            infos: channel_info_list,
        };
        let user = &self.users[&user_id];
        user.send(&message);
    }
}

pub trait MOTDProvider {
    fn motd(&self) -> Option<Vec<Vec<u8>>>;
}

fn validate_channel_name(
    user: &RegisteredUser,
    channel_name: &str,
) -> Result<(), ServerStateError> {
    if channel_name.is_empty() || !channel_name.starts_with('#') {
        return Err(ServerStateError::BadChanMask {
            client: user.nickname.to_string(),
            channel: channel_name.to_string(),
        });
    }
    Ok(())
}
