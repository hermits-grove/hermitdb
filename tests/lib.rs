extern crate gitdb;
extern crate tempfile;
extern crate crdts;
extern crate time;

#[derive(Debug, PartialEq)]
struct User {
    name: String,
    age: f64
}

impl gitdb::Dao for User {
    fn val(prefix: &str, db: &gitdb::DB, sess: &gitdb::Session) -> Result<Option<User>, gitdb::Error> {
        let key = format!("{}$user", prefix);
        let map = if let Some(block) = db.get(&key.into_bytes(), &sess)? {
            block.to_map()?
        } else {
            return Ok(None);
        };

        let name = if let Some(block) = map.get(&"name".as_bytes().to_vec()) {
            block.to_reg()?.val.to_string()?
        } else {
            return Err(gitdb::Error::DaoField("name".into()))
        };

        let age = if let Some(block) = map.get(&"age".as_bytes().to_vec()) {
            block.to_reg()?.val.to_f64()?
        } else {
            return Err(gitdb::Error::DaoField("age".into()))
        };

        Ok(Some(User { name, age }))
    }

    fn update<F>(prefix: &str, db: &mut gitdb::DB, sess: &gitdb::Session, func: F) -> Result<(), gitdb::Error>
        where F: FnOnce(Option<Self>) -> Option<Self>
    {
        
        let key = format!("{}$user", prefix);
        let user = gitdb::Dao::val(&prefix, &db, &sess)?;
        if let Some(User { name, age }) = func(user) {
            let time = time::get_time();
            let dot = ((time.sec, time.nsec), sess.actor);
            
            let map = if let Some(block) = db.get(&key.clone().into_bytes(), &sess)? {
                let mut map = block.to_map().unwrap(); // this should be safe since it's checked in val()
                map.update("name".as_bytes().to_vec(), |block| {
                    let mut reg = block.unwrap().to_reg().unwrap(); // this should be safe
                    reg.update(gitdb::Prim::Str(name), dot).unwrap();
                    Some(gitdb::Block::Reg(reg))
                }, sess.actor);

                map.update("age".as_bytes().to_vec(), |block| {
                    let mut reg = block.unwrap().to_reg().unwrap(); // this should be safe
                    reg.update(gitdb::Prim::F64(age), dot).unwrap();
                    Some(gitdb::Block::Reg(reg))
                }, sess.actor);
                map
            } else {
                let mut map = crdts::Map::new();
                map.insert("name".as_bytes().to_vec(), gitdb::Block::Reg(crdts::LWWReg { val: gitdb::Prim::Str(name), dot }), sess.actor);
                map.insert("age".as_bytes().to_vec(), gitdb::Block::Reg(crdts::LWWReg { val: gitdb::Prim::F64(age), dot }), sess.actor);
                map
            };
            db.set(key.into_bytes(), gitdb::Block::Map(map), &sess)?;
        } else {
            db.del(&key.into_bytes(), &sess)?;
        }
        Ok(())
    }
}

#[test]
fn init() {
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_owned();
    let git_root = dir_path.join("db");
    gitdb::DB::init(&git_root).unwrap();
    assert!(git_root.is_dir());
}

#[test]
fn dao_read_write_read() {
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_owned();
    let git_root = dir_path.join("db");
    let mut db = gitdb::DB::init(&git_root).unwrap();

    let kdf = gitdb::crypto::KDF {
        pbkdf2_iters: 1000,
        salt: gitdb::crypto::rand_256().unwrap(),
        entropy: gitdb::crypto::create_entropy_file(&dir_path).unwrap()
    };

    let sess = gitdb::Session {
        actor: 0,
        master_key: kdf.master_key("super secret".as_bytes())
    };

    let res: Result<Option<User>, _> = gitdb::Dao::val("bob", &db, &sess);
    assert!(res.is_err()); // key should not exist

    gitdb::Dao::update("bob", &mut db, &sess, |user_opt| match user_opt {
        Some(_) => panic!("we should not have any data yet"),
        None => Some(User {
            name: "Bob".to_string(),
            age: 37.9
        })
    }).unwrap();

    let res2 = gitdb::Dao::val("bob", &db, &sess);
    assert!(res2.is_ok());
    let bob = res2.unwrap();
    assert_eq!(
        bob,
        Some(User {
            name: "Bob".to_string(),
            age: 37.9
        })
    );
}

