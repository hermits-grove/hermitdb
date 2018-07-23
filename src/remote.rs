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
extern crate crdts;
extern crate serde;
extern crate time;

#[derive(Debug, PartialEq, Eq, Clone)]
struct Auth {
    user: String,
    pass: String
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Remote {
    pub name: String,
    pub url: String,
    auth: Option<Auth>
}

impl Remote {
    pub fn auth(name: String, url: String, user: String, pass: String) -> Self {
        Remote {
            name: name,
            url: url,
            auth: Some(Auth { user, pass })
        }
    }

    pub fn no_auth(name: String, url: String) -> Self {
        Remote {
            name: name,
            url: url,
            auth: None
        }
    }

    pub fn git_callbacks(&self) -> git2::RemoteCallbacks {
        let mut cbs = git2::RemoteCallbacks::new();
        cbs.credentials(move |_, _, _| {
            match self.auth {
                Some(Auth {ref user, ref pass} ) =>
                    git2::Cred::userpass_plaintext(user, pass),
                None => {
                    panic!("This should never be called!");
                }
            }
        });
        cbs
    }
}
