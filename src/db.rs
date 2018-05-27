// TODO: rename KEY_FILE to ENTROPY_FILE
extern crate time;
extern crate git2;
extern crate rmp_serde;
extern crate ditto;
extern crate ring;

use self::git2::{Repository, Commit};

use std::path::{PathBuf, Path};

use db_error::{Result, DBErr};
use crypto::{Session, Plaintext, Encrypted, Config, gen_rand_256};
use remote::Remote;
use block::{Block, Blockable};
use encoding;

pub struct DB {
    pub root: PathBuf,
    pub repo: Repository
}

mod git_helper {
    use super::*;

    pub fn fetch<'a>(repo: &'a Repository, remote: &Remote) -> Result<git2::Remote<'a>> {
        println!("fetching remote {}", &remote.name());

        let mut git_remote = match repo.find_remote(&remote.name()) {
            Ok(git_remote) => git_remote,
            Err(_) => {
                // does not exist, we add this remote to git
                repo.remote(&remote.name(), &remote.url())?
            }
        };

        let mut fetch_opt = git2::FetchOptions::new();
        fetch_opt.remote_callbacks(remote.git_callbacks());

        git_remote.fetch(&["master"], Some(&mut fetch_opt), None)?;
        Ok(git_remote)
    }

    pub fn commit(repo: &Repository, msg: &str, extra_parents: &[&Commit]) -> Result<()> {
        println!("committing");

        let mut index = repo.index()?;
        let tree = index.write_tree()
            .and_then(|tree_oid| repo.find_tree(tree_oid))?;
        
        let parent: Option<Commit> = match repo.head() {
            Ok(head_ref) => {
                let head_oid = head_ref.target()
                    .ok_or(DBErr::State(format!("Failed to find oid referenced by HEAD")))?;
                let head_commit = repo.find_commit(head_oid)?;
                Some(head_commit)
            },
            Err(_) => None // initial commit (no parent)
        };

        match parent {
            Some(ref commit) => {
                let prev_tree = commit.tree()?;
                let stats = repo.diff_tree_to_tree(Some(&tree), Some(&prev_tree), None)?.stats()?;
                if stats.files_changed() == 0 {
                    println!("aborting commit, no files changed");
                    return Ok(())
                }
            },
            None => {
                if index.is_empty() {
                    println!("aborting commit, Index is empty, nothing to commit");
                    return Ok(());
                }
            }
        }

        let sig = repo.signature()?;

        let mut parent_commits = Vec::new();
        if let Some(ref commit) = parent {
            parent_commits.push(commit)
        }
        parent_commits.extend(extra_parents);
        
        repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &parent_commits)?;
        Ok(())
    }

    pub fn stage_file(repo: &Repository, file: &Path) -> Result<()> {
        let mut index = repo.index()?;
        index.add_path(&file)?;
        index.write()?;
        Ok(())
    }

    pub fn fast_forward(repo: &Repository, branch: &git2::Branch) -> Result<()> {
        println!("fast forwarding repository to match branch {:?}", branch.name()?);
        let remote_commit_oid = branch.get().resolve()?.target()
            .ok_or(DBErr::State("remote ref didn't resolve to commit".into()))?;

        let remote_commit = repo.find_commit(remote_commit_oid)?;

        if let Ok(branch) = repo.find_branch("master", git2::BranchType::Local) {
            let mut branch_ref = &mut branch.into_reference();
            branch_ref.set_target(remote_commit_oid, "fast forward")?;
        } else {
            println!("creating local master branch");
            repo.branch("master", &remote_commit, false)?;
        }
        repo.set_head("refs/heads/master")?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))?;
        Ok(())
    }

    pub fn sync<'a>(repo: &Repository, remote: &Remote, mut merger: &mut (FnMut(git2::DiffDelta, f32) -> bool + 'a)) -> Result<()> {
        // we assume all files to be synced have already been added to the index
        git_helper::commit(&repo, "sync commit from site", &[])?;

        // fetch and merge
        let mut git_remote = git_helper::fetch(&repo, &remote)?;

        println!("searching for remote master branch");
        let remote_master_ref = format!("{}/master", &remote.name());
        if let Ok(branch) = repo.find_branch(&remote_master_ref, git2::BranchType::Remote) {
            println!("found remote master branch");
            let remote_commit_oid = branch.get().resolve()?.target()
                .ok_or(DBErr::State("remote ref didn't resolve to commit".into()))?;

            let remote_annotated_commit = repo.find_annotated_commit(remote_commit_oid)?;

            let (analysis, _) = repo.merge_analysis(&[&remote_annotated_commit])?;

            use self::git2::MergeAnalysis;
            if analysis == MergeAnalysis::ANALYSIS_NORMAL {
                let remote_commit = repo.find_commit(remote_commit_oid)?;
                let remote_tree = remote_commit.tree()?;

                // now the tricky part, detecting and handling conflicts
                // we want to merge the local tree with the remote_tree

                // TODO: see if there are any diff options we can use to speed up the diff
                let diff = repo.diff_tree_to_index(Some(&remote_tree), None, None)?;
                println!("iterating foreach");
                diff.foreach(&mut merger, None, None, None)?;
                git_helper::commit(&repo, "merge commit", &[&remote_commit])?;
            } else if analysis.contains(MergeAnalysis::ANALYSIS_FASTFORWARD) {
                git_helper::fast_forward(&repo, &branch)?;
            } else if analysis == git2::MergeAnalysis::ANALYSIS_UP_TO_DATE {
                println!("nothing to merge, ahead of remote");
            } else {
                return Err(DBErr::State(format!("Bad merge analysis result: {:?}", analysis)));
            }
        }
        
        println!("pushing git_remote");
        let mut push_opt = git2::PushOptions::new();
        push_opt.remote_callbacks(remote.git_callbacks());
        git_remote.push(&[&"refs/heads/master"], Some(&mut push_opt))?;
        println!("Finish push");
        
        // TAI: should return stats struct
        Ok(())
    }
}

