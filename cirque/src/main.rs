use cirque_parser::stream::{LendingIterator, StreamParser};
use std::collections::HashSet;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, fs::File};
use std::{path::PathBuf, str::FromStr};
use tokio::io::AsyncWriteExt;
use transport::{TCPListener, TLSListener};

mod client_to_server;
mod config;
mod server_state;
mod server_to_client;
mod transport;
use crate::server_state::ServerState;
use crate::transport::AnyStream;
use crate::transport::Listener;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct UserID(uuid::Uuid);

impl UserID {
    fn generate() -> Self {
        UserID(uuid::Uuid::new_v4())
    }
}

type ChannelID = String;

#[derive(Debug)]
struct User {
    id: UserID,
    nickname: String,
    username: String,
    mailbox: tokio::sync::mpsc::UnboundedSender<server_to_client::Message>,
}

impl User {
    fn send(&self, message: &server_to_client::Message) {
        let _ = self.mailbox.send(message.clone());
    }

    fn fullspec(&self) -> String {
        format!("{}!{}@hidden", self.nickname, self.username)
    }
}

#[derive(Debug)]
struct ConnectingUser {
    cap: Option<Vec<String>>,
    nick: Option<String>,
    user: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct Topic {
    pub content: Vec<u8>,
    pub ts: u64,
    pub from_nickname: String,
}

impl Topic {
    pub fn is_valid(&self) -> bool {
        !self.content.is_empty() && self.ts > 0
    }
}

#[derive(Debug, Default)]
struct Channel {
    topic: Topic,
    users: HashSet<UserID>,
}

type SharedServerState = Arc<Mutex<ServerState>>;
struct ConnectingSession {
    stream: AnyStream,
}

impl ConnectingSession {
    pub async fn connect_user(mut self) -> Result<(Session, User), anyhow::Error> {
        let mut chosen_nick = None;
        let mut chosen_user = None;
        let mut ping_token = None;
        let mut sp = StreamParser::default();

        'outer: loop {
            sp.feed_from_stream(&mut self.stream).await?;

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

struct Session {
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
                    server_state
                        .lock()
                        .unwrap()
                        .user_topic(self.user_id, &target, &content);
                }
                _ => {
                    println!("illegal command from connected client");
                }
            };
        }
        Ok(false)
    }

    pub async fn run(mut self, server_state: SharedServerState) -> anyhow::Result<()> {
        let mut sp = StreamParser::default();
        loop {
            tokio::select! {
                _ = sp.feed_from_stream(&mut self.stream) => {
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

async fn run_server(
    listener: impl Listener,
    server_state: SharedServerState,
) -> anyhow::Result<()> {
    loop {
        let stream = listener.accept().await?;
        let stream = stream.with_debug();

        let server_state = server_state.clone();
        let fut = async move {
            let session = ConnectingSession { stream };
            let (session, user) = session.connect_user().await?;
            server_state.lock().unwrap().add_user(user);
            session.run(server_state).await?;
            anyhow::Ok(())
        };

        tokio::spawn(async move {
            if let Err(err) = fut.await {
                eprintln!("{:?}", err);
            }
        });
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let server_state = Arc::new(Mutex::new(ServerState::new()));

    let config_path = PathBuf::from_str("ircd.yml")?;
    if let Ok(config) = config::Config::load_from_path(&config_path) {
        let mut certs = None;
        if let Some(cert_file_path) = config.cert_file_path {
            certs = Some(
                rustls_pemfile::certs(&mut BufReader::new(&mut File::open(cert_file_path)?))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        let mut private_key = None;
        if let Some(private_key_file_path) = config.private_key_file_path {
            private_key = rustls_pemfile::private_key(&mut BufReader::new(&mut File::open(
                private_key_file_path,
            )?))?;
        }

        if certs.is_some() && private_key.is_some() {
            let listener = TLSListener::try_new(certs.unwrap(), private_key.unwrap()).await?;
            run_server(listener, server_state).await
        } else {
            anyhow::bail!("Config incomplete");
        }
    } else {
        dbg!("listening without TLS on 6667");
        let listener = TCPListener::try_new(6667).await?;
        run_server(listener, server_state).await
    }
}
