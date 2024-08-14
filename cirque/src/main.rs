use std::collections::HashSet;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, fs::File};
mod config;
use cirque_parser::stream::StreamParser;
use std::{path::PathBuf, str::FromStr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use transport::{TCPListener, TLSListener};

mod client_to_server;
mod server_to_client;
mod transport;

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

#[derive(Debug)]
struct Channel {
    name: String,
    topic: Vec<u8>,
    users: HashSet<UserID>,
}

impl Channel {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            topic: vec![],
            users: Default::default(),
        }
    }
}

impl Channel {}

enum LookupResult<'r> {
    Channel(&'r Channel),
    User(&'r User),
}

type SharedServerState = Arc<Mutex<ServerState>>;

#[derive(Debug)]
struct ServerState {
    users: HashMap<UserID, User>,
    connecting_users: Vec<ConnectingUser>,
    channels: HashMap<ChannelID, Channel>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            users: Default::default(),
            connecting_users: vec![],
            channels: Default::default(),
        }
    }

    fn user_joins_channel(&mut self, user_id: UserID, channel_name: &str) {
        let channel = self
            .channels
            .entry(channel_name.to_owned())
            .or_insert_with(|| Channel::new(channel_name));

        if channel.users.contains(&user_id) {
            return;
        }

        channel.users.insert(user_id);

        // notify everyone, including the joiner
        let mut nicknames = vec![];
        let joiner_spec = self.users[&user_id].fullspec();
        let message = server_to_client::Message::Join(server_to_client::JoinMessage {
            channel: channel_name.to_owned(),
            user_fullspec: joiner_spec,
        });
        for user_id in &channel.users {
            let user = &self.users[user_id];
            nicknames.push(user.nickname.clone());
            user.send(&message);
        }

        // send topic and names to the joiner
        let user = &self.users[&user_id];
        let message = server_to_client::Message::Topic(server_to_client::TopicMessage {
            nickname: user.nickname.to_owned(),
            channel: channel_name.to_owned(),
            topic: if !channel.topic.is_empty() {
                Some(channel.topic.clone())
            } else {
                None
            },
        });
        user.send(&message);
        let message = server_to_client::Message::Names(server_to_client::NamesMessage {
            nickname: user.nickname.clone(),
            names: vec![(channel_name.to_owned(), nicknames)],
        });
        user.send(&message);
    }

    fn user_leaves_channel(&mut self, user_id: UserID, channel: &str) {}

    fn user_disconnects(&mut self, user_id: UserID) {}

    fn lookup_target<'r>(&'r self, target: &str) -> Option<LookupResult<'r>> {
        if let Some(channel) = self.channels.get(target) {
            Some(LookupResult::Channel(channel))
        } else if let Some(user) = self.users.values().find(|&u| u.nickname == target) {
            Some(LookupResult::User(user))
        } else {
            None
        }
    }

    fn user_messages_target(&mut self, user_id: UserID, target: &str, content: &[u8]) {
        let Some(obj) = self.lookup_target(target) else {
            // TODO: ERR_NOSUCHNICK
            return;
        };

        let user = &self.users[&user_id];

        let message = server_to_client::Message::PrivMsg(server_to_client::PrivMsgMessage {
            from_user: user.fullspec(),
            target: target.to_string(),
            content: content.to_vec(),
        });

        match obj {
            LookupResult::Channel(channel) => {
                if !channel.users.contains(&user_id) {
                    // TODO: ERR_CANNOTSENDTOCHAN
                    return;
                }

                channel
                    .users
                    .iter()
                    .filter(|&uid| *uid != user_id)
                    .flat_map(|u| self.users.get(u))
                    .for_each(|u| u.send(&message));
            }
            LookupResult::User(target_user) => {
                target_user.send(&message);
            }
        }
    }

    fn user_messages_to_user(&mut self, user_id: UserID, dst_user_id: UserID, msg: &[u8]) {}

    fn user_asks_channel_mode(&mut self, user_id: UserID, channel: &str) {
        let user = &self.users[&user_id];
        let message =
            server_to_client::Message::ChannelMode(server_to_client::ChannelModeMessage {
                nickname: user.nickname.clone(),
                channel: channel.to_owned(),
                mode: "+n".to_string(),
            });
        user.send(&message);
    }
}

struct ConnectingSession {
    stream: AnyStream,
}

impl ConnectingSession {
    pub async fn connect_user(mut self) -> Result<(Session, User), anyhow::Error> {
        let mut chosen_nick = None;
        let mut chosen_user = None;
        let mut ping_token = None;
        let mut sp = StreamParser::default();
        let mut buffer = Vec::with_capacity(512);

        'outer: loop {
            buffer.clear();
            self.stream.read_buf(&mut buffer).await?;

            anyhow::ensure!(!buffer.is_empty(), "stream ended");

            for message in sp.try_read(&buffer) {
                let message = message?;
                let message = message.try_into()?;
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

        self.stream.write_all(b"001 ").await?;
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
    async fn process_buffer(
        &mut self,
        sp: &mut StreamParser,
        server_state: &SharedServerState,
        buffer: &[u8],
    ) -> anyhow::Result<bool> {
        for message in sp.try_read(buffer) {
            let message = message?;
            let message = message.try_into()?;
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
                client_to_server::Message::AskModeChannel(channel) => {
                    server_state
                        .lock()
                        .unwrap()
                        .user_asks_channel_mode(self.user_id, &channel);
                }
                client_to_server::Message::Ping(token) => {
                    let msg =
                        server_to_client::Message::Pong(server_to_client::PongMessage { token });
                    msg.write_to(&mut self.stream).await?;
                }
                client_to_server::Message::Quit => return Ok(true),
                client_to_server::Message::PrivMsg(target, content) => {
                    server_state.lock().unwrap().user_messages_target(
                        self.user_id,
                        &target,
                        &content,
                    );
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
        let mut buffer = Vec::with_capacity(512);
        loop {
            buffer.clear();
            tokio::select! {
                result = self.stream.read_buf(&mut buffer) => {
                    let _ = result?;
                    anyhow::ensure!(!buffer.is_empty(), "stream ended");
                    let quit = self.process_buffer(&mut sp, &server_state, &buffer).await?;
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
            server_state.lock().unwrap().users.insert(user.id, user);
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
