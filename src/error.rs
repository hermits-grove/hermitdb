extern crate git2;
extern crate bincode;
extern crate data_encoding;
extern crate crdts;
extern crate sled;

use std::{self, fmt};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotFound,
    BlockTypeConflict,
    NoRemote,
    DaoField(String),
    Parse(String),
    Crypto(String),
    Version(String),
    State(String),
    Bincode(bincode::Error),
    CRDT(crdts::Error),
    Git(git2::Error),
    IO(std::io::Error),
    DataEncodingDecode(data_encoding::DecodeError),
    SledGeneric(sled::Error<()>)
}

impl fmt::Display for Error {
    fn fmt(&self, mut f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NotFound =>
                write!(f, "Key not found"),
            Error::BlockTypeConflict =>
                write!(f, "Attempted to merge different block types"),
            Error::NoRemote =>
                write!(f, "No Git remote has been added to the db"),
            Error::DaoField(s) =>
                write!(f, "Dao Field error: {}", s),
            Error::Parse(s) =>
                write!(f, "Parsing failed: {}", s),
            Error::Crypto(s) =>
                write!(f, "Crypto failure: {}", s),
            Error::Version(s) =>
                write!(f, "Version failure: {}", s),
            Error::State(s) =>
                write!(f, "Gitdb entered a bad state: {}", s),
            Error::Bincode(e) => e.fmt(&mut f),
            Error::CRDT(e) => e.fmt(&mut f),
            Error::Git(e) => e.fmt(&mut f),
            Error::IO(e) => e.fmt(&mut f),
            Error::DataEncodingDecode(e) => e.fmt(&mut f),
            Error::SledGeneric(e) => e.fmt(&mut f)
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        match self {
            Error::NotFound => "Key was not found",
            Error::BlockTypeConflict => "Merge blocks with different types",
            Error::NoRemote => "No Git remote has been added to the db",
            Error::DaoField(_) => "Problem with field while processing Dao request",
            Error::Parse(_) => "Parsing failed",
            Error::Crypto(_) => "Crypto failure",
            Error::Version(_) => "Version failure",
            Error::State(_) => "Gitdb entered a bad state",
            Error::Bincode(e) => e.description(),
            Error::CRDT(e) => e.description(),
            Error::Git(e) => e.description(),
            Error::IO(e) => e.description(),
            Error::DataEncodingDecode(e) => e.description(),
            Error::SledGeneric(e) => e.description()
        }
    }
    fn cause(&self) -> Option<&std::error::Error> {
        match self {
            Error::NotFound => None,
            Error::BlockTypeConflict => None,
            Error::NoRemote => None,
            Error::DaoField(_) => None,
            Error::Parse(_) => None,
            Error::Crypto(_) => None,
            Error::Version(_) => None,
            Error::State(_) => None,
            Error::Bincode(e) => Some(e),
            Error::CRDT(e) => Some(e),
            Error::Git(e) => Some(e),
            Error::IO(e) => Some(e),
            Error::DataEncodingDecode(e) => Some(e),
            Error::SledGeneric(e) => Some(e)
        }
    }
}

impl From<crdts::Error> for Error {
    fn from(err: crdts::Error) -> Self {
        Error::CRDT(err)
    }
}

impl From<sled::Error<()>> for Error {
    fn from(err: sled::Error<()>) -> Self {
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

impl From<data_encoding::DecodeError> for Error {
    fn from(err: data_encoding::DecodeError) -> Self {
        Error::DataEncodingDecode(err)
    }
}
