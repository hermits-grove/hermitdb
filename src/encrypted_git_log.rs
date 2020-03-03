/// An Encrypted Git Log
/// Implementation wraps the unencypted git log with an encryption layer.
use std::str::FromStr;
use std::string::ToString;
use std::fmt::{self, Debug};
use std::marker::PhantomData;

use serde_derive::{Serialize, Deserialize};

use git2;
use crdts::{CmRDT, Actor};

use crate::error::{Result};
use crate::crypto::{KeyHierarchy, Encrypted, rand_256};
use crate::log::{TaggedOp, LogReplicable};
use crate::git_log;

struct EncryptedCRDT<C: CmRDT> {
    phantom_crdt: PhantomData<C>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedOp {
    salt: [u8; 256 / 8],
    op: Encrypted
}

unsafe impl Send for EncryptedOp {}

pub struct Log<A: Actor, C: CmRDT>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned
{
    root_key: KeyHierarchy,
    actor_key: KeyHierarchy,
    log: git_log::Log<A, EncryptedCRDT<C>>
}


pub struct LoggedOp<A: Actor, C: CmRDT>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned
{
    encrypted_logged_op: git_log::LoggedOp<A, EncryptedCRDT<C>>,
    plaintext_op: C::Op
}

impl<A: Actor, C: CmRDT> Debug for LoggedOp<A, C>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "LoggedOp {{ encrypted_logged_op: {:?}, plaintext_op: {:?} }}",
            self.encrypted_logged_op,
            self.plaintext_op
        )
    }
}

impl<C: CmRDT> CmRDT for EncryptedCRDT<C>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned
{
    type Op = EncryptedOp;

    fn apply(&mut self, _op: &Self::Op) {
        panic!("this should never be called");
    }
}

impl EncryptedOp {
    fn encrypt<C: CmRDT>(op: &C::Op, root: &KeyHierarchy) -> Result<Self>
        where C::Op: serde::Serialize + serde::de::DeserializeOwned
    {
        let bytes = bincode::serialize(&op)?;
        let salt = rand_256()?;
        let crypto_key = root.key_for(&salt);
        Ok(EncryptedOp {
            salt: salt,
            op: crypto_key.encrypt(&bytes)?
        })
    }

    fn decrypt<C: CmRDT>(&self, root: &KeyHierarchy) -> Result<C::Op>
        where C::Op: serde::Serialize + serde::de::DeserializeOwned
    {
        let crypto_key = root.key_for(&self.salt);
        let bytes = crypto_key.decrypt(&self.op)?;
        let op = bincode::deserialize(&bytes)?;
        Ok(op)
    }
}

impl<A: Actor, C: CmRDT> TaggedOp<C> for LoggedOp<A, C>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned
{
    type ID = <git_log::LoggedOp<A, C> as TaggedOp<C>>::ID;

    fn id(&self) -> Self::ID {
        self.encrypted_logged_op.id()
    }

    fn op(&self) -> &C::Op {
        &self.plaintext_op
    }
}

impl<A: Actor, C: CmRDT> LogReplicable<A, C> for Log<A, C>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned,
          A: Actor + ToString + FromStr + serde::Serialize
{
    type LoggedOp = LoggedOp<A, C>;
    type Remote = git_log::Remote;

    fn next(&self) -> Result<Option<Self::LoggedOp>> {
        match self.log.next() {
            Ok(Some(encrypted_logged_op)) => {
                let actor_bytes = bincode::serialize(encrypted_logged_op.actor())?;
                let actor_key = self.root_key
                    .derive_child(&actor_bytes);

                let plaintext_op = {
                    let encrypted_op = encrypted_logged_op.op();
                    encrypted_op
                        .decrypt::<C>(&actor_key)?
                };

                Ok(Some(LoggedOp {
                    encrypted_logged_op,
                    plaintext_op
                }))
            },
            Ok(None) => Ok(None),
            Err(e) => Err(e)
        }
    }

    fn ack(&mut self, logged_op: &Self::LoggedOp) -> Result<()> {
        self.log.ack(&logged_op.encrypted_logged_op)
    }

    fn commit(&mut self, op: C::Op) -> Result<Self::LoggedOp> {
        let encrypted_op = EncryptedOp::encrypt::<C>(
            &op,
            &self.actor_key
        )?;

        let encrypted_logged_op = self.log.commit(encrypted_op)?;
        Ok(LoggedOp {
            encrypted_logged_op,
            plaintext_op: op
        })
    }

    fn pull(&mut self, remote: &Self::Remote) -> Result<()> {
        self.log.pull(remote)
    }

    fn push(&self, remote: &mut Self::Remote) -> Result<()> {
        self.log.push(remote)
    }
}

impl<A, C: CmRDT> Log<A, C>
    where C::Op: serde::Serialize + serde::de::DeserializeOwned,
          A: Actor + serde::Serialize + serde::de::DeserializeOwned
{
    pub fn new(actor: A, repo: git2::Repository, root_key: KeyHierarchy) -> Self {
        let actor_bytes = bincode::serialize(&actor).unwrap();
        let actor_key = root_key
            .derive_child(&actor_bytes);

        Log { root_key, actor_key, log: git_log::Log::new(actor, repo) }
    }
}
