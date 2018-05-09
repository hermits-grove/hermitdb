extern crate git2;
use std;

#[derive(Debug)]
pub enum DBErr {
    Parse(String),
    Crypto(String),
    Version(String),
    State(String),
    Git(git2::Error),
    IO(std::io::Error)
}
