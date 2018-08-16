use crdts::{CmRDT, VClock, Dot};

use error::Result;
use map;
use data::{Data, Op, Actor, Kind};
use log::{TaggedOp, LogReplicable};

pub type Map = map::Map<(String, Kind), Data, Actor>;

pub struct DB<L: LogReplicable<Actor, Map>> {
    log: L,
    map: Map
}

impl<L: LogReplicable<Actor, Map>> DB<L> {
    pub fn new(log: L, map: Map) -> Self {
        DB { log, map }
    }

    pub fn dot(&self, actor: Actor) -> Result<Dot<Actor>> {
        self.map.dot(actor)
    }

    pub fn get(&self, key: &(String, Kind)) -> Result<Option<map::Entry<Data, Actor>>> {
        self.map.get(key)
    }

    pub fn update<F, O>(&mut self, key: (impl Into<String>, Kind), dot: Dot<Actor>, updater: F) -> Result<()>
        where F: FnOnce(Data, Dot<Actor>) -> O,
              O: Into<Op>
    {
        let (key_str, key_kind) = key;
        let key = (key_str.into(), key_kind);

        let map_op = self.map.update(key, dot, updater)?.into();
        let tagged_op = self.log.commit(map_op)?;
        self.map.apply(tagged_op.op())?;
        self.log.ack(&tagged_op)
    }

    pub fn rm(&mut self, key: (impl Into<String>, Kind), context: VClock<Actor>) -> Result<()> {
        let (key_str, key_kind) = key;
        let key = (key_str.into(), key_kind);

        let op = self.map.rm(key, context);
        let tagged_op = self.log.commit(op)?;
        self.map.apply(tagged_op.op())?;
        self.log.ack(&tagged_op)
    }
    

    pub fn iter<'a>(&'a self) -> map::Iter<'a, (String, Kind), Data, Actor> {
        self.map.iter()
    }

    pub fn sync(&mut self, remote: &mut L::Remote ) -> Result<()> {
        self.log.sync(remote)?;

        while let Some(tagged_op) = self.log.next()? {
            self.map.apply(tagged_op.op())?;
            self.log.ack(&tagged_op)?;
        }
        Ok(())
    }
}
