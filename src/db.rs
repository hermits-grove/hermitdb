extern crate time;
extern crate git2;
extern crate rmp_serde;

use self::git2::Repository;

use std;

use db_error::{Result, DBErr};
use crypto::{Session, Plaintext, Encrypted, Config, gen_rand_256};
use git_creds;
use block::{Json, Tree, TreeEntry};
use path::{Path};

pub struct DB {
    pub root: std::path::PathBuf,
    pub repo: git2::Repository
}

impl DB {
    pub fn init(root: &std::path::Path, mut sess: &mut Session) -> Result<DB> {
        let repo = Repository::open(&root)
            .or_else(|_| Repository::init(&root))
            .map_err(DBErr::Git)?;

        let db = DB {
            root: root.to_path_buf(),
            repo: repo
        };
        
        db.consistancy_check(&mut sess)?;
        Ok(db)
    }

    fn consistancy_check(&self, mut sess: &mut Session) -> Result<()> {
        let remotes = std::path::Path::new("remotes");
        let path_salt = std::path::Path::new("path_salt");
        let cryptic = self.root.join("cryptic");

        if !self.root.join(&remotes).is_file() {
            self.write_remotes(&git_creds::Remotes::empty(), &mut sess)?;
            self.stage_file(&remotes)?;
        }

        let path_salt_filepath = self.root.join(&path_salt);
        if !path_salt_filepath.is_file() {
            let salt = gen_rand_256()?;

            Plaintext {
                data: salt.to_vec(),
                config: Config::fresh_default()?
            }.encrypt(&mut sess)?.write(&path_salt_filepath)?;
                
            self.stage_file(&path_salt)?;
        }

        if !cryptic.is_dir() {
            std::fs::create_dir(&cryptic).map_err(DBErr::IO)?;
        }
        Ok(())
    }

    fn path_salt(&self, mut sess: &mut Session) -> Result<Vec<u8>> {
        let path_salt_file = self.root.join("path_salt");
        let path_salt = Encrypted::read(&path_salt_file)
            ?.decrypt(&mut sess)
            ?.data;

        Ok(path_salt)
    }

    pub fn read_tree(&self, path: &Path, mut sess: &mut Session) -> Result<Tree> {
        let path_salt = self.path_salt(&mut sess)?;
        let tree_filepath = self.root
            .join("cryptic")
            .join(&path.derive_filepath(&path_salt));
        
        if tree_filepath.exists() {
            let plaintext = Encrypted::read(&tree_filepath)?.decrypt(&mut sess)?;
            let tree = rmp_serde::from_slice(&plaintext.data)
                .map_err(DBErr::SerdeDe)?;
            Ok(tree)
        } else {
            Err(DBErr::NotFound)
        }
    }

    pub fn write_tree(&self, path: &Path, tree: &Tree, mut sess: &mut Session) -> Result<()> {
        let path_salt = self.path_salt(&mut sess)?;
        let tree_filepath = self.root
            .join("cryptic")
            .join(&path.derive_filepath(&path_salt));

        if !tree_filepath.exists() && !path.is_root() {
            // recursively add entry to parent tree until root

            let parent_path = path.parent()
                .ok_or(DBErr::State("non-root path has no parent!".into()))?;
            let base_comp = path.base_comp()
                .ok_or(DBErr::State("non-root path has no base component!".into()))?;

            let tree_entry = TreeEntry::tree(&base_comp);

            let mut parent = match self.read_tree(&parent_path, &mut sess) {
                Ok(mut parent) => parent, 
                Err(DBErr::NotFound) => Tree::empty(&sess.site_id)?,
                Err(e) => return Err(e)
            };

            parent.add(&tree_entry)?;
            self.write_tree(&parent_path, &parent, &mut sess)?;
        }

        if tree_filepath.exists() {
            // test that existing file is a Tree
            let plaintext = Encrypted::read(&tree_filepath)?.decrypt(&mut sess)?;
            let _: Tree = rmp_serde::from_slice(&plaintext.data)
                .map_err(|_| DBErr::State("Attempted to write tree on top of existing non-tree".into()))?;
        }   

        Plaintext {
            data: rmp_serde::to_vec(&tree).map_err(DBErr::SerdeEn)?,
            config: Config::fresh_default()?
        }.encrypt(&mut sess)?.write(&tree_filepath)?;
        Ok(())
    }

