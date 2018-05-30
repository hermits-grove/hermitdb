// TODO: rename KEY_FILE to ENTROPY_FILE
extern crate time;
extern crate git2;
extern crate rmp_serde;
extern crate ditto;
extern crate ring;

use self::git2::Repository;

use std;
use std::io::{Read, Write};
use std::path::{PathBuf, Path};

use error::{Result, Error};
use crypto::{Session, Plaintext, Encrypted, rand_256};
use remote::Remote;
use block::{Block, Blockable};
use encoding;
use git_helper;

pub struct DB {
    root: PathBuf,
    repo: Repository
}

#[derive(Serialize, Deserialize)]
struct Entry {
    key: String,
    val: Vec<u8>
}

impl DB {
    pub fn init(root: &Path) -> Result<DB> {
        println!("initializing gitdb at {:?}", root);
        let repo = Repository::open(&root)
            .or_else(|_| Repository::init(&root))?;

        let db = DB {
            root: root.to_path_buf(),
            repo: repo
        };

        Ok(db)
    }

    pub fn init_from_remote(root: &Path, remote: &Remote) -> Result<DB> {
        println!("initializing from remote");
        let empty_repo = Repository::init(&root)?;
        git_helper::sync(&empty_repo, &remote, &mut |_, _| false)?;

        DB::init(&root)
    }

    pub fn read_file(&self, rel_path: &Path) -> Result<Vec<u8>> {
        let path = self.root.join(&rel_path);
        if path.is_file() {
            let mut f = std::fs::File::open(path)?;
            let mut data = Vec::new();
            f.read_to_end(&mut data)?;
            Ok(data)
        } else {
            Err(Error::NotFound)
        }
    }

