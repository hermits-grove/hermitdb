extern crate crdts;

use std::collection::BTreeMap;

trait TaggedOp {
    type ID: Eq;

    fn id(&self) -> Self::ID;
}

trait Log {
    type Op: TaggedOp;

    fn next(&self) -> Option<Self::Op>;
    fn ack(&mut self, op: &Self::Op::ID) -> Option<()>;
}

struct MemoryOp<A: Actor, C: CmRDT> {
    actor: A,
    index: usize,
    op: C::Op
}

impl<A: Actor> TaggedOp for MemoryOp<A> {
    type ID = (A, usize);
    fn id(&self) -> Self::ID {
        (self.actor.clone(), self.index)
    }
}

struct MemoryLog<A: crdts::Actor, C: crdts::CmRDT> {
    actor_logs: BTreeMap<A, (usize, Vec<C::Op>)>
}

impl<A: crdts::Actor, C: crdts::CmRDT> Log<A, usize> for MemoryLog {
    type Op = MemoryOp<A>;

    fn next(&self) -> Option<Self::Op> {
        let largest_lag = self.actor_logs.iter()
            .max_by_key(|(a, (index, log))| log.size() - index());

        if let Some((actor, (index, log))) = largest_lag {
            if index >= log.len() {
                None
            } else {
                Some(MemoryOp {
                    actor: actor.clone(),
                    index: index,
                    op: log[index].clone()
                })
            }
        } else {
            None
        }
    }

    fn ack(&mut self, op: &Self::Op) {
        /// We can ack ops that are not present in the log
        
        let (actor, index) = op.id();
        
        let log = self.actor_logs.entry(&actor)
            .or_insert((0, Vec::new()));
            
        *log.0 = index;
    }
}
