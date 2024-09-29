use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;

use crate::client_to_server::{ListFilter, ListOperation, ListOption, MessageDecodingError};
use crate::error::ServerStateError;
use crate::message_writer::MailboxSink;
use crate::nickname::cure_nickname;
use crate::server_to_client::{
    self, ChannelInfo, MessageContext, NamesReply, UserhostReply, WhoReply,
};
use crate::types::{
    Channel, ChannelMode, ChannelUserMode, RegisteredUser, RegisteringUser, UserID, WelcomeConfig,
};
use crate::user_state::{RegisteredState, RegisteringState, UserState};

enum LookupResult<'r> {
    Channel(&'r ChannelID, &'r Channel),
    RegisteredUser(&'r RegisteredUser),
}

#[derive(Eq)]
struct ChannelID(String);

impl PartialEq for ChannelID {
    fn eq(&self, other: &Self) -> bool {
        let me = BorrowedChannelID::new(self);
        let other = BorrowedChannelID::new(other);
        me.eq(other)
    }
}

impl std::hash::Hash for ChannelID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        BorrowedChannelID::new(self).hash(state)
    }
}

impl std::borrow::Borrow<BorrowedChannelID> for ChannelID {
    fn borrow(&self) -> &BorrowedChannelID {
        BorrowedChannelID::new(self)
    }
}

impl AsRef<str> for ChannelID {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl std::fmt::Display for ChannelID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Eq)]
#[repr(transparent)]
struct BorrowedChannelID(str);

impl BorrowedChannelID {
    fn new<'a>(value: impl AsRef<str>) -> &'a Self {
        let str = value.as_ref();
        // SAFETY: This is a `newtype`-like transformation. `repr(transparent)` ensures
        // that this is safe and correct.
        unsafe { core::mem::transmute(str) }
    }
}

impl PartialEq for BorrowedChannelID {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl std::hash::Hash for BorrowedChannelID {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .for_each(|byte| {
                state.write_u8(byte);
            });
    }
}

#[derive(Clone)]
pub struct ServerState(Arc<RwLock<ServerStateInner>>);

struct ServerStateInner {
    users: HashMap<UserID, RegisteredUser>,
    registering_users: HashMap<UserID, RegisteringUser>,
    channels: HashMap<ChannelID, Channel>,

    // related to config:
    server_name: String,
    welcome_config: WelcomeConfig,
    password: Option<Vec<u8>>,
    motd: Option<Vec<Vec<u8>>>,
    default_channel_mode: ChannelMode,
    message_context: MessageContext,
    messages_per_second_limit: u32,
    timeout: Option<Duration>,
}

impl ServerState {
    pub fn new(
        server_name: &str,
        welcome_config: &WelcomeConfig,
        motd: Option<Vec<Vec<u8>>>,
        password: Option<Vec<u8>>,
        timeout: Option<Duration>,
    ) -> Self {
        let sv = ServerStateInner {
            users: Default::default(),
            registering_users: Default::default(),
            channels: Default::default(),

            server_name: server_name.to_owned(),
            welcome_config: welcome_config.to_owned(),
            motd,
            password,
            message_context: server_to_client::MessageContext {
                server_name: server_name.to_string(),
            },
            default_channel_mode: Default::default(),
            messages_per_second_limit: 10,
            timeout,
        };
        ServerState(Arc::new(RwLock::new(sv)))
    }

    pub fn dispose_state(&self, state: UserState) {
        match state {
            UserState::Registering(state) => {
                self.ruser_disconnects_suddently(state);
            }
            UserState::Registered(state) => {
                self.user_disconnects_suddently(state);
            }
            UserState::Disconnected => {}
        }
    }
}

impl ServerStateInner {
    fn lookup_target<'r>(&'r self, target: &str) -> Option<LookupResult<'r>> {
        let maybe_channel = self
            .channels
            .get_key_value(BorrowedChannelID::new(target))
            .map(|(name, channel)| LookupResult::Channel(name, channel));
        let maybe_user = self
            .users
            .values()
            .find(|&u| u.nickname.eq_ignore_ascii_case(target))
            .map(LookupResult::RegisteredUser);
        maybe_channel.into_iter().chain(maybe_user).next()
    }

    fn check_nickname(
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

        let nickname_is_valid = if let Some(first_char) = nickname.chars().nth(0) {
            (first_char.is_alphanumeric() || first_char == '_') && nickname.len() <= 16
        } else {
            false
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
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(&cured)
            });
        let another_ruser_has_same_nick = self
            .registering_users
            .values()
            .filter(|u| Some(u.user_id) != user_id)
            .any(|u| {
                cure_nickname(u.nickname.as_deref().unwrap_or_default())
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(&cured)
            });

        if another_user_has_same_nick || another_ruser_has_same_nick {
            return Err(ServerStateError::NicknameInUse {
                client: client.to_string(),
                nickname: nickname.into(),
            });
        }

        Ok(())
    }
}

