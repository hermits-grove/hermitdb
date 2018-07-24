use std::collections::BTreeMap;
use std::fmt::Debug;

use crdts::{CmRDT, Actor};
use log::{TaggedOp, LogReplicable};
use error::Result;

#[derive(Debug, Clone)]
pub struct MemoryLog<A: Actor, C: Debug + CmRDT> {
    actor: A,
    logs: BTreeMap<A, (u64, Vec<C::Op>)>
}

#[derive(Debug, Clone)]
pub struct MemoryOp<A: Actor, C: Debug + CmRDT> {
    actor: A,
    index: u64,
    op: C::Op
}

impl<A: Actor, C: Debug + CmRDT> TaggedOp<C> for MemoryOp<A, C> {
    type ID = (A, u64);

    fn id(&self) -> Self::ID {
        (self.actor.clone(), self.index)
    }

    fn op(&self) -> &C::Op {
        &self.op
    }
}

impl<A: Actor, C: Debug + CmRDT> LogReplicable<A, C> for MemoryLog<A, C> {
    type Op = MemoryOp<A, C>;

    fn next(&self) -> Result<Option<Self::Op>> {
        let largest_lag = self.logs.iter()
            .max_by_key(|(_, (index, log))| (log.len() as u64) - *index);

        if let Some((actor, (index, log))) = largest_lag {
            if *index >= log.len() as u64 {
                Ok(None)
            } else {
                Ok(Some(MemoryOp {
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

    fn commit(&mut self, op: C::Op) -> Result<()> {
        let log = self.logs.entry(self.actor.clone())
            .or_insert_with(|| (0, Vec::new()));

        log.1.push(op);
        Ok(())
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

impl<A: Actor, C: Debug + CmRDT> MemoryLog<A, C> {
    pub fn new(actor: A) -> Self {
        MemoryLog {
            actor: actor,
            logs: BTreeMap::new()
        }
    }
}
