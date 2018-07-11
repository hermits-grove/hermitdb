use error::{Result};
use db;
use crypto::Session;

pub trait Dao {
    // TAI: prefix should be bytes instead of str hmmmmm
    fn val(prefix: &str, db: &db::DB, sess: &Session) -> Result<Option<Self>>
        where Self: Sized;

    fn update<F>(prefix: &str, db: &mut db::DB, sess: &Session, func: F) -> Result<()>
        where Self: Sized,
              F: FnMut(Option<Self>) -> Option<Self>;
}
