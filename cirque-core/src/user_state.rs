use crate::client_to_server;
use crate::server_state::ServerState;
use crate::types::UserID;

#[derive(Debug)]
pub struct RegisteringState {
    pub(crate) user_id: UserID,
}

impl RegisteringState {
    pub(crate) fn new(user_id: UserID) -> Self {
        Self { user_id }
    }

    fn handle_message(
        self,
        server_state: &ServerState,
        message: cirque_parser::Message<'_>,
    ) -> UserState {
        let message = match client_to_server::Message::try_from(&message) {
            Ok(message) => message,
            Err(error) => {
                return server_state.ruser_sends_invalid_message(self, error);
            }
        };

        match message {
            client_to_server::Message::Cap => {
                // ignore for now
                UserState::Registering(self)
            }
            client_to_server::Message::Pass(password) => {
                server_state.ruser_uses_password(self, &password)
            }
            client_to_server::Message::Nick(nick) => server_state.ruser_uses_nick(self, &nick),
            client_to_server::Message::User(username, realname) => {
                server_state.ruser_uses_username(self, &username, &realname)
            }
            client_to_server::Message::Quit(reason) => {
                server_state.ruser_disconnects_voluntarily(self, reason.as_deref())
            }
            client_to_server::Message::Ping(token) => server_state.ruser_pings(self, &token),
            client_to_server::Message::Unknown(command) => {
                server_state.ruser_sends_unknown_command(self, &command)
            }
            client_to_server::Message::PrivMsg(_, _) => {
                // some valid commands should return ErrNotRegistered when not registered
                server_state.ruser_sends_command_but_is_not_registered(self)
            }
            _ => {
                // valid commands should not return replies
                UserState::Registering(self)
            }
        }
    }
}

#[derive(Debug)]
pub struct RegisteredState {
    pub(crate) user_id: UserID,
}

impl RegisteredState {
    fn handle_message(
        self,
        server_state: &ServerState,
        message: cirque_parser::Message<'_>,
    ) -> UserState {
        let message = match client_to_server::Message::try_from(&message) {
            Ok(message) => message,
            Err(error) => {
                return server_state.user_sends_invalid_message(self, error);
            }
        };

        match message {
            client_to_server::Message::Join(channels) => {
                server_state.user_joins_channels(self, &channels)
            }
            client_to_server::Message::Names(channels) => {
                server_state.user_names_channels(self, &channels)
            }
            client_to_server::Message::Nick(nick) => server_state.user_changes_nick(self, &nick),
            client_to_server::Message::Part(channels, reason) => {
                server_state.user_leaves_channels(self, &channels, reason.as_deref())
            }
            client_to_server::Message::AskModeChannel(channel) => {
                server_state.user_asks_channel_mode(self, &channel)
            }
            client_to_server::Message::ChangeModeChannel(channel, modechar, param) => {
                server_state.user_changes_channel_mode(self, &channel, &modechar, param.as_deref())
            }
            client_to_server::Message::Ping(token) => server_state.user_pings(self, &token),
            client_to_server::Message::Pong(_token) => UserState::Registered(self),
            client_to_server::Message::Quit(reason) => {
                server_state.user_disconnects_voluntarily(self, reason.as_deref())
            }
            client_to_server::Message::PrivMsg(target, content) => {
                server_state.user_messages_target(self, &target, &content)
            }
            client_to_server::Message::Notice(target, content) => {
                server_state.user_notices_target(self, &target, &content)
            }
            client_to_server::Message::SetTopic(target, content) => {
                server_state.user_sets_topic(self, &target, &content)
            }
            client_to_server::Message::GetTopic(target) => {
                server_state.user_wants_topic(self, &target)
            }
            client_to_server::Message::MOTD() => server_state.user_wants_motd(self),
            client_to_server::Message::Away(away_message) => {
                server_state.user_indicates_away(self, away_message.as_deref())
            }
            client_to_server::Message::Userhost(nicknames) => {
                server_state.user_asks_userhosts(self, &nicknames)
            }
            client_to_server::Message::Whois(nickname) => {
                server_state.user_asks_whois(self, &nickname)
            }
            client_to_server::Message::Who(mask) => server_state.user_asks_who(self, &mask),
            client_to_server::Message::Lusers() => server_state.user_asks_lusers(self),
            client_to_server::Message::Unknown(command) => {
                server_state.user_sends_unknown_command(self, &command)
            }
            client_to_server::Message::List(list_channels, list_option) => {
                server_state.user_sends_list_info(self, list_channels, list_option)
            }

            // weird behaviors from the client:
            client_to_server::Message::Cap => UserState::Registered(self),
            client_to_server::Message::User(_, _) => UserState::Registered(self),
            client_to_server::Message::Pass(_) => UserState::Registered(self),
        }
    }
}

#[derive(Debug)]
pub enum UserState {
    Registering(RegisteringState),
    Registered(RegisteredState),
    Disconnected,
}

impl UserState {
    pub fn is_alive(&self) -> bool {
        match self {
            UserState::Registering(_) => true,
            UserState::Registered(_) => true,
            UserState::Disconnected => false,
        }
    }

    pub fn handle_message(
        self,
        server_state: &ServerState,
        message: cirque_parser::Message<'_>,
    ) -> UserState {
        match self {
            UserState::Registering(session_state) => {
                session_state.handle_message(server_state, message)
            }
            UserState::Registered(session_state) => {
                session_state.handle_message(server_state, message)
            }
            UserState::Disconnected => self,
        }
    }
}
