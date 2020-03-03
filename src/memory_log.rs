use std::collections::BTreeMap;
use std::fmt::{self, Debug};

use crdts::{Actor, CmRDT};

use crate::error::Result;
use crate::log::{LogReplicable, TaggedOp};

pub struct Log<A: Actor, C: CmRDT> {
    actor: A,
    logs: BTreeMap<A, (u64, Vec<C::Op>)>,
}

pub struct LoggedOp<A: Actor, C: CmRDT> {
    actor: A,
    index: u64,
    op: C::Op,
}

impl<A: Actor, C: CmRDT> Debug for LoggedOp<A, C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "LoggedOp {{ actor: {:?}, index: {:?}, op: {:?} }}",
            self.actor, self.index, self.op
        )
    }
}

impl<A: Actor, C: CmRDT> TaggedOp<C> for LoggedOp<A, C> {
    type ID = (A, u64);

    fn id(&self) -> Self::ID {
        (self.actor.clone(), self.index)
    }

    fn op(&self) -> &C::Op {
        &self.op
    }
}

impl<A: Actor, C: CmRDT> LogReplicable<A, C> for Log<A, C> {
    type LoggedOp = LoggedOp<A, C>;
    type Remote = Self;

    fn next(&self) -> Result<Option<Self::LoggedOp>> {
        let largest_lag = self
            .logs
            .iter()
            .max_by_key(|(_, (index, log))| (log.len() as u64) - *index);

        if let Some((actor, (index, log))) = largest_lag {
            if *index >= log.len() as u64 {
                Ok(None)
            } else {
                Ok(Some(LoggedOp {
                    actor: actor.clone(),
                    index: *index,
                    op: log[*index as usize].clone(),
                }))
            }
        } else {
            Ok(None)
        }
    }

    fn ack(&mut self, logged_op: &Self::LoggedOp) -> Result<()> {
        // We can ack ops that are not present in the log

        let (actor, index) = logged_op.id();

        let log = self.logs.entry(actor).or_insert_with(|| (0, Vec::new()));

        log.0 = index + 1;
        Ok(())
    }

    fn commit(&mut self, op: C::Op) -> Result<Self::LoggedOp> {
        let log = self
            .logs
            .entry(self.actor.clone())
            .or_insert_with(|| (0, Vec::new()));

        log.1.push(op.clone());

        Ok(LoggedOp {
            actor: self.actor.clone(),
            index: log.0,
            op,
        })
    }

    fn pull(&mut self, remote: &Self::Remote) -> Result<()> {
        for (actor, (_, log)) in remote.logs.iter() {
            let entry = self
                .logs
                .entry(actor.clone())
                .or_insert_with(|| (0, vec![]));

            if log.len() > entry.1.len() {
                for i in (entry.1.len())..log.len() {
                    entry.1.push(log[i as usize].clone());
                }
            }
        }
        Ok(())
    }

    fn push(&self, remote: &mut Self::Remote) -> Result<()> {
        remote.pull(self)
    }
}

impl<A: Actor, C: CmRDT> Log<A, C> {
    pub fn new(actor: A) -> Self {
        Log {
            actor,
            logs: BTreeMap::new(),
        }
    }
}
