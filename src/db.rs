extern crate time;
extern crate git2;

use self::git2::Repository;

use std;
use std::io::Write;

use db_error::{DBErr};
use crypto;
use git_creds;

pub struct DB {
    pub repo: git2::Repository
}

impl DB {
    pub fn init(db_root: &std::path::Path, mut sess: &mut crypto::Session) -> Result<DB, DBErr> {
        let repo = Repository::open(&db_root)
            .or_else(|_| Repository::init(&db_root))
            .map_err(DBErr::Git)?;

        let db = DB {
            repo: repo
        };
        
        db.consistancy_check(&mut sess)?;
        Ok(db)
    }

    fn consistancy_check(&self, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
        let git_root = self.git_root()?;
        let remotes = std::path::Path::new("remotes");
        let path_salt = std::path::Path::new("path_salt");

        if !git_root.join(&remotes).is_file() {
            self.write_remotes(&git_creds::Remotes::empty(), &mut sess)?;
            self.stage_file(&remotes)?;
        }

        let path_salt_filepath = git_root.join(&path_salt);
        if !path_salt_filepath.is_file() {
            let salt = crypto::gen_rand_256()?;
            let mut f = std::fs::File::create(path_salt_filepath).map_err(DBErr::IO)?;
            f.write_all(&salt).map_err(DBErr::IO)?;
            self.stage_file(&path_salt)?;
        }

        Ok(())
    }

//    fn put_entry(&self, entry: path: Path, data: &crypto::Encrypted, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
//        let root = self.root()?;
//        let entry_path = root.join(&entry.garbled_path);
//
//        data.write(&entry_path)?;
//
//        let manifest_old = self.manifest(&mut sess)?;
//        for e in manifest_old.entries.iter() {
//            if e.path == entry.path {
//                self.rm(&entry.path, &mut sess)?; // TODO: use proper error messages so that we don't have to loop over manifest twice here
//                break;
//            }
//        }
//
//        let manifest = self.manifest(&mut sess)?;
//        let mut updated_entries: Vec<manifest::Entry> = manifest.entries.clone();
//        updated_entries.push(entry);
//        
//        let updated_manifest = manifest::Manifest {
//            entries: updated_entries,
//            ..manifest
//        };
//
//        self.write_manifest(&updated_manifest, &mut sess)
//    }

//    pub fn put(&self, entry_req: &manifest::EntryRequest, data: &crypto::Encrypted, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
//        entry_req.validate()?;
//        
//        let root = self.root()?;
//
//        let mut garbled = encoding::encode(&crypto::generate_rand_bits(96)?);
//        while root.join(&garbled).exists() {
//            garbled = encoding::encode(&crypto::generate_rand_bits(96)?);
//        }
//
//        let entry = manifest::Entry {
//            path: entry_req.path.clone(),
//            tags: entry_req.tags.clone(),
//            garbled_path: garbled
//        };
//
//        self.put_entry(entry, &data, &mut sess)?;
//        Ok(())     
//    }

//    pub fn get(&self, path: &String, mut sess: &mut crypto::Session) -> Result<crypto::Encrypted, DBErr> {
//        let root = self.root()?;
//        let manifest = self.manifest(&mut sess)?;
//        for e in manifest.entries.iter() {
//            if &e.path == path {
//                return crypto::Encrypted::read(&root.join(&e.garbled_path));
//            }
//        }
//        // TODO: we are using Err to represent a get for a non-existing entity, we should have different result type which would tell you if there is no element and distinguish from regular errors
//        Err(format!("No entry with given path: {}", path))
//    }

//    pub fn rm(&self, path: &String, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
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

//    pub fn add_remote(&self, remote: &git_creds::Remote, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
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

//    pub fn remove_remote(&self, name: &String, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
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

//    pub fn remotes(&self, mut sess: &mut crypto::Session) -> Result<git_creds::Remotes, DBErr> {
//        let path = self.root()?.join("remotes");
//        let remotes_toml_bytes = crypto::Encrypted::read(&path)?.decrypt(&mut sess)?.data;
//        git_creds::Remotes::from_toml_bytes(&remotes_toml_bytes)
//    }

    fn stage_file(&self, file: &std::path::Path) -> Result<(), DBErr> {
        let mut index = self.repo.index()
            .map_err(DBErr::Git)?;
        index.add_path(&file)
            .map_err(DBErr::Git)?;
        index.write()
            .map_err(DBErr::Git)?;
        Ok(())
    }

//    fn commit(&self, commit_msg: &String, extra_parents: &Vec<&git2::Commit>) -> Result<(), DBErr> {
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
        
              
//    fn pull_remote(&self, remote: &git_creds::Remote) -> Result<(), DBErr> {
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

//    pub fn push_remote(&self, remote: &git_creds::Remote) -> Result<(), DBErr> {
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

//    pub fn sync(&self, mut sess: &mut crypto::Session) -> Result<(), DBErr> {
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
    
    pub fn git_root(&self) -> Result<&std::path::Path, DBErr> {
        self.repo.workdir()
            .ok_or(DBErr::State(String::from("The repository is bare, no workdir")))
    }
    
    // PRIVATE METHODS ====================

    fn write_remotes(&self, remotes: &git_creds::Remotes, mut sess: &mut crypto::Session) -> Result<(), DBErr>{
        let root = self.git_root()?;
        crypto::Plaintext {
            data: remotes.to_toml_bytes()?,
            config: crypto::Config::fresh_default()?
        }.encrypt(&mut sess)?.write(&root.join("remotes"))
    }
}

#[cfg(test)]
mod test {
    extern crate tempfile;
    use super::*;

    #[test]
    pub fn init() {
        let dir = tempfile::tempdir().unwrap();
        let mut sess = crypto::Session::new(dir.path());
        sess.create_key_file().unwrap();
        sess.set_pass(":P".as_bytes());

        let git_root = &dir.path().join("db");
        DB::init(git_root, &mut sess).unwrap();
        assert!(git_root.is_dir());

        let remotes_filepath = git_root.join("remotes");
        assert!(remotes_filepath.is_file());

        let remotes_plain = crypto::Block::read(&remotes_filepath)
            .unwrap()
            .decrypt(&mut sess)
            .unwrap();
        let remotes = git_creds::Remotes::from_toml_bytes(&remotes_plain.data).unwrap();
        assert_eq!(remotes, git_creds::Remotes::empty());

        let path_salt_path = git_root.join("path_salt");
        assert!(path_salt_path.is_file());
        assert_eq!(std::fs::metadata(&path_salt_path).unwrap().len(), 256/8);
    }
}
