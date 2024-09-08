use std::time::{Duration, Instant};

use crate::client_to_server;
use crate::server_state::ServerState;
use crate::types::UserID;

#[derive(Debug)]
struct Ping {
    token: Vec<u8>,
    at: Instant,
}

#[derive(Debug)]
struct Pong {
    token: Vec<u8>,
}

#[derive(Debug)]
struct PingState {
    created: Instant,
    last_sent: Option<Ping>,
    last_received: Option<Pong>,
}

#[derive(Debug)]
pub struct RegisteringState {
    pub(crate) user_id: UserID,
    ping_state: PingState,
}

impl RegisteringState {
    pub(crate) fn new(user_id: UserID) -> Self {
        Self {
            user_id,
            ping_state: PingState {
                created: Instant::now(),
                last_sent: None,
                last_received: None,
            },
        }
    }

    fn handle_message(
        mut self,
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
            client_to_server::Message::Pong(token) => {
                self.ping_state.last_received = Some(Pong { token });
                UserState::Registering(self)
            }
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
    ping_state: PingState,
}

impl RegisteredState {
    pub(crate) fn from_registering_state(user_state: RegisteringState) -> Self {
        Self {
            user_id: user_state.user_id,
            ping_state: user_state.ping_state,
        }
    }

    fn handle_message(
        mut self,
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
            client_to_server::Message::Pong(token) => {
                self.ping_state.last_received = Some(Pong { token });
                UserState::Registered(self)
            }
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
            Self::Registering(_) => true,
            Self::Registered(_) => true,
            Self::Disconnected => false,
        }
    }

    pub fn handle_message(
        self,
        server_state: &ServerState,
        message: cirque_parser::Message<'_>,
    ) -> Self {
        match self {
            Self::Registering(session_state) => session_state.handle_message(server_state, message),
            Self::Registered(session_state) => session_state.handle_message(server_state, message),
            Self::Disconnected => self,
        }
    }

    pub fn check_timeout(self, server_state: &ServerState) -> Self {
        let Some(timeout) = server_state.get_timeout() else {
            return self;
        };

        #[derive(Debug)]
        enum PingStatus {
            AllGood,
            Timeout(Duration),
            NeedToSend,
        }

        fn check_ping_status(status: &PingState, timeout: Duration) -> PingStatus {
            if status.created.elapsed().as_secs_f32() < 5. {
                return PingStatus::AllGood;
            }
            match (&status.last_sent, &status.last_received) {
                (None, None) => PingStatus::NeedToSend,
                (None, Some(_)) => PingStatus::NeedToSend,
                (Some(ping), None) => {
                    let elapsed = ping.at.elapsed();
                    if elapsed < timeout {
                        PingStatus::AllGood
                    } else {
                        PingStatus::Timeout(elapsed)
                    }
                }
                (Some(ping), Some(pong)) => {
                    let elapsed_sent = ping.at.elapsed();

                    if elapsed_sent < timeout {
                        // not yet timed-out, nothing to do
                        return PingStatus::AllGood;
                    }

                    if ping.token == pong.token {
                        // if the time is up, but the token was replied, we need to send a new ping
                        return PingStatus::NeedToSend;
                    }

                    // the client didn't reply to the ping in time
                    PingStatus::Timeout(elapsed_sent)
                }
            }
        }

        fn generate_token() -> Vec<u8> {
            uuid::Uuid::new_v4().to_string().into()
        }

        let status = match &self {
            UserState::Registering(state) => check_ping_status(&state.ping_state, timeout),
            UserState::Registered(state) => check_ping_status(&state.ping_state, timeout),
            UserState::Disconnected => PingStatus::AllGood,
        };

        match status {
            PingStatus::AllGood => self,
            PingStatus::Timeout(duration) => {
                let reason = format!("Timeout ({:.2}s)", duration.as_secs_f32());
                let reason = reason.as_bytes();
                match self {
                    UserState::Registering(state) => {
                        server_state.ruser_disconnects_voluntarily(state, Some(reason))
                    }
                    UserState::Registered(state) => {
                        server_state.user_disconnects_voluntarily(state, Some(reason))
                    }
                    UserState::Disconnected => self,
                }
            }
            PingStatus::NeedToSend => {
                let token = generate_token();
                match self {
                    UserState::Registering(mut state) => {
                        state.ping_state.last_sent = Some(Ping {
                            token: token.clone(),
                            at: Instant::now(),
                        });
                        server_state.send_ping_to_ruser(state, &token)
                    }
                    UserState::Registered(mut state) => {
                        state.ping_state.last_sent = Some(Ping {
                            token: token.clone(),
                            at: Instant::now(),
                        });
                        server_state.send_ping_to_user(state, &token)
                    }
                    UserState::Disconnected => self,
                }
            }
        }
    }
}
