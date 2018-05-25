// git remotes need credentials.
//
// To avoid having the user re-enter their
// credentials on each synch, we store an encrypted mapping
// from remote to credentials in the Mona git-db.
//
// This has the added benefit of all mona clients
// automatically learning of changes made to
// remotes by one client.
extern crate git2;
extern crate ditto;
extern crate serde;

use db_error::{Result, DBErr};
use block::{Blockable, Block, Prim};
use db::DB;
use crypto::Session;

#[derive(Debug, PartialEq, Clone)]
pub enum Remote {
    UserPassAuth {
        name: ditto::Register<Prim>,
        url: ditto::Register<Prim>,
        username: ditto::Register<Prim>,
        password: ditto::Register<Prim>,
    },
    NoAuth {
        name: ditto::Register<Prim>,
        url: ditto::Register<Prim>,
    }
}

impl Eq for Remote {}

impl Blockable for Remote {
    fn blocks(&self) -> Vec<(String, Block)> {
        match self {
            Remote::UserPassAuth { name, url, username, password } => {
                vec![
                    ("#userpassauth$name".into(), Block::Val(name.clone())),
                    ("#userpassauth$url".into(), Block::Val(url.clone())),
                    ("#userpassauth$username".into(), Block::Val(username.clone())),
                    ("#userpassauth$password".into(), Block::Val(password.clone()))
                ]
            },
            Remote::NoAuth { name, url } => {
                vec![
                    ("#noauth$name".into(), Block::Val(name.clone())),
                    ("#noauth$url".into(), Block::Val(url.clone()))
                ]
            }
        } 
    }
}

impl Remote {
    pub fn from_db(prefix: &str, db: &DB, mut sess: &mut Session) -> Result<Self> {
        let res = Remote::noauth_from_db(&prefix, &db, &mut sess);
        if let Ok(remote) = res {
            Ok(remote)
        } else {
            Remote::userpass_from_db(&prefix, &db, &mut sess)
        }   
    }

    fn userpass_from_db(prefix: &str, db: &DB, mut sess: &mut Session) -> Result<Self> {
        let pre = format!("{}#userpassauth$", prefix);

        let name = db.read_block(&format!("{}name", pre), &mut sess)?.to_val()?;
        let url = db.read_block(&format!("{}url", pre), &mut sess)?.to_val()?;
        let username = db.read_block(&format!("{}username", pre), &mut sess)?.to_val()?;
        let password = db.read_block(&format!("{}password", pre), &mut sess)?.to_val()?;

        Ok(Remote::UserPassAuth { name, url, username, password })
    }

    fn noauth_from_db(prefix: &str, db: &DB, mut sess: &mut Session) -> Result<Self> {
        let pre = format!("{}#noauth$", prefix);

        let name =  db.read_block(&format!("{}name", pre), &mut sess)?.to_val()?;
        let url = db.read_block(&format!("{}url", pre), &mut sess)?.to_val()?;

        Ok(Remote::NoAuth { name, url })
    }
    
    pub fn no_auth(name: &str, url: &str, site_id: ditto::dot::SiteId) -> Self {
        return Remote::NoAuth {
            name: ditto::Register::new(name.into(), site_id),
            url: ditto::Register::new(url.into(), site_id),
        }
    }

    pub fn name(&self) -> String {
        match self {
            Remote::UserPassAuth { name, ..} => name.get().to_string().unwrap(),
            Remote::NoAuth { name, ..} => name.get().to_string().unwrap()
        }
    }
    
    pub fn url(&self) -> String {
        match self {
            Remote::UserPassAuth { url, ..} => url.get().to_string().unwrap(),
            Remote::NoAuth { url, ..} => url.get().to_string().unwrap()
        }
    }

    pub fn git_callbacks(&self) -> git2::RemoteCallbacks {
        match self {
            Remote::UserPassAuth { username, password, ..} => {
                let mut cbs = git2::RemoteCallbacks::new();
                cbs.credentials(move |_, _, _| {
                    git2::Cred::userpass_plaintext(
                        &username.get().to_string().unwrap(),
                        &password.get().to_string().unwrap()
                    )
                });
                cbs
            },
            Remote::NoAuth { .. } => {
                let mut cbs = git2::RemoteCallbacks::new();
                cbs.credentials(move |_, _, _| {
                    panic!("This should never be called!");
                });
                cbs
            }
        }
    }
}