impl DB {
    pub fn init(root: &Path, mut sess: &mut Session) -> Result<DB> {
        println!("initializing gitdb at {:?}", root);
        let repo = Repository::open(&root)
            .or_else(|_| Repository::init(&root))?;

        let db = DB {
            root: root.to_path_buf(),
            repo: repo
        };

        db.create_key_salt(&mut sess)?;
        Ok(db)
    }

    pub fn init_from_remote(root: &Path, remote: &Remote, mut sess: &mut Session) -> Result<DB> {
        println!("initializing from remote");
        let empty_repo = Repository::init(&root)?;
        git_helper::sync(&empty_repo, &remote, &mut |_, _| false)?;

        let db = DB::init(&root, &mut sess)?;
        db.write_remote(&remote, &mut sess)?;
        Ok(db)
    }

    fn create_key_salt(&self, mut sess: &mut Session) -> Result<()> {
        println!("creating key salt");
        let key_salt = Path::new("key_salt");
        let key_salt_filepath = self.root.join(&key_salt);

        if !key_salt_filepath.exists() {
            println!("key salt file {:?} not found so creating new one", key_salt_filepath);
            let salt = gen_rand_256()?;

            Plaintext {
                data: salt.to_vec(),
                config: Config::fresh_default()?
            }.encrypt(&mut sess)?.write(&key_salt_filepath)?;

            git_helper::stage_file(&self.repo, &key_salt)?;
        }

        Ok(())
    }

    fn key_salt(&self, mut sess: &mut Session) -> Result<Vec<u8>> {
        println!("fetching key_salt");
        let key_salt_file = self.root.join("key_salt");
        let key_salt = Encrypted::read(&key_salt_file)
            ?.decrypt(&mut sess)
            ?.data;

        Ok(key_salt)
    }

    fn derive_key_filepath(&self, key: &str, mut sess: &mut Session) -> Result<PathBuf> {
        let key_salt = self.key_salt(&mut sess)?;
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        ctx.update(&key_salt);
        // TAI: consider avoiding building the path string here
        //      we should be able to update the ctx with path components
        ctx.update(key.as_bytes());
        let digest = ctx.finish();
        let encoded_hash = encoding::encode(&digest.as_ref());
        let (dir_part, file_part) = encoded_hash.split_at(2);
        let filepath = PathBuf::from(dir_part)
            .join(file_part);

        Ok(filepath)
    }

    pub fn read_block(&self, key: &str, mut sess: &mut Session) -> Result<Block> {
        let block_filepath = self.root
            .join("cryptic")
            .join(&self.derive_key_filepath(&key, &mut sess)?);

        println!("read_block {}\n\t{:?}", key, block_filepath);
        if block_filepath.exists() {
            let plaintext = Encrypted::read(&block_filepath)?.decrypt(&mut sess)?;
            let block_reg: ditto::Register<Block> = rmp_serde::from_slice(&plaintext.data)?;
            Ok(block_reg.get().to_owned())
        } else {
            Err(DBErr::NotFound)
        }
    }