impl ServerState {
    pub fn new_registering_user(&self) -> (UserState, MailboxSink) {
        let mut sv = self.0.write();

        let mailbox_capacity = 128;
        let (user, rx) = RegisteringUser::new(mailbox_capacity);
        let user_id = user.user_id;
        let state = UserState::Registering(RegisteringState::new(user_id, sv.timeout));

        sv.registering_users.insert(user.user_id, user);

        (state, rx)
    }

    pub fn set_server_name(&self, server_name: &str) {
        let mut sv = self.0.write();
        sv.server_name = server_name.to_string();
        sv.message_context = server_to_client::MessageContext {
            server_name: server_name.to_string(),
        };
    }

    pub fn set_password(&self, password: Option<&[u8]>) {
        let mut sv = self.0.write();
        sv.password = password.map(|s| s.into());
    }

    pub fn set_motd(&self, motd: Option<Vec<Vec<u8>>>) {
        let mut sv = self.0.write();
        sv.motd = motd;
    }

    pub fn get_messages_per_second_limit(&self) -> u32 {
        let sv = self.0.read();
        sv.messages_per_second_limit
    }

    /// Warning: changing the value on ServerState does not affect existing clients.
    pub fn set_messages_per_second_limit(&self, max_messages_per_second: u32) {
        let mut sv = self.0.write();
        sv.messages_per_second_limit = max_messages_per_second;
    }

    pub fn set_default_channel_mode(&self, default_channel_mode: &ChannelMode) {
        let mut sv = self.0.write();
        sv.default_channel_mode = default_channel_mode.clone();
    }

    pub fn get_timeout(&self) -> Option<Duration> {
        let sv = self.0.read();
        sv.timeout
    }

    pub fn set_timeout(&self, timeout: Option<Duration>) {
        let mut sv = self.0.write();
        sv.timeout = timeout;
    }
}

/// Functions for registering users
impl ServerState {
    pub(crate) fn ruser_sends_invalid_message(
        &self,
        user_state: RegisteringState,
        error: MessageDecodingError<'_>,
    ) -> UserState {
        let sv = self.0.read();

        let user_id = user_state.user_id;
        let Some(user) = sv.registering_users.get(&user_state.user_id) else {
            return UserState::Disconnected;
        };

        let client = user.maybe_nickname();
        if let Some(err) = ServerStateError::from_decoding_error_with_client(error, client) {
            sv.send_error(user_id, err);
        }

        UserState::Registering(user_state)
    }

    pub(crate) fn ruser_uses_password(
        &self,
        user_state: RegisteringState,
        password: &[u8],
    ) -> UserState {
        {
            let mut sv = self.0.write();

            let user_id = user_state.user_id;
            let Some(user) = sv.registering_users.get_mut(&user_id) else {
                return UserState::Disconnected;
            };
            user.password = Some(password.into());
        }

        self.check_ruser_registration_state(user_state)
    }

    pub(crate) fn ruser_uses_nick(&self, user_state: RegisteringState, nick: &str) -> UserState {
        {
            let mut sv = self.0.write();

            let user_id = user_state.user_id;
            if let Err(err) = sv.check_nickname(nick, Some(user_id)) {
                sv.send_error(user_id, err);
                return UserState::Registering(user_state);
            }
            let Some(user) = sv.registering_users.get_mut(&user_id) else {
                return UserState::Disconnected;
            };
            user.nickname = Some(nick.into());
        }

        self.check_ruser_registration_state(user_state)
    }

    pub(crate) fn ruser_uses_username(
        &self,
        user_state: RegisteringState,
        username: &str,
        realname: &[u8],
    ) -> UserState {
        {
            let mut sv = self.0.write();

            let Entry::Occupied(mut user) = sv.registering_users.entry(user_state.user_id) else {
                return UserState::Disconnected;
            };
            let user = user.get_mut();
            user.username = Some(username.into());
            user.realname = Some(realname.into());
        }

        self.check_ruser_registration_state(user_state)
    }

    pub(crate) fn ruser_pings(&self, user_state: RegisteringState, token: &[u8]) -> UserState {
        let sv = self.0.read();

        let Some(user) = sv.registering_users.get(&user_state.user_id) else {
            return UserState::Disconnected;
        };
        let message = server_to_client::Message::Pong { token };
        user.send(&message, &sv.message_context);
        UserState::Registering(user_state)
    }

