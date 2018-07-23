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
use remote::Remote;
use git_helper;

pub struct DB {
    repo: Repository,
    tree: sled::Tree
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
}
