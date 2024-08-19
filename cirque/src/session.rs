use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::server_state::{ServerStateError, SharedServerState};
use crate::transport::AnyStream;
use crate::types::{RegisteredUser, RegisteringUser, UserID};
use crate::{client_to_server, server_to_client, ServerState};
use cirque_parser::{LendingIterator, StreamParser};

struct SessionStateRegistering {
    user: RegisteringUser,
}

impl SessionStateRegistering {
    fn process_buffer(
        &self,
        server_state: &mut ServerState,
        stream_parser: &mut StreamParser,
    ) -> anyhow::Result<bool> {
        todo!()
    }
}

struct SessionStateRegistered {
    user_id: UserID,
}

impl SessionStateRegistered {
    fn process_buffer(
        &self,
        server_state: &mut ServerState,
        stream_parser: &mut StreamParser,
    ) -> Result<bool, anyhow::Error> {
        let mut iter = stream_parser.consume_iter();
        while let Some(message) = iter.next() {
            let message = match message {
                Ok(m) => m,
                Err(e) => anyhow::bail!(e.to_string()),
            };
            let message = match client_to_server::Message::try_from(&message) {
                Ok(message) => message,
                Err(error) => {
                    server_state.user_sends_invalid_message(self.user_id, error);
                    continue;
                }
            };
            dbg!(&message);

            match message {
                client_to_server::Message::Join(channels) => {
                    for channel in channels {
                        server_state.user_joins_channel(self.user_id, &channel);
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
                    server_state.user_asks_channel_mode(self.user_id, &channel);
                }
                client_to_server::Message::Ping(token) => {
                    server_state.user_pings(self.user_id, token.as_deref());
                }
                client_to_server::Message::Pong(_) => {}
                client_to_server::Message::Quit(reason) => {
                    server_state.user_disconnects_voluntarily(self.user_id, reason.as_deref());
                    return Ok(true);
                }
                client_to_server::Message::PrivMsg(target, content) => {
                    server_state.user_messages_target(self.user_id, &target, &content);
                }
                client_to_server::Message::Notice(target, content) => {
                    server_state.user_notices_target(self.user_id, &target, &content);
                }
                client_to_server::Message::SetTopic(target, content) => {
                    if let Err(err) = server_state.user_sets_topic(self.user_id, &target, &content)
                    {
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
                client_to_server::Message::Unknown(command) => {
                    server_state.user_sends_unknown_command(self.user_id, &command);
                }
                _ => {
                    println!("illegal command from connected client");
                }
            };
        }
        Ok(false)
    }
}

enum SessionState {
    Registering(SessionStateRegistering),
    Registered(SessionStateRegistered),
}

impl SessionState {
    fn process_buffer(
        &mut self,
        server_state: &mut ServerState,
        stream_parser: &mut StreamParser,
    ) -> anyhow::Result<bool> {
        match self {
            SessionState::Registering(session_state) => {
                session_state.process_buffer(server_state, stream_parser)
            }
            SessionState::Registered(session_state) => {
                session_state.process_buffer(server_state, stream_parser)
            }
        }
    }
}

pub(crate) struct ConnectingSession {
    stream: AnyStream,
    stream_parser: StreamParser,
}

impl ConnectingSession {
    pub(crate) fn new(stream: AnyStream) -> Self {
        Self {
            stream,
            stream_parser: Default::default(),
        }
    }
}

impl ConnectingSession {
    pub(crate) async fn run(
        mut self,
        server_state: &SharedServerState,
    ) -> Result<Session, anyhow::Error> {
        let (mut user, rx) = RegisteringUser::new();

        'outer: loop {
            let received = self.stream.read_buf(&mut self.stream_parser).await?;
            anyhow::ensure!(received > 0, "stream ended");

            let mut iter = self.stream_parser.consume_iter();
            while let Some(message) = iter.next() {
                let message = match message {
                    Ok(m) => m,
                    Err(e) => anyhow::bail!(e.to_string()),
                };
                let message = match client_to_server::Message::try_from(&message) {
                    Ok(message) => message,
                    Err(err) => {
                        let client = user.nickname.clone().unwrap_or("*".to_string());
                        if let Some(err) =
                            ServerStateError::from_decoding_error_with_client(err, client)
                        {
                            server_to_client::Message::Err(err)
                                .write_to(&mut self.stream)
                                .await?;
                        }
                        continue;
                    }
                };
                dbg!(&message);

                match message {
                    client_to_server::Message::Cap => {
                        // ignore for now
                    }
                    client_to_server::Message::Nick(nick) => {
                        let is_nickname_valid =
                            server_state.lock().unwrap().check_nickname(&nick, None);
                        match is_nickname_valid {
                            Ok(()) => user.nickname = Some(nick),
                            Err(err) => {
                                self.stream.write_all(b":srv ").await?;
                                err.write_to(&mut self.stream).await?;
                                self.stream.write_all(b"\r\n").await?;
                            }
                        }
                    }
                    client_to_server::Message::User(username) => user.username = Some(username),
                    client_to_server::Message::Quit(reason) => {
                        todo!();
                    }
                    client_to_server::Message::Unknown(command) => {
                        let message =
                            server_to_client::Message::Err(ServerStateError::UnknownCommand {
                                client: "*".to_string(),
                                command: command.to_owned(),
                            });
                        message.write_to(&mut self.stream).await?;
                    }
                    client_to_server::Message::PrivMsg(_, _) => {
                        // some valid commands should return ErrNotRegistered when not registered
                        let msg = server_to_client::Message::Err(
                            crate::server_state::ServerStateError::NotRegistered {
                                client: user.nickname.clone().unwrap_or("*".to_string()),
                            },
                        );
                        msg.write_to(&mut self.stream).await?;
                        //anyhow::bail!("illegal command during connection");
                    }
                    _ => {
                        // valid commands should not return replies
                    }
                };

                if user.is_ready() {
                    break 'outer;
                }
            }
        }

        let user = RegisteredUser::from(user);
        let session = Session {
            stream: self.stream,
            stream_parser: self.stream_parser,
            user_id: user.user_id,
            mailbox: rx,
            state: SessionState::Registered(SessionStateRegistered {
                user_id: user.user_id,
            }),
        };

        server_state.lock().unwrap().user_connects(user);

        Ok(session)
    }
}

pub(crate) struct Session {
    stream: AnyStream,
    stream_parser: StreamParser,
    user_id: UserID,

    // move this inner to run(), returned by process_buffer
    state: SessionState,

    mailbox: tokio::sync::mpsc::UnboundedReceiver<server_to_client::Message>,
}

impl Session {
    pub(crate) fn init(stream: AnyStream) -> Self {
        let (user, rx) = RegisteringUser::new();
        Self {
            stream,
            stream_parser: StreamParser::default(),
            user_id: user.user_id,
            state: SessionState::Registering(SessionStateRegistering { user }),
            mailbox: rx,
        }
    }

    pub(crate) async fn run(mut self, server_state: SharedServerState) -> anyhow::Result<()> {
        // first, process potential messages in the stream parser, leftovers from the ConnectingSession
        let mut client_quit = self
            .state
            .process_buffer(&mut server_state.lock().unwrap(), &mut self.stream_parser)?;

        while !client_quit {
            tokio::select! {
                result = self.stream.read_buf(&mut self.stream_parser) => {
                    let received = result?;

                    if received == 0 {
                        break;
                    }

                    client_quit = self.state.process_buffer(&mut server_state.lock().unwrap(), &mut self.stream_parser)?;
                },
                Some(msg) = self.mailbox.recv() => {
                    msg.write_to(&mut self.stream).await?;
                }
            }
        }

        if client_quit {
            // the client sent a QUIT, handle the disconnection gracefully
            // TODO: maybe tolerate a timeout to send the last messages and then force quit
            while let Ok(msg) = self.mailbox.try_recv() {
                msg.write_to(&mut self.stream).await?;
            }
        } else {
            // the connection was closed without notification
            server_state
                .lock()
                .unwrap()
                .user_disconnects_suddently(self.user_id);
        }

        Ok(())
    }
}