    pub fn write_block(&self, key: &str, block: &Block, mut sess: &mut Session) -> Result<()> {
        if key.len() == 0 {
            return Err(DBErr::State("Attempting to write empty key to root path".into()));
        }

        let rel_path = Path::new("cryptic")
            .join(self.derive_key_filepath(&key, &mut sess)?);
        let block_filepath = self.root
            .join(&rel_path);

        println!("write_block {}\n\t{:?}", key, rel_path);

        let register = if block_filepath.exists() {
            let plaintext = Encrypted::read(&block_filepath)?.decrypt(&mut sess)?;

            let mut existing_reg: ditto::Register<Block> = rmp_serde::from_slice(&plaintext.data)?;
            let mut existing_block = existing_reg.clone().get().to_owned();

            let new_block: Block = match existing_block.merge(&block) {
                Ok(()) => Ok(existing_block),
                Err(DBErr::BlockTypeConflict) => Ok(block.clone()),
                Err(e) => Err(e)
            }?;

            existing_reg.update(new_block, sess.site_id);
            existing_reg
        } else {
            ditto::Register::new(block.clone(), sess.site_id)
        };
        
        Plaintext {
            data: rmp_serde::to_vec(&register)?,
            config: Config::fresh_default()?
        }.encrypt(&mut sess)?.write(&block_filepath)?;

        git_helper::stage_file(&self.repo, &rel_path)?;
        Ok(())
    }

    pub fn write(&self, prefix: &str, data: &impl Blockable, mut sess: &mut Session) -> Result<()> {
        for (suffix, block) in data.blocks().into_iter() {
            let mut key = String::with_capacity(prefix.len() + suffix.len());
            key.push_str(&prefix);
            key.push_str(&suffix);

            self.write_block(&key, &block, &mut sess)?;
        }
        Ok(())
    }

    pub fn sync(&self, mut sess: &mut Session) -> Result<()> {
        let remote = self.read_remote(&mut sess)?;
        let mut merger = &mut |delta: git2::DiffDelta, sim: f32| {
            println!(
                "delta! {:?} {:?} {:?} {}",
                delta.status(),
                delta.old_file().path(),
                delta.new_file().path(),
                sim
            );

            match delta.status() {
                git2::Delta::Modified => {
                    println!("both files modified");
                    let old = delta.old_file();
                    let new = delta.new_file();
                    self.merge_mod_files(&old, &new, &mut sess).is_ok()
                },
                git2::Delta::Added => {
                    // this file was added locally
                    true
                },
                git2::Delta::Deleted => {
                    // remote additions are seen as deletions from the other tree
                    println!("remote added a file");
                    self.merge_add_file(&delta.old_file()).is_ok()
                }
                _ => unimplemented!()
            }
        };

        git_helper::sync(&self.repo, &remote, &mut merger)?;
        Ok(())
    }

    fn merge_add_file(&self, new: &git2::DiffFile) -> Result<()> {
        let rel_path = new.path()
            .ok_or_else(|| DBErr::State("added file doesn't have a path!?".into()))?;
        let filepath = self.root.join(&rel_path);

        println!("merging added file {:?}", rel_path);

        let new_blob = self.repo.find_blob(new.id())?;
        Encrypted::from_bytes(&new_blob.content())?.write(&filepath)?;
        
        println!("wrote added file to workdir");
        git_helper::stage_file(&self.repo, &rel_path)?;
        Ok(())
    }

    fn merge_mod_files(&self, old: &git2::DiffFile, new: &git2::DiffFile, mut sess: &mut Session) -> Result<()> {
        let rel_path = old.path()
            .ok_or_else(|| DBErr::State("old file doesn't have a path!?".into()))?;
        let filepath = self.root.join(&rel_path);

        println!("merging {:?}", rel_path);
        let old_blob = self.repo.find_blob(old.id())?;
        let new_blob = self.repo.find_blob(new.id())?;
        let old_cryptic = old_blob.content();
        let new_cryptic = new_blob.content();

        let old_plain = Encrypted::from_bytes(&old_cryptic)?.decrypt(&mut sess)?;
        let new_plain = Encrypted::from_bytes(&new_cryptic)?.decrypt(&mut sess)?;

        println!("decrypted old and new");
        
        let mut old_reg: ditto::Register<Block> = rmp_serde::from_slice(&old_plain.data)?;

        let new_reg: ditto::Register<Block> = rmp_serde::from_slice(&new_plain.data)?;

        println!("parsed old and new registers");

        let mut old_block = old_reg.clone().get().to_owned();
        let new_block = new_reg.clone().get().to_owned();

        let merged_block = match old_block.merge(&new_block) {
            Ok(()) => Ok(old_block.to_owned()),
            Err(DBErr::BlockTypeConflict) => Ok(new_block),
            Err(e) => Err(e)
        }?;

        old_reg.merge(&new_reg);
        old_reg.update(merged_block, sess.site_id);
        
        Plaintext {
            data: rmp_serde::to_vec(&old_reg)?,
            config: Config::fresh_default()?
        }.encrypt(&mut sess)?.write(&filepath)?;

        git_helper::stage_file(&self.repo, &rel_path)?;

        Ok(())
    }

