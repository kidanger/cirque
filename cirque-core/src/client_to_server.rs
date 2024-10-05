use unicase::UniCase;

#[derive(Debug, Default, PartialEq)]
pub(crate) enum ListFilter {
    ChannelCreation,
    TopicUpdate,
    #[default]
    UserNumber,
}

#[derive(Debug, Default, PartialEq)]
pub(crate) enum ListOperation {
    #[default]
    Inf,
    Sup,
}

#[derive(Debug, Default)]
pub(crate) struct ListOption {
    pub filter: ListFilter,
    pub operation: ListOperation,
    pub number: u64,
}

#[derive(Debug)]
pub(crate) enum Message<'m> {
    Nick(&'m str),
    User(&'m str, &'m [u8]),
    Pass(&'m [u8]),
    Ping(&'m [u8]),
    Pong(&'m [u8]),
    Join(Vec<&'m str>),
    Names(Vec<&'m str>),
    GetTopic(&'m str),
    SetTopic(&'m str, &'m [u8]),
    AskModeChannel(&'m str),
    ChangeModeChannel(&'m str, &'m str, Option<&'m str>),
    PrivMsg(&'m str, &'m [u8]),
    Notice(&'m str, &'m [u8]),
    Part(Vec<&'m str>, Option<&'m [u8]>),
    List(Option<Vec<String>>, Option<Vec<ListOption>>),
    #[allow(clippy::upper_case_acronyms)]
    MOTD(),
    Away(Option<&'m [u8]>),
    Userhost(Vec<&'m str>),
    Whois(&'m str),
    Who(&'m str),
    Lusers(),
    Quit(Option<&'m [u8]>),
    Unknown(&'m str),
}

pub(crate) enum MessageDecodingError<'m> {
    CannotDecodeUtf8 { command: &'m [u8] },
    NotEnoughParameters { command: &'m str },
    CannotParseInteger { command: &'m [u8] },
    NoNicknameGiven {},
    NoTextToSend {},
    NoRecipient { command: &'m str },
    SilentError {},
}

fn str2<'a, 'm>(command: &'m str, s: &'a [u8]) -> Result<&'a str, MessageDecodingError<'m>> {
    std::str::from_utf8(s).map_err(|_| MessageDecodingError::CannotDecodeUtf8 {
        command: command.as_bytes(),
    })
}

fn opt2<'m, 'a: 'm>(
    command: &'a str,
    opt: Option<&'a [u8]>,
) -> Result<&'a [u8], MessageDecodingError<'m>> {
    opt.ok_or(MessageDecodingError::NotEnoughParameters { command })
}

fn optstr<'m, 'a: 'm>(
    command: &'a str,
    opt: Option<&'a [u8]>,
) -> Result<&'a str, MessageDecodingError<'m>> {
    let opt = opt
        .map(|s| std::str::from_utf8(s))
        .transpose()
        .map_err(|_| MessageDecodingError::CannotDecodeUtf8 {
            command: command.as_bytes(),
        })?;
    opt.ok_or(MessageDecodingError::NotEnoughParameters { command })
}

fn handle_user<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let user = optstr(command, message.first_parameter())?;
    let params = message.parameters();
    let Some(realname) = params.get(3) else {
        return Err(MessageDecodingError::NotEnoughParameters { command });
    };
    if user.is_empty() || realname.is_empty() {
        return Err(MessageDecodingError::NotEnoughParameters { command });
    }
    Ok(Message::User(user, realname))
}

fn handle_nick<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let nick = message
        .first_parameter()
        .ok_or(MessageDecodingError::NoNicknameGiven {})?;
    let nick = str2(command, nick)?;
    if nick.is_empty() {
        return Err(MessageDecodingError::NoNicknameGiven {});
    }
    Ok(Message::Nick(nick))
}

fn handle_pass<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let pass = message
        .first_parameter()
        .ok_or(MessageDecodingError::NotEnoughParameters { command })?;
    Ok(Message::Pass(pass))
}

fn handle_ping<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    Ok(Message::Ping(opt2(command, message.first_parameter())?))
}

fn handle_pong<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    Ok(Message::Pong(opt2(command, message.first_parameter())?))
}

fn handle_join<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let channels = message
        .first_parameter()
        .ok_or(MessageDecodingError::NotEnoughParameters { command })?
        .split(|&c| c == b',')
        .flat_map(|s| str2(command, s))
        .collect::<Vec<_>>();
    Ok(Message::Join(channels))
}

fn handle_names<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let channels = message
        .first_parameter()
        .ok_or(MessageDecodingError::NotEnoughParameters { command })?
        .split(|&c| c == b',')
        .flat_map(|s| str2(command, s))
        .collect::<Vec<_>>();
    Ok(Message::Names(channels))
}

fn handle_topic<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let target = optstr(command, message.first_parameter())?;
    let params = message.parameters();
    let msg = match params.get(1) {
        Some(content) => Message::SetTopic(target, content),
        None => Message::GetTopic(target),
    };
    Ok(msg)
}

fn handle_mode<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let target = optstr(command, message.first_parameter())?;

    // for now we will assume that the target is a channel
    if !target.starts_with('#') {
        return Err(MessageDecodingError::NoRecipient { command });
    }

    let params = message.parameters();
    if let Some(change) = params.get(1) {
        let param = if let Some(param) = params.get(2) {
            Some(str2(command, param)?)
        } else {
            None
        };
        let modechar = str2(command, change)?;
        Ok(Message::ChangeModeChannel(target, modechar, param))
    } else {
        Ok(Message::AskModeChannel(target))
    }
}

fn handle_privmsg<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let target = message
        .first_parameter()
        .ok_or(MessageDecodingError::NoRecipient { command })?;
    let target = str2(command, target)?;
    let params = message.parameters();
    let content = params.get(1).ok_or(MessageDecodingError::NoTextToSend {})?;
    Ok(Message::PrivMsg(target, content))
}

fn handle_notice<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let target = message
        .first_parameter()
        .ok_or(MessageDecodingError::SilentError {})?;
    let target = str2(command, target)?;
    let params = message.parameters();
    let content = params.get(1).ok_or(MessageDecodingError::SilentError {})?;
    Ok(Message::Notice(target, content))
}

fn handle_part<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let channels = message
        .first_parameter()
        .ok_or(MessageDecodingError::NotEnoughParameters { command })?
        .split(|&c| c == b',')
        .flat_map(|s| str2(command, s))
        .collect::<Vec<_>>();
    let params = message.parameters();
    let reason = params.get(1).copied();
    Ok(Message::Part(channels, reason))
}

fn handle_list<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let (channels, start_index) = if let Some(first_parameter) = message.first_parameter() {
        let chans = first_parameter
            .split(|&c| c == b',')
            .flat_map(|s| String::from_utf8(s.to_owned()))
            .map(|mut s| {
                s.make_ascii_lowercase();
                s
            })
            .collect::<Vec<_>>();
        let empty = chans.is_empty();
        (Some(chans), if empty { 0 } else { 1 })
    } else {
        (None, 0)
    };

    let mut list_options = Vec::new();
    let params = message.parameters();
    for (param_index, option) in params.get(start_index..).into_iter().flatten().enumerate() {
        // offset because enumerate starts at 0 and not param_index
        let param_index = param_index + start_index;

        let mut list_option: ListOption = Default::default();

        // TODO/kid: not sure this implementation is correct, the spec is not clear

        let mut index = param_index;
        let Some(option) = option.first() else {
            return Err(MessageDecodingError::NotEnoughParameters { command });
        };
        if option.is_ascii() {
            list_option.filter = match option {
                b'C' => ListFilter::ChannelCreation,
                b'U' => ListFilter::UserNumber,
                b'T' => ListFilter::TopicUpdate,
                _ => {
                    return Err(MessageDecodingError::NotEnoughParameters { command });
                }
            };
            index += 1;
        }

        let operation = message.parameters().get(param_index + index);
        if let Some(operation) = operation {
            list_option.operation = match *operation {
                b"<" => ListOperation::Inf,
                b">" => ListOperation::Sup,
                _ => return Err(MessageDecodingError::NotEnoughParameters { command }),
            };
            index += 1;
        }

        let number = message.parameters().get(param_index + index);
        if let Some(number) = number {
            let count = str2(command, number)?;
            let count =
                count
                    .parse::<u64>()
                    .map_err(|_| MessageDecodingError::CannotParseInteger {
                        command: command.as_bytes(),
                    })?;
            list_option.number = count;
        }
        list_options.push(list_option);
    }
    Ok(Message::List(
        channels,
        if list_options.is_empty() {
            None
        } else {
            Some(list_options)
        },
    ))
}

fn handle_motd<'m>(
    _message: cirque_parser::Message<'m>,
    _command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    // don't parse the "server" argument, we don't support multi-server setups
    Ok(Message::MOTD())
}

fn handle_away<'m>(
    message: cirque_parser::Message<'m>,
    _command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let away_message =
        message
            .first_parameter()
            .and_then(|m| if m.is_empty() { None } else { Some(m) });
    Ok(Message::Away(away_message))
}

