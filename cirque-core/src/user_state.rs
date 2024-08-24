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
        // TODO: take a client_to_server::Message instead
        message: cirque_parser::Message<'_>,
    ) -> UserState {
        let message = match client_to_server::Message::try_from(&message) {
            Ok(message) => message,
            Err(error) => {
                server_state.ruser_sends_invalid_message(self.user_id, error);
                return UserState::Registering(self);
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
                server_state.user_sends_invalid_message(self.user_id, error);
                return UserState::Registered(self);
            }
        };

        match message {
            client_to_server::Message::Join(channels) => {
                for channel in channels {
                    if let Err(err) = server_state.user_joins_channel(self.user_id, &channel) {
                        server_state.send_error(self.user_id, err);
                    }
                }
            }
            client_to_server::Message::Names(channels) => {
                for channel in channels {
                    if let Err(err) = server_state.user_names_channel(self.user_id, &channel) {
                        server_state.send_error(self.user_id, err);
                    }
                }
            }
            client_to_server::Message::Nick(nick) => {
                if let Err(err) = server_state.user_changes_nick(self.user_id, &nick) {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::Part(channels, reason) => {
                for channel in channels {
                    if let Err(err) =
                        server_state.user_leaves_channel(self.user_id, &channel, reason.as_deref())
                    {
                        server_state.send_error(self.user_id, err);
                    }
                }
            }
            client_to_server::Message::AskModeChannel(channel) => {
                if let Err(err) = server_state.user_asks_channel_mode(self.user_id, &channel) {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::ChangeModeChannel(channel, modechar, param) => {
                if let Err(err) = server_state.user_changes_channel_mode(
                    self.user_id,
                    &channel,
                    &modechar,
                    param.as_deref(),
                ) {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::Ping(token) => {
                server_state.user_pings(self.user_id, &token);
            }
            client_to_server::Message::Pong(_token) => {}
            client_to_server::Message::Quit(reason) => {
                server_state.user_disconnects_voluntarily(self.user_id, reason.as_deref());
                return UserState::Disconnected {};
            }
            client_to_server::Message::PrivMsg(target, content) => {
                if let Err(err) = server_state.user_messages_target(self.user_id, &target, &content)
                {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::Notice(target, content) => {
                server_state.user_notices_target(self.user_id, &target, &content);
            }
            client_to_server::Message::SetTopic(target, content) => {
                if let Err(err) = server_state.user_sets_topic(self.user_id, &target, &content) {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::GetTopic(target) => {
                if let Err(err) = server_state.user_wants_topic(self.user_id, &target) {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::MOTD() => {
                server_state.user_wants_motd(self.user_id);
            }
            client_to_server::Message::Away(away_message) => {
                server_state.user_indicates_away(self.user_id, away_message.as_deref());
            }
            client_to_server::Message::Userhost(nicknames) => {
                server_state.user_asks_userhosts(self.user_id, &nicknames);
            }
            client_to_server::Message::Whois(nickname) => {
                server_state.user_asks_whois(self.user_id, &nickname);
            }
            client_to_server::Message::Who(mask) => {
                server_state.user_asks_who(self.user_id, &mask);
            }
            client_to_server::Message::Lusers() => {
                server_state.user_asks_lusers(self.user_id);
            }
            client_to_server::Message::Unknown(command) => {
                server_state.user_sends_unknown_command(self.user_id, &command);
            }
            client_to_server::Message::List(list_channels, list_option) => {
                server_state.user_sends_list_info(self.user_id, list_channels, list_option);
            }
            _ => {
                // TODO: log
                //println!("illegal command from connected client");
            }
        };

        UserState::Registered(self)
    }
}

#[derive(Debug)]
pub enum UserState {
    Registering(RegisteringState),
    Registered(RegisteredState),
    Disconnected,
}

impl UserState {
    /// If true, the user is probably still connected and the mailbox may contain important
    /// messages to send them.
    pub fn client_disconnected_voluntarily(&self) -> bool {
        match self {
            UserState::Registering(_) => false,
            UserState::Registered(_) => false,
            UserState::Disconnected => true,
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