    pub fn read_json(&self, path: &Path, mut sess: &mut Session) -> Result<Json> {
        let path_salt = self.path_salt(&mut sess)?;
        let json_filepath = self.root
            .join("cryptic")
            .join(&path.derive_filepath(&path_salt));

        if json_filepath.exists() {
            let plaintext = Encrypted::read(&json_filepath)?.decrypt(&mut sess)?;
            let json = rmp_serde::from_slice(&plaintext.data).map_err(DBErr::SerdeDe)?;
            Ok(json)
        } else {
            Err(DBErr::NotFound)
        }
    }

    pub fn write_json(&self, path: &Path, json: &Json, mut sess: &mut Session) -> Result<()> {
        if path.is_root() {
            return Err(DBErr::State("Attempting to write Json to root path".into()));
        }
        
        let path_salt = self.path_salt(&mut sess)?;
        let json_filepath = self.root
            .join("cryptic")
            .join(&path.derive_filepath(&path_salt));

        if !json_filepath.exists() {
            // recursively add entry to parent tree until root

            let parent_path = path.parent()
                .ok_or(DBErr::State("non-root path has no parent!".into()))?;
            let base_comp = path.base_comp()
                .ok_or(DBErr::State("non-root path has no base component!".into()))?;

            let mut parent = match self.read_tree(&parent_path, &mut sess) {
                Ok(mut parent) => parent, 
                Err(DBErr::NotFound) => Tree::empty(&sess.site_id)?,
                Err(e) => return Err(e)
            };

            parent.add(&TreeEntry::json(&base_comp))?;
            self.write_tree(&parent_path, &parent, &mut sess)?;
        } else {
            // test that existing file is a Json
            let plaintext = Encrypted::read(&json_filepath)?.decrypt(&mut sess)?;
            let _: Json = rmp_serde::from_slice(&plaintext.data)
                .map_err(|_| DBErr::State("Attempted to write json on top of existing non-json".into()))?;
        }
        
        Plaintext {
            data: rmp_serde::to_vec(&json).map_err(DBErr::SerdeEn)?,
            config: Config::fresh_default()?
        }.encrypt(&mut sess)?.write(&json_filepath)?;
        Ok(())
    }

//    pub fn rm(&self, path: &String, mut sess: &mut Session) -> Result<()> {
//        let manifest = self.manifest(&mut sess)?;
//        let matching_entries: Vec<&manifest::Entry> = manifest.entries.iter().filter(|e| &e.path == path).collect();
//        if matching_entries.len() == 0 {
//            return Err(format!("No entry with given path: {}", path));
//        } else if matching_entries.len() > 1 {
//            return Err(format!("Multiple entries with given path: {}, this should not happen!", path));
//        }
//
//        let entry = matching_entries[0];
//
//        let root = self.root()?;
//        remove_file(&root.join(&entry.garbled_path))
//            .map_err(|s| format!("Failed to remove encrypted: {}", s))?;
//        remove_file(&root.join(&entry.garbled_path).with_extension("toml"))
//            .map_err(|s| format!("Failed to remove encrypted: {}", s))?;
//
//        let updated_entries: Vec<manifest::Entry> = manifest.entries.iter()
//            .filter(|e| &e.path != path)
//            .map(|e| e.clone())
//            .collect();
//        
//        let updated_manifest = manifest::Manifest {
//            entries: updated_entries,
//            ..manifest
//        };
//        self.write_manifest(&updated_manifest, &mut sess)
//    }

//    pub fn add_remote(&self, remote: &git_creds::Remote, mut sess: &mut Session) -> Result<()> {
//        let remotes = self.remotes(&mut sess)?;
//        let mut updated_remotes = remotes.remotes.clone();
//        updated_remotes.push(remote.clone());
//
//        let updated_remotes = git_creds::Remotes {
//            remotes: updated_remotes,
//            ..remotes
//        };
//
//        self.write_remotes(&updated_remotes, &mut sess)?;
//        self.repo.remote(&remote.name, &remote.url)
//            .map(|_| ()) // return Ok(())
//            .map_err(|e| format!("Failed to add remote: {:?}", e))
//    }

//    pub fn remove_remote(&self, name: &String, mut sess: &mut Session) -> Result<()> {
//        let remotes = self.remotes(&mut sess)?;
//        let updated_remotes: Vec<_> = remotes
//            .remotes
//            .iter()
//            .filter(|r| &r.name != name)
//            .map(|r| r.clone())
//            .collect();
//
//        let updated_remotes = git_creds::Remotes {
//            remotes: updated_remotes,
//            ..remotes
//        };
//
//        self.write_remotes(&updated_remotes, &mut sess)?;
//        
//        self.repo.remote_delete(&name)
//            .map_err(|e| format!("Failed to remove remote: {:?}", e))
//    }

//    pub fn remotes(&self, mut sess: &mut Session) -> Result<git_creds::Remotes> {
//        let path = self.root()?.join("remotes");
//        let remotes_toml_bytes = Encrypted::read(&path)?.decrypt(&mut sess)?.data;
//        git_creds::Remotes::from_toml_bytes(&remotes_toml_bytes)
//    }

