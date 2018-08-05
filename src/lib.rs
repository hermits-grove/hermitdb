#[macro_use]
extern crate serde_derive;
extern crate serde;

pub extern crate crdts;
pub extern crate git2;
pub extern crate sled;

extern crate bincode;

#[macro_use]
extern crate assert_matches;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;

pub mod git_helper;
pub mod error;
pub mod crypto;
pub mod encoding;
pub mod remote;
pub mod db;
// pub mod dao;
pub mod log;
pub mod memory_log;
pub mod git_log;
pub mod map;
pub mod data;

pub use error::Error;
pub use db::DB;
pub use crypto::{Session, Plaintext, Encrypted};
pub use remote::Remote;
// pub use dao::Dao;
pub use log::{LogReplicable, TaggedOp};
