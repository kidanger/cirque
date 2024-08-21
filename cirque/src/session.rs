use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::server_state::SharedServerState;
use crate::transport::AnyStream;
use crate::types::{RegisteringUser, UserID};
use crate::{client_to_server, server_to_client, ServerState};
use cirque_parser::{LendingIterator, MessageIteratorError, StreamParser};

#[derive(Debug)]
struct RegisteringState {
    user_id: UserID,
}

impl RegisteringState {
    fn handle_message(
        self,
        server_state: &mut ServerState,
        message: cirque_parser::Message,
    ) -> SessionState {
        let message = match client_to_server::Message::try_from(&message) {
            Ok(message) => message,
            Err(error) => {
                server_state.ruser_sends_invalid_message(self.user_id, error);
                return SessionState::Registering(self);
            }
        };

        dbg!(&message);

        match message {
            client_to_server::Message::Cap => {
                // ignore for now
            }
            client_to_server::Message::Pass(password) => {
                server_state.ruser_uses_password(self.user_id, &password);
            }
            client_to_server::Message::Nick(nick) => {
                if let Err(err) = server_state.ruser_uses_nick(self.user_id, &nick) {
                    server_state.send_error(self.user_id, err);
                }
            }
            client_to_server::Message::User(username) => {
                server_state.ruser_uses_username(self.user_id, &username);
            }
            client_to_server::Message::Quit(reason) => {
                server_state.ruser_disconnects_voluntarily(self.user_id, reason.as_deref());
                return SessionState::Disconnected;
            }
            client_to_server::Message::Ping(token) => {
                server_state.ruser_pings(self.user_id, &token);
            }
            client_to_server::Message::Unknown(command) => {
                server_state.ruser_sends_unknown_command(self.user_id, &command)
            }
            client_to_server::Message::PrivMsg(_, _) => {
                // some valid commands should return ErrNotRegistered when not registered
                server_state.ruser_sends_command_but_is_not_registered(self.user_id)
            }
            _ => {
                // valid commands should not return replies
            }
        };

        match server_state.check_ruser_registration_state(self.user_id) {
            Ok(true) => SessionState::Registered(RegisteredState {
                user_id: self.user_id,
            }),
            Ok(false) => SessionState::Registering(self),
            Err(()) => SessionState::Disconnected,
        }
    }
}

#[derive(Debug)]
struct RegisteredState {
    user_id: UserID,
}

impl RegisteredState {
    fn handle_message(
        self,
        server_state: &mut ServerState,
        message: cirque_parser::Message,
    ) -> SessionState {
        let message = match client_to_server::Message::try_from(&message) {
            Ok(message) => message,
            Err(error) => {
                server_state.user_sends_invalid_message(self.user_id, error);
                return SessionState::Registered(self);
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
                        server_state.user_leaves_channel(self.user_id, &channel, &reason)
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
            client_to_server::Message::Pong(_) => {}
            client_to_server::Message::Quit(reason) => {
                server_state.user_disconnects_voluntarily(self.user_id, reason.as_deref());
                return SessionState::Disconnected {};
            }
            client_to_server::Message::PrivMsg(target, content) => {
                server_state.user_messages_target(self.user_id, &target, &content);
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
                server_state.user_ask_userhosts(self.user_id, &nicknames);
            }
            client_to_server::Message::Unknown(command) => {
                server_state.user_sends_unknown_command(self.user_id, &command);
            }
            client_to_server::Message::List(list_channels, list_option) => {
                server_state.user_sends_list_info(self.user_id, list_channels, list_option);
            }
            _ => {
                println!("illegal command from connected client");
            }
        };

        SessionState::Registered(self)
    }
}

#[derive(Debug)]
enum SessionState {
    Registering(RegisteringState),
    Registered(RegisteredState),
    Disconnected,
}

impl SessionState {
    fn client_disconnected_voluntarily(&self) -> bool {
        match self {
            SessionState::Registering(_) => false,
            SessionState::Registered(_) => false,
            SessionState::Disconnected => true,
        }
    }

    fn handle_message(
        self,
        server_state: &mut ServerState,
        message: cirque_parser::Message,
    ) -> SessionState {
        match self {
            SessionState::Registering(session_state) => {
                session_state.handle_message(server_state, message)
            }
            SessionState::Registered(session_state) => {
                session_state.handle_message(server_state, message)
            }
            SessionState::Disconnected => self,
        }
    }
}

pub(crate) struct Session {
    stream: AnyStream,
}

impl Session {
    pub(crate) fn init(stream: AnyStream) -> Self {
        Self { stream }
    }

    pub(crate) async fn run(mut self, server_state: SharedServerState) -> anyhow::Result<()> {
        let message_context = server_to_client::MessageContext {
            server_name: server_state.lock().unwrap().server_name().to_owned(),
        };
        let mut stream_parser = StreamParser::default();

        let (user, mut rx) = RegisteringUser::new();
        let user_id = user.user_id;
        server_state.lock().unwrap().ruser_connects(user);
        let mut state = SessionState::Registering(RegisteringState { user_id });

        while !state.client_disconnected_voluntarily() {
            tokio::select! {
                result = self.stream.read_buf(&mut stream_parser) => {
                    let received = result?;

                    if received == 0 {
                        break;
                    }

                    let mut iter = stream_parser.consume_iter();
                    let mut reset_buffer = false;
                    while let Some(message) = iter.next() {
                        let message = match message {
                            Ok(m) => m,
                            Err(MessageIteratorError::BufferFullWithoutMessageError) => {
                                reset_buffer = true;
                                break;
                            }
                            Err(MessageIteratorError::ParsingError(e)) => {
                                dbg!("weird message: {}", e);
                                continue;
                            }
                        };

                        let mut server_state = server_state.lock().unwrap();
                        state = state.handle_message(&mut server_state, message);
                    }
                    if reset_buffer {
                        stream_parser.clear();
                    }
                },
                Some(message) = rx.recv() => {
                    // some tests from irctest requires buffered messages unfortunately
                    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
                    message.write_to(&mut buf, &message_context).await?;
                    self.stream.write_all(&buf.into_inner()).await?;
                }
            }
        }

        if state.client_disconnected_voluntarily() {
            // the client sent a QUIT, handle the disconnection gracefully
            // TODO: maybe tolerate a timeout to send the last messages and then force quit
            while let Ok(msg) = rx.try_recv() {
                msg.write_to(&mut self.stream, &message_context).await?;
            }
            //self.stream.flush().await?;
        } else if let SessionState::Registering(_) = state {
            // the connection was closed without notification
            server_state
                .lock()
                .unwrap()
                .ruser_disconnects_suddently(user_id);
        } else if let SessionState::Registered(_) = state {
            // the connection was closed without notification
            server_state
                .lock()
                .unwrap()
                .user_disconnects_suddently(user_id);
        }

        Ok(())
    }
}
