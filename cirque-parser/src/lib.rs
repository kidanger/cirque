use smallvec::SmallVec;

mod parser;
mod stream;

pub use crate::stream::{LendingIterator, MessageIteratorError, ParsingError, StreamParser};

/// Note: Server sources (used for server-to-server communications) are not handled.
#[derive(Debug, PartialEq, Eq)]
pub struct Source<'s> {
    nickname: &'s [u8],
    user: Option<&'s [u8]>,
    host: Option<&'s [u8]>,
}

pub type Command = [u8];
pub type Parameters<'a> = SmallVec<[&'a [u8]; 15]>;

///
/// See: https://modern.ircdocs.horse/#client-to-server-protocol-structure
///
#[derive(Debug)]
pub struct Message<'m> {
    source: Option<Source<'m>>,
    command: &'m Command,
    parameters: Parameters<'m>,
}

impl<'m> Message<'m> {
    pub fn source(&self) -> &Option<Source<'m>> {
        &self.source
    }

    pub fn command(&self) -> &'m Command {
        self.command
    }

    pub fn parameters(&self) -> &Parameters<'m> {
        &self.parameters
    }

    pub fn first_parameter(&self) -> Option<&'m [u8]> {
        self.parameters.first().copied()
    }

    pub fn first_parameter_as_vec(&self) -> Option<Vec<u8>> {
        self.first_parameter().map(|s| s.to_vec())
    }
}
