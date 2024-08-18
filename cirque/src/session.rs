use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::server_state::{ServerStateError, SharedServerState};
use crate::transport::AnyStream;
use crate::types::{User, UserID};
use crate::{client_to_server, server_to_client, ServerState};
use cirque_parser::{LendingIterator, StreamParser};

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
    pub(crate) async fn connect_user(
        mut self,
        server_state: &SharedServerState,
    ) -> Result<(Session, User), anyhow::Error> {
        let mut chosen_nick = None;
        let mut chosen_user = None;

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
                        let client = chosen_nick.clone().unwrap_or("*".to_string());
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
                            Ok(()) => chosen_nick = Some(nick),
                            Err(err) => {
                                self.stream.write_all(b":srv ").await?;
                                err.write_to(&mut self.stream).await?;
                                self.stream.write_all(b"\r\n").await?;
                            }
                        }
                    }
                    client_to_server::Message::User(user) => chosen_user = Some(user),
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
                                client: chosen_nick.clone().unwrap_or("*".to_string()),
                            },
                        );
                        msg.write_to(&mut self.stream).await?;
                        //anyhow::bail!("illegal command during connection");
                    }
                    _ => {
                        // valid commands should not return replies
                    }
                };

                if chosen_user.is_some() && chosen_nick.is_some() {
                    break 'outer;
                }
            }
        }

        let user_id = UserID::generate();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let user = User {
            id: user_id,
            username: chosen_user.unwrap(),
            nickname: chosen_nick.unwrap(),
            mailbox: tx,
        };

        let session = Session {
            stream: self.stream,
            stream_parser: self.stream_parser,
            user_id,
            mailbox: rx,
        };

        Ok((session, user))
    }
}

pub(crate) struct Session {
    stream: AnyStream,
    stream_parser: StreamParser,
    user_id: UserID,
    mailbox: tokio::sync::mpsc::UnboundedReceiver<server_to_client::Message>,
}

impl Session {
    fn process_buffer(&mut self, server_state: &mut ServerState) -> anyhow::Result<bool> {
        let mut iter = self.stream_parser.consume_iter();
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
                client_to_server::Message::Topic(target, content) => {
                    if let Err(err) = server_state.user_topic(self.user_id, &target, &content) {
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

    pub(crate) async fn run(mut self, server_state: SharedServerState) -> anyhow::Result<()> {
        // first, process potential messages in the stream parser, leftovers from the ConnectingSession
        let mut client_quit = self.process_buffer(&mut server_state.lock().unwrap())?;

        while !client_quit {
            tokio::select! {
                result = self.stream.read_buf(&mut self.stream_parser) => {
                    let received = result?;

                    if received == 0 {
                        break;
                    }

                    client_quit = self.process_buffer(&mut server_state.lock().unwrap())?;
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
