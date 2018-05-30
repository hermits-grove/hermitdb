extern crate gitdb;
extern crate tempfile;
extern crate ditto;

#[derive(Debug, PartialEq)]
struct User {
    name: ditto::Register<gitdb::Prim>,
    age: ditto::Register<gitdb::Prim>
}

impl gitdb::Blockable for User {
    fn blocks(&self) -> Vec<(String, gitdb::Block)> {
        vec![
            ("$name".to_string(), gitdb::Block::Val(self.name.clone())),
            ("$age".to_string(), gitdb::Block::Val(self.age.clone())),
        ]
    }
}

impl User {
    fn from_db(user_key: &str, db: &gitdb::DB, sess: &gitdb::Session) -> Result<Self, gitdb::Error> {
        let name_key = format!("users@{}$name", user_key);
        let age_key = format!("users@{}$age", user_key);

        let name = db.read_block(&name_key, &sess)?.to_val()?;
        let age = db.read_block(&age_key, &sess)?.to_val()?;

        Ok(User {
            name: name,
            age: age
        })
    }
}

#[test]
fn init() {
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_owned();

    let kdf = gitdb::crypto::KDF {
        pbkdf2_iters: 1000,
        salt: gitdb::crypto::rand_256().unwrap(),
        entropy: gitdb::crypto::create_entropy_file(&dir_path).unwrap()
    };

    let sess = gitdb::Session {
        site_id: 0,
        master_key: kdf.master_key("super secret".as_bytes())
    };

    let git_root = dir_path.join("db");
    let db = gitdb::DB::init(&git_root).unwrap();
    assert!(git_root.is_dir());

    let key_salt_path = git_root.join("key_salt");
    assert!(!key_salt_path.is_file());

    db.create_key_salt(&sess).unwrap();
    assert!(key_salt_path.is_file());

    let key_salt = db.key_salt(&sess).unwrap();
    assert_eq!(key_salt.len(), 256/8);

    let db2 = gitdb::DB::init(&git_root).unwrap();
    assert_eq!(db2.key_salt(&sess).unwrap(), key_salt);
}

#[test]
fn read_write_read_block() {
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_owned();
    let git_root = dir_path.join("db");
    let db = gitdb::DB::init(&git_root).unwrap();

    let kdf = gitdb::crypto::KDF {
        pbkdf2_iters: 1000,
        salt: gitdb::crypto::rand_256().unwrap(),
        entropy: gitdb::crypto::create_entropy_file(&dir_path).unwrap()
    };

    let sess = gitdb::Session {
        site_id: 0,
        master_key: kdf.master_key("super secret".as_bytes())
    };
    
    let bob = User {
        name: ditto::Register::new("bob".into(), sess.site_id),
        age: ditto::Register::new(1f64.into(), sess.site_id)
    };

    db.create_key_salt(&sess).unwrap();
    let res = db.read_block("users@bob$name", &sess);
    assert!(res.is_err()); // key should not exist

    db.write("users@bob", &bob, &sess).unwrap();

    let bob_from_db = User::from_db("bob", &db, &sess).unwrap();
    assert_eq!(bob_from_db, bob);
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

    let db_a = gitdb::DB::init(&git_root_a).unwrap();
    let sess_a = gitdb::Session {
        site_id: 1,
        master_key: kdf.master_key("super secret".as_bytes())
    };

    let remote_url = format!("file://{}", remote_root.to_str().unwrap());
    let remote = gitdb::Remote::no_auth(
        "local_remote".into(),
        &remote_url,
        sess_a.site_id
    );

    db_a.create_key_salt(&sess_a).unwrap();
    db_a.write_remote(&remote, &sess_a).unwrap();
    db_a.sync(&sess_a).unwrap();

    let db_b = gitdb::DB::init_from_remote(&git_root_b, &remote).unwrap();

    let sess_b = gitdb::Session {
        site_id: 2,
        master_key: kdf.master_key("super secret".as_bytes())
    };

    assert_eq!(db_a.key_salt(&sess_a).unwrap(), db_b.key_salt(&sess_b).unwrap());

    // PRE SYNC:
    //   create A:users@sam
    //   create B:users@bob
    // POST SYNC:
    //   both sites A and B should have same sam and bob entries
    db_a.write(
        "users@sam",
        &User {
            name: ditto::Register::new("sam".into(), sess_a.site_id),
            age: ditto::Register::new(12.5.into(), sess_a.site_id)
        },
        &sess_a
    ).unwrap();
    
    db_b.write(
        "users@bob",
        &User {
            name: ditto::Register::new("bob".into(), sess_b.site_id),
            age: ditto::Register::new(11.25.into(), sess_b.site_id)
        },
        &sess_b
    ).unwrap();

    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();

    let sam_from_a = User::from_db("sam", &db_a, &sess_a).unwrap();
    let sam_from_b = User::from_db("sam", &db_b, &sess_b).unwrap();
    assert_eq!(sam_from_a, sam_from_b);

    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();
    let bob_from_a = User::from_db("bob", &db_a, &sess_a).unwrap();
    let bob_from_b = User::from_db("bob", &db_b, &sess_b).unwrap();
    assert_eq!(bob_from_a, bob_from_b);

    // PRE SYNC:
    //   create A:users@alice (with age 32)
    //   create B:users@alice (with age 32.5)
    // POST SYNC:
    //   both sites A and B should converge to the same alice age value
    db_a.write(
        "users@alice",
        &User {
            name: ditto::Register::new("alice".into(), sess_a.site_id),
            age: ditto::Register::new(32f64.into(), sess_a.site_id)
        },
        &sess_a
    ).unwrap();
    
    db_b.write(
        "users@alice",
        &User {
            name: ditto::Register::new("alice".into(), sess_b.site_id),
            age: ditto::Register::new(32.5.into(), sess_b.site_id)
        },
        &sess_b
    ).unwrap();
    
    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();
    db_a.sync(&sess_a).unwrap();

    {
        let alice_from_a = User::from_db("alice", &db_a, &sess_a).unwrap();
        let alice_from_b = User::from_db("alice", &db_b, &sess_b).unwrap();
        assert_eq!(alice_from_a, alice_from_b);

        let mut alice = User::from_db("alice", &db_b, &sess_b).unwrap();
        alice.age.update(33f64.into(), sess_b.site_id);
        db_b.write("users@alice", &alice, &sess_b).unwrap();
    }
    
    db_a.sync(&sess_a).unwrap();
    db_b.sync(&sess_b).unwrap();
    db_a.sync(&sess_a).unwrap();

    {
        let alice_from_a = User::from_db("alice", &db_a, &sess_a).unwrap();
        let alice_from_b = User::from_db("alice", &db_b, &sess_b).unwrap();
        assert_eq!(alice_from_a, alice_from_b);
        assert_eq!(alice_from_a.age.get(), &33f64.into());
    }
}