    pub(crate) fn send_ping_to_ruser(
        &self,
        user_state: RegisteringState,
        token: &[u8],
    ) -> UserState {
        let sv = self.0.read();

        let Some(user) = sv.registering_users.get(&user_state.user_id) else {
            return UserState::Disconnected;
        };
        let message = server_to_client::Message::Ping { token };
        user.send(&message, &sv.message_context);
        UserState::Registering(user_state)
    }

    pub(crate) fn ruser_sends_unknown_command(
        &self,
        user_state: RegisteringState,
        command: &str,
    ) -> UserState {
        let sv = self.0.read();

        let Some(user) = sv.registering_users.get(&user_state.user_id) else {
            return UserState::Disconnected;
        };
        let message = server_to_client::Message::Err(ServerStateError::UnknownCommand {
            client: user.maybe_nickname(),
            command: command.to_owned(),
        });
        user.send(&message, &sv.message_context);
        UserState::Registering(user_state)
    }

    pub(crate) fn ruser_sends_command_but_is_not_registered(
        &self,
        user_state: RegisteringState,
    ) -> UserState {
        let sv = self.0.read();

        let Some(user) = sv.registering_users.get(&user_state.user_id) else {
            return UserState::Disconnected;
        };
        let message = server_to_client::Message::Err(ServerStateError::NotRegistered {
            client: user.maybe_nickname(),
        });
        user.send(&message, &sv.message_context);
        UserState::Registering(user_state)
    }

    fn check_ruser_registration_state(&self, user_state: RegisteringState) -> UserState {
        let mut sv = self.0.write();

        let user_id = user_state.user_id;
        let Entry::Occupied(user) = sv.registering_users.entry(user_id) else {
            return UserState::Disconnected;
        };

        if !user.get().is_ready() {
            return UserState::Registering(user_state);
        }

        let user = user.remove();

        use subtle::ConstantTimeEq;
        let user_password = user.password.as_deref().unwrap_or_default();
        let sv_password = sv.password.as_deref().unwrap_or_default();
        if user_password.ct_ne(sv_password).into() {
            let message = server_to_client::Message::Err(ServerStateError::PasswdMismatch {
                client: user.maybe_nickname(),
            });
            user.send(&message, &sv.message_context);
            return UserState::Disconnected;
        }

        let user = RegisteredUser::from(user);
        sv.user_registers(user);
        UserState::Registered(RegisteredState::from_registering_state(user_state))
    }

    pub(crate) fn ruser_disconnects_voluntarily(
        &self,
        user_state: RegisteringState,
        reason: Option<&[u8]>,
    ) -> UserState {
        let mut sv = self.0.write();

        let reason = reason.unwrap_or(b"Client Quit");

        let reason = &b"Closing Link: "
            .iter()
            .copied()
            .chain(sv.server_name.as_bytes().iter().copied())
            .chain(b" (".iter().copied())
            .chain(reason.iter().copied())
            .chain(b")".iter().copied())
            .collect::<Vec<u8>>();

        let user_id = user_state.user_id;
        let Entry::Occupied(user) = sv.registering_users.entry(user_id) else {
            return UserState::Disconnected;
        };

        let message = server_to_client::Message::FatalError { reason };
        let user = user.remove();
        user.send(&message, &sv.message_context);
        UserState::Disconnected
    }

    pub fn ruser_disconnects_suddently(&self, user_state: RegisteringState) -> UserState {
        let mut sv = self.0.write();

        let reason = b"connection closed";

        let user_id = user_state.user_id;
        let Entry::Occupied(user) = sv.registering_users.entry(user_id) else {
            return UserState::Disconnected;
        };
        let message = server_to_client::Message::FatalError { reason };
        let user = user.remove();
        user.send(&message, &sv.message_context);
        UserState::Disconnected
    }
}

/// Functions for registered users
impl ServerStateInner {
    fn send_error(&self, user_id: UserID, error: ServerStateError) {
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
            log::error!("user {user_id:?} not found on send_error for {error}");
        }
    }
}

impl ServerState {
    pub(crate) fn user_joins_channels(
        &self,
        user_state: RegisteredState,
        channels: &[&str],
    ) -> UserState {
        let mut sv = self.0.write();

        let user_id = user_state.user_id;
        for &channel in channels {
            if let Err(err) = sv.user_joins_channel(user_id, channel) {
                sv.send_error(user_id, err);
            }
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_joins_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };
        validate_channel_name(user, channel_name)?;

        let channel = self
            .channels
            .entry(ChannelID(channel_name.to_string()))
            .or_default();

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
        let joiner_spec = &user.fullspec();
        let message = server_to_client::Message::Join {
            channel: channel_name,
            user_fullspec: joiner_spec,
        };
        for (user_id, user_mode) in &channel.users {
            let Some(user) = self.users.get(user_id) else {
                return Ok(()); // internal error
            };
            nicknames.push((&user.nickname, user_mode));
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
            names: &[NamesReply {
                channel_name,
                channel_mode: &channel.mode,
                nicknames: &nicknames,
            }],
        };
        user.send(&message, &self.message_context);

        Ok(())
    }
}

