use nom::{
    branch::alt,
    bytes::complete::{tag, take_till, take_while, take_while1, take_while_m_n},
    character::{
        complete::{char, space0},
        is_alphabetic, is_digit,
    },
    combinator::{opt, peek, rest},
    multi::{many0, many1},
    sequence::{preceded, terminated},
    IResult,
};

use crate::{Command, Message, Parameters, Source};

// Nicknames are non-empty strings with the following restrictions:
//
//  They MUST NOT contain any of the following characters: space (' ', 0x20), comma (',', 0x2C), asterisk ('*', 0x2A), question mark ('?', 0x3F), exclamation mark ('!', 0x21), at sign ('@', 0x40).
//  They MUST NOT start with any of the following characters: dollar ('$', 0x24), colon (':', 0x3A).
//  They MUST NOT start with a character listed as a channel type, channel membership prefix, or prefix listed in the IRCv3 multi-prefix Extension.
//  They SHOULD NOT contain any dot character ('.', 0x2E).
//
// Servers MAY have additional implementation-specific nickname restrictions and SHOULD avoid the use of nicknames which are ambiguous with commands or command parameters where this could lead to confusion or error.
fn nickname(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    let is_valid_nickname_char = |c: u8| c.is_ascii_alphanumeric() || c == b'-';

    let (buf, nickname) = take_while1(is_valid_nickname_char)(buf)?;
    Ok((buf, nickname))
}

fn user(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    let is_valid_user_char = |c: u8| c.is_ascii_alphanumeric();

    let (buf, user) = take_while1(is_valid_user_char)(buf)?;
    Ok((buf, user))
}

fn host(buf: &[u8]) -> IResult<&[u8], &[u8]> {
    let is_valid_host_char = |c: u8| c.is_ascii_alphanumeric();

    let (buf, user) = take_while1(is_valid_host_char)(buf)?;
    Ok((buf, user))
}

/// NOTE: The following implementation and the functions above does not respect the specification,
///       simply because an IRC server never receives a source.
fn parse_source_inner(buf: &[u8]) -> IResult<&[u8], Source<'_>> {
    let (buf, nickname) = nickname(buf)?;
    let (buf, user) = opt(preceded(char('!'), user))(buf)?;
    let (buf, host) = opt(preceded(char('@'), host))(buf)?;

    let source = Source {
        nickname,
        user,
        host,
    };
    Ok((buf, source))
}

// source ::= <servername> / ( <nickname> [ "!" <user> ] [ "@" <host> ] )
// nick ::= <any characters except NUL, CR, LF, chantype character, and SPACE> <possibly empty sequence of any characters except NUL, CR, LF, and SPACE>
// user ::= <sequence of any characters except NUL, CR, LF, and SPACE>
fn parse_source(buf: &[u8]) -> IResult<&[u8], Source<'_>> {
    let colon = char(':');
    let (buf, source) = preceded(colon, parse_source_inner)(buf)?;
    Ok((buf, source))
}

// command ::= letter* / 3digit
fn parse_command(buf: &[u8]) -> IResult<&[u8], &Command> {
    let letters = take_while1(is_alphabetic);
    let digits = take_while_m_n(3, 3, is_digit);

    let (buf, command) = alt((letters, digits))(buf)?;
    Ok((buf, command))
}

fn parse_parameters(mut buf: &[u8]) -> IResult<&[u8], Parameters<'_>> {
    let is_space = |c: u8| -> bool { c == b' ' };

    let mut params: Parameters<'_> = smallvec::smallvec!();
    loop {
        if buf.is_empty() {
            break;
        }

        let (buf_, _spaces) = take_while(is_space)(buf)?;
        buf = buf_;

        buf = if peek(tag::<_, _, nom::error::Error<&[u8]>>(b":"))(buf).is_ok() {
            let (buf_, rest) = preceded(tag(b":"), rest)(buf)?;
            params.push(rest);
            buf_
        } else {
            let (buf_, param) = take_till(is_space)(buf)?;
            params.push(param);
            buf_
        }
    }

    Ok((buf, params))
}

// message ::= ['@' <tags> SPACE] [':' <source> SPACE] <command> <parameters> <crlf>
pub fn parse_message(buf: &[u8]) -> IResult<&[u8], Message<'_>> {
    let space = &char(' ');
    let (buf, _) = space0(buf)?;
    let (buf, source) = opt(terminated(parse_source, many1(space)))(buf)?;
    let (buf, command) = parse_command(buf)?;
    let (buf, parameters) = preceded(many0(space), parse_parameters)(buf)?;
    Ok((
        buf,
        Message {
            source,
            command,
            parameters,
        },
    ))
}

#[cfg(test)]
mod tests {
    mod command {
        use super::super::*;
        use nom::combinator::all_consuming;

        #[test]
        fn ping() {
            let (buf, cmd) = parse_command(b"PING").unwrap();
            assert!(buf.is_empty());
            assert_eq!(cmd, b"PING");
        }

        #[test]
        fn fail_pre_space() {
            let result = all_consuming(parse_command)(b" PING");
            assert!(result.is_err());
        }

