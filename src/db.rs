// TODO: rename KEY_FILE to ENTROPY_FILE
extern crate time;
extern crate git2;
extern crate bincode;
extern crate ring;
extern crate sled;
extern crate crdts;

use self::git2::Repository;

use std;
use std::io::Write;
use std::path::Path;

use error::{Result, Error};
use crypto::Session;
use dao::Dao;
use remote::Remote;
use block::Block;
use git_helper;

pub struct DB {
    repo: Repository,
    tree: sled::Tree
}

#[derive(Serialize, Deserialize)]
struct Entry {
    // 128 bits is enough for using random actors
    clock: crdts::VClock<u128>,
    birth_clock: crdts::VClock<u128>,
    val: Block
}

impl DB {
    pub fn open(root: &Path) -> Result<DB> {
        let repo = Repository::open(&root)?;

        let config = sled::ConfigBuilder::new()
            .path(root)
            .build();
        let tree = sled::Tree::start(config)?;

        Ok(DB { repo, tree })
    }

    pub fn init(root: &Path) -> Result<DB> {
        eprintln!("initializing gitdb at {:?}", root);
        Repository::init(&root)?;
        DB::open(&root)
    }

    pub fn init_from_remote(root: &Path, remote: &Remote) -> Result<DB> {
        eprintln!("initializing from remote");
        let empty_repo = Repository::init(&root)?;
        git_helper::sync(
            &empty_repo,
            &remote,
            &mut |_, _| panic!("I should not be called!")
        )?;

        DB::open(&root)
    }

    fn bump_actor(&self, sess: &Session) -> Result<u64> {
        let mut clock = if let Some(b) = self.tree.get("db_clock".as_bytes())? {
            bincode::deserialize(&b)?
        } else {
            crdts::VClock::new()
        };
        let actor_version = clock.increment(sess.actor);
        self.tree.set("db_global_clock".as_bytes().to_vec(), bincode::serialize(&clock)?)?;
        Ok(actor_version)
    }

    pub fn get(&self, key: &[u8], _sess: &Session) -> Result<Option<Block>> {
        let res = if let Some(bytes) = self.tree.get(key)? {
            let entry: Entry = bincode::deserialize(&bytes)?;
            Some(entry.val)
        } else {
            None
        };

        Ok(res)
    }
    
    /// WARNING, `set` discards causality of the underlying CRDT
    ///          it also resets the birth_clock
    /// TODO: explain semantics of this method more
    pub fn set(&mut self, key: Vec<u8>, val: Block, sess: &Session) -> Result<()> {
        let actor_version = self.bump_actor(&sess)?;

        let mut clock = crdts::VClock::new();
        clock.witness(sess.actor, actor_version)?;

        let birth_clock = crdts::VClock::new();

        let entry = Entry { clock, birth_clock, val };

        let val_bytes = bincode::serialize(&entry)?;
        self.tree.set(key, val_bytes)?;
        self.tree.flush()?;

        git_helper::stage_globs(&self.repo, &["db", "conf", "snap.*"])?;
        Ok(())
    }

    pub fn del(&mut self, key: &[u8], sess: &Session) -> Result<Option<Block>> {
        self.bump_actor(&sess)?;
        
        let res = if let Some(bytes) = self.tree.del(key)? {
            let entry: Entry = bincode::deserialize(&bytes)?;
            Some(entry.val)
        } else {
            None
        };
        self.tree.flush()?;

        git_helper::stage_globs(&self.repo, &["db", "conf", "snap.*"])?;
        Ok(res)
    }

    // pub fn iter<'a>(&'a self, sess: &Session) -> std::iter::Map<sled::Iter<'a>, (Vec<u8>, Block)> {
    //     self.tree
    //         .iter()
    //         .map(|res: sled::DbResult<(Vec<u8>, Vec<u8>), ()>| -> (Vec<u8>, Block) {
    //             let (key, val_bytes) = res.unwrap();
    //             let entry: Entry = bincode::deserialize(&val_bytes).unwrap();
    //             (key, entry.val)
    //         })
    // }

    pub fn sync(&mut self, sess: &Session) -> Result<()> {
        self.tree.flush()?;

        let remote: Remote = Dao::val("db", self, &sess)
            ?.ok_or(Error::NoRemote)?;

        let mut merger = &mut |delta: git2::DiffDelta, sim: f32| {
            eprintln!(
                "delta! {:?} {:?} {:?} {}",
                delta.status(),
                delta.old_file().path(),
                delta.new_file().path(),
                sim
            );

            match delta.status() {
                git2::Delta::Modified => {
                    eprintln!("both files modified");
                    let old = delta.old_file();
                    let new = delta.new_file();
                    self.merge_mod_files(&old, &new, &sess).is_ok()
                },
                git2::Delta::Added => {
                    // this file was added locally
                    true
                },
                git2::Delta::Deleted => {
                    // remote additions are seen as deletions from the other tree
                    eprintln!("remote added a file");
                    self.merge_add_file(&delta.old_file()).is_ok()
                }
                _ => unimplemented!()
            }
        };

        git_helper::sync(&self.repo, &remote, &mut merger)
    }

    fn write_file(path: &Path, data: &[u8]) -> Result<()> {
        match path.parent() {
            Some(parent) =>
                std::fs::create_dir_all(&parent),
            None => Ok(()) // no parent to create
        }?;

        // File::create will replace existing files
        let mut f = std::fs::File::create(&path)?;

        match f.write_all(&data) {
            Err(e) => {
                std::fs::remove_file(&path)?;
                Err(Error::IO(e))
            },
            _ => Ok(())
        }
    }

    fn merge_add_file(&self, new: &git2::DiffFile) -> Result<()> {
        let rel_path = new.path()
            .ok_or_else(|| Error::State("added file doesn't have a path!?".into()))?;

        // we expect to be in a non-bare repo so a workdir must exist!
        let path = self.repo.workdir().unwrap();
        
        eprintln!("merging added file {:?}", rel_path);

        let new_blob = self.repo.find_blob(new.id())?;
        
        DB::write_file(&path.join(rel_path), new_blob.content())?;

        eprintln!("wrote added file to workdir");
        
        git_helper::stage_file(&self.repo, &rel_path)?;
        Ok(())
    }

    fn merge_mod_files(&self, _old: &git2::DiffFile, _new: &git2::DiffFile, _sess: &Session) -> Result<()> {
        unimplemented!();
        // let rel_path = old.path()
        //     .ok_or_else(|| Error::State("old file doesn't have a path!?".into()))?;
        // 
        // eprintln!("merging {:?}", rel_path);
        // let old_blob = self.repo.find_blob(old.id())?;
        // let new_blob = self.repo.find_blob(new.id())?;
        // 
        // self.write_file(&rel_path, &bincode::deserialize(&encrypted)?)?;
        // git_helper::stage_file(&self.repo, &rel_path)?;
        // 
        // Ok(())
    }
}
