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
                let message = client_to_server::Message::try_from(message)?;
                dbg!(&message);

                match message {
                    client_to_server::Message::Cap => {
                        // ignore for now
                    }
                    client_to_server::Message::Nick(nick) => {
                        let is_nickname_valid =
                            server_state.lock().unwrap().check_nickname(&nick, None);
                        if is_nickname_valid.is_ok() {
                            chosen_nick = Some(nick)
                        } else {
                            self.stream.write_all(b":srv ").await?;
                            self.stream
                                .write_all(is_nickname_valid.err().unwrap().to_string().as_bytes())
                                .await?;
                            self.stream.write_all(b"\r\n").await?;
                        }
                    }
                    client_to_server::Message::User(user) => chosen_user = Some(user),
                    client_to_server::Message::Quit => {
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
                    _ => {
                        // valid commands should return ErrNotRegistered when not registered
                        let msg = server_to_client::Message::Err(
                            crate::server_state::ServerStateError::NotRegistered {
                                client: chosen_nick.unwrap_or("*".to_string()),
                            },
                        );
                        msg.write_to(&mut self.stream).await?;
                        anyhow::bail!("illegal command during connection");
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

        self.stream.write_all(b":srv 001 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream
            .write_all(b" :Welcome to the Internet Relay Network ")
            .await?;
        self.stream.write_all(user.fullspec().as_bytes()).await?;
        self.stream.write_all(b"\r\n").await?;

        self.stream.write_all(b":srv 002 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream
            .write_all(b" :Your host is 'srv', running cirque.\r\n")
            .await?;

        self.stream.write_all(b":srv 003 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream
            .write_all(b" :This server was created <datetime>.\r\n")
            .await?;

        self.stream.write_all(b":srv 004 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream.write_all(b" srv 0 + +\r\n").await?;

        self.stream.write_all(b":srv 251 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream
            .write_all(b" :There are N users and 0 invisible on 1 servers\r\n")
            .await?;

        self.stream.write_all(b":srv 252 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream.write_all(b" 0 :operator(s) online\r\n").await?;

        self.stream.write_all(b":srv 253 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream
            .write_all(b" 0 :unknown connection(s)\r\n")
            .await?;

        self.stream.write_all(b":srv 254 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream.write_all(b" 0 :channels formed\r\n").await?;

        self.stream.write_all(b":srv 255 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream
            .write_all(b" :I have 1 clients and 0 servers\r\n")
            .await?;

        self.stream.write_all(b":srv 422 ").await?;
        self.stream.write_all(user.nickname.as_bytes()).await?;
        self.stream.write_all(b" :MOTD File is missing\r\n").await?;

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
            let message = client_to_server::Message::try_from(message)?;
            dbg!(&message);

            match message {
                client_to_server::Message::Join(channels) => {
                    for channel in channels {
                        server_state.user_joins_channel(self.user_id, &channel);
                    }
                }
                client_to_server::Message::Part(channels, reason) => {
                    for channel in channels {
                        server_state.user_leaves_channel(self.user_id, &channel, &reason);
                    }
                }
                client_to_server::Message::AskModeChannel(channel) => {
                    server_state.user_asks_channel_mode(self.user_id, &channel);
                }
                client_to_server::Message::Ping(token) => {
                    server_state.user_pings(self.user_id, &token);
                }
                client_to_server::Message::Pong(_) => {}
                client_to_server::Message::Quit => return Ok(true),
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
        let mut quit = self.process_buffer(&mut server_state.lock().unwrap())?;

        while !quit {
            tokio::select! {
                result = self.stream.read_buf(&mut self.stream_parser) => {
                    let received = result?;
                    anyhow::ensure!(received > 0, "stream ended");

                    quit = self.process_buffer(&mut server_state.lock().unwrap())?;
                    if quit {
                        break;
                    }
                },
                Some(msg) = self.mailbox.recv() => {
                    msg.write_to(&mut self.stream).await?;
                }
            }
        }

        server_state.lock().unwrap().user_disconnects(self.user_id);

        Ok(())
    }
}