    fn stage_file(&self, file: &std::path::Path) -> Result<()> {
        let mut index = self.repo.index()
            .map_err(DBErr::Git)?;
        index.add_path(&file)
            .map_err(DBErr::Git)?;
        index.write()
            .map_err(DBErr::Git)?;
        Ok(())
    }

//    fn commit(&self, commit_msg: &String, extra_parents: &Vec<&git2::Commit>) -> Result<()> {
//        let tree = self.repo.index()
//            .and_then(|mut index| {
//                index.write()?; // make sure the index on disk is up to date
//                index.write_tree()
//            })
//            .and_then(|tree_oid| self.repo.find_tree(tree_oid))
//            .map_err(|e| format!("Failed to write index as tree: {:?}", e))?;
//
//        let parents = match self.repo.head() {
//            Ok(head_ref) => {
//                let head_commit = head_ref
//                    .target()
//                    .ok_or(format!("Failed to find oid referenced by HEAD"))
//                    .and_then(|head_oid| {
//                        self.repo.find_commit(head_oid)
//                            .map_err(|e| format!("Failed to find the head commit: {:?}", e))
//                    })?;
//
//                vec![head_commit]
//            },
//            Err(_) => Vec::new() // this is likely the initial commit (no parent)
//        };
//
//
//        let mut borrowed_parents: Vec<_> = parents.iter().map(|p| p).collect();
//        borrowed_parents.extend(extra_parents);
//        
//        let sig = self.repo.signature()
//            .map_err(|e| format!("Failed to generate a commit signature: {:?}", e))?;
//
//        self.repo.commit(Some("HEAD"), &sig, &sig, &commit_msg, &tree, borrowed_parents.as_slice())
//            .map_err(|e| format!("Failed commit with parent (in sync): {:?}", e))?;
//        Ok(())
//    }
        
              
//    fn pull_remote(&self, remote: &git_creds::Remote) -> Result<()> {
//        println!("Pulling from remote: {}", remote.name);
//        let mut git_remote = self.repo.find_remote(&remote.name)
//            .map_err(|e| format!("Failed to find remote {}: {:?}", remote.name, e))?;
//
//        let mut fetch_opt = git2::FetchOptions::new();
//        fetch_opt.remote_callbacks(remote.git_callbacks());
//        git_remote.fetch(&["master"], Some(&mut fetch_opt), None)
//            .map_err(|e| format!("Failed to fetch remote {}: {:?}", remote.name, e))?;
//
//        let branch_res = self.repo.find_branch("master", git2::BranchType::Remote);
//
//        if branch_res.is_err() {
//            return Ok(()); // remote does not have a tracking branch, this happens on initialization (client has not pushed yet)
//        }
//        
//        let remote_branch_oid = branch_res.unwrap().get() // branch reference
//            .resolve() // direct reference
//            .map_err(|e| format!("Failed to resolve remote branch {} OID: {:?}", remote.name, e))
//            ?.target() // OID of latest commit on remote branch
//            .ok_or(format!("Failed to fetch remote oid: remote {}", remote.name))?;
//
//        let remote_commit = self.repo
//            .find_annotated_commit(remote_branch_oid)
//            .map_err(|e| format!("Failed to find commit for remote banch {}: {:?}", remote.name, e))?;
//
//        self.repo.merge(&[&remote_commit], None, None)
//            .map_err(|e| format!("Failed merge from remote {}: {:?}", remote.name, e))?;
//        
//        let index = self.repo.index()
//            .map_err(|e| format!("Failed to read index: {:?}", e))?;
//
//        if index.has_conflicts() {
//            panic!("I don't know how to handle conflicts yet!!!!!!!!!!!!!");
//        }
//
//        let stats = self.repo.diff_index_to_workdir(None, None)
//            .map_err(|e| format!("Failed diff index: {:?}", e))?.stats()
//            .map_err(|e| format!("Failed to get diff stats: {:?}", e))?;
//
//        if stats.files_changed() > 0 {
//            println!("{} files changed (+{}, -{})",
//                     stats.files_changed(),
//                     stats.insertions(),
//                     stats.deletions());
//
//            let remote_commit = self.repo.find_commit(remote_branch_oid)
//                .map_err(|e| format!("Failed to find remote commit: {:?}", e))?;
//
//            let msg = format!("Mona Sync from {}: {}",
//                              remote.name,
//                              time::now().asctime());
//
//            self.commit(&msg, &vec![&remote_commit])?;
//            self.push_remote(&remote)?;
//        }
//        
//        // TAI: should return stats struct
//        Ok(())
//    }

//    pub fn push_remote(&self, remote: &git_creds::Remote) -> Result<()> {
//        println!("Pushing to remote {} {}", remote.name, remote.url);
//        let mut git_remote = self.repo.find_remote(&remote.name)
//            .map_err(|e| format!("Failed to find remote with name {}: {:?}", remote.name, e))?;
//
//        let mut fetch_opt = git2::PushOptions::new();
//        fetch_opt.remote_callbacks(remote.git_callbacks());
//
//        git_remote.push(&[&"refs/heads/master:refs/heads/master"], Some(&mut fetch_opt))
//            .map_err(|e| format!("Failed to push remote {}: {:?}", remote.name, e))?;
//        println!("Finish push");
//        Ok(())
//    }

//    pub fn sync(&self, mut sess: &mut Session) -> Result<()> {
//        for remote in self.remotes(&mut sess)?.remotes.iter() {
//            self.pull_remote(&remote)?;
//        }
//
//        let mut index = self.repo.index()
//            .map_err(|e| format!("Failed to fetch index: {:?}", e))?;
//
//        let stats = self.repo.diff_index_to_workdir(None, None)
//            .map_err(|e| format!("Failed diff index: {:?}", e))?.stats()
//            .map_err(|e| format!("Failed to get diff stats: {:?}", e))?;
//
//        println!("files changed: {}", stats.files_changed());
//
//        if stats.files_changed() > 0 {
//            index.add_all(["*"].iter(), git2::ADD_DEFAULT, None)
//                .map_err(|e| format!("Failed to add files to index: {:?}", e))?;
//            let timestamp_commit_msg = format!("Mona: {}", time::now().asctime());
//            self.commit(&timestamp_commit_msg, &Vec::new())?;
//        }
//
//        // TODO: is this needed?
//        &self.repo.checkout_head(None)
//            .map_err(|e| format!("Failed to checkout head: {:?}", e))?;
//
//        // now need to push to all remotes
//        for remote in self.remotes(&mut sess)?.remotes.iter() {
//            self.push_remote(&remote)?;
//        }
//        Ok(())
//    }
    
