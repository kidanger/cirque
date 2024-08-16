use std::collections::HashSet;

use crate::server_to_client;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserID(uuid::Uuid);

impl UserID {
    pub(crate) fn generate() -> Self {
        UserID(uuid::Uuid::new_v4())
    }
}

pub type ChannelID = String;

#[derive(Debug)]
pub struct User {
    pub(crate) id: UserID,
    pub(crate) nickname: String,
    pub(crate) username: String,
    pub(crate) mailbox: tokio::sync::mpsc::UnboundedSender<server_to_client::Message>,
}

impl User {
    pub(crate) fn send(&self, message: &server_to_client::Message) {
        let _ = self.mailbox.send(message.clone());
    }

    pub(crate) fn fullspec(&self) -> String {
        format!("{}!{}@hidden", self.nickname, self.username)
    }
}

#[derive(Debug)]
pub(crate) struct ConnectingUser {
    cap: Option<Vec<String>>,
    nick: Option<String>,
    user: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct Topic {
    pub content: Vec<u8>,
    pub ts: u64,
    pub from_nickname: String,
}

impl Topic {
    pub(crate) fn is_valid(&self) -> bool {
        !self.content.is_empty() && self.ts > 0
    }
}

#[derive(Debug, Default)]
pub struct Channel {
    pub(crate) topic: Topic,
    pub(crate) users: HashSet<UserID>,
}
