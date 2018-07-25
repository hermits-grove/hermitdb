use std::collections::BTreeMap;
use std::fmt::Debug;

use crdts::{CmRDT, Actor};
use log::{TaggedOp, LogReplicable};
use error::Result;

#[derive(Debug, Clone)]
pub struct Log<A: Actor, C: Debug + CmRDT> {
    actor: A,
    logs: BTreeMap<A, (u64, Vec<C::Op>)>
}

#[derive(Debug, Clone)]
pub struct Op<A: Actor, C: Debug + CmRDT> {
    actor: A,
    index: u64,
    op: C::Op
}

impl<A: Actor, C: Debug + CmRDT> TaggedOp<C> for Op<A, C> {
    type ID = (A, u64);

    fn id(&self) -> Self::ID {
        (self.actor.clone(), self.index)
    }

    fn op(&self) -> &C::Op {
        &self.op
    }
}

impl<A: Actor, C: Debug + CmRDT> LogReplicable<A, C> for Log<A, C> {
    type Op = Op<A, C>;

    fn next(&self) -> Result<Option<Self::Op>> {
        let largest_lag = self.logs.iter()
            .max_by_key(|(_, (index, log))| (log.len() as u64) - *index);

        if let Some((actor, (index, log))) = largest_lag {
            if *index >= log.len() as u64 {
                Ok(None)
            } else {
                Ok(Some(Op {
                    actor: actor.clone(),
                    index: *index,
                    op: log[*index as usize].clone()
                }))
            }
        } else {
            Ok(None)
        }
    }

    fn ack(&mut self, op: &Self::Op) -> Result<()> {
        // We can ack ops that are not present in the log
        
        let (actor, index) = op.id();
        
        let log = self.logs.entry(actor)
            .or_insert_with(|| (0, Vec::new()));
            
        log.0 = index + 1;
        Ok(())
    }

    fn commit(&mut self, op: C::Op) -> Result<Self::Op> {
        let log = self.logs.entry(self.actor.clone())
            .or_insert_with(|| (0, Vec::new()));

        log.1.push(op.clone());

        Ok(Op {
            actor: self.actor.clone(),
            index: log.0,
            op: op
        })
    }

    fn pull(&mut self, other: &Self) -> Result<()> {
        for (actor, (_, log)) in other.logs.iter() {
            let entry = self.logs.entry(actor.clone())
                .or_insert_with(|| (0, vec![]));

            if log.len() > entry.1.len() {
                for i in (entry.1.len())..log.len() {
                    entry.1.push(log[i as usize].clone());
                }
            }
        }
        Ok(())
    }

    fn push(&self, other: &mut Self) -> Result<()> {
        other.pull(self)
    }
}

impl<A: Actor, C: Debug + CmRDT> Log<A, C> {
    pub fn new(actor: A) -> Self {
        Log {
            actor: actor,
            logs: BTreeMap::new()
        }
    }
}
