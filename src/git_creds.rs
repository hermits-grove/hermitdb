// git remotes need credentials.
//
// To avoid having the user re-enter their
// credentials on each synch, we store an encrypted mapping
// from remote to credentials in the Mona git-db.
//
// This has the added benefit of all mona clients
// automatically learning of changes made to
// remotes by one client.

extern crate git2;

use toml;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Remotes {
    pub version: String,
    pub remotes: Vec<Remote>
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Remote {
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String
}

impl Remotes {
    pub fn empty() -> Remotes {
        Remotes {
            version: String::from("0.0.1"),
            remotes: Vec::new()
        }
    }
    
    pub fn from_toml_bytes(bytes: &Vec<u8>) -> Result<Remotes, String> {
        toml::from_slice(&bytes)
            .map_err(|e| format!("Failed to read remotes from TOML: {:?}", e))
    }

    pub fn to_toml_bytes(&self) -> Result<Vec<u8>, String> {
        toml::to_vec(&self)
            .map_err(|e| format!("Failed to serialize remotes into TOML: {:?}", e))
    }
}

impl Remote {
    pub fn git_callbacks(&self) -> git2::RemoteCallbacks {
        let mut callbacks = git2::RemoteCallbacks::new();
        callbacks.credentials(move |_user, _username_from_url, _allowed_types| {
            println!("git cred cb: {:?} {:?} {:?}", _user, _username_from_url, _allowed_types);
            git2::Cred::userpass_plaintext(&self.username, &self.password)
        });
        callbacks
    }
}
