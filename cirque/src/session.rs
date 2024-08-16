use cirque_parser::stream::{LendingIterator, StreamParser};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::server_state::SharedServerState;
use crate::transport::AnyStream;
use crate::types::{User, UserID};
use crate::{client_to_server, server_to_client};

pub(crate) struct ConnectingSession {
    stream: AnyStream,
}

impl ConnectingSession {
    pub(crate) fn new(stream: AnyStream) -> Self {
        Self { stream }
    }
}

impl ConnectingSession {
    pub(crate) async fn connect_user(mut self) -> Result<(Session, User), anyhow::Error> {
        let mut chosen_nick = None;
        let mut chosen_user = None;
        let mut ping_token = None;
        let mut sp = StreamParser::default();

        'outer: loop {
            let received = self.stream.read_buf(&mut sp).await?;
            anyhow::ensure!(received > 0, "stream ended");

            let mut iter = sp.consume_iter();
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
                    client_to_server::Message::Nick(nick) => chosen_nick = Some(nick),
                    client_to_server::Message::User(user) => chosen_user = Some(user),
                    client_to_server::Message::Pong(token) => {
                        if Some(token) == ping_token {
                            break 'outer;
                        }
                        break;
                    }
                    client_to_server::Message::Quit => {
                        todo!();
                    }
                    _ => {
                        anyhow::bail!("illegal command during connection");
                    }
                };
            }

            if chosen_user.is_some() && chosen_nick.is_some() {
                let token = b"something".to_vec();
                self.stream.write_all(b"PING :").await?;
                self.stream.write_all(&token).await?;
                self.stream.write_all(b"\r\n").await?;
                ping_token = Some(token);
            }
        }

        self.stream.write_all(b":srv 001 ").await?;
        self.stream
            .write_all(chosen_nick.as_ref().unwrap().as_bytes())
            .await?;
        self.stream.write_all(b" :welcome\r\n").await?;

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
            user_id,
            mailbox: rx,
        };

        Ok((session, user))
    }
}

pub(crate) struct Session {
    stream: AnyStream,
    user_id: UserID,
    mailbox: tokio::sync::mpsc::UnboundedReceiver<server_to_client::Message>,
}

impl Session {
    fn process_buffer(
        &mut self,
        sp: &mut StreamParser,
        server_state: &SharedServerState,
    ) -> anyhow::Result<bool> {
        let mut iter = sp.consume_iter();
        while let Some(message) = iter.next() {
            let message = match message {
                Ok(m) => m,
                Err(e) => anyhow::bail!(e.to_string()),
            };
            let message = client_to_server::Message::try_from(message)?;
            dbg!(&message);

            match message {
                client_to_server::Message::Unknown => {}
                client_to_server::Message::Join(channels) => {
                    for channel in channels {
                        server_state
                            .lock()
                            .unwrap()
                            .user_joins_channel(self.user_id, &channel);
                    }
                }
                client_to_server::Message::Part(channels, reason) => {
                    for channel in channels {
                        server_state.lock().unwrap().user_leaves_channel(
                            self.user_id,
                            &channel,
                            &reason,
                        );
                    }
                }
                client_to_server::Message::AskModeChannel(channel) => {
                    server_state
                        .lock()
                        .unwrap()
                        .user_asks_channel_mode(self.user_id, &channel);
                }
                client_to_server::Message::Ping(token) => {
                    server_state
                        .lock()
                        .unwrap()
                        .user_pings(self.user_id, &token);
                }
                client_to_server::Message::Pong(_) => {}
                client_to_server::Message::Quit => return Ok(true),
                client_to_server::Message::PrivMsg(target, content) => {
                    server_state.lock().unwrap().user_messages_target(
                        self.user_id,
                        &target,
                        &content,
                    );
                }
                client_to_server::Message::Topic(target, content) => {
                    let _result =
                        server_state
                            .lock()
                            .unwrap()
                            .user_topic(self.user_id, &target, &content);
                    if _result.is_err() {
                        server_state
                            .lock()
                            .unwrap()
                            .send_error(self.user_id, _result.err().unwrap())
                    }
                }
                _ => {
                    println!("illegal command from connected client");
                }
            };
        }
        Ok(false)
    }

    pub(crate) async fn run(mut self, server_state: SharedServerState) -> anyhow::Result<()> {
        let mut sp = StreamParser::default();
        loop {
            tokio::select! {
                result = self.stream.read_buf(&mut sp) => {
                    let received = result?;
                    anyhow::ensure!(received > 0, "stream ended");

                    let quit = self.process_buffer(&mut sp, &server_state)?;
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
