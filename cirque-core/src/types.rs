use std::collections::HashMap;

use crate::{
    error::ServerStateError,
    message_writer::{Mailbox, MailboxSink},
    server_to_client::{self, MessageContext},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserID(uuid::Uuid);

impl UserID {
    pub(crate) fn generate() -> Self {
        UserID(uuid::Uuid::new_v4())
    }
}

#[derive(Debug)]
pub struct RegisteredUser {
    pub(crate) user_id: UserID,
    pub(crate) nickname: String,
    pub(crate) username: String,
    pub(crate) realname: Vec<u8>,
    pub(crate) away_message: Option<Vec<u8>>,
    fullspec: String,
    hostname: &'static str,
    mailbox: Mailbox,
}

impl RegisteredUser {
    pub(crate) fn send(&self, message: &server_to_client::Message<'_>, context: &MessageContext) {
        self.mailbox.ingest(message, context);
    }

    pub(crate) fn shown_hostname(&self) -> &str {
        self.hostname
    }

    pub(crate) fn fullspec(&self) -> &str {
        &self.fullspec
    }

    pub fn is_away(&self) -> bool {
        self.away_message.is_some()
    }

    pub(crate) fn change_nickname(&mut self, new_nick: &str) {
        self.nickname = new_nick.to_string();
        self.fullspec = format!("{}!{}@{}", self.nickname, self.username, self.hostname);
    }
}

#[derive(Debug)]
pub(crate) struct RegisteringUser {
    pub(crate) user_id: UserID,
    pub(crate) nickname: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) realname: Option<Vec<u8>>,
    pub(crate) password: Option<Vec<u8>>,
    mailbox: Mailbox,
}

impl RegisteringUser {
    pub(crate) fn new(mailbox_capacity: usize) -> (Self, MailboxSink) {
        let user_id = UserID::generate();
        let (mailbox, mailbox_sink) = Mailbox::new(mailbox_capacity);
        let user = Self {
            user_id,
            nickname: None,
            username: None,
            realname: None,
            password: None,
            mailbox,
        };
        (user, mailbox_sink)
    }

    pub(crate) fn send(&self, message: &server_to_client::Message<'_>, context: &MessageContext) {
        self.mailbox.ingest(message, context);
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
        // we assert that the registration is valid, so the unwraps are fine
        assert!(value.is_ready());

        #[allow(clippy::unwrap_used)]
        let nickname = value.nickname.unwrap();
        #[allow(clippy::unwrap_used)]
        let username = value.username.unwrap();
        let hostname = "hidden";

        let fullspec = format!("{}!{}@{}", nickname, username, hostname);

        Self {
            user_id: value.user_id,
            nickname,
            username,
            realname: value.realname.unwrap_or_default(),
            away_message: None,
            fullspec,
            hostname,
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChannelMode {
    secret: bool,
    topic_protected: bool,
    moderated: bool,
    no_external: bool,
}

impl Default for ChannelMode {
    fn default() -> Self {
        Self {
            secret: Default::default(),
            topic_protected: Default::default(),
            moderated: Default::default(),
            no_external: true,
        }
    }
}

impl TryFrom<&str> for ChannelMode {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.chars().try_fold(Self::default(), |mode, c| match c {
            's' => Ok(mode.with_secret()),
            't' => Ok(mode.with_topic_protected()),
            'm' => Ok(mode.with_moderated()),
            'n' => Ok(mode.with_no_external()),
            c => Err(format!("unknown channel modechar '{c}'")),
        })
    }
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

        let can_talk = {
            if !self.mode.is_moderated() {
                true
            } else if let Some(user_mode) = user_mode {
                user_mode.is_op() || user_mode.is_voice()
            } else {
                false
            }
        };
        if !can_talk {
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