    pub fn read_remote(&self, mut sess: &mut Session) -> Result<Remote> {
        Remote::from_db("db$config$remote", &self, &mut sess)
    }

    pub fn write_remote(&self, remote: &Remote, mut sess: &mut Session) -> Result<()> {
        // TODO: remove the other remote before writing, read has a noauth bias
        // TODO: https://docs.rs/git2/0.7.0/git2/struct.Remote.html#method.is_valid_name
        self.write("db$config$remote", remote, &mut sess)
    }
}

#[cfg(test)]
mod test {
    extern crate tempfile;
    extern crate ditto;

    use self::ditto::register::Register;
    use super::*;

    use block::Prim;
    
    #[derive(Debug, PartialEq)]
    struct User {
        name: Register<Prim>,
        age: Register<Prim>
    }

    impl Blockable for User {
        fn blocks(&self) -> Vec<(String, Block)> {
            vec![
                ("$name".into(), Block::Val(self.name.clone())),
                ("$age".into(), Block::Val(self.age.clone())),
            ]
        }
    }

    impl User {
        fn from_db(user_key: &str, db: &DB, mut sess: &mut Session) -> Result<Self> {
            let name_key = format!("users@{}$name", user_key);
            let age_key = format!("users@{}$age", user_key);

            let name = db.read_block(&name_key, &mut sess)?.to_val()?;
            let age = db.read_block(&age_key, &mut sess)?.to_val()?;

            Ok(User {
                name: name,
                age: age
            }) 
        }
    }

