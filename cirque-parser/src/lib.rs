/// Note: Source (used for server-to-server or server-to-client communications) is not handled.
use smallvec::SmallVec;

mod parser;
mod stream;

pub use crate::stream::{LendingIterator, ParsingError, StreamParser};

pub type Command = [u8];
pub type Parameters<'a> = SmallVec<[&'a [u8]; 15]>;

///
/// See: https://modern.ircdocs.horse/#client-to-server-protocol-structure
///
#[derive(Debug)]
pub struct Message<'m> {
    command: &'m Command,
    parameters: Parameters<'m>,
}

impl<'m> Message<'m> {
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
