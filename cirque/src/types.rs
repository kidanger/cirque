use std::collections::HashMap;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{server_state::ServerStateError, server_to_client};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserID(uuid::Uuid);

impl UserID {
    pub(crate) fn generate() -> Self {
        UserID(uuid::Uuid::new_v4())
    }
}

pub type ChannelID = String;

#[derive(Debug)]
pub struct RegisteredUser {
    pub(crate) user_id: UserID,
    pub(crate) nickname: String,
    pub(crate) username: String,
    mailbox: tokio::sync::mpsc::UnboundedSender<server_to_client::Message>,
}

impl RegisteredUser {
    pub(crate) fn send(&self, message: &server_to_client::Message) {
        let _ = self.mailbox.send(message.clone());
    }

    pub(crate) fn fullspec(&self) -> String {
        format!("{}!{}@hidden", self.nickname, self.username)
    }
}

#[derive(Debug)]
pub(crate) struct RegisteringUser {
    pub(crate) user_id: UserID,
    pub(crate) nickname: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<Vec<u8>>,
    mailbox: UnboundedSender<server_to_client::Message>,
}

impl RegisteringUser {
    pub(crate) fn new() -> (Self, UnboundedReceiver<server_to_client::Message>) {
        let user_id = UserID::generate();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let user = Self {
            user_id,
            nickname: None,
            username: None,
            password: None,
            mailbox: tx,
        };
        (user, rx)
    }

    pub(crate) fn send(&self, message: &server_to_client::Message) {
        let _ = self.mailbox.send(message.clone());
    }

    pub(crate) fn maybe_nickname(&self) -> String {
        self.nickname.clone().unwrap_or("*".to_string())
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.nickname.is_some() && self.username.is_some()
    }
}

impl From<RegisteringUser> for RegisteredUser {
    fn from(value: RegisteringUser) -> Self {
        assert!(value.is_ready());
        Self {
            user_id: value.user_id,
            nickname: value.nickname.unwrap(),
            username: value.username.unwrap(),
            mailbox: value.mailbox,
        }
    }
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

#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelUserMode {
    // it's not clear yet if a user can be both thing at once
    op: bool,
    voice: bool,
}

impl ChannelUserMode {
    pub(crate) fn new_op() -> Self {
        Self {
            op: true,
            ..Default::default()
        }
    }

    pub(crate) fn new_voice() -> Self {
        Self {
            voice: true,
            ..Default::default()
        }
    }

    pub fn is_op(&self) -> bool {
        self.op
    }

    pub(crate) fn is_voice(&self) -> bool {
        self.voice
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ChannelMode {
    secret: bool,
    topic_protected: bool,
}

impl ChannelMode {
    pub fn is_secret(&self) -> bool {
        self.secret
    }

    pub(crate) fn with_secret(&self) -> Self {
        Self {
            secret: true,
            ..self.clone()
        }
    }

    pub(crate) fn without_secret(&self) -> Self {
        Self {
            secret: false,
            ..self.clone()
        }
    }

    pub fn is_topic_protected(&self) -> bool {
        self.topic_protected
    }

    pub(crate) fn with_topic_protected(&self) -> Self {
        Self {
            topic_protected: true,
            ..self.clone()
        }
    }

    pub(crate) fn without_topic_protected(&self) -> Self {
        Self {
            topic_protected: false,
            ..self.clone()
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Channel {
    pub(crate) topic: Topic,
    pub(crate) users: HashMap<UserID, ChannelUserMode>,
    pub(crate) mode: ChannelMode,
}
impl Channel {
    pub(crate) fn ensure_user_can_set_topic(
        &self,
        user: &RegisteredUser,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user_id = &user.user_id;

        let user_mode = self
            .users
            .get(user_id)
            .ok_or_else(|| ServerStateError::NotOnChannel {
                client: user.nickname.clone(),
                channel: channel_name.into(),
            })?;

        if !user_mode.is_op() && self.mode.is_topic_protected() {
            return Err(ServerStateError::ChanOpPrivsNeeded {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        }

        Ok(())
    }

    pub(crate) fn ensure_user_can_set_channel_mode(
        &self,
        user: &RegisteredUser,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user_id = &user.user_id;

        let user_mode = self
            .users
            .get(user_id)
            .ok_or_else(|| ServerStateError::NotOnChannel {
                client: user.nickname.clone(),
                channel: channel_name.into(),
            })?;

        if !user_mode.is_op() {
            return Err(ServerStateError::ChanOpPrivsNeeded {
                client: user.nickname.clone(),
                channel: channel_name.to_string(),
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct WelcomeConfig {
    pub send_isupport: bool,
}

impl Default for WelcomeConfig {
    fn default() -> Self {
        Self {
            send_isupport: true,
        }
    }
}
