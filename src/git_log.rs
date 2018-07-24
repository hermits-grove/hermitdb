extern crate bincode;
extern crate serde;

use std::str::FromStr;
use std::string::ToString;
use std::fmt::Debug;
use std::marker::PhantomData;

use self::serde::de::DeserializeOwned;
use self::serde::Serialize;

use git2;

use error::{Error, Result};
use crdts::{CmRDT, Actor};
use log::{TaggedOp, LogReplicable};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Auth {
    user: String,
    pass: String
}

pub struct GitLog<A: Actor, C: Debug + CmRDT>
    where C::Op : DeserializeOwned + Serialize + Eq
{
    actor: A,
    name: String,
    url: String,
    auth: Option<Auth>,
    repo: git2::Repository,
    phantom_crdt: PhantomData<C>
}

#[serde(bound(deserialize = ""))]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op<A: Actor, C: Debug + CmRDT + Eq>
    where C::Op : DeserializeOwned + Serialize + Eq
{
    actor: A,
    oid: Vec<u8>,
    op: C::Op
}

impl<A: Actor, C: Debug + CmRDT + Eq> TaggedOp<C> for Op<A, C>
    where C::Op : DeserializeOwned + Serialize + Eq
{
    type ID = git2::Oid;

    fn id(&self) -> Self::ID {
        git2::Oid::from_bytes(&self.oid).unwrap()
    }

    fn op(&self) -> &C::Op {
        &self.op
    }
}

impl<A: Actor, C: Debug + CmRDT + Eq> Op<A, C>
    where C::Op : DeserializeOwned + Serialize + Eq
{
    pub fn from_commit(actor: A, repo: &git2::Repository, commit: &git2::Commit) -> Result<Self> {
        let tree = commit.tree()?;
        let tree_entry = tree.get_name("op")
            .ok_or(Error::LogCommitDoesNotContainOp)?;
        let id = tree_entry.id();
        let blob = repo.find_blob(id)?;
        let bytes = blob.content();
        let op = bincode::deserialize(bytes)?;
        Ok(Op {
            actor: actor,
            oid: commit.id().as_bytes().to_vec(),
            op: op
        })
    }

    pub fn next_from_branches(
        actor: A,
        repo: &git2::Repository,
        unacked: Option<git2::Branch>,
        acked: Option<git2::Branch>
    ) -> Result<Option<Op<A, C>>> {
        match (unacked, acked) {
            (Some(unacked), Some(acked)) => {
                let local_unacked_oid = unacked.get().target()
                    .ok_or(Error::BranchIsNotADirectReference)?;
                let local_acked_oid = acked.get().target()
                    .ok_or(Error::BranchIsNotADirectReference)?;

                if local_unacked_oid != local_acked_oid {
                    let mut curr_oid = local_unacked_oid;
                    let mut commit;
                    loop {
                        commit = repo.find_commit(curr_oid)?;
                        let parents: Vec<git2::Oid> = commit.parent_ids().collect();
                        assert_eq!(parents.len(), 1);
                        if parents[0] == local_acked_oid {
                            break;
                        }
                        curr_oid = parents[0];
                    }

                    let op = Op::from_commit(actor, &repo, &commit)?;
                    Ok(Some(op))
                } else {
                    Ok(None)
                }
            },
            (Some(unacked), None) => {
                let mut curr_oid = unacked.get().target()
                    .ok_or(Error::BranchIsNotADirectReference)?;
                let mut commit;
                loop {
                    commit = repo.find_commit(curr_oid)?;
                    let parents: Vec<git2::Oid> = commit.parent_ids().collect();
                    if parents.len() == 0 {
                        break;
                    }

                    assert_eq!(parents.len(), 1);
                    curr_oid = parents[0];
                }

                let op = Op::from_commit(actor, &repo, &commit)?;
                Ok(Some(op))
            },
            (None, Some(_)) => panic!("we have acked ops that were never unacked!"),
            _ => Ok(None)
        }
    }
}


