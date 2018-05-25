extern crate git2;
extern crate ditto;
extern crate rmp_serde;
extern crate data_encoding;

use std;

pub type Result<T> = std::result::Result<T, DBErr>;

#[derive(Debug)]
pub enum DBErr {
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


impl From<git2::Error> for DBErr {
    fn from(err: git2::Error) -> Self {
        DBErr::Git(err)
    }
}

impl From<std::io::Error> for DBErr {
    fn from(err: std::io::Error) -> Self {
        DBErr::IO(err)
    }
}

impl From<rmp_serde::decode::Error> for DBErr {
    fn from(err: rmp_serde::decode::Error) -> Self {
        DBErr::SerdeRMPDecode(err)
    }
}

impl From<rmp_serde::encode::Error> for DBErr {
    fn from(err: rmp_serde::encode::Error) -> Self {
        DBErr::SerdeRMPEncode(err)
    }
}

impl From<data_encoding::DecodeError> for DBErr {
    fn from(err: data_encoding::DecodeError) -> Self {
        DBErr::DataEncodingDecode(err)
    }
}