impl ServerState {
    pub(crate) fn user_names_channels(
        &self,
        user_state: RegisteredState,
        channels: &[&str],
    ) -> UserState {
        let sv = self.0.read();

        let user_id = user_state.user_id;
        for &channel in channels {
            if let Err(err) = sv.user_names_channel(user_id, channel) {
                sv.send_error(user_id, err);
            }
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_names_channel(
        &self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };

        let channel_id = BorrowedChannelID::new(channel_name);
        let Some(channel) = self.channels.get(channel_id) else {
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
            let Some(user) = self.users.get(user_id) else {
                return Ok(()); // internal error
            };
            nicknames.push((&user.nickname, user_mode));
        }

        let message = server_to_client::Message::Names {
            client: &user.nickname,
            names: &[NamesReply {
                channel_name,
                channel_mode: &channel.mode,
                nicknames: &nicknames,
            }],
        };
        user.send(&message, &self.message_context);
        Ok(())
    }
}

impl ServerState {
    pub(crate) fn user_leaves_channels(
        &self,
        user_state: RegisteredState,
        channels: &[&str],
        reason: Option<&[u8]>,
    ) -> UserState {
        let mut sv = self.0.write();

        let user_id = user_state.user_id;
        for &channel in channels {
            if let Err(err) = sv.user_leaves_channel(user_id, channel, reason) {
                sv.send_error(user_id, err)
            }
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_leaves_channel(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        reason: Option<&[u8]>,
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };
        validate_channel_name(user, channel_name)?;

        let channel_id = BorrowedChannelID::new(channel_name);
        let Some(channel) = self.channels.get_mut(channel_id) else {
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
            let Some(user) = self.users.get(user_id) else {
                return Ok(()); // internal error
            };
            user.send(&message, &self.message_context);
        }

        channel.users.remove(&user_id);

        if channel.users.is_empty() {
            self.channels.remove(channel_id);
        }

        Ok(())
    }
}

impl ServerState {
    pub(crate) fn user_disconnects_voluntarily(
        &self,
        user_state: RegisteredState,
        reason: Option<&[u8]>,
    ) -> UserState {
        let mut sv = self.0.write();
        sv.user_disconnects_voluntarily(user_state.user_id, reason);
        UserState::Disconnected
    }
}

impl ServerStateInner {
    fn user_disconnects_voluntarily(&mut self, user_id: UserID, reason: Option<&[u8]>) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let reason = reason.unwrap_or(b"Client Quit");

        let message = server_to_client::Message::Quit {
            user_fullspec: &user.fullspec(),
            reason,
        };
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                channel.users.remove(&user_id);
                for user_id in channel.users.keys() {
                    let Some(user) = self.users.get(user_id) else {
                        return; // internal error
                    };
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
}

impl ServerState {
    pub fn user_disconnects_suddently(&self, user_state: RegisteredState) -> UserState {
        let mut sv = self.0.write();
        sv.user_disconnects_suddently(user_state.user_id);
        UserState::Disconnected
    }
}

impl ServerStateInner {
    fn user_disconnects_suddently(&mut self, user_id: UserID) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let reason = b"connection closed";

        let message = server_to_client::Message::Quit {
            user_fullspec: &user.fullspec(),
            reason,
        };
        for channel in self.channels.values_mut() {
            if channel.users.contains_key(&user_id) {
                channel.users.remove(&user_id);
                for user_id in channel.users.keys() {
                    let Some(user) = self.users.get(user_id) else {
                        return; // internal error
                    };
                    user.send(&message, &self.message_context);
                }
            }
        }

        let message = server_to_client::Message::FatalError { reason };
        user.send(&message, &self.message_context);

