use std::io::BufReader;
use std::sync::Arc;
use std::{collections::HashMap, fs::File};
mod config;
use anyhow::{anyhow, Ok};
use cirque_parser::Message;
use std::{path::PathBuf, str::FromStr};
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio_rustls::{rustls, TlsAcceptor};

type UserID = usize;
type ChannelID = String;

struct User {
    id: UserID,
    username: Vec<u8>,
    ip: String,
    mailbox: tokio::sync::mpsc::UnboundedSender<MessageToSend>,
}

struct ConnectingUser {
    cap: Option<Vec<String>>,
    nick: Option<String>,
    user: Option<String>,
}

struct Channel {
    name: String,
    users: Vec<UserID>,
}

impl Channel {}

struct ServerState {
    users: HashMap<UserID, User>,
    connecting_users: Vec<ConnectingUser>,
    channels: HashMap<ChannelID, Channel>,
}

struct ConnectingSession {
    stream: tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
}

enum IRCMessage {
    PRIVMSG(String, String),
    PING,
    PONG,
}

impl TryFrom<cirque_parser::Message<'_>> for IRCMessage {
    type Error = anyhow::Error;

    fn try_from(value: cirque_parser::Message) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl ConnectingSession {
    pub async fn deal_with_connection(&mut self) -> Result<Session, anyhow::Error> {
        let chosen_nick: Option<String> = None;
        let chosen_user: Option<String> = None;
        let mut buffer = Vec::with_capacity(512);
        loop {
            let _size = self.stream.read_buf(&mut buffer).await?;

            let (buffer, message) = cirque_parser::parse_message(&buffer)?;
            let message = message.try_into()?;

            match message {
                IRCMessage::PRIVMSG(_, _) => todo!(),
                IRCMessage::PING => todo!(),
                IRCMessage::PONG => todo!(),
                _ => anyhow::bail!("illegal command during connection"),
            }

            if message.command() == b"MESSAGE" {
                channel = state.get_channel(command.args[0]);
                channel.send_message_to_everyone(command.args[1..]);
            }
        }

        Ok(Session::new())
    }
}

struct Session {
    buffer: Vec<u8>,
    mailbox: tokio::sync::mpsc::UnboundedReceiver<MessageToSend>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(512),
        }
    }

    pub async fn read_buf(
        &mut self,
        stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    ) -> Result<(), anyhow::Error> {
        let _size = stream.read_buf(&mut self.buffer).await?;

        if command == "MESSAGE" {
            channel = state.get_channel(command.args[0]);
            channel.send_message_to_everyone(command.args[1..]);
        }

        let user: User;
        user.mailbox.send(message_to_send);

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dbg!("hello world");
    let config_path = PathBuf::from_str("ircd.yml")?;
    let c = config::Config::load_from_path(&config_path)?;

    let mut certs = None;
    if let Some(cert_file_path) = c.cert_file_path {
        certs = Some(
            rustls_pemfile::certs(&mut BufReader::new(&mut File::open(cert_file_path)?))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    let mut private_key = None;
    if let Some(private_key_file_path) = c.private_key_file_path {
        private_key = rustls_pemfile::private_key(&mut BufReader::new(&mut File::open(
            private_key_file_path,
        )?))?;
    }

    if certs.is_some() && private_key.is_some() {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs.unwrap(), private_key.unwrap())?;

        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind(format!("[::]:{}", 6697)).await?;
        loop {
            let (stream, _peer_addr) = listener.accept().await?;
            let acceptor = acceptor.clone();

            let mut session = Session::new();

            let fut = async move {
                let mut stream = acceptor.accept(stream).await?;

                let session = ConnectingSession { stream };
                let session = session.deal_with_connection();

                loop {
                    session.read_buf(&mut stream).await?;
                }
                Ok(())
            };

            tokio::spawn(async move {
                if let Err(err) = fut.await {
                    eprintln!("{:?}", err);
                }
            });
        }
    } else {
        Err(anyhow!("Config incomplete"))
    }
}