    pub fn write_file(&self, rel_path: &Path, data: &[u8]) -> Result<()> {
        let path = self.root.join(&rel_path);
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

    pub fn create_key_salt(&self, sess: &Session) -> Result<()> {
        println!("creating key salt");
        let path = Path::new("key_salt");

        match self.read_file(&path) {
            Ok(_) => return Err(Error::State("Attempting to create key_salt when one exists".into())),
            _ => {
                let salt = rand_256()?;
                let encrypted = Plaintext(salt.to_vec()).encrypt(&sess)?;
                self.write_file(&path, &rmp_serde::to_vec(&encrypted)?)?;
                git_helper::stage_file(&self.repo, &path)?;
            }
        }
        Ok(())
    }

    pub fn key_salt(&self, sess: &Session) -> Result<Vec<u8>> {
        println!("reading key_salt");
        let data = self.read_file(&Path::new("key_salt"))?;
        let encrypted: Encrypted = rmp_serde::from_slice(&data)?;
        let key_salt = encrypted.decrypt(&sess)?.0;
        Ok(key_salt)
    }

    pub fn create_salt(&self) -> Result<()> {
        self.write_file(&Path::new("salt"), &rand_256()?)
    }

    pub fn salt(&self) -> Result<[u8; 256/8]>{
        let bytes = self.read_file(&Path::new("salt"))?;
        if bytes.len() != 256 / 8 {
            return Err(Error::State("salt must be 256 bits".into()));
        }
        let mut salt = [0u8; 256/8];
        for i in 0..(256/8) {
            salt[i] = bytes[i];
        }

        Ok(salt)
    }

    /// Each key/value pair maps to a single file on disk
    /// We preserve key confidentiality by mixing a key_salt with the key through a
    /// cryptographically secure hash, the key_salt must have enough entropy to
    /// protect from a bruteforce attack
    /// 
    /// For inputs key: "users#spud", key_salt: "@@@"
    /// the filepath derivation is equivalent to the python snippet:
    /// ``` python
    /// import os
    /// import hashlib
    /// hash = hashlib.sha256(b"@@@users#spud").hexdigest()
    /// dir_part, file_part = hash[:2], hash[2:]
    /// os.path.join(dir_part, file_part)
    /// # => '85/c91e65b03e0f0e5a9250f875188567ad6b2dece10e2635f8f72934ea9fca18'
    /// ```
    pub fn derive_key_filepath(&self, key: &str, sess: &Session) -> Result<PathBuf> {
        let key_salt = self.key_salt(&sess)?;
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        ctx.update(&key_salt);
        ctx.update(key.as_bytes());
        let encoded_hash = encoding::encode(&ctx.finish().as_ref());
        let (dir_part, file_part) = encoded_hash.split_at(2);
        Ok(PathBuf::from(dir_part).join(file_part))
    }

    fn read_entry(&self, path: &Path, sess: &Session) -> Result<Entry> {
        println!("read_entry {:?}", path);
        
        let data = self.read_file(&path)?;
        let encrypted: Encrypted = rmp_serde::from_slice(&data)?;

        let plaintext = encrypted.decrypt(&sess)?;
        let entry: Entry = rmp_serde::from_slice(&plaintext.0)?;
        Ok(entry)
    }

    pub fn read_block(&self, key: &str, sess: &Session) -> Result<Block> {
        let path = Path::new("cryptic")
            .join(&self.derive_key_filepath(&key, &sess)?);
        let entry = self.read_entry(&path, &sess)?;
        let block_reg: ditto::Register<Block> = rmp_serde::from_slice(&entry.val)?;
        Ok(block_reg.get().to_owned())
    }

    pub fn write_block(&self, key: &str, block: &Block, sess: &Session) -> Result<()> {
        if key.len() == 0 {
            return Err(Error::State("key must not be empty".into()));
        }

        let path = Path::new("cryptic")
            .join(self.derive_key_filepath(&key, &sess)?);

        println!("write_block {}\n\t@ {:?}", key, path);

        let register = match self.read_entry(&path, &sess) {
            Ok(entry) => {
                let mut reg: ditto::Register<Block> =
                    rmp_serde::from_slice(&entry.val)?;

                // TODO: is there a better way than to clone here?
                let mut old_block = reg
                    .clone()
                    .get()
                    .to_owned();

                // TODO: can we avoid the block.clone() here?
                let new_block: Block = match old_block.merge(&block) {
                    Ok(()) => Ok(old_block),
                    Err(Error::BlockTypeConflict) => Ok(block.clone()),
                    Err(e) => Err(e)
                }?;

                reg.update(new_block, sess.site_id);
                reg
            },
            Err(Error::NotFound) => {
                ditto::Register::new(block.clone(), sess.site_id)
            },
            Err(e) => return Err(e)
        };

        let entry = Entry {
            key: key.to_string(),
            val: rmp_serde::to_vec(&register)?
        };

        let encrypted = Plaintext(rmp_serde::to_vec(&entry)?)
            .encrypt(&sess)?;

        let bytes = rmp_serde::to_vec(&encrypted)?;
        self.write_file(&path, &bytes)?;
        git_helper::stage_file(&self.repo, &path)?;
        Ok(())
    }

    pub fn write(&self, prefix: &str, data: &impl Blockable, sess: &Session) -> Result<()> {
        for (suffix, block) in data.blocks().into_iter() {
            let mut key = String::with_capacity(prefix.len() + suffix.len());
            key.push_str(&prefix);
            key.push_str(&suffix);

            self.write_block(&key, &block, &sess)?;
        }
        Ok(())
    }

    pub fn prefix_scan(&self, prefix: &str, sess: &Session) -> Result<impl Iterator<Item=(String, Block)>> {
        let mut matches: Vec<(String, Block)> = Vec::new();

        // folder structure is:
        // cryptic/xx/yyyy...yyyy
        for outer_entry in std::fs::read_dir(self.root.join("cryptic"))? {
            let outer_entry = outer_entry?;
            let outer_path = outer_entry.path();
            if !outer_path.is_dir() {
                return Err(Error::State(format!("cryptic directory should only contain directories, found {:?}", outer_path)))
            }
            for inner_entry in std::fs::read_dir(&outer_path)? {
                let inner_entry = inner_entry?;
                let inner_path = inner_entry.path();
                if !inner_path.is_file() {
                    return Err(Error::State(format!("inner cryptic directory should only contain files, found {:?}", inner_path)))
                }
                let entry = self.read_entry(&inner_path, &sess)?;
                if entry.key.starts_with(&prefix) {
                    let reg: ditto::Register<Block> = rmp_serde::from_slice(&entry.val)?;
                    matches.push((entry.key, reg.get().clone()));
                }
            }
        }

        matches.sort_by(|&(ref a, _), &(ref b, _)| a.cmp(&b));
        Ok(matches.into_iter())
    }

    pub fn sync(&self, sess: &Session) -> Result<()> {
        let remote = self.read_remote(&sess)?;
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
                    self.merge_mod_files(&old, &new, &sess).is_ok()
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
            .ok_or_else(|| Error::State("added file doesn't have a path!?".into()))?;

        println!("merging added file {:?}", rel_path);

        let new_blob = self.repo.find_blob(new.id())?;
        self.write_file(&rel_path, new_blob.content())?;

        println!("wrote added file to workdir");
        git_helper::stage_file(&self.repo, &rel_path)?;
        Ok(())
    }

    fn merge_mod_files(&self, old: &git2::DiffFile, new: &git2::DiffFile, sess: &Session) -> Result<()> {
        let rel_path = old.path()
            .ok_or_else(|| Error::State("old file doesn't have a path!?".into()))?;

        println!("merging {:?}", rel_path);
        let old_blob = self.repo.find_blob(old.id())?;
        let new_blob = self.repo.find_blob(new.id())?;
        let old_cryptic: Encrypted = rmp_serde::from_slice(&old_blob.content())?;
        let new_cryptic: Encrypted = rmp_serde::from_slice(&new_blob.content())?;

        let old_plain = old_cryptic.decrypt(&sess)?;
        let new_plain = new_cryptic.decrypt(&sess)?;

        println!("decrypted old and new");
        
        let mut old_reg: ditto::Register<Block> = rmp_serde::from_slice(&old_plain.0)?;
        let new_reg: ditto::Register<Block> = rmp_serde::from_slice(&new_plain.0)?;

        println!("parsed old and new registers");

        let mut old_block = old_reg.clone().get().to_owned();
        let new_block = new_reg.clone().get().to_owned();

        let merged_block = match old_block.merge(&new_block) {
            Ok(()) => Ok(old_block.to_owned()),
            Err(Error::BlockTypeConflict) => Ok(new_block),
            Err(e) => Err(e)
        }?;

        old_reg.merge(&new_reg);
        old_reg.update(merged_block, sess.site_id);

        let encrypted = Plaintext(rmp_serde::to_vec(&old_reg)?)
            .encrypt(&sess)?;

        self.write_file(&rel_path, &rmp_serde::to_vec(&encrypted)?)?;
        git_helper::stage_file(&self.repo, &rel_path)?;

        Ok(())
    }

    pub fn read_remote(&self, sess: &Session) -> Result<Remote> {
        Remote::from_db("db$config$remote", &self, &sess)
    }

    pub fn write_remote(&self, remote: &Remote, sess: &Session) -> Result<()> {
        // TODO: remove the other remote before writing, read has a noauth bias
        // TODO: https://docs.rs/git2/0.7.0/git2/struct.Remote.html#method.is_valid_name
        self.write("db$config$remote", remote, &sess)
    }
}

#[cfg(test)]
mod test {
    extern crate tempfile;
    use super::*;
    use crypto;

    #[test]
    fn key_salt_used_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_owned();

        let git_root = &dir.path().join("db");
        let db = DB::init(git_root).unwrap();
        
        let kdf = crypto::KDF {
            pbkdf2_iters: 1000,
            salt: crypto::rand_256().unwrap(),
            entropy: crypto::create_entropy_file(&dir_path).unwrap()
        };
        let sess = Session {
            site_id: 0,
            master_key: kdf.master_key("super secret".as_bytes())
        };
        
        // fix the path salt to "$"
        let encrypted = Plaintext("$".as_bytes().to_vec())
            .encrypt(&sess).unwrap();

        db.write_file(
            &Path::new("key_salt"),
            &rmp_serde::to_vec(&encrypted).unwrap()
        ).unwrap();

        let key_salt = db.key_salt(&sess).unwrap();
        assert_eq!(key_salt, "$".as_bytes());
        let filepath = db.derive_key_filepath("/a/b/c", &sess).unwrap();

        //test vector comes from the python code:
        //>>> import hashlib
        //>>> hashlib.sha256(b"$/a/b/c").hexdigest()
        //'63b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79'
        let expected = PathBuf::from("63")
            .join("b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79");
        assert_eq!(filepath, expected);
    }
}