        self.channels.retain(|_, channel| !channel.users.is_empty());
        self.users.remove(&user_id);
    }
}

impl ServerState {
    pub(crate) fn user_changes_nick(
        &self,
        user_state: RegisteredState,
        new_nick: &str,
    ) -> UserState {
        let mut sv = self.0.write();

        let user_id = user_state.user_id;

        if let Err(err) = sv.check_nickname(new_nick, Some(user_id)) {
            sv.send_error(user_id, err);
            return UserState::Registered(user_state);
        }

        let Some(user) = sv.users.get_mut(&user_id) else {
            return UserState::Disconnected;
        };

        if user.nickname == new_nick {
            return UserState::Registered(user_state);
        }

        let message = server_to_client::Message::Nick {
            previous_user_fullspec: &user.fullspec(),
            nickname: new_nick,
        };

        user.nickname = new_nick.to_string();

        let mut users = HashSet::new();
        users.insert(user_id);
        for channel in sv.channels.values() {
            if channel.users.contains_key(&user_id) {
                for &user_id in channel.users.keys() {
                    users.insert(user_id);
                }
            }
        }

        for user_id in users {
            let Some(user) = sv.users.get(&user_id) else {
                return UserState::Disconnected; // internal error
            };
            user.send(&message, &sv.message_context);
        }

        UserState::Registered(user_state)
    }

