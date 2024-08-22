use std::collections::{HashMap, HashSet};
use std::ops::Div;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;

use crate::client_to_server::{ListFilter, ListOperation, ListOption, MessageDecodingError};
use crate::message_writer::{MailboxSink, OnGoingMessage};
use crate::nickname::cure_nickname;
use crate::server_to_client::{self, ChannelInfo, MessageContext, UserhostReply, WhoReply};
use crate::types::RegisteringUser;
use crate::types::UserID;
use crate::types::{Channel, ChannelUserMode};
use crate::types::{ChannelID, WelcomeConfig};
use crate::types::{ChannelMode, RegisteredUser};

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

    fn from_decoding_error_with_client(
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
    Channel(&'r String, &'r Channel),
    RegisteredUser(&'r RegisteredUser),
}

pub struct ServerState {
    server_name: String,
    users: HashMap<UserID, RegisteredUser>,
    registering_users: HashMap<UserID, RegisteringUser>,
    channels: HashMap<ChannelID, Channel>,
    welcome_config: WelcomeConfig,
    motd_provider: Arc<dyn MOTDProvider + Send + Sync>,
    password: Option<Vec<u8>>,
    message_context: MessageContext,
    default_channel_mode: ChannelMode,
}

impl ServerState {
    pub fn new<MP>(
        server_name: &str,
        welcome_config: &WelcomeConfig,
        motd_provider: Arc<MP>,
        password: Option<Vec<u8>>,
    ) -> Self
    where
        MP: MOTDProvider + Send + Sync + 'static,
    {
        Self {
            server_name: server_name.to_owned(),
            users: Default::default(),
            registering_users: Default::default(),
            channels: Default::default(),
            welcome_config: welcome_config.to_owned(),
            motd_provider,
            password,
            message_context: server_to_client::MessageContext {
                server_name: server_name.to_string(),
            },
            default_channel_mode: ChannelMode::default().with_no_external(),
        }
    }

    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    pub fn set_server_name(&mut self, server_name: &str) {
        self.server_name = server_name.to_string();
        self.message_context = server_to_client::MessageContext {
            server_name: server_name.to_string(),
        };
    }

    pub fn set_password(&mut self, password: Option<&[u8]>) {
        self.password = password.map(|s| s.into());
    }

    pub fn set_motd_provider<MP>(&mut self, motd_provider: Arc<MP>)
    where
        MP: MOTDProvider + Send + Sync + 'static,
    {
        self.motd_provider = motd_provider;
    }

    pub fn shared(self) -> SharedServerState {
        Arc::new(Mutex::new(self))
    }

    pub(crate) fn check_nickname(
        &self,
        nickname: &str,
        user_id: Option<UserID>,
    ) -> Result<(), ServerStateError> {
        let mut client = "*";
        if let Some(user_id) = user_id {
            if let Some(user) = self.users.get(&user_id) {
                client = &user.nickname;
            }
        }

        let nickname_is_valid = !nickname.is_empty() && {
            let first_char = nickname.chars().nth(0).unwrap();
            first_char.is_alphanumeric() || first_char == '_'
        };

        if !nickname_is_valid {
            return Err(ServerStateError::ErroneousNickname {
                client: client.to_string(),
                nickname: nickname.into(),
            });
        }

        let Some(cured) = cure_nickname(nickname) else {
            return Err(ServerStateError::ErroneousNickname {
                client: client.to_string(),
                nickname: nickname.into(),
            });
        };

        let another_user_has_same_nick = self
            .users
            .values()
            .filter(|u| Some(u.user_id) != user_id)
            .any(|u| {
                cure_nickname(&u.nickname)
                    .unwrap()
                    .eq_ignore_ascii_case(&cured)
            });
        let another_ruser_has_same_nick = self
            .registering_users
            .values()
            .filter(|u| Some(u.user_id) != user_id)
            .any(|u| {
                decancer::cure!(u.nickname.as_deref().unwrap_or_default())
                    .unwrap()
                    .eq_ignore_ascii_case(&decancer::cure!(nickname).unwrap())
            });

        if another_user_has_same_nick || another_ruser_has_same_nick {
            return Err(ServerStateError::NicknameInUse {
                client: client.to_string(),
                nickname: nickname.into(),
            });
        }

        Ok(())
    }

    pub(crate) fn send_error(&self, user_id: UserID, error: ServerStateError) {
        if let Some(user) = self.users.get(&user_id) {
            user.send(
                &server_to_client::Message::Err(error),
                &self.message_context,
            );
        } else if let Some(user) = self.registering_users.get(&user_id) {
            user.send(
                &server_to_client::Message::Err(error),
                &self.message_context,
            );
        } else {
            panic!("user not found");
        }
    }
}

/// Functions for registering users
impl ServerState {
    pub(crate) fn new_registering_user(&mut self) -> (UserID, MailboxSink) {
        let (user, rx) = RegisteringUser::new();
        let user_id = user.user_id;
        self.registering_users.insert(user.user_id, user);
        (user_id, rx)
    }

    pub(crate) fn ruser_sends_invalid_message(
        &mut self,
        user_id: UserID,
        error: MessageDecodingError,
    ) {
        let user = &self.registering_users[&user_id];
        let client = user.maybe_nickname();
        if let Some(err) = ServerStateError::from_decoding_error_with_client(error, client) {
            self.send_error(user_id, err);
        }
    }

    pub(crate) fn ruser_uses_password(&mut self, user_id: UserID, password: &[u8]) {
        let user = self.registering_users.get_mut(&user_id).unwrap();
        user.password = Some(password.into());
    }

    pub(crate) fn ruser_uses_nick(
        &mut self,
        user_id: UserID,
        nick: &str,
    ) -> Result<(), ServerStateError> {
        self.check_nickname(nick, Some(user_id))?;
        let user = self.registering_users.get_mut(&user_id).unwrap();
        user.nickname = Some(nick.into());
        Ok(())
    }

    pub(crate) fn ruser_uses_username(&mut self, user_id: UserID, username: &str, realname: &[u8]) {
        let user = self.registering_users.get_mut(&user_id).unwrap();
        user.username = Some(username.into());
        user.realname = Some(realname.into());
    }

    pub(crate) fn ruser_pings(&mut self, user_id: UserID, token: &[u8]) {
        let user = &self.registering_users[&user_id];
        let message = server_to_client::Message::Pong { token };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn ruser_sends_unknown_command(&mut self, user_id: UserID, command: &str) {
        let user = &self.registering_users[&user_id];
        let message = server_to_client::Message::Err(ServerStateError::UnknownCommand {
            client: user.maybe_nickname(),
            command: command.to_owned(),
        });
        user.send(&message, &self.message_context);
    }

    pub(crate) fn ruser_sends_command_but_is_not_registered(&mut self, user_id: UserID) {
        let user = &self.registering_users[&user_id];
        let message = server_to_client::Message::Err(ServerStateError::NotRegistered {
            client: user.maybe_nickname(),
        });
        user.send(&message, &self.message_context);
    }

    pub(crate) fn check_ruser_registration_state(&mut self, user_id: UserID) -> Result<bool, ()> {
        let user = &self.registering_users[&user_id];
        if !user.is_ready() {
            return Ok(false);
        }

        let user = self.registering_users.remove(&user_id).unwrap();

        if user.password != self.password {
            let message = server_to_client::Message::Err(ServerStateError::PasswdMismatch {
                client: user.maybe_nickname(),
            });
            user.send(&message, &self.message_context);
            return Err(());
        }

        let user = RegisteredUser::from(user);
        self.user_registers(user);
        Ok(true)
    }

    pub(crate) fn ruser_disconnects_voluntarily(&mut self, user_id: UserID, reason: Option<&[u8]>) {
        let user = &self.registering_users[&user_id];
        let reason = reason.unwrap_or(b"Client Quit");

        let reason = &b"Closing Link: "
            .iter()
            .copied()
            .chain(self.server_name.as_bytes().iter().copied())
            .chain(b" (".iter().copied())
            .chain(reason.iter().copied())
            .chain(b")".iter().copied())
            .collect::<Vec<u8>>();
        let message = server_to_client::Message::FatalError { reason };
        user.send(&message, &self.message_context);

        self.registering_users.remove(&user_id);
    }

    pub(crate) fn ruser_disconnects_suddently(&mut self, user_id: UserID) {
        let user = &self.registering_users[&user_id];
        let reason = b"Disconnected suddently.";

        let message = server_to_client::Message::FatalError { reason };
        user.send(&message, &self.message_context);

        self.registering_users.remove(&user_id);
    }
}

/// Functions for registered users
impl ServerState {
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
            channel.mode = self.default_channel_mode.clone();
            ChannelUserMode::default().with_op()
        } else {
            ChannelUserMode::default()
        };

        channel.users.insert(user_id, user_mode);

        // notify everyone, including the joiner
        let mut nicknames = vec![];
        let joiner_spec = &self.users[&user_id].fullspec();
        let message = server_to_client::Message::Join {
            channel: channel_name,
            user_fullspec: joiner_spec,
        };
        for (user_id, user_mode) in &channel.users {
            let user: &RegisteredUser = &self.users[user_id];
            nicknames.push((user.nickname.clone(), user_mode.clone()));
            user.send(&message, &self.message_context);
        }

        // send topic and names to the joiner
        if channel.topic.is_valid() {
            let message = server_to_client::Message::RplTopic {
                client: &user.nickname,
                channel: channel_name,
                topic: Some(&channel.topic),
            };
            user.send(&message, &self.message_context);
        }

        let message = server_to_client::Message::Names {
            client: &user.nickname,
            names: vec![(channel_name, &channel.mode, nicknames)],
        };
        user.send(&message, &self.message_context);

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
                client: &user.nickname,
                channel: channel_name,
            };
            user.send(&message, &self.message_context);
            return Ok(());
        };

        if channel.mode.is_secret() && !channel.users.contains_key(&user_id) {
            let message = server_to_client::Message::EndOfNames {
                client: &user.nickname,
                channel: channel_name,
            };
            user.send(&message, &self.message_context);
            return Ok(());
        }

        let mut nicknames = vec![];
        for (user_id, user_mode) in &channel.users {
            let user: &RegisteredUser = &self.users[user_id];
            nicknames.push((user.nickname.clone(), user_mode.clone()));
        }

        let message = server_to_client::Message::Names {
            client: &user.nickname,
            names: vec![(channel_name, &channel.mode, nicknames)],
        };
        user.send(&message, &self.message_context);
        Ok(())
    }

    pub(crate) fn user_leaves_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        reason: Option<&[u8]>,
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
            user_fullspec: &user.fullspec(),
            channel: channel_name,
            reason,
        };
        for user_id in channel.users.keys() {
            let user = &self.users[user_id];
            user.send(&message, &self.message_context);
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
            user_fullspec: &user.fullspec(),
            reason,
        };
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                channel.users.remove(&user_id);
                for user_id in channel.users.keys() {
                    let user = &self.users[user_id];
                    user.send(&message, &self.message_context);
                }
            }
        }

        let reason = &b"Closing Link: "
            .iter()
            .copied()
            .chain(self.server_name.as_bytes().iter().copied())
            .chain(b" (".iter().copied())
            .chain(reason.iter().copied())
            .chain(b")".iter().copied())
            .collect::<Vec<u8>>();
        let message = server_to_client::Message::FatalError { reason };
        user.send(&message, &self.message_context);

        self.channels.retain(|_, channel| !channel.users.is_empty());
        self.users.remove(&user_id);
    }

    pub(crate) fn user_disconnects_suddently(&mut self, user_id: UserID) {
        let user = &self.users[&user_id];
        let reason = b"Disconnected suddently.";

        let message = server_to_client::Message::Quit {
            user_fullspec: &user.fullspec(),
            reason,
        };
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                channel.users.remove(&user_id);
                for user_id in channel.users.keys() {
                    let user = &self.users[user_id];
                    user.send(&message, &self.message_context);
                }
            }
        }

        let message = server_to_client::Message::FatalError { reason };
        user.send(&message, &self.message_context);

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

        if user.nickname == new_nick {
            return Ok(());
        }

        let message = server_to_client::Message::Nick {
            previous_user_fullspec: &user.fullspec(),
            nickname: new_nick,
        };
        new_nick.clone_into(&mut user.nickname);

        let mut users = HashSet::new();
        users.insert(user_id);
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                for &user_id in channel.users.keys() {
                    users.insert(user_id);
                }
            }
        }

        for user_id in users {
            let user = &self.users[&user_id];
            user.send(&message, &self.message_context);
        }

        Ok(())
    }

    fn lookup_target<'r>(&'r self, target: &str) -> Option<LookupResult<'r>> {
        let maybe_channel = self
            .channels
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(target))
            .map(|(name, channel)| LookupResult::Channel(name, channel));
        let maybe_user = self
            .users
            .values()
            .find(|&u| u.nickname.eq_ignore_ascii_case(target))
            .map(LookupResult::RegisteredUser);
        maybe_channel.into_iter().chain(maybe_user).next()
    }

    pub(crate) fn user_messages_target(
        &mut self,
        user_id: UserID,
        target: &str,
        content: &[u8],
    ) -> Result<(), ServerStateError> {
        let user = &self.users[&user_id];

        if content.is_empty() {
            return Err(ServerStateError::NoTextToSend {
                client: user.nickname.clone(),
            });
        }

        let Some(obj) = self.lookup_target(target) else {
            return Err(ServerStateError::NoSuchNick {
                client: user.nickname.to_string(),
                target: target.to_string(),
            });
        };

        let message = server_to_client::Message::PrivMsg {
            from_user: &user.fullspec(),
            target,
            content,
        };

        match obj {
            LookupResult::Channel(_, channel) => {
                channel.ensure_user_can_send_message(user, target)?;

                channel
                    .users
                    .keys()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message, &self.message_context));
            }
            LookupResult::RegisteredUser(target_user) => {
                target_user.send(&message, &self.message_context);
                if let Some(away_message) = &target_user.away_message {
                    let message = server_to_client::Message::RplAway {
                        client: &user.nickname,
                        target_nickname: &target_user.nickname,
                        away_message,
                    };
                    user.send(&message, &self.message_context);
                }
            }
        }

        Ok(())
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
            from_user: &user.fullspec(),
            target,
            content,
        };

        match obj {
            LookupResult::Channel(_, channel) => {
                if channel.ensure_user_can_send_message(user, target).is_err() {
                    // NOTICE shouldn't receive an error
                    return;
                }

                channel
                    .users
                    .keys()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message, &self.message_context));
            }
            LookupResult::RegisteredUser(target_user) => {
                target_user.send(&message, &self.message_context);
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
            client: &user.nickname,
            channel: channel_name,
            mode: &channel.mode,
        };

        user.send(&message, &self.message_context);
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

        let mut new_channel_mode = channel.mode.clone();
        // TODO handle multiple modechars
        match modechar {
            "+s" => new_channel_mode = new_channel_mode.with_secret(),
            "-s" => new_channel_mode = new_channel_mode.without_secret(),
            "+t" => new_channel_mode = new_channel_mode.with_topic_protected(),
            "-t" => new_channel_mode = new_channel_mode.without_topic_protected(),
            "+m" => new_channel_mode = new_channel_mode.with_moderated(),
            "-m" => new_channel_mode = new_channel_mode.without_moderated(),
            "+n" => new_channel_mode = new_channel_mode.with_no_external(),
            "-n" => new_channel_mode = new_channel_mode.without_no_external(),
            "+o" | "+v" => {
                let Some(target) = param else {
                    return Err(ServerStateError::NeedMoreParams {
                        client: user.nickname.clone(),
                        command: "MODE".to_string(),
                    });
                };
                let target_user_id = lookup_user(target)?;
                let cur_target_mode = channel.users.get_mut(&target_user_id).unwrap();
                let new_target_mode = match modechar {
                    "+o" => cur_target_mode.with_op(),
                    "+v" => cur_target_mode.with_voice(),
                    _ => panic!(),
                };
                if *cur_target_mode != new_target_mode {
                    *cur_target_mode = new_target_mode;
                    let message = server_to_client::Message::Mode {
                        user_fullspec: &user.fullspec(),
                        target: channel_name,
                        modechar,
                        param: Some(target),
                    };
                    for user_id in channel.users.keys() {
                        let user = &self.users[user_id];
                        user.send(&message, &self.message_context);
                    }
                }
            }
            "-o" | "-v" => {
                let Some(target) = param else {
                    return Err(ServerStateError::NeedMoreParams {
                        client: user.nickname.clone(),
                        command: "MODE".to_string(),
                    });
                };
                let target_user_id = lookup_user(target)?;
                let cur_target_mode = channel.users.get_mut(&target_user_id).unwrap();
                let new_target_mode = match modechar {
                    "-o" => cur_target_mode.without_op(),
                    "-v" => cur_target_mode.without_voice(),
                    _ => panic!(),
                };
                if *cur_target_mode != new_target_mode {
                    *cur_target_mode = new_target_mode;
                    let message = server_to_client::Message::Mode {
                        user_fullspec: &user.fullspec(),
                        target: channel_name,
                        modechar,
                        param: Some(target),
                    };
                    for user_id in channel.users.keys() {
                        let user = &self.users[user_id];
                        user.send(&message, &self.message_context);
                    }
                }
            }
            _ => {
                // remove the + or -
                let letters = modechar.chars().skip(1).collect();
                return Err(ServerStateError::UnknownMode {
                    client: user.nickname.clone(),
                    modechar: letters,
                });
            }
        }

        if new_channel_mode != channel.mode {
            channel.mode = new_channel_mode;

            let message = server_to_client::Message::Mode {
                user_fullspec: &user.fullspec(),
                target: channel_name,
                modechar,
                param: None,
            };
            for user_id in channel.users.keys() {
                let user = &self.users[user_id];
                user.send(&message, &self.message_context);
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
            user_fullspec: &user.fullspec(),
            channel: channel_name,
            topic: &channel.topic,
        };
        channel
            .users
            .keys()
            .flat_map(|u| self.users.get(u))
            .for_each(|u| u.send(message, &self.message_context));
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
            client: &user.nickname,
            channel: channel_name,
            topic: if topic.is_valid() {
                Some(&channel.topic)
            } else {
                None
            },
        };
        user.send(&message, &self.message_context);
        Ok(())
    }

    pub(crate) fn user_registers(&mut self, user: RegisteredUser) {
        let message = server_to_client::Message::Welcome {
            nickname: &user.nickname,
            user_fullspec: &user.fullspec(),
            welcome_config: &self.welcome_config,
        };
        user.send(&message, &self.message_context);

        let message = server_to_client::Message::LUsers {
            client: &user.nickname,
            n_operators: 0,
            n_unknown_connections: self.registering_users.len(),
            n_channels: self.channels.len(),
            n_clients: self.users.len(),
            n_other_servers: 0,
            extra_info: false,
        };
        user.send(&message, &self.message_context);

        let message = server_to_client::Message::MOTD {
            client: &user.nickname,
            motd: self.motd_provider.motd(),
        };
        user.send(&message, &self.message_context);

        self.users.insert(user.user_id, user);
    }

    pub(crate) fn user_pings(&mut self, user_id: UserID, token: &[u8]) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Pong { token };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn user_sends_unknown_command(&mut self, user_id: UserID, command: &str) {
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Err(ServerStateError::UnknownCommand {
            client: user.nickname.clone(),
            command: command.to_owned(),
        });
        user.send(&message, &self.message_context);
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
            client: &user.nickname,
            motd: self.motd_provider.motd(),
        };
        user.send(&message, &self.message_context);
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
        let channels = if let Some(list_channels) = list_channels {
            list_channels
                .into_iter()
                .filter_map(|channel_name| {
                    self.channels.get(&channel_name).map(|c| (channel_name, c))
                })
                .collect::<Vec<_>>()
        } else {
            self.channels
                .iter()
                .map(|(name, channel)| (name.to_string(), channel))
                .collect::<Vec<_>>()
        };

        let channel_info_list = channels
            .into_iter()
            .filter(|(_, channel)| {
                !channel.mode.is_secret() || channel.users.contains_key(&user_id)
            })
            .filter(|(_, channel)| {
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
            .map(|(channel_name, channel)| ChannelInfo {
                count: channel.users.len(),
                name: channel_name.clone(),
                topic: channel.topic.content.clone(),
            })
            .collect::<Vec<_>>();

        let user = &self.users[&user_id];
        let message = server_to_client::Message::List {
            client: &user.nickname,
            infos: &channel_info_list,
        };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn user_indicates_away(&mut self, user_id: UserID, away_message: Option<&[u8]>) {
        let user = self.users.get_mut(&user_id).unwrap();
        user.away_message = away_message.map(|m| m.into());

        let message = if user.is_away() {
            server_to_client::Message::NowAway {
                client: &user.nickname,
            }
        } else {
            server_to_client::Message::UnAway {
                client: &user.nickname,
            }
        };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn user_asks_userhosts(&self, user_id: UserID, nicknames: &[String]) {
        let user = &self.users[&user_id];
        let mut replies = vec![];
        for nick in nicknames {
            if let Some(user) = self.users.values().find(|&u| &u.nickname == nick) {
                let reply = UserhostReply {
                    nickname: user.nickname.clone(),
                    is_op: false, // no one is OP for now
                    is_away: user.is_away(),
                    hostname: user.shown_hostname().into(),
                };
                replies.push(reply);
            }
        }
        let message = server_to_client::Message::RplUserhost {
            client: &user.nickname,
            info: &replies,
        };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn user_asks_whois(&self, user_id: UserID, nickname: &str) {
        let user = &self.users[&user_id];
        let Some(target_user) = self.users.values().find(|&u| u.nickname == nickname) else {
            let message = server_to_client::Message::Err(ServerStateError::NoSuchNick {
                client: user.nickname.to_string(),
                target: nickname.to_string(),
            });
            user.send(&message, &self.message_context);
            let message = server_to_client::Message::RplEndOfWhois {
                client: &user.nickname,
                target_nickname: nickname,
            };
            user.send(&message, &self.message_context);
            return;
        };

        let message = server_to_client::Message::RplWhois {
            client: &user.nickname,
            target_nickname: nickname,
            away_message: target_user.away_message.as_deref(),
            hostname: target_user.shown_hostname(),
            username: &target_user.username,
            realname: &target_user.realname,
        };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn user_asks_who(&self, user_id: UserID, mask: &str) {
        let user = &self.users[&user_id];

        // mask patterns are not handled
        let result = self.lookup_target(mask);

        let mut replies = vec![];
        match result {
            Some(LookupResult::Channel(channel_name, channel)) => {
                for (user_id, user_mode) in &channel.users {
                    let user = &self.users[user_id];
                    let reply = WhoReply {
                        channel: Some(channel_name.to_string()),
                        channel_user_mode: Some(user_mode.clone()),
                        nickname: user.nickname.clone(),
                        is_op: false,
                        is_away: user.is_away(),
                        hostname: user.shown_hostname().into(),
                        username: user.username.clone(),
                        realname: user.realname.clone(),
                    };
                    replies.push(reply);
                }
            }
            Some(LookupResult::RegisteredUser(user)) => {
                let reply = WhoReply {
                    channel: None,
                    channel_user_mode: None,
                    nickname: user.nickname.clone(),
                    is_op: false,
                    is_away: user.is_away(),
                    hostname: user.shown_hostname().into(),
                    username: user.username.clone(),
                    realname: user.realname.clone(),
                };
                replies.push(reply);
            }
            None => {
                if mask == "*" {
                    for user in self.users.values().take(10) {
                        let reply = WhoReply {
                            channel: None,
                            channel_user_mode: None,
                            nickname: user.nickname.clone(),
                            is_op: false,
                            is_away: user.is_away(),
                            hostname: user.shown_hostname().into(),
                            username: user.username.clone(),
                            realname: user.realname.clone(),
                        };
                        replies.push(reply);
                    }
                }
            }
        }

        let message = server_to_client::Message::Who {
            client: &user.nickname,
            mask,
            replies: &replies,
        };
        user.send(&message, &self.message_context);
    }

    pub(crate) fn user_asks_lusers(&self, user_id: UserID) {
        let user = &self.users[&user_id];

        let message = server_to_client::Message::LUsers {
            client: &user.nickname,
            n_operators: 0,
            n_unknown_connections: self.registering_users.len(),
            n_channels: self.channels.len(),
            n_clients: self.users.len(),
            n_other_servers: 0,
            extra_info: true,
        };
        user.send(&message, &self.message_context);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FixedMOTDProvider(Option<String>);

    impl MOTDProvider for FixedMOTDProvider {
        fn motd(&self) -> Option<Vec<Vec<u8>>> {
            self.0.as_ref().map(|motd| vec![motd.as_bytes().to_vec()])
        }
    }

    fn new_server_state() -> ServerState {
        let welcome_config = WelcomeConfig::default();
        let motd_provider = FixedMOTDProvider(None).into();
        ServerState::new("srv", &welcome_config, motd_provider, None)
    }

    #[test]
    fn test_nick_change_same() -> anyhow::Result<()> {
        let mut server_state = new_server_state();
        let nick1 = "test";

        let (user1, _rx1) = server_state.new_registering_user();
        server_state.ruser_uses_nick(user1, "jester")?;
        server_state.ruser_uses_username(user1, nick1, nick1.as_bytes());
        assert!(server_state.check_ruser_registration_state(user1).unwrap());

        let (user2, _rx2) = server_state.new_registering_user();
        server_state.ruser_uses_nick(user2, nick1)?;
        server_state.ruser_uses_username(user2, nick1, nick1.as_bytes());
        assert!(server_state.check_ruser_registration_state(user2).unwrap());

        server_state.user_changes_nick(user1, nick1).unwrap_err();
        Ok(())
    }

    #[test]
    fn test_nick_change_homoglyph() -> anyhow::Result<()> {
        let mut server_state = new_server_state();
        let nick1 = "test";
        let nick2 = "tėst";

        let (user1, _rx1) = server_state.new_registering_user();
        server_state.ruser_uses_nick(user1, "jester")?;
        server_state.ruser_uses_username(user1, nick1, nick1.as_bytes());
        assert!(server_state.check_ruser_registration_state(user1).unwrap());

        let (user2, _rx2) = server_state.new_registering_user();
        server_state.ruser_uses_nick(user2, nick1)?;
        server_state.ruser_uses_username(user2, nick1, nick1.as_bytes());
        assert!(server_state.check_ruser_registration_state(user2).unwrap());

        server_state.user_changes_nick(user1, nick2).unwrap_err();
        Ok(())
    }
}