    #[test]
    fn init() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), 0);
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = dir.path().join("db");

        let db = DB::init(&git_root, &mut sess).unwrap();
        assert!(git_root.is_dir());

        let key_salt_path = git_root.join("key_salt");
        assert!(key_salt_path.is_file());
        
        let key_salt = db.key_salt(&mut sess).unwrap();
        assert_eq!(key_salt.len(), 256/8);

        let db2 = DB::init(&git_root, &mut sess).unwrap();
        assert_eq!(db2.key_salt(&mut sess).unwrap(), key_salt);
    }

    #[test]
    fn key_salt_used_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), 0);
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = &dir.path().join("db");

        let db = DB::init(git_root, &mut sess).unwrap();
        // fix the path salt to "$"
        let encrypted = Plaintext {
            data: "$".as_bytes().to_vec(),
            config: Config::fresh_default().unwrap()
        }.encrypt(&mut sess).unwrap();
            
        encrypted.write(&db.root.join("key_salt"))
            .unwrap();

        let key_salt = db.key_salt(&mut sess).unwrap();
        assert_eq!(key_salt, "$".as_bytes());
        let filepath = db.derive_key_filepath("/a/b/c", &mut sess).unwrap();

        //test vector comes from the python code:
        //>>> import hashlib
        //>>> hashlib.sha256(b"$/a/b/c").hexdigest()
        //'63b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79'
        let expected = PathBuf::from("63")
            .join("b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79");
        assert_eq!(filepath, expected);
    }

    #[test]
    fn read_write_read_block() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), 1);
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = &dir.path().join("db");

        let db = DB::init(git_root, &mut sess).unwrap();

        let bob = User {
            name: Register::new("bob".into(), sess.site_id),
            age: Register::new(1f64.into(), sess.site_id)
        };

        let res = db.read_block("users@bob$name", &mut sess);
        assert!(res.is_err()); // key should not exist
        
        db.write("users@bob", &bob, &mut sess).unwrap();

        let bob_from_db = User::from_db("bob", &db, &mut sess).unwrap();
        assert_eq!(bob_from_db, bob);
    }

    #[test]
    fn sync() {
        use std::io::{Write, stdout};
        println!("Running sync");
        stdout().flush().ok();
        let remote_root_dir = tempfile::tempdir().unwrap();
        let remote_root = remote_root_dir.path();
        let root_a_dir = tempfile::tempdir().unwrap();
        let root_a = root_a_dir.path();
        let root_b_dir = tempfile::tempdir().unwrap();
        let root_b = root_b_dir.path();
        let git_root_a = root_a.join("db");
        let git_root_b = root_b.join("db");

        println!("created temp dirs");

        Repository::init_bare(&remote_root).unwrap();
        
        let mut sess_a = Session::new(&root_a, 1);
        let mut sess_b = Session::new(&root_b, 2);
        println!("created sessions");
        sess_a.create_key_file().unwrap();

        println!("created key_file_a");
        {
            // copy the key_file to the root_b
            let key_file = sess_a.key_file().unwrap();
            use std::fs::File;
            let mut f2 = File::create(&root_b.join("key_file")).unwrap();
            use std::io::Write;
            f2.write_all(&key_file).unwrap();

            assert_eq!(sess_a.key_file().unwrap(), sess_b.key_file().unwrap());
        }

        println!("copied to key_file_b");
        sess_a.set_pass("secret_pass".as_bytes());
        sess_b.set_pass("secret_pass".as_bytes());

        let db_a = DB::init(&git_root_a, &mut sess_a).unwrap();

        let remote_url = format!("file://{}", remote_root.to_str().unwrap());
        let remote = Remote::no_auth(
            "local_remote".into(),
            &remote_url,
            sess_a.site_id
        );
        
        println!("remote url: '{}'", remote_url);
        db_a.write_remote(&remote, &mut sess_a).unwrap();
        db_a.sync(&mut sess_a).unwrap();

        println!("Finished init of a, a is synced with remote");

        let db_b = DB::init_from_remote(&git_root_b, &remote, &mut sess_b).unwrap();

        assert_eq!(db_a.key_salt(&mut sess_a).unwrap(), db_b.key_salt(&mut sess_b).unwrap());
        
        println!("both db's are initted");
        println!("initial sync");

        // PRE:
        //   create A:users@sam
        //   create B:users@bob
        // POST:
        //   both sites A and B should have same sam and bob entries
        db_a.write(
            "users@sam",
            &User {
                name: Register::new("sam".into(), sess_a.site_id),
                age: Register::new(12.5.into(), sess_a.site_id)
            },
            &mut sess_a
        ).unwrap();
        
        db_b.write(
            "users@bob",
            &User {
                name: Register::new("bob".into(), sess_b.site_id),
                age: Register::new(11.25.into(), sess_b.site_id)
            },
            &mut sess_b
        ).unwrap();

        db_a.sync(&mut sess_a).unwrap();
        db_b.sync(&mut sess_b).unwrap();
        println!("second sync");

        let sam_from_a = User::from_db("sam", &db_a, &mut sess_a).unwrap();
        let sam_from_b = User::from_db("sam", &db_b, &mut sess_b).unwrap();
        assert_eq!(sam_from_a, sam_from_b);

        db_a.sync(&mut sess_a).unwrap();
        db_b.sync(&mut sess_b).unwrap();
        let bob_from_a = User::from_db("bob", &db_a, &mut sess_a).unwrap();
        let bob_from_b = User::from_db("bob", &db_b, &mut sess_b).unwrap();
        assert_eq!(bob_from_a, bob_from_b);

        // PRE:
        //   create A:users@alice (with age 32)
        //   create B:users@alice (with age 32.5)
        // POST:
        //   both sites A and B should converge to the same alice age value
        db_a.write(
            "users@alice",
            &User {
                name: Register::new("alice".into(), sess_a.site_id),
                age: Register::new(32f64.into(), sess_a.site_id)
            },
            &mut sess_a
        ).unwrap();
        
        db_b.write(
            "users@alice",
            &User {
                name: Register::new("alice".into(), sess_b.site_id),
                age: Register::new(32.5.into(), sess_b.site_id)
            },
            &mut sess_b
        ).unwrap();
        
        db_a.sync(&mut sess_a).unwrap();
        db_b.sync(&mut sess_b).unwrap();
        db_a.sync(&mut sess_a).unwrap();

        {
            let alice_from_a = User::from_db("alice", &db_a, &mut sess_a).unwrap();
            let alice_from_b = User::from_db("alice", &db_b, &mut sess_b).unwrap();
            assert_eq!(alice_from_a, alice_from_b);

            let mut alice = User::from_db("alice", &db_b, &mut sess_b).unwrap();
            alice.age.update(33f64.into(), sess_b.site_id);
            db_b.write("users@alice", &alice, &mut sess_b).unwrap();
        }
        
        db_a.sync(&mut sess_a).unwrap();
        db_b.sync(&mut sess_b).unwrap();
        db_a.sync(&mut sess_a).unwrap();

        {
            let alice_from_a = User::from_db("alice", &db_a, &mut sess_a).unwrap();
            let alice_from_b = User::from_db("alice", &db_b, &mut sess_b).unwrap();
            assert_eq!(alice_from_a, alice_from_b);
            assert_eq!(alice_from_a.age.get(), &33f64.into());
        }
    }
}
