extern crate git2;
extern crate ditto;
extern crate rmp_serde;
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
    SerdeDe(rmp_serde::decode::Error),
    SerdeEn(rmp_serde::encode::Error),
    CRDT(ditto::Error),
    Git(git2::Error),
    IO(std::io::Error)
}
