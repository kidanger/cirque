use std::io::BufReader;
use std::{collections::HashMap, fs::File};
mod config;
use std::{path::PathBuf, str::FromStr};
use tokio::io::AsyncReadExt;
use transport::{TCPListener, TLSListener};

mod transport;

use crate::transport::AnyStream;
use crate::transport::Listener;

type UserID = usize;
type ChannelID = String;

struct MessageToSend {}

#[derive(Debug)]
struct User {
    id: UserID,
    username: Vec<u8>,
    ip: String,
    mailbox: tokio::sync::mpsc::UnboundedSender<MessageToSend>,
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
    users: Vec<UserID>,
}

impl Channel {}

#[derive(Debug)]
struct ServerState {
    users: HashMap<UserID, User>,
    connecting_users: Vec<ConnectingUser>,
    channels: HashMap<ChannelID, Channel>,
}

#[derive(Debug)]
enum IRCMessage {
    CAP,
    NICK(Vec<u8>),
    USER(Vec<u8>),
    PING,
    PONG,
    UNKNOWN,
}

impl TryFrom<cirque_parser::Message> for IRCMessage {
    type Error = anyhow::Error;

    fn try_from(value: cirque_parser::Message) -> Result<Self, Self::Error> {
        let message = match value.command().as_slice() {
            b"CAP" => IRCMessage::CAP,
            b"NICK" => IRCMessage::NICK(value.parameters()[0].clone()),
            b"USER" => IRCMessage::USER(value.parameters()[0].clone()),
            _ => todo!(),
        };
        Ok(message)
    }
}

struct ConnectingSession {
    stream: AnyStream,
}

impl ConnectingSession {
    pub async fn connect_user(mut self) -> Result<(Session, User), anyhow::Error> {
        let mut chosen_nick = None;
        let mut chosen_user = None;

        let mut buffer = Vec::with_capacity(512);
        loop {
            buffer.clear();
            self.stream.read_buf(&mut buffer).await?;

            anyhow::ensure!(!buffer.is_empty(), "stream ended");
            dbg!("recv: {}", String::from_utf8_lossy(&buffer));

            let message = cirque_parser::parse_one_message(buffer.clone());
            let Some(message) = message else {
                dbg!("cannot parse message");
                continue;
            };

            dbg!(&message);
            let message = message.try_into()?;
            dbg!(&message);

            match message {
                IRCMessage::CAP => {
                    // ignore for now
                }
                IRCMessage::NICK(nick) => chosen_nick = Some(nick),
                IRCMessage::USER(user) => chosen_user = Some(user),
                _ => {
                    anyhow::bail!("illegal command during connection");
                }
            };

            if chosen_user.is_some() && chosen_nick.is_some() {
                break;
            }
        }

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let session = Session {
            stream: self.stream,
            mailbox: rx,
        };
        let user = User {
            id: 0,
            username: chosen_user.unwrap(),
            ip: "0.0.0.0".to_string(),
            mailbox: tx,
        };

        Ok((session, user))
    }
}

struct Session {
    stream: AnyStream,
    mailbox: tokio::sync::mpsc::UnboundedReceiver<MessageToSend>,
}

impl Session {
    pub async fn run(mut self) -> Result<(), anyhow::Error> {
        // TODO: handle mailbox
        let mut buffer = Vec::with_capacity(512);
        loop {
            buffer.clear();
            self.stream.read_buf(&mut buffer).await?;
            dbg!("recv: {}", &buffer);

            anyhow::ensure!(!buffer.is_empty(), "stream ended");

            let message = cirque_parser::parse_one_message(buffer.clone());
            let Some(message) = message else {
                dbg!("cannot parse message");
                continue;
            };

            dbg!(&message);
            let message = message.try_into()?;
            dbg!(&message);

            match message {
                IRCMessage::PING => {
                    todo!();
                }
                IRCMessage::PONG => {
                    break;
                }
                _ => {
                    anyhow::bail!("illegal command during connection");
                }
            };
        }

        Ok(())
    }
}

async fn run_server(listener: impl Listener) -> anyhow::Result<()> {
    loop {
        let stream = listener.accept().await?;

        let fut = async move {
            let session = ConnectingSession { stream };
            let (session, user) = session.connect_user().await?;
            session.run().await?;
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
            run_server(listener).await
        } else {
            anyhow::bail!("Config incomplete");
        }
    } else {
        dbg!("listening wihtout TLS on 6667");
        let listener = TCPListener::try_new(6667).await?;
        run_server(listener).await
    }
}
