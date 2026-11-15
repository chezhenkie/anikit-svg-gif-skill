//! Errors shared by every anikit crate.

use std::fmt;

/// One error type for the whole workspace, so every route reports the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Bad input: a missing file, a value out of range, a malformed document.
    Invalid(String),
    /// A read or write failed.
    Io(String),
    /// The written file did not match what was asked for.
    Verify(String),
    /// A time or frame plan could not be built.
    Time(String),
}

impl Error {
    pub fn invalid(message: impl Into<String>) -> Self {
        Error::Invalid(message.into())
    }

    pub fn io(message: impl Into<String>) -> Self {
        Error::Io(message.into())
    }

    pub fn verify(message: impl Into<String>) -> Self {
        Error::Verify(message.into())
    }

    pub fn time(message: impl Into<String>) -> Self {
        Error::Time(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(m) | Error::Io(m) | Error::Verify(m) | Error::Time(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
