use std::{self, fmt};

use crate::data::Kind;

pub type Result<T> = std::result::Result<T, Error>;

// TODO: audit usage of these error types, I have a feeling not all of these are used
#[derive(Debug)]
pub enum Error {
    UnexpectedKind(Kind, Kind),
    BranchNameEncodingError,
    BranchIsNotADirectReference,
    LogCommitDoesNotContainOp,
    Parse(String),
    Crypto(String),
    State(String),
    Bincode(bincode::Error),
    Git(git2::Error),
    IO(std::io::Error),
    SledGeneric(sled::Error)
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::UnexpectedKind(expected, got) =>
                write!(f, "Unexpected kind! got: {:?}, expected: {:?}", got, expected),
            Error::BranchNameEncodingError =>
                write!(f, "A branch name is not utf8 encoded"),
            Error::BranchIsNotADirectReference =>
                write!(f, "A branch reference isn't a direct ref to an oid"),
            Error::LogCommitDoesNotContainOp =>
                write!(f, "Trees attached to commits in git are expected to have an 'op' entry"),
            Error::Parse(s) =>
                write!(f, "Parsing failed: {}", s),
            Error::Crypto(s) =>
                write!(f, "Crypto failure: {}", s),
            Error::State(s) =>
                write!(f, "Gitdb entered a bad state: {}", s),
            Error::Bincode(e) => e.fmt(f),
            Error::Git(e) => e.fmt(f),
            Error::IO(e) => e.fmt(f),
            Error::SledGeneric(e) => e.fmt(f)
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::UnexpectedKind(_, _) => None,
            Error::BranchNameEncodingError => None,
            Error::BranchIsNotADirectReference => None,
            Error::LogCommitDoesNotContainOp => None,
            Error::Parse(_) => None,
            Error::Crypto(_) => None,
            Error::State(_) => None,
            Error::Bincode(e) => Some(e),
            Error::Git(e) => Some(e),
            Error::IO(e) => Some(e),
            Error::SledGeneric(e) => Some(e)
        }
    }
}

impl From<sled::Error> for Error {
    fn from(err: sled::Error) -> Self {
        Error::SledGeneric(err)
    }
}

impl From<git2::Error> for Error {
    fn from(err: git2::Error) -> Self {
        Error::Git(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IO(err)
    }
}

impl From<bincode::Error> for Error {
    fn from(err: bincode::Error) -> Self {
        Error::Bincode(err)
    }
}
