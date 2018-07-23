extern crate gitdb;
extern crate tempfile;
extern crate crdts;
extern crate time;

#[macro_use]
extern crate assert_matches;

#[macro_use]
extern crate quickcheck;

use gitdb::{DB, Session, Dao};
use std::path::Path;

// #[derive(Debug, PartialEq)]
// struct User {
//     name: String,
//     age: f64
// }
// 
// impl Dao for User {
//     fn val(prefix: &str, db: &DB, sess: &Session) -> Result<Option<User>, gitdb::Error> {
//         let key = format!("{}$user", prefix);
//         let map = if let Some(block) = db.get(&key.into_bytes(), &sess)? {
//             block.to_map()?
//         } else {
//             return Ok(None);
//         };
// 
//         let name = if let Some(block) = map.get(&"name".as_bytes().to_vec()) {
//             block.to_reg()?.val.to_string()?
//         } else {
//             return Err(gitdb::Error::DaoField("name".into()))
//         };
// 
//         let age = if let Some(block) = map.get(&"age".as_bytes().to_vec()) {
//             block.to_reg()?.val.to_f64()?
//         } else {
//             return Err(gitdb::Error::DaoField("age".into()))
//         };
// 
//         Ok(Some(User { name, age }))
//     }
// 
//     fn update<F>(prefix: &str, db: &mut DB, sess: &Session, func: F) -> Result<(), gitdb::Error>
//         where F: FnOnce(Option<Self>) -> Option<Self>
//     {
//         
//         let key = format!("{}$user", prefix);
//         let user = User::val(&prefix, &db, &sess)?;
// 
//         if let Some(User { name, age }) = func(user) {
//             let time = time::get_time();
//             let dot = ((time.sec, time.nsec), sess.actor);
//             let map = if let Some(block) = db.get(&key.clone().into_bytes(), &sess)? {
//                 let mut map = block.to_map().unwrap(); // this should be safe since it's checked in val()
//                 map.update("name".as_bytes().to_vec(), |block| {
//                     let mut reg = block.unwrap().to_reg().unwrap(); // this should be safe
//                     reg.update(Prim::Str(name), dot).unwrap();
//                     Some(Block::Reg(reg))
//                 }, sess.actor);
// 
//                 map.update("age".as_bytes().to_vec(), |block| {
//                     let mut reg = block.unwrap().to_reg().unwrap(); // this should be safe
//                     reg.update(Prim::F64(age), dot).unwrap();
//                     Some(Block::Reg(reg))
//                 }, sess.actor);
//                 map
//             } else {
//                 let mut map = crdts::Map::new();
//                 map.insert("name".as_bytes().to_vec(), Block::Reg(crdts::LWWReg { val: Prim::Str(name), dot }), sess.actor);
//                 map.insert("age".as_bytes().to_vec(), Block::Reg(crdts::LWWReg { val: Prim::F64(age), dot }), sess.actor);
//                 map
//             };
//             db.set(key.into_bytes(), Block::Map(map), &sess)?;
//         } else {
//             db.del(&key.into_bytes(), &sess)?;
//         }
//         Ok(())
//     }
// }
// 
// #[test]
// fn init() {
//     let dir = tempfile::tempdir().unwrap();
//     let dir_path = dir.path().to_owned();
//     let git_root = dir_path.join("db");
//     DB::init(&git_root).unwrap();
//     assert!(git_root.is_dir());
// }
// 
// #[test]
// fn dao_read_write_read() {
//     let dir = tempfile::tempdir().unwrap();
//     let dir_path = dir.path().to_owned();
//     let git_root = dir_path.join("db");
//     let mut db = DB::init(&git_root).unwrap();
// 
//     let kdf = gitdb::crypto::KDF {
//         pbkdf2_iters: 1000,
//         salt: gitdb::crypto::rand_256().unwrap(),
//         entropy: gitdb::crypto::create_entropy_file(&dir_path).unwrap()
//     };
// 
//     let sess = Session {
//         actor: 0,
//         master_key: kdf.master_key("super secret".as_bytes())
//     };
// 
//     // key should not exist yet
//     assert_matches!(User::val("bob", &db, &sess), Ok(None));
// 
//     User::update("bob", &mut db, &sess, |user_opt| match user_opt {
//         Some(_) => panic!("we should not have any data yet"),
//         None => Some(User {
//             name: "Bob".to_string(),
//             age: 37.9
//         })
//     }).unwrap();
// 
//     let res = User::val("bob", &db, &sess);
//     assert_matches!(res, Ok(Some(_)));
//     let bob = res.unwrap().unwrap();
//     assert_eq!(
//         bob,
//         User {
//             name: "Bob".to_string(),
//             age: 37.9
//         }
//     );
// }
// 
// #[test]
// fn sync() {
//     use std::io::{Write, stdout};
//     stdout().flush().ok();
//     // let remote_root_dir = tempfile::tempdir().unwrap();
//     // let remote_root = remote_root_dir.path();
//     // let root_a_dir = tempfile::tempdir().unwrap();
//     // let root_a = root_a_dir.path();
//     // let root_b_dir = tempfile::tempdir().unwrap();
//     // let root_b = root_b_dir.path();
//     let remote_root: &Path = Path::new("/Users/davidrusu/gitdb/remote");
//     let root_a: &Path = Path::new("/Users/davidrusu/gitdb/a");
//     let root_b: &Path = Path::new("/Users/davidrusu/gitdb/b");
//     let git_root_a: &Path = &root_a.join("db");
//     let git_root_b: &Path = &root_b.join("db");
// 
//     gitdb::git2::Repository::init_bare(&remote_root).unwrap();
// 
//     let kdf = gitdb::crypto::KDF {
//         pbkdf2_iters: 1000,
//         salt: gitdb::crypto::rand_256().unwrap(),
//         entropy: gitdb::crypto::create_entropy_file(&remote_root).unwrap()
//     };
// 
//     let mut db_a = DB::init(&git_root_a).unwrap();
//     let sess_a = Session {
//         actor: 1,
//         master_key: kdf.master_key("super secret".as_bytes())
//     };
// 
//     let remote_url = format!("file://{}", remote_root.to_str().unwrap());
//     let remote = gitdb::Remote::no_auth("local_remote".into(), remote_url);
// 
// 
//     gitdb::Remote::update("db", &mut db_a, &sess_a, |block| match block {
//         None => {
//             Some(remote.clone())
//         },
//         Some(_) => panic!("No remotes should exist yet!")
//     }).unwrap();
// 
//     assert_matches!(db_a.sync(&sess_a), Ok(()));
// 
//     let mut db_b = DB::init_from_remote(&git_root_b, &remote).unwrap();
//     println!("finished initializing db_b from remote");
//     let sess_b = Session {
//         actor: 2,
//         master_key: kdf.master_key("super secret".as_bytes())
//     };
// 
//     // PRE SYNC:
//     //   create A:users@sam
//     //   create B:users@bob
//     // POST SYNC:
//     //   both sites A and B should have same sam and bob entries
//     
//     User::update("sam", &mut db_a, &sess_a, |val| match val {
//         Some(_) => panic!("we should not have any data yet"),
//         None => Some(User {
//             name: "Sam".to_string(),
//             age: 12.5
//         })
//     }).unwrap();
//     
//     User::update("bob", &mut db_b, &sess_b, |val| match val {
//         Some(_) => panic!("we should not have any data yet"),
//         None => Some(User {
//             name: "Bob".to_string(),
//             age: 11.25
//         })
//     }).unwrap();
// 
//     db_a.sync(&sess_a).unwrap();
//     db_b.sync(&sess_b).unwrap();
// 
//     let sam_from_a = User::val("sam", &db_a, &sess_a).unwrap().unwrap();
//     let sam_from_b = User::val("sam", &db_b, &sess_b).unwrap().unwrap();
//     assert_eq!(sam_from_a, sam_from_b);
// 
//     db_a.sync(&sess_a).unwrap();
//     db_b.sync(&sess_b).unwrap();
//     let bob_from_a = User::val("bob", &db_a, &sess_a).unwrap().unwrap();
//     let bob_from_b = User::val("bob", &db_b, &sess_b).unwrap().unwrap();
//     
//     assert_eq!(bob_from_a, bob_from_b);
// 
//     // PRE SYNC:
//     //   create A:users@alice (with age 32)
//     //   create B:users@alice (with age 32.5)
//     // POST SYNC:
//     //   both sites A and B should converge to the same alice age value
// 
//     User::update("alice", &mut db_a, &sess_a, |val| match val {
//         Some(_) => panic!("we should not have any data yet"),
//         None => Some(User {
//             name: "Alice".to_string(),
//             age: 32.
//         })
//     }).unwrap();
// 
//     User::update("alice", &mut db_b, &sess_b, |val| match val {
//         Some(_) => panic!("we should not have any data yet"),
//         None => Some(User {
//             name: "Alice".to_string(),
//             age: 32.5
//         })
//     }).unwrap();
//     
//     db_a.sync(&sess_a).unwrap();
//     db_b.sync(&sess_b).unwrap();
//     db_a.sync(&sess_a).unwrap();
// 
//     {
//         let alice_from_a = User::val("alice", &db_a, &sess_a).unwrap().unwrap();
//         let alice_from_b = User::val("alice", &db_b, &sess_b).unwrap().unwrap();
//         assert_eq!(alice_from_a, alice_from_b);
// 
//         User::update("alice", &mut db_b, &sess_b, |val| match val {
//             Some(User { name, age }) => Some(User { name, age: age + 0.5 }),
//             None => panic!("we should have data!")
//         }).unwrap();
//     }
//     
//     db_a.sync(&sess_a).unwrap();
//     db_b.sync(&sess_b).unwrap();
//     db_a.sync(&sess_a).unwrap();
// 
//     {
//         
//         let alice_from_a = User::val("alice", &db_a, &sess_a).unwrap().unwrap();
//         let alice_from_b = User::val("alice", &db_b, &sess_b).unwrap().unwrap();
// 
//         assert_eq!(alice_from_a, alice_from_b);
//         assert_eq!(alice_from_a.age, 33.);
//     }
// }