    // PRIVATE METHODS ====================

    fn write_remotes(&self, remotes: &git_creds::Remotes, mut sess: &mut Session) -> Result<()> {
        Plaintext {
            data: remotes.to_toml_bytes()?,
            config: Config::fresh_default()?
        }.encrypt(&mut sess)?.write(&self.root.join("remotes"))
    }
}

#[cfg(test)]
mod test {
    extern crate tempfile;
    use super::*;

    #[test]
    fn init() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), None);
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = dir.path().join("db");

        let db = DB::init(&git_root, &mut sess).unwrap();
        assert!(git_root.is_dir());

        let remotes_filepath = git_root.join("remotes");
        assert!(remotes_filepath.is_file());

        let remotes_plain = Encrypted::read(&remotes_filepath)
            .unwrap()
            .decrypt(&mut sess)
            .unwrap();
        let remotes = git_creds::Remotes::from_toml_bytes(&remotes_plain.data).unwrap();
        assert_eq!(remotes, git_creds::Remotes::empty());

        let path_salt_path = git_root.join("path_salt");
        assert!(path_salt_path.is_file());
        
        let path_salt = db.path_salt(&mut sess).unwrap();
        assert_eq!(path_salt.len(), 256/8);

        let db2 = DB::init(&git_root, &mut sess).unwrap();
        assert_eq!(db2.path_salt(&mut sess).unwrap(), path_salt);

        assert!(db.root.join("cryptic").is_dir());
    }

    #[test]
    fn path_salt_used_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), None);
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = &dir.path().join("db");

        let db = DB::init(git_root, &mut sess).unwrap();
        // fix the path salt to "$"
        let encrypted = Plaintext {
            data: "$".as_bytes().to_vec(),
            config: Config::fresh_default().unwrap()
        }.encrypt(&mut sess).unwrap();
            
        encrypted.write(&db.root.join("path_salt"))
            .unwrap();

        let path_salt = db.path_salt(&mut sess).unwrap();
        assert_eq!(path_salt, "$".as_bytes());
        let filepath = Path::new("/a/b/c")
            .unwrap()
            .derive_filepath(&path_salt);

        //test vector comes from the python code:
        //>>> import hashlib
        //>>> hashlib.sha256(b"$/a/b/c").hexdigest()
        //'63b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79'
        let expected = std::path::PathBuf::from("63")
            .join("b2c7879bd2a4d08a4671047a19fdd4c88e580efb66d853045a210eea0afe79");
        assert_eq!(filepath, expected);
    }

    #[test]
    fn tree() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), Some(1));
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = &dir.path().join("db");

        let db = DB::init(git_root, &mut sess).unwrap();

        let paths_to_check = [
            Path::new("/").unwrap(),
            Path::new("/a").unwrap(),
            Path::new("/a/b").unwrap(),
            Path::new("/a/b/c").unwrap()
        ];

        let path_salt = db.path_salt(&mut sess).unwrap();

        for p in paths_to_check.iter() {
            let path = db.root.join("cryptic").join(&p.derive_filepath(&path_salt));
            assert!(!path.exists()); // should not exist yet
        }
        
        db.write_tree(
            &Path::new("/a/b/c").unwrap(),
            &Tree::empty(&sess.site_id).unwrap(),
            &mut sess
        ).unwrap();

        for p in paths_to_check.iter() {
            let path = db.root.join("cryptic").join(&p.derive_filepath(&path_salt));
            println!("path: {:?}", path);
            assert!(path.exists()); // all prefix paths should now exist
        }

        let tree = db.read_tree(&Path::new("/a/b/c").unwrap(), &mut sess).unwrap();
        assert_eq!(tree, Tree::empty(&sess.site_id).unwrap());
    }

    #[test]
    fn json() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = Session::new(dir.path(), Some(1));
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());
        let git_root = &dir.path().join("db");

        let db = DB::init(git_root, &mut sess).unwrap();

        let json = Json::from_str(r#"
        {
            "foo": 123,
            "bar": [
                "Hello",
                "Aloha",
                "Hola"
            ],
            "baz": true
        }"#).unwrap();

        let res = db.read_json(&Path::new("/a/b/c").unwrap(), &mut sess);
        assert!(res.is_err()); // nothing should exist
        
        db.write_json(&Path::new("/a/b/c").unwrap(), &json, &mut sess).unwrap();
        
        let read_json = db.read_json(&Path::new("/a/b/c").unwrap(), &mut sess).unwrap();
        assert_eq!(json, read_json);

        let res = db.write_json(&Path::new("/a/b/c").unwrap(), &json, &mut sess);
        assert!(res.is_ok());
        
        let res = db.write_json(&Path::new("/a/b").unwrap(), &json, &mut sess);
        assert!(res.is_err()); // should fail to overwrite tree
        
        let res = db.write_tree(&Path::new("/a/b/c").unwrap(), &Tree::empty(&sess.site_id).unwrap(), &mut sess);
        assert!(res.is_err()); // should fail to overwrite json with tree
    }
}
