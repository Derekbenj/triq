//! One error type for the whole crate.
//!
//! Any `std::error::Error` converts into it, so `?` works inside `Node::run`
//! on whatever the node's own code returns. It crosses the control plane as
//! JSON, so a request refused by another runtime fails with the same variant.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    /// A name is malformed, too long, or already taken.
    Name(String),
    /// No runtime, node, port, factory or pool with this name.
    Unknown(String),
    /// An input and an output carry different message types, or an option does not fit the port.
    Mismatch(String),
    /// A reliable output could not deliver to every subscriber in time.
    Undelivered,
    /// Another runtime did not answer in time.
    Timeout,
    /// Anything else, including iceoryx2 failures and errors raised by a node's own code.
    Other(String),
}

pub type Result<T = ()> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(reason) => write!(f, "invalid name: {reason}"),
            Self::Unknown(what) => write!(f, "unknown {what}"),
            Self::Mismatch(reason) => write!(f, "mismatch: {reason}"),
            Self::Undelivered => f.write_str("a subscriber did not take the sample in time"),
            Self::Timeout => f.write_str("no answer in time"),
            Self::Other(reason) => f.write_str(reason),
        }
    }
}

// `Error` deliberately does not implement `std::error::Error`: that is what
// allows this blanket conversion.
impl<E: std::error::Error> From<E> for Error {
    fn from(error: E) -> Self {
        Self::Other(error.to_string())
    }
}

impl From<Error> for Box<dyn std::error::Error> {
    fn from(error: Error) -> Self {
        error.to_string().into()
    }
}

impl From<Error> for Box<dyn std::error::Error + Send + Sync> {
    fn from(error: Error) -> Self {
        error.to_string().into()
    }
}
