use std::collections::HashMap;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    server_state::ServerStateError,
    server_to_client::{self, MessageContext},
};

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
    pub(crate) realname: Vec<u8>,
    pub(crate) away_message: Option<Vec<u8>>,
    // maybe this shouldn't be in User
    mailbox: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl RegisteredUser {
    pub(crate) fn send(&self, message: &server_to_client::Message, context: &MessageContext) {
        crate::message_pool::MessagePool::ingest_into_channel(message, &self.mailbox, context);
    }

    pub(crate) fn shown_hostname(&self) -> &str {
        "hidden"
    }

    pub(crate) fn fullspec(&self) -> String {
        format!(
            "{}!{}@{}",
            self.nickname,
            self.username,
            self.shown_hostname()
        )
    }

    pub fn is_away(&self) -> bool {
        self.away_message.is_some()
    }
}

#[derive(Debug)]
pub(crate) struct RegisteringUser {
    pub(crate) user_id: UserID,
    pub(crate) nickname: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) realname: Option<Vec<u8>>,
    pub(crate) password: Option<Vec<u8>>,
    mailbox: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl RegisteringUser {
    pub(crate) fn new() -> (Self, UnboundedReceiver<Vec<u8>>) {
        let user_id = UserID::generate();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let user = Self {
            user_id,
            nickname: None,
            username: None,
            realname: None,
            password: None,
            mailbox: tx,
        };
        (user, rx)
    }

    pub(crate) fn send(&self, message: &server_to_client::Message, context: &MessageContext) {
        crate::message_pool::MessagePool::ingest_into_channel(message, &self.mailbox, context);
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
            realname: value.realname.unwrap_or_default(),
            away_message: None,
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

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct ChannelUserMode {
    op: bool,
    voice: bool,
}

impl ChannelUserMode {
    pub(crate) fn with_op(&self) -> Self {
        Self {
            op: true,
            ..self.clone()
        }
    }

    pub(crate) fn without_op(&self) -> Self {
        Self {
            op: false,
            ..self.clone()
        }
    }

    pub(crate) fn with_voice(&self) -> Self {
        Self {
            voice: true,
            ..self.clone()
        }
    }

    pub(crate) fn without_voice(&self) -> Self {
        Self {
            voice: true,
            ..self.clone()
        }
    }

    pub fn is_op(&self) -> bool {
        self.op
    }

    pub(crate) fn is_voice(&self) -> bool {
        self.voice
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct ChannelMode {
    secret: bool,
    topic_protected: bool,
    moderated: bool,
    no_external: bool,
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

    pub fn is_moderated(&self) -> bool {
        self.moderated
    }

    pub(crate) fn with_moderated(&self) -> Self {
        Self {
            moderated: true,
            ..self.clone()
        }
    }

    pub(crate) fn without_moderated(&self) -> Self {
        Self {
            moderated: false,
            ..self.clone()
        }
    }

    pub(crate) fn is_no_external(&self) -> bool {
        self.no_external
    }

    pub(crate) fn with_no_external(&self) -> Self {
        Self {
            no_external: true,
            ..self.clone()
        }
    }

    pub(crate) fn without_no_external(&self) -> Self {
        Self {
            no_external: false,
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

    pub(crate) fn ensure_user_can_send_message(
        &self,
        user: &RegisteredUser,
        channel_name: &str,
    ) -> Result<(), ServerStateError> {
        let user_id = &user.user_id;

        let user_mode = self.users.get(user_id);
        let is_in_channel = user_mode.is_some();

        if self.mode.is_no_external() && !is_in_channel {
            return Err(ServerStateError::CannotSendToChan {
                client: user.nickname.clone(),
                channel: channel_name.into(),
            });
        }

        let user_mode = user_mode.cloned().unwrap_or_default();
        if self.mode.is_moderated() && !(user_mode.is_op() || user_mode.is_voice()) {
            return Err(ServerStateError::CannotSendToChan {
                client: user.nickname.to_string(),
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