impl<A, C> LogReplicable<A, C> for GitLog<A, C> where
    A: Actor + FromStr + ToString + Debug,
    C: Debug + CmRDT + Eq + Serialize + DeserializeOwned, // NOT SURE WHY I NEED SERDE BOUNDS ON `C`
    C::Op : DeserializeOwned + Serialize + Eq
{
    type Op = Op<A, C>;
    fn next(&self) -> Result<Option<Self::Op>> {
        let local_name = format!("actor_{}", self.actor.to_string());
        let local_acked = format!("acked_actor_{}", self.actor.to_string());

        let unacked = self.repo.find_branch(&local_name, git2::BranchType::Local);
        let acked = self.repo.find_branch(&local_acked, git2::BranchType::Local);
        if let Some(op) = Op::next_from_branches(
            self.actor.clone(),
            &self.repo,
            unacked.ok(),
            acked.ok()
        )? {
            return Ok(Some(op));
        }

        // we have no local unacked ops, check for remote ops
        for branch in self.repo.branches(Some(git2::BranchType::Remote))? {
            let (remote_branch, _) = branch?;

            println!("branch name: {}", remote_branch.name()
                     ?.ok_or(Error::BranchNameEncodingError)?);

            let actor = {
                let branch_name = remote_branch.name()
                    ?.ok_or(Error::BranchNameEncodingError)?;
                let split: Vec<&str> = branch_name.split("/actor_").collect();
                println!("branch_name split: {:?}", split);
                let actor: A = match split.as_slice() {
                    [_, s] => s.parse()
                        .map_err(|_| Error::Parse(
                            format!("Failed to parse actor from branch: {}", s)))?,
                    _ => continue
                };
                println!("actor {:?}", actor.to_string());
                actor
            };
            
            let tracking_branch = self.repo
                .find_branch(&format!("actor_{}", actor.to_string()), git2::BranchType::Local);

            let next_op = Op::next_from_branches(
                actor,
                &self.repo,
                Some(remote_branch),
                tracking_branch.ok()
            )?;

            if let Some(op) = next_op {
                return Ok(Some(op));
            }
        }
        Ok(None)
    }

    fn ack(&mut self, op: &Self::Op) -> Result<()> {
        match self.next()? {
            Some(expected) => {
                if &expected != op {
                    return Err(Error::State("Attempting to ack an op that is not the next op".into()));
                }
            },
            None => {
                return Err(
                    Error::State("Attempting to ack an op when no op has been committed".into())
                );
            }
        }

        let branch_name: String = if op.actor == self.actor {
            format!("acked_actor_{}", op.actor.to_string())
        } else {
            format!("actor_{}", op.actor.to_string())
        };

        let commit = self.repo.find_commit(op.id())?;
        println!("updating commit on {}, to {:?}", branch_name, commit.id());
        self.repo.branch(&branch_name, &commit, true)?;
        Ok(())
    }

    fn commit(&mut self, op: C::Op) -> Result<()> {
        let name = format!("actor_{}", self.actor.to_string());
        let parent = match self.repo.find_branch(&name, git2::BranchType::Local) {
            Ok(branch) => {
                let target = branch
                    .get()
                    .target()
                    .ok_or(Error::BranchIsNotADirectReference)?;
                let commit = self.repo.find_commit(target)?;
                Some(commit)
            },
            _ => None
        };

        let op_bytes = bincode::serialize(&op)?;
        let op_oid = self.repo.blob(&op_bytes)?;
        let mut builder = self.repo.treebuilder(None)?;
        builder.insert("op", op_oid, 0o100644)?;
        let tree_oid = builder.write()?;
        let tree = self.repo.find_tree(tree_oid)?;

        let sig = self.repo.signature()?;

        let mut parent_commits = Vec::new();
        if let Some(ref commit) = parent {
            parent_commits.push(commit)
        }

        let branch_ref = format!("refs/heads/{}", name);
        println!("committing to branch ref: {}", branch_ref);

        self.repo
            .commit(Some(&branch_ref), &sig, &sig, "db op", &tree, &parent_commits)?;
        Ok(())
    }

    fn pull(&mut self, other: &Self) -> Result<()> {
        println!("fetching remote: {}", &other.name);

        println!("searching for existing remote in repo");
        let mut git_remote = match self.repo.find_remote(&other.name) {
            Ok(git_remote) => git_remote,
            Err(_) => {
                eprintln!("Failed to find remote '{}', adding remote to git", other.name);
                // this remote is not added to git yet, we add it
                self.repo.remote(&other.name, &other.url)?
            }
        };

        println!("found a remote, starting fetch...");
        
        let mut fetch_opt = git2::FetchOptions::new();
        fetch_opt.remote_callbacks(other.git_callbacks());
        let refspec_iter = git_remote.fetch_refspecs()?;
        let refspecs: Vec<&str> = refspec_iter.iter()
            .map(|r| r.unwrap())
            .collect();
        git_remote.fetch(&refspecs, Some(&mut fetch_opt), None)?;
        println!("finished fetch");
        Ok(())
    }

    fn push(&self, other: &mut Self) -> Result<()> {
        println!("searching for existing remote in repo");
        let mut git_remote = match self.repo.find_remote(&other.name) {
            Ok(git_remote) => git_remote,
            Err(_) => {
                eprintln!("Failed to find remote '{}', adding remote to git", other.name);
                // this remote is not added to git yet, we add it
                self.repo.remote(&other.name, &other.url)?
            }
        };

        let mut push_opt = git2::PushOptions::new();
        push_opt.remote_callbacks(other.git_callbacks());

        let branches: Vec<String> = self.repo.branches(Some(git2::BranchType::Local))
            ?.map(|b| b.unwrap())
            .map(|(branch, _)| branch)
            .map(|b| {
                let b = b.name().unwrap().unwrap();
                format!("refs/heads/{}", b)
            })
            .collect();

        let borrowed: Vec<&str> = branches.iter().map(|s| s.as_ref()).collect();
        
        println!("branches to push: {:?}", borrowed);
        git_remote.push(&borrowed, Some(&mut push_opt))?;
        eprintln!("Finish push");
        Ok(())
    }
}

impl<A: Actor, C: Debug + CmRDT> GitLog<A, C>
    where C::Op : DeserializeOwned + Serialize + Eq
{
    pub fn auth(actor: A, repo: git2::Repository, name: String, url: String, user: String, pass: String) -> Self {
        GitLog {
            name: name,
            url: url,
            auth: Some(Auth { user, pass }),
            actor: actor,
            repo: repo,
            phantom_crdt: PhantomData
        }
    }

    pub fn no_auth(actor: A, repo: git2::Repository, name: String, url: String) -> Self {
        GitLog {
            name: name,
            url: url,
            auth: None,
            actor: actor,
            repo: repo,
            phantom_crdt: PhantomData
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