fn handle_userhost<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let params = message.parameters();
    // up-to five nicknames, in separate parameters
    // the first one is mandatory
    if params.is_empty() {
        return Err(MessageDecodingError::NotEnoughParameters { command });
    }
    let mut nicknames = vec![];
    for p in params.iter().take(5) {
        let nick = str2(command, p)?;
        nicknames.push(nick);
    }
    Ok(Message::Userhost(nicknames))
}

fn handle_whois<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let params = message.parameters();
    let nickname = if let Some(p) = params.get(1) {
        str2(command, p)?
    } else {
        optstr(command, message.first_parameter())?
    };
    Ok(Message::Whois(nickname))
}

fn handle_who<'m>(
    message: cirque_parser::Message<'m>,
    command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let mask = optstr(command, message.first_parameter())?;
    Ok(Message::Who(mask))
}

fn handle_lusers<'m>(
    _message: cirque_parser::Message<'m>,
    _command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    Ok(Message::Lusers())
}

fn handle_quit<'m>(
    message: cirque_parser::Message<'m>,
    _command: &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>> {
    let reason = message.first_parameter();
    Ok(Message::Quit(reason))
}

type Handler = for<'m> fn(
    cirque_parser::Message<'m>,
    &'m str,
) -> Result<Message<'m>, MessageDecodingError<'m>>;