    pub(crate) fn user_messages_target(
        &self,
        user_state: RegisteredState,
        target: &str,
        content: &[u8],
    ) -> UserState {
        let sv = self.0.read();

        let user_id = user_state.user_id;
        if let Err(err) = sv.user_messages_target(user_id, target, content) {
            sv.send_error(user_id, err);
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_messages_target(
        &self,
        user_id: UserID,
        target: &str,
        content: &[u8],
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };

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

        match obj {
            LookupResult::Channel(channel_name, channel) => {
                let message = server_to_client::Message::PrivMsg {
                    from_user: &user.fullspec(),
                    target: channel_name.as_ref(),
                    content,
                };

                channel.ensure_user_can_send_message(user, target)?;

                channel
                    .users
                    .keys()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message, &self.message_context));
            }
            LookupResult::RegisteredUser(target_user) => {
                let message = server_to_client::Message::PrivMsg {
                    from_user: &user.fullspec(),
                    target,
                    content,
                };
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
}

impl ServerState {
    pub(crate) fn user_notices_target(
        &self,
        user_state: RegisteredState,
        target: &str,
        content: &[u8],
    ) -> UserState {
        let sv = self.0.read();

        let user_id = user_state.user_id;
        sv.user_notices_target(user_id, target, content);

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_notices_target(&self, user_id: UserID, target: &str, content: &[u8]) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };

        if content.is_empty() {
            // NOTICE shouldn't receive an error
            return;
        }

        let Some(obj) = self.lookup_target(target) else {
            // NOTICE shouldn't receive an error
            return;
        };

        match obj {
            LookupResult::Channel(channel_name, channel) => {
                if channel.ensure_user_can_send_message(user, target).is_err() {
                    // NOTICE shouldn't receive an error
                    return;
                }

                let message = server_to_client::Message::PrivMsg {
                    from_user: &user.fullspec(),
                    target: channel_name.as_ref(),
                    content,
                };

                channel
                    .users
                    .keys()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message, &self.message_context));
            }
            LookupResult::RegisteredUser(target_user) => {
                let message = server_to_client::Message::Notice {
                    from_user: &user.fullspec(),
                    target,
                    content,
                };
                target_user.send(&message, &self.message_context);
            }
        }
    }
}

impl ServerState {
    pub(crate) fn user_asks_channel_mode(
        &self,
        user_state: RegisteredState,
        channel_name: &str,
    ) -> UserState {
        let sv = self.0.read();
        let user_id = user_state.user_id;
        if let Err(err) = sv.user_asks_channel_mode(user_id, channel_name) {
            sv.send_error(user_id, err);
        }
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_asks_channel_mode(
        &self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };
        validate_channel_name(user, channel_name)?;

        let channel_id = BorrowedChannelID::new(channel_name);
        let Some(channel) = self.channels.get(channel_id) else {
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
}

impl ServerState {
    pub(crate) fn user_changes_channel_mode(
        &self,
        user_state: RegisteredState,
        channel_name: &str,
        modechar: &str,
        param: Option<&str>,
    ) -> UserState {
        let mut sv = self.0.write();

        let user_id = user_state.user_id;
        if let Err(err) = sv.user_changes_channel_mode(user_id, channel_name, modechar, param) {
            sv.send_error(user_id, err);
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_changes_channel_mode(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        modechar: &str,
        param: Option<&str>,
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };
        validate_channel_name(user, channel_name)?;

        let channel_id = BorrowedChannelID::new(channel_name);
        let Some(channel) = self.channels.get_mut(channel_id) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        };

        channel.ensure_user_can_set_channel_mode(user, channel_name)?;

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
            "+o" | "-o" | "+v" | "-v" => {
                let Some(target) = param else {
                    return Err(ServerStateError::NeedMoreParams {
                        client: user.nickname.clone(),
                        command: "MODE".to_string(),
                    });
                };

                let Some(target_user) = self.users.values().find(|&u| u.nickname == target) else {
                    return Err(ServerStateError::NoSuchNick {
                        client: user.nickname.clone(),
                        target: target.to_string(),
                    });
                };

                let user_id = target_user.user_id;
                let Some(cur_target_mode) = channel.users.get_mut(&user_id) else {
                    return Err(ServerStateError::UserNotInChannel {
                        client: user.nickname.clone(),
                        nickname: target.to_string(),
                        channel: channel_name.to_string(),
                    });
                };

                let new_target_mode = match modechar {
                    "+o" => cur_target_mode.with_op(),
                    "+v" => cur_target_mode.with_voice(),
                    "-o" => cur_target_mode.without_op(),
                    "-v" => cur_target_mode.without_voice(),
                    _ => {
                        // remove the + or -
                        let letters = modechar.chars().skip(1).collect();
                        return Err(ServerStateError::UnknownMode {
                            client: user.nickname.clone(),
                            modechar: letters,
                        });
                    }
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
                        let Some(user) = self.users.get(user_id) else {
                            return Ok(()); // internal error
                        };
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
                let Some(user) = self.users.get(user_id) else {
                    return Ok(()); // internal error
                };
                user.send(&message, &self.message_context);
            }
        }

        Ok(())
    }
}

impl ServerState {
    pub(crate) fn user_sets_topic(
        &self,
        user_state: RegisteredState,
        channel_name: &str,
        content: &[u8],
    ) -> UserState {
        let mut sv = self.0.write();

        let user_id = user_state.user_id;
        if let Err(err) = sv.user_sets_topic(user_id, channel_name, content) {
            sv.send_error(user_id, err);
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_sets_topic(
        &mut self,
        user_id: UserID,
        channel_name: &str,
        content: &[u8],
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };

        let channel_id = BorrowedChannelID::new(channel_name);
        let Some(channel) = self.channels.get_mut(channel_id) else {
            return Err(ServerStateError::NoSuchChannel {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        };

        channel.ensure_user_can_set_topic(user, channel_name)?;

        channel.topic.content = content.to_vec();
        channel.topic.ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
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
}

impl ServerState {
    pub(crate) fn user_wants_topic(
        &self,
        user_state: RegisteredState,
        channel_name: &str,
    ) -> UserState {
        let sv = self.0.read();

        let user_id = user_state.user_id;
        if let Err(err) = sv.user_wants_topic(user_id, channel_name) {
            sv.send_error(user_id, err);
        }

        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_wants_topic(
        &self,
        user_id: UserID,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let Some(user) = self.users.get(&user_id) else {
            return Ok(()); // internal error
        };

        let channel_id = BorrowedChannelID::new(channel_name);
        let Some(channel) = self.channels.get(channel_id) else {
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
}

impl ServerStateInner {
    fn user_registers(&mut self, user: RegisteredUser) {
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
            motd: self.motd.as_deref(),
        };
        user.send(&message, &self.message_context);

        self.users.insert(user.user_id, user);
    }
}

impl ServerState {
    pub(crate) fn user_pings(&self, user_state: RegisteredState, token: &[u8]) -> UserState {
        let sv = self.0.read();
        sv.user_pings(user_state.user_id, token);
        UserState::Registered(user_state)
    }

    pub(crate) fn send_ping_to_user(&self, user_state: RegisteredState, token: &[u8]) -> UserState {
        let sv = self.0.read();

        let Some(user) = sv.users.get(&user_state.user_id) else {
            return UserState::Disconnected;
        };
        let message = server_to_client::Message::Ping { token };
        user.send(&message, &sv.message_context);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_pings(&self, user_id: UserID, token: &[u8]) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let message = server_to_client::Message::Pong { token };
        user.send(&message, &self.message_context);
    }
}

impl ServerState {
    pub(crate) fn user_sends_unknown_command(
        &self,
        user_state: RegisteredState,
        command: &str,
    ) -> UserState {
        let sv = self.0.read();
        sv.user_sends_unknown_command(user_state.user_id, command);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_sends_unknown_command(&self, user_id: UserID, command: &str) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let message = server_to_client::Message::Err(ServerStateError::UnknownCommand {
            client: user.nickname.clone(),
            command: command.to_owned(),
        });
        user.send(&message, &self.message_context);
    }
}

impl ServerState {
    pub(crate) fn user_sends_invalid_message(
        &self,
        user_state: RegisteredState,
        error: MessageDecodingError<'_>,
    ) -> UserState {
        let sv = self.0.read();
        sv.user_sends_invalid_message(user_state.user_id, error);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_sends_invalid_message(&self, user_id: UserID, error: MessageDecodingError<'_>) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let client = user.nickname.clone();
        if let Some(err) = ServerStateError::from_decoding_error_with_client(error, client) {
            self.send_error(user_id, err);
        }
    }
}

impl ServerState {
    pub(crate) fn user_wants_motd(&self, user_state: RegisteredState) -> UserState {
        let sv = self.0.read();
        sv.user_wants_motd(user_state.user_id);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_wants_motd(&self, user_id: UserID) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let message = server_to_client::Message::MOTD {
            client: &user.nickname,
            motd: self.motd.as_deref(),
        };
        user.send(&message, &self.message_context);
    }

    fn filter_channel(&self, list_option: &ListOption, channel: &Channel) -> bool {
        use std::ops::Div;
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .div(60);
        match list_option.filter {
            ListFilter::ChannelCreation => match list_option.operation {
                ListOperation::Inf => false,
                ListOperation::Sup => false,
            },
            ListFilter::TopicUpdate => match list_option.operation {
                ListOperation::Inf => channel.topic.ts.div(60) - current_time < list_option.number,
                ListOperation::Sup => channel.topic.ts.div(60) - current_time > list_option.number,
            },
            ListFilter::UserNumber => match list_option.operation {
                ListOperation::Inf => channel.users.len() > list_option.number as usize,
                ListOperation::Sup => channel.users.len() < list_option.number as usize,
            },
        }
    }
}

impl ServerState {
    pub(crate) fn user_sends_list_info(
        &self,
        user_state: RegisteredState,
        list_channels: Option<Vec<String>>,
        list_options: Option<Vec<ListOption>>,
    ) -> UserState {
        let sv = self.0.read();
        sv.user_sends_list_info(user_state.user_id, list_channels, list_options);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_sends_list_info(
        &self,
        user_id: UserID,
        list_channels: Option<Vec<String>>,
        list_options: Option<Vec<ListOption>>,
    ) {
        let channels = if let Some(list_channels) = list_channels {
            list_channels
                .into_iter()
                .filter_map(|channel_name| {
                    self.channels
                        .get(BorrowedChannelID::new(&channel_name))
                        .map(|c| (channel_name, c))
                })
                .collect::<Vec<_>>()
        } else {
            self.channels
                .iter()
                .map(|(name, channel)| (name.to_string(), channel))
                .collect::<Vec<_>>()
        };

        let channel_info_list = channels
            .iter()
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
                name: channel_name,
                count: channel.users.len(),
                topic: &channel.topic.content,
            })
            .collect::<Vec<_>>();

        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let message = server_to_client::Message::List {
            client: &user.nickname,
            infos: &channel_info_list,
        };
        user.send(&message, &self.message_context);
    }
}

impl ServerState {
    pub(crate) fn user_indicates_away(
        &self,
        user_state: RegisteredState,
        away_message: Option<&[u8]>,
    ) -> UserState {
        let mut sv = self.0.write();
        sv.user_indicates_away(user_state.user_id, away_message);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_indicates_away(&mut self, user_id: UserID, away_message: Option<&[u8]>) {
        let Some(user) = self.users.get_mut(&user_id) else {
            return;
        };

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
}

impl ServerState {
    pub(crate) fn user_asks_userhosts(
        &self,
        user_state: RegisteredState,
        nicknames: &[&str],
    ) -> UserState {
        let sv = self.0.read();
        sv.user_asks_userhosts(user_state.user_id, nicknames);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_asks_userhosts(&self, user_id: UserID, nicknames: &[&str]) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
        let mut replies = vec![];
        for nick in nicknames {
            if let Some(user) = self.users.values().find(|&u| &u.nickname == nick) {
                let reply = UserhostReply {
                    nickname: &user.nickname,
                    is_op: false, // no one is OP for now
                    is_away: user.is_away(),
                    hostname: user.shown_hostname(),
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
}

impl ServerState {
    pub(crate) fn user_asks_whois(&self, user_state: RegisteredState, nickname: &str) -> UserState {
        let sv = self.0.read();
        sv.user_asks_whois(user_state.user_id, nickname);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_asks_whois(&self, user_id: UserID, nickname: &str) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };
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
}

impl ServerState {
    pub(crate) fn user_asks_who(&self, user_state: RegisteredState, mask: &str) -> UserState {
        let sv = self.0.read();
        sv.user_asks_who(user_state.user_id, mask);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_asks_who(&self, user_id: UserID, mask: &str) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };

        // mask patterns are not handled
        let result = self.lookup_target(mask);

        let mut replies = vec![];
        match result {
            Some(LookupResult::Channel(channel_name, channel)) => {
                for (user_id, user_mode) in &channel.users {
                    let Some(user) = self.users.get(user_id) else {
                        return; // internal error
                    };
                    let reply = WhoReply {
                        channel: Some(channel_name.as_ref()),
                        channel_user_mode: Some(user_mode),
                        nickname: &user.nickname,
                        is_op: false,
                        is_away: user.is_away(),
                        hostname: user.shown_hostname(),
                        username: &user.username,
                        realname: &user.realname,
                    };
                    replies.push(reply);
                }
            }
            Some(LookupResult::RegisteredUser(user)) => {
                let reply = WhoReply {
                    channel: None,
                    channel_user_mode: None,
                    nickname: &user.nickname,
                    is_op: false,
                    is_away: user.is_away(),
                    hostname: user.shown_hostname(),
                    username: &user.username,
                    realname: &user.realname,
                };
                replies.push(reply);
            }
            None => {
                if mask == "*" {
                    for user in self.users.values().take(10) {
                        let reply = WhoReply {
                            channel: None,
                            channel_user_mode: None,
                            nickname: &user.nickname,
                            is_op: false,
                            is_away: user.is_away(),
                            hostname: user.shown_hostname(),
                            username: &user.username,
                            realname: &user.realname,
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
}

impl ServerState {
    pub(crate) fn user_asks_lusers(&self, user_state: RegisteredState) -> UserState {
        let sv = self.0.read();
        sv.user_asks_lusers(user_state.user_id);
        UserState::Registered(user_state)
    }
}

impl ServerStateInner {
    fn user_asks_lusers(&self, user_id: UserID) {
        let Some(user) = self.users.get(&user_id) else {
            return; // internal error
        };

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
    #![allow(clippy::panic)] // fine in tests
    #![allow(clippy::panic_in_result_fn)] // fine in tests
    #![allow(clippy::indexing_slicing)] // fine in tests
    use super::*;

    fn new_server_state() -> ServerState {
        let welcome_config = WelcomeConfig::default();
        let motd = None;
        ServerState::new("srv", &welcome_config, motd, None, None)
    }

    fn r1(user_state: UserState) -> RegisteringState {
        match user_state {
            UserState::Registering(r) => r,
            UserState::Registered(_) => panic!(),
            UserState::Disconnected => panic!(),
        }
    }

    fn r2(user_state: UserState) -> RegisteredState {
        match user_state {
            UserState::Registering(_) => panic!(),
            UserState::Registered(r) => r,
            UserState::Disconnected => panic!(),
        }
    }

    fn collect_mail(sink: &mut MailboxSink) -> Vec<Vec<u8>> {
        let mut messages = vec![];
        while let Ok(m) = sink.try_recv() {
            messages.push(m);
        }
        messages
    }

    #[test]
    fn test_nick_change_same() {
        let server_state = new_server_state();
        let nick1 = "test";

        let (mut state1, mut rx1) = server_state.new_registering_user();
        state1 = server_state.ruser_uses_nick(r1(state1), "jester");
        state1 = server_state.ruser_uses_username(r1(state1), nick1, nick1.as_bytes());
        assert!(collect_mail(&mut rx1).len() > 6);

        let (mut state2, _rx2) = server_state.new_registering_user();
        state2 = server_state.ruser_uses_nick(r1(state2), nick1);
        server_state.ruser_uses_username(r1(state2), nick1, nick1.as_bytes());

        server_state.user_changes_nick(r2(state1), nick1);
        let mails = collect_mail(&mut rx1);
        assert_eq!(
            mails[0],
            b":srv 433 jester test :Nickname is already in use\r\n"
        );
    }

    #[test]
    fn test_nick_change_homoglyph() {
        let server_state = new_server_state();
        let nick1 = "test";
        let nick2 = "tėst";

        let (mut state1, mut rx1) = server_state.new_registering_user();
        state1 = server_state.ruser_uses_nick(r1(state1), "jester");
        state1 = server_state.ruser_uses_username(r1(state1), nick1, nick1.as_bytes());
        assert!(collect_mail(&mut rx1).len() > 6);

        let (mut state2, _rx2) = server_state.new_registering_user();
        state2 = server_state.ruser_uses_nick(r1(state2), nick1);
        server_state.ruser_uses_username(r1(state2), nick1, nick1.as_bytes());

        server_state.user_changes_nick(r2(state1), nick2);
        let mails = collect_mail(&mut rx1);
        assert_eq!(
            mails[0],
            b":srv 433 jester t\xC4\x97st :Nickname is already in use\r\n"
        );
    }
}
