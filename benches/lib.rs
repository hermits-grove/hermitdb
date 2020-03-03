#![feature(test)]
extern crate test;

extern crate gitdb;
extern crate tempfile;

// #[bench]
// fn time_to_first_write(b: &mut test::Bencher) {
//     b.iter(|| {
//         let dir = tempfile::tempdir().unwrap();
//         let dir_path = dir.path().to_owned();
//         let git_root = dir_path.join("db");
//         let db = gitdb::DB::init(&git_root).unwrap();
//
//         let kdf = gitdb::crypto::KDF {
//             pbkdf2_iters: 100_000,
//             salt: gitdb::crypto::rand_256().unwrap(),
//             entropy: gitdb::crypto::create_entropy_file(&dir_path).unwrap()
//         };
//
//         let sess = gitdb::Session {
//             site_id: 0,
//             master_key: kdf.master_key(b"super secret")
//         };
//
//         db.create_key_salt(&sess).unwrap();
//
//         let val = gitdb::ditto::Register::new(12.into(), sess.site_id);
//         db.write_block("val", &gitdb::Block::Val(val), &sess).unwrap();
//     })
// }
//
// #[bench]
// fn write_100_blocks(b: &mut test::Bencher) {
//     let kdf = gitdb::crypto::KDF {
//         pbkdf2_iters: 100_000,
//         salt: gitdb::crypto::rand_256().unwrap(),
//         entropy: gitdb::crypto::rand_256().unwrap()
//     };
//
//     let sess = gitdb::Session {
//         site_id: 0,
//         master_key: kdf.master_key(b"super secret")
//     };
//
//     b.iter(|| {
//         let dir = tempfile::tempdir().unwrap();
//         let dir_path = dir.path().to_owned();
//         let git_root = dir_path.join("db");
//         let db = gitdb::DB::init(&git_root).unwrap();
//         db.create_key_salt(&sess).unwrap();
//
//         for i in 0..100 {
//             let val = gitdb::ditto::Register::new(i.into(), sess.site_id);
//             db.write_block(&format!("val#{}", i), &gitdb::Block::Val(val), &sess).unwrap();
//         }
//     })
// }
//
// #[bench]
// fn read_block(b: &mut test::Bencher) {
//     let kdf = gitdb::crypto::KDF {
//         pbkdf2_iters: 100_000,
//         salt: gitdb::crypto::rand_256().unwrap(),
//         entropy: gitdb::crypto::rand_256().unwrap()
//     };
//
//     let sess = gitdb::Session {
//         site_id: 0,
//         master_key: kdf.master_key(b"super secret")
//     };
//
//     let dir = tempfile::tempdir().unwrap();
//     let dir_path = dir.path().to_owned();
//     let git_root = dir_path.join("db");
//     let db = gitdb::DB::init(&git_root).unwrap();
//     db.create_key_salt(&sess).unwrap();
//
//     let val = gitdb::ditto::Register::new(37.into(), sess.site_id);
//     db.write_block("val_key", &gitdb::Block::Val(val), &sess).unwrap();
//
//     b.iter(|| {
//         db.read_block("val_key", &sess).unwrap();
//     })
// }
//
// #[bench]
// fn read_1000_blocks(b: &mut test::Bencher) {
//     let kdf = gitdb::crypto::KDF {
//         pbkdf2_iters: 100_000,
//         salt: gitdb::crypto::rand_256().unwrap(),
//         entropy: gitdb::crypto::rand_256().unwrap()
//     };
//
//     let sess = gitdb::Session {
//         site_id: 0,
//         master_key: kdf.master_key(b"super secret")
//     };
//
//     let dir = tempfile::tempdir().unwrap();
//     let dir_path = dir.path().to_owned();
//     let git_root = dir_path.join("db");
//     let db = gitdb::DB::init(&git_root).unwrap();
//     db.create_key_salt(&sess).unwrap();
//
//     for i in 0..1000 {
//         let val = gitdb::ditto::Register::new(i.into(), sess.site_id);
//         db.write_block(&format!("val#{}", i), &gitdb::Block::Val(val), &sess).unwrap();
//     }
//
//     b.iter(|| {
//         for i in 0..1000 {
//             let block = db.read_block(&format!("val#{}", i), &sess).unwrap();
//             assert_eq!(block.to_val().unwrap().get(), &gitdb::Prim::U64(i));
//         }
//     })
// }
//
//
// #[bench]
// fn prefix_scan_1000_blocks(b: &mut test::Bencher) {
//     let kdf = gitdb::crypto::KDF {
//         pbkdf2_iters: 100_000,
//         salt: gitdb::crypto::rand_256().unwrap(),
//         entropy: gitdb::crypto::rand_256().unwrap()
//     };
//
//     let sess = gitdb::Session {
//         site_id: 0,
//         master_key: kdf.master_key(b"super secret")
//     };
//
//     let dir = tempfile::tempdir().unwrap();
//     let dir_path = dir.path().to_owned();
//     let git_root = dir_path.join("db");
//     let db = gitdb::DB::init(&git_root).unwrap();
//     db.create_key_salt(&sess).unwrap();
//
//     for i in 0..1000 {
//         let val = gitdb::ditto::Register::new(gitdb::Prim::U64(i), sess.site_id);
//         db.write_block(&format!("val#{}", "x".repeat(i as usize)), &gitdb::Block::Val(val), &sess).unwrap();
//     }
//
//     b.iter(|| {
//         let mut i: u64 = 0;
//         for (_key, block) in db.prefix_scan("val", &sess).unwrap() {
//             assert_eq!(block.to_val().unwrap().get(), &gitdb::Prim::U64(i));
//             i += 1;
//         }
//
//         assert_eq!(i, 1000);
//     })
// }
//