        #[test]
        fn fail_post_space() {
            let result = all_consuming(parse_command)(b"PING ");
            assert!(result.is_err());
        }

        #[test]
        fn fail_1digit() {
            let result = all_consuming(parse_command)(b"0");
            assert!(result.is_err());
        }

        #[test]
        fn fail_2digit() {
            let result = all_consuming(parse_command)(b"00");
            assert!(result.is_err());
        }

        #[test]
        fn success_3digit() {
            let (buf, cmd) = all_consuming(parse_command)(b"000").unwrap();
            assert!(buf.is_empty());
            assert_eq!(cmd, b"000");
        }

        #[test]
        fn fail_4digit() {
            let result = all_consuming(parse_command)(b"0000");
            assert!(result.is_err());
        }
    }

    mod source {
        use super::super::*;
        use nom::combinator::all_consuming;

        #[test]
        fn nick_user() {
            let (buf, cmd) = all_consuming(parse_source)(b":nick!user").unwrap();
            assert_eq!(
                cmd,
                Source {
                    nickname: b"nick",
                    user: Some(b"user"),
                    host: None,
                }
            );
            assert!(buf.is_empty());
        }

        #[test]
        fn nick_host() {
            let (buf, cmd) = all_consuming(parse_source)(b":nick@host").unwrap();
            assert_eq!(
                cmd,
                Source {
                    nickname: b"nick",
                    user: None,
                    host: Some(b"host")
                }
            );
            assert!(buf.is_empty());
        }

        #[test]
        fn nick_user_host() {
            let (buf, cmd) = all_consuming(parse_source)(b":nick!user@host").unwrap();
            assert_eq!(
                cmd,
                Source {
                    nickname: b"nick",
                    user: Some(b"user"),
                    host: Some(b"host")
                }
            );
            assert!(buf.is_empty());
        }

        #[test]
        fn fail_empty_nickname() {
            let result = all_consuming(parse_source)(b"n");
            assert!(result.is_err());
        }

        #[test]
        fn fail_empty_user() {
            let result = all_consuming(parse_source)(b"n!@h");
            assert!(result.is_err());
        }

        #[test]
        fn fail_empty_host() {
            let result = all_consuming(parse_source)(b"n!u@");
            assert!(result.is_err());
        }
    }

    mod parameters {
        use super::super::*;
        use nom::combinator::all_consuming;

        #[test]
        fn ex1() {
            let (buf, params) = all_consuming(parse_parameters)(b"* LIST :").unwrap();
            assert_eq!(params[0], b"*");
            assert_eq!(params[1], b"LIST");
            assert_eq!(params[2], b"");
            assert_eq!(params.len(), 3);
            assert!(buf.is_empty());
        }

        #[test]
        fn ex2() {
            let (buf, params) =
                all_consuming(parse_parameters)(b"* LS :multi-prefix sasl").unwrap();
            assert_eq!(params[0], b"*");
            assert_eq!(params[1], b"LS");
            assert_eq!(params[2], b"multi-prefix sasl");
            assert_eq!(params.len(), 3);
            assert!(buf.is_empty());
        }

        #[test]
        fn ex3() {
            let (buf, params) =
                all_consuming(parse_parameters)(b"REQ :sasl message-tags foo").unwrap();
            assert_eq!(params[0], b"REQ");
            assert_eq!(params[1], b"sasl message-tags foo");
            assert_eq!(params.len(), 2);
            assert!(buf.is_empty());
        }

        #[test]
        fn ex4() {
            let (buf, params) = all_consuming(parse_parameters)(b"#chan :Hey!").unwrap();
            assert_eq!(params[0], b"#chan");
            assert_eq!(params[1], b"Hey!");
            assert_eq!(params.len(), 2);
            assert!(buf.is_empty());
        }

        #[test]
        fn ex5() {
            let (buf, params) = all_consuming(parse_parameters)(b"#chan Hey!").unwrap();
            assert_eq!(params[0], b"#chan");
            assert_eq!(params[1], b"Hey!");
            assert_eq!(params.len(), 2);
            assert!(buf.is_empty());
        }

        #[test]
        fn ex6() {
            let (buf, params) = all_consuming(parse_parameters)(b"#chan ::-)").unwrap();
            assert_eq!(params[0], b"#chan");
            assert_eq!(params[1], b":-)");
            assert_eq!(params.len(), 2);
            assert!(buf.is_empty());
        }
    }

    mod message {
        use super::super::*;
        use nom::combinator::all_consuming;

        #[test]
        fn ex1() {
            let (buf, message) =
                all_consuming(parse_message)(b"  :dan-!d@localhost QUIT :Quit: Bye for now!")
                    .unwrap();
            assert_eq!(message.command(), b"QUIT");
            //let params = message.parameters();
            //assert_eq!(params.len(), 1);
            assert!(buf.is_empty());
        }

        #[test]
        fn ex2() {
            let (buf, message) = all_consuming(parse_message)(b"CAP LS 302").unwrap();
            assert_eq!(message.command(), b"CAP");
            assert!(buf.is_empty());
        }
    }
}