#[test]
fn sync() {
    use std::io::{Write, stdout};
    stdout().flush().ok();
    let remote_root_dir = tempfile::tempdir().unwrap();
    let remote_root = remote_root_dir.path();
    let root_a_dir = tempfile::tempdir().unwrap();
    let root_a = root_a_dir.path();
    let root_b_dir = tempfile::tempdir().unwrap();
    let root_b = root_b_dir.path();
    let git_root_a = root_a.join("db");
    let git_root_b = root_b.join("db");

    gitdb::git2::Repository::init_bare(&remote_root).unwrap();

    let kdf = gitdb::crypto::KDF {
        pbkdf2_iters: 1000,
        salt: gitdb::crypto::rand_256().unwrap(),
        entropy: gitdb::crypto::create_entropy_file(&remote_root).unwrap()
    };

    let mut db_a = gitdb::DB::init(&git_root_a).unwrap();
    let sess_a = gitdb::Session {
        actor: 1,
        master_key: kdf.master_key("super secret".as_bytes())
    };

    let remote_url = format!("file://{}", remote_root.to_str().unwrap());
    let remote = gitdb::Remote::no_auth("local_remote".into(), remote_url);
    
    gitdb::Dao::update("db", &mut db_a, &sess_a, |block| match block {
        None => Some(remote.clone()),
        Some(_) => panic!("No remotes should exist yet!")
    }).unwrap();

    db_a.sync(&sess_a).unwrap();

    let mut db_b = gitdb::DB::init_from_remote(&git_root_b, &remote).unwrap();

    let sess_b = gitdb::Session {
        actor: 2,
        master_key: kdf.master_key("super secret".as_bytes())
    };

    // PRE SYNC:
    //   create A:users@sam
    //   create B:users@bob
    // POST SYNC:
    //   both sites A and B should have same sam and bob entries
    
    gitdb::Dao::update("sam", &mut db_a, &sess_a, |val| match val {
        Some(_) => panic!("we should not have any data yet"),
        None => Some(User {
            name: "Sam".to_string(),
            age: 12.5
        })
    }).unwrap();
    
    gitdb::Dao::update("bob", &mut db_b, &sess_b, |val| match val {
        Some(_) => panic!("we should not have any data yet"),
        None => Some(User {
            name: "Bob".to_string(),
            age: 11.25
        })
    }).unwrap();

    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();

    let sam_from_a: User = gitdb::Dao::val("sam", &db_a, &sess_a).unwrap().unwrap();
    let sam_from_b: User = gitdb::Dao::val("sam", &db_b, &sess_b).unwrap().unwrap();
    assert_eq!(sam_from_a, sam_from_b);

    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();
    let bob_from_a: User = gitdb::Dao::val("bob", &db_a, &sess_a).unwrap().unwrap();
    let bob_from_b: User = gitdb::Dao::val("bob", &db_b, &sess_b).unwrap().unwrap();
    
    assert_eq!(bob_from_a, bob_from_b);

    // PRE SYNC:
    //   create A:users@alice (with age 32)
    //   create B:users@alice (with age 32.5)
    // POST SYNC:
    //   both sites A and B should converge to the same alice age value

    gitdb::Dao::update("alice", &mut db_a, &sess_a, |val| match val {
        Some(_) => panic!("we should not have any data yet"),
        None => Some(User {
            name: "Alice".to_string(),
            age: 32.
        })
    }).unwrap();

    gitdb::Dao::update("alice", &mut db_b, &sess_b, |val| match val {
        Some(_) => panic!("we should not have any data yet"),
        None => Some(User {
            name: "Alice".to_string(),
            age: 32.5
        })
    }).unwrap();
    
    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();
    db_a.sync(&sess_a).unwrap();

    {
        let alice_from_a: User = gitdb::Dao::val("alice", &db_a, &sess_a).unwrap().unwrap();
        let alice_from_b: User = gitdb::Dao::val("alice", &db_b, &sess_b).unwrap().unwrap();
        assert_eq!(alice_from_a, alice_from_b);

        gitdb::Dao::update("alice", &mut db_b, &sess_b, |val| match val {
            Some(User { name, age }) => Some(User { name, age: age + 0.5 }),
            None => panic!("we should have data!")
        }).unwrap();
    }
    
    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();
    db_a.sync(&sess_a).unwrap();

    {
        
        let alice_from_a: User = gitdb::Dao::val("alice", &db_a, &sess_a).unwrap().unwrap();
        let alice_from_b: User = gitdb::Dao::val("alice", &db_b, &sess_b).unwrap().unwrap();

        assert_eq!(alice_from_a, alice_from_b);
        assert_eq!(alice_from_a.age, 33.);
    }
}
