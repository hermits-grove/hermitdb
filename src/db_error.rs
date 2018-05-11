extern crate git2;
extern crate ditto;

use std;

#[derive(Debug)]
pub enum DBErr {
    NotFound,
    Parse(String),
    Crypto(String),
    Version(String),
    State(String),
    Tree(String),
    CRDT(ditto::Error),
    Git(git2::Error),
    IO(std::io::Error)
}
