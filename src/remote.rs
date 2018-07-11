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
extern crate crdts;
extern crate serde;
extern crate time;

use error::{Error, Result};
use block::{Block, Prim};
use dao::Dao;
use db::DB;
use crypto::Session;

#[derive(Debug, PartialEq, Eq, Clone)]
struct Auth {
    user: String,
    pass: String
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Remote {
    pub name: String,
    pub url: String,
    auth: Option<Auth>
}

impl Dao for Remote {
    fn val(prefix: &str, db: &DB, sess: &Session) -> Result<Option<Self>> {
        let key = format!("{}$remote", prefix);
        let map_opt = if let Some(block) = db.get(&key.into_bytes(), &sess)? {
            let map = block.to_map()?;
            Some(map)
        } else {
            None
        };
        
        let remote_opt = if let Some(map) = map_opt {
            let remote = Remote::from_map(&map)?;
            Some(remote)
        } else {
            None
        };

        Ok(remote_opt)
    }

    fn update<F>(prefix: &str, db: &mut DB, sess: &Session, func: F) -> Result<()>
        where F: FnOnce(Option<Self>) -> Option<Self> {
        let key = format!("{}$remote", prefix);

        let map_opt = db.get(&key.clone().into_bytes(), &sess)
            .and_then(
                |block_opt| match block_opt {
                    Some(block) => block.to_map()
                        .map(|parsed_map| Some(parsed_map)),
                    None => Ok(None)
                }
            )?;

        let remote_opt = map_opt.clone()
            .and_then(|map| Remote::from_map(&map).ok());

        if let Some(Remote { name, url, auth }) = func(remote_opt) {
            let time = time::get_time();
            let dot = ((time.sec, time.nsec), sess.actor);

            let new_map = if let Some(mut map) = map_opt {
                // These unwraps are safe to do since we've checked them above
                map.update("name".as_bytes().to_vec(), |name_reg| {
                    let mut name_reg = name_reg.unwrap().to_reg().unwrap();
                    name_reg.update(Prim::Str(name), dot).unwrap(); // TAI: how to deal with failures within the update function?
                    Some(Block::Reg(name_reg))
                }, sess.actor);

                map.update("url".as_bytes().to_vec(), |url_reg| {
                    let mut url_reg = url_reg.unwrap().to_reg().unwrap();
                    url_reg.update(Prim::Str(url), dot).unwrap(); // TAI: failure in update function
                    Some(Block::Reg(url_reg))
                }, sess.actor);

                if let Some(Auth { user, pass }) = auth {
                    map.update("auth".as_bytes().to_vec(), |auth_map| {
                        if let Some(map_block) = auth_map {
                            let mut map = map_block.to_map().unwrap();
                            map.update("user".as_bytes().to_vec(), |user_reg| {
                                let mut user_reg = user_reg.unwrap().to_reg().unwrap();
                                user_reg.update(Prim::Str(user), dot).unwrap();  // TAI: failure in update function
                                Some(Block::Reg(user_reg))
                            }, sess.actor);

                            map.update("pass".as_bytes().to_vec(), |pass_reg| {
                                let mut pass_reg = pass_reg.unwrap().to_reg().unwrap();
                                pass_reg.update(Prim::Str(pass), dot).unwrap(); // TAI: failure in update function
                                Some(Block::Reg(pass_reg))
                            }, sess.actor);
                            Some(Block::Map(map))
                        } else {
                            let mut auth_map = crdts::Map::new();
                            let user_reg = crdts::LWWReg { val: Prim::Str(user), dot: dot };
                            let pass_reg = crdts::LWWReg { val: Prim::Str(pass), dot: dot };
                            auth_map.insert("user".as_bytes().to_vec(), Block::Reg(user_reg), sess.actor);
                            auth_map.insert("pass".as_bytes().to_vec(), Block::Reg(pass_reg), sess.actor);
                            Some(Block::Map(auth_map))
                        }
                    }, sess.actor)
                } else {
                    map.remove("auth".as_bytes().to_vec(), sess.actor);
                }

                map
            } else {
                let mut map = crdts::Map::new();
                map.insert("name".as_bytes().to_vec(), Block::Reg(crdts::LWWReg { val: Prim::Str(name), dot: dot }), sess.actor);
                map.insert("url".as_bytes().to_vec(), Block::Reg(crdts::LWWReg { val: Prim::Str(url), dot: dot }), sess.actor);

                if let Some(auth) = auth {
                    let mut auth_map = crdts::Map::new();
                    let user_reg = crdts::LWWReg { val: Prim::Str(auth.user), dot: dot };
                    let pass_reg = crdts::LWWReg { val: Prim::Str(auth.pass), dot: dot };
                    auth_map.insert("user".as_bytes().to_vec(), Block::Reg(user_reg), sess.actor);
                    auth_map.insert("pass".as_bytes().to_vec(), Block::Reg(pass_reg), sess.actor);
                    map.insert("auth".as_bytes().to_vec(), Block::Map(auth_map), sess.actor);
                }

                map
            };
            db.set(key.into_bytes(), Block::Map(new_map), &sess)?;
        } else {
            db.del(&key.into_bytes(), &sess)?;
        };
        Ok(())
    }
}

impl Remote {

    fn from_map(map: &crdts::Map<Vec<u8>, Block, u128>) -> Result<Self> {
        let name = map.get(&"name".as_bytes().to_vec())
            .ok_or(Error::DaoField("name".to_string()))
            ?.to_reg()
            ?.val.to_string()?;

        let url = map.get(&"url".as_bytes().to_vec())
            .ok_or(Error::DaoField("url".to_string()))
            ?.to_reg()
            ?.val.to_string()?;
        
        let auth = if let Some(auth_block) = map.get(&"auth".as_bytes().to_vec()) {
            let auth_map = auth_block.to_map()?;
            let auth = Auth {
                user: auth_map.get(&"user".as_bytes().to_vec())
                    .ok_or(Error::DaoField("auth/user".to_string()))
                    ?.to_reg()
                    ?.val.to_string()?,
                pass: auth_map.get(&"pass".as_bytes().to_vec())
                    .ok_or(Error::DaoField("auth/pass".to_string()))
                    ?.to_reg()
                    ?.val.to_string()?
            };
            Some(auth)
        } else {
            None
        };

        Ok(Remote { name, url, auth })
    }
    
    pub fn auth(name: String, url: String, user: String, pass: String) -> Self {
        Remote {
            name: name,
            url: url,
            auth: Some(Auth { user, pass })
        }
    }

    pub fn no_auth(name: String, url: String) -> Self {
        Remote {
            name: name,
            url: url,
            auth: None
        }
    }

    pub fn git_callbacks(&self) -> git2::RemoteCallbacks {
        let mut cbs = git2::RemoteCallbacks::new();
        cbs.credentials(move |_, _, _| {
            match self.auth {
                Some(Auth {ref user, ref pass} ) =>
                    git2::Cred::userpass_plaintext(user, pass),
                None => {
                    panic!("This should never be called!");
                }
            }
        });
        cbs
    }
}
