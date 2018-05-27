extern crate git2;
extern crate ditto;
extern crate rmp_serde;
extern crate data_encoding;

use std;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    NotFound,
    BlockTypeConflict,
    Parse(String),
    Crypto(String),
    Version(String),
    State(String),
    Tree(String),
    SerdeRMPDecode(rmp_serde::decode::Error),
    SerdeRMPEncode(rmp_serde::encode::Error),
    CRDT(ditto::Error),
    Git(git2::Error),
    IO(std::io::Error),
    DataEncodingDecode(data_encoding::DecodeError)
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

impl From<rmp_serde::decode::Error> for Error {
    fn from(err: rmp_serde::decode::Error) -> Self {
        Error::SerdeRMPDecode(err)
    }
}

impl From<rmp_serde::encode::Error> for Error {
    fn from(err: rmp_serde::encode::Error) -> Self {
        Error::SerdeRMPEncode(err)
    }
}

impl From<data_encoding::DecodeError> for Error {
    fn from(err: data_encoding::DecodeError) -> Self {
        Error::DataEncodingDecode(err)
    }
}