static REGISTRY: phf::Map<unicase::UniCase<&str>, Handler> = phf::phf_map! {
    UniCase::ascii("USER") => handle_user,
    UniCase::ascii("NICK") => handle_nick,
    UniCase::ascii("PASS") => handle_pass,
    UniCase::ascii("PING") => handle_ping,
    UniCase::ascii("PONG") => handle_pong,
    UniCase::ascii("JOIN") => handle_join,
    UniCase::ascii("NAMES") => handle_names,
    UniCase::ascii("TOPIC") => handle_topic,
    UniCase::ascii("MODE") => handle_mode,
    UniCase::ascii("PRIVMSG") => handle_privmsg,
    UniCase::ascii("NOTICE") => handle_notice,
    UniCase::ascii("PART") => handle_part,
    UniCase::ascii("LIST") => handle_list,
    UniCase::ascii("MOTD") => handle_motd,
    UniCase::ascii("AWAY") => handle_away,
    UniCase::ascii("USERHOST") => handle_userhost,
    UniCase::ascii("WHOIS") => handle_whois,
    UniCase::ascii("WHO") => handle_who,
    UniCase::ascii("LUSERS") => handle_lusers,
    UniCase::ascii("QUIT") => handle_quit,
};

impl<'m> TryFrom<cirque_parser::Message<'m>> for Message<'m> {
    type Error = MessageDecodingError<'m>;

    fn try_from(message: cirque_parser::Message<'m>) -> Result<Self, Self::Error> {
        let command = message.command();
        let command = std::str::from_utf8(command)
            .map_err(|_| MessageDecodingError::CannotDecodeUtf8 { command })?;

        let Some(handler) = REGISTRY.get(&command.into()) else {
            return Ok(Message::Unknown(command));
        };

        handler(message, command)
    }
}
