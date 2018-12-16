use std::marker::PhantomData;
use std::fmt::Debug;
use std::collections::{HashMap, BTreeSet};

use bincode;
use sled;
use serde_derive::{Serialize, Deserialize};
use crdts::{Causal, CvRDT, CmRDT, VClock, Dot, Actor, ReadCtx, AddCtx, RmCtx};

use crate::error::{Error, Result};

/// Key Trait alias to reduce redundancy in type decl.
pub trait Key: Debug + Ord + Clone + Send {}
impl<T: Debug + Ord + Clone + Send> Key for T {}

/// Val Trait alias to reduce redundancy in type decl.
pub trait Val<A: Actor>
    : Debug + Default + Clone + Send + Causal<A> + CmRDT + CvRDT
{}

impl<A: Actor, T> Val<A> for T where
    T: Debug + Default + Clone + Send + Causal<A> + CmRDT + CvRDT
{}

#[derive(Debug)]
pub struct Map<K: Key, V: Val<A>, A: Actor> {
    // This clock stores the current version of the Map, it should
    // be greator or equal to all Entry clock's in the Map.
    tree: sled::Tree,
    phantom_key: PhantomData<K>,
    phantom_val: PhantomData<V>,
    phantom_actor: PhantomData<A>
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<V: Val<A>, A: Actor> {
    // The entry clock tells us which actors edited this entry.
    pub clock: VClock<A>,

    // The nested CRDT
    pub val: V
}

pub struct Iter<'a, K: Key, V: Val<A>, A: Actor> {
    iter: sled::Iter<'a>,
    clock: VClock<A>,
    phantom_key: PhantomData<K>,
    phantom_val: PhantomData<V>,
    phantom_actor: PhantomData<A>
}

/// Operations which can be applied to the Map CRDT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op<K: Key, V: Val<A>, A: Actor> {
    /// No change to the CRDT
    Nop,
    /// Remove a key from the map
    Rm {
        /// Remove context
        clock: VClock<A>,
        /// Key to remove
        key: K
    },
    /// Update an entry in the map
    Up {
        /// Update context
        dot: Dot<A>,
        /// Key of the value to update
        key: K,
        /// The operation to apply on the value under `key`
        op: V::Op
    }
}

impl<'a, K, V, A> Iterator for Iter<'a, K, V, A>
    where K: Key + serde::de::DeserializeOwned,
          A: Actor + serde::de::DeserializeOwned,
          V: Val<A> + serde::de::DeserializeOwned
{
    type Item = Result<(K, ReadCtx<V, A>)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Ok((k, v))) => {
                let res = bincode::deserialize(&k[KEY_PREFIX.len()..])
                    .and_then(|key: K| {
                        let entry: Entry<V, A> = bincode::deserialize(&v)?;
                        Ok((key, ReadCtx {
                            add_clock: self.clock.clone(),
                            rm_clock: entry.clock,
                            val: entry.val
                        }))
                    });

                Some(res.map_err(|e| Error::from(e)))
            },
            Some(Err(e)) => Some(Err(Error::from(e))),
            None => None
        }
    }
}

impl<K, V, A> CmRDT for Map<K, V, A>
    where K: Key + Debug + serde::Serialize + serde::de::DeserializeOwned,
          A: Actor + serde::Serialize + serde::de::DeserializeOwned,
          V: Val<A> + Debug + serde::Serialize + serde::de::DeserializeOwned,
{
    type Op = Op<K, V, A>;

    fn apply(&mut self, op: &Self::Op) {
        match op.clone()  {
            Op::Nop => {/* do nothing */},
            Op::Rm { clock, key } => {
                self.apply_rm(key, &clock).unwrap();
                self.tree.flush().unwrap();
            },
            Op::Up { dot, key, op } => {
                let mut map_clock = self.get_clock().unwrap();
                if map_clock.get(&dot.actor) >= dot.counter {
                    // we've seen this op already
                    return;
                }

                let key_bytes = self.key_bytes(&key).unwrap();
                
                let mut entry = match self.tree.get(&key_bytes).unwrap() {
                    Some(bytes) => bincode::deserialize(&bytes).unwrap(),
                    None => Entry {
                        clock: VClock::new(),
                        val: V::default()
                    }
                };

                entry.clock.apply(&dot);
                entry.val.apply(&op);
                let entry_bytes = bincode::serialize(&entry).unwrap();
                self.tree.set(key_bytes, entry_bytes).unwrap();

                map_clock.apply(&dot);
                self.put_clock(map_clock).unwrap();
                self.apply_deferred().unwrap();
                self.tree.flush().unwrap();
            }
        }
    }
}

/// Key prefix is added to the front of all user added keys
const KEY_PREFIX: [u8; 1] = [1];

/// Meta prefix is added to the front of all housekeeping keys created by the database
const META_PREFIX: [u8; 1] = [0];

impl<K, V, A> Map<K, V, A>
    where K: Key + Debug + serde::Serialize + serde::de::DeserializeOwned,
          A: Actor + serde::Serialize + serde::de::DeserializeOwned,
          V: Val<A> + Debug + serde::Serialize + serde::de::DeserializeOwned,
{
    /// Constructs an empty Map
    pub fn new(tree: sled::Tree) -> Map<K, V, A> {
        Map {
            tree: tree,
            phantom_key: PhantomData,
            phantom_val: PhantomData,
            phantom_actor: PhantomData
         }
    }

    pub fn key_bytes(&self, key: &K) -> Result<Vec<u8>> {
        let mut bytes = bincode::serialize(&key)?;
        bytes.splice(0..0, KEY_PREFIX.iter().cloned());
        Ok(bytes)
    }

    pub fn meta_key_bytes(&self, mut key: Vec<u8>) -> Vec<u8> {
        key.splice(0..0, META_PREFIX.iter().cloned());
        key
    }

    /// Get a value stored under a key
    pub fn get(&self, key: &K) -> Result<ReadCtx<Option<V>, A>> {
        let key_bytes = self.key_bytes(&key)?;

        let entry_opt = if let Some(val_bytes) = self.tree.get(&key_bytes)? {
            let entry: Entry<V, A> = bincode::deserialize(&val_bytes)?;
            Some(entry)
        } else {
            None
        };

        Ok(ReadCtx {
            add_clock: self.get_clock()?,
            rm_clock: entry_opt.clone()
                .map(|map_entry| map_entry.clock.clone())
                .unwrap_or_else(|| VClock::new()),
            val: entry_opt
                .map(|map_entry| map_entry.val.clone())
        })
    }

    /// Update a value under some key, if the key is not present in the map,
    /// the updater will be given `None`, otherwise `Some(val)` is given.
    ///
    /// The updater must return Some(val) to have the updated val stored back in
    /// the Map. If None is returned, this entry is removed from the Map.
    pub fn update<F, O, I>(&self, key: I, ctx: AddCtx<A>, f: F) -> Result<Op<K, V, A>>
        where F: FnOnce(&V, AddCtx<A>) -> O,
              O: Into<V::Op>,
              I: Into<K>
    {
        let key = key.into();
        let op = if let Some(data) = self.get(&key)?.val {
            f(&data, ctx.clone()).into()
        } else {
            f(&V::default(), ctx.clone()).into()
        };
        Ok(Op::Up { dot: ctx.dot, key, op })
    }

    /// Remove an entry from the Map
    pub fn rm(&self, key: impl Into<K>, ctx: RmCtx<A>) -> Op<K, V, A> {
        Op::Rm { clock: ctx.clock, key: key.into() }
    }

    pub fn iter<'a>(&'a self) -> Result<Iter<'a, K, V, A>> {
        Ok(Iter {
            iter: self.tree.scan(&KEY_PREFIX),
            clock: self.get_clock()?,
            phantom_key: PhantomData,
            phantom_val: PhantomData,
            phantom_actor: PhantomData
        })
    }

    fn apply_deferred(&mut self) -> Result<()> {
        let deferred = self.get_deferred()?;
        // TODO: it would be good to not clear the deferred map if we can avoid it.
        //       this could be a point of data loss if we have a failure before we
        //       finish applying all the deferred removes
        self.put_deferred(HashMap::new())?;
        for (clock, keys) in deferred {
            for key in keys {
                self.apply_rm(key, &clock)?;
            }
        }
        Ok(())
    }

    /// Apply a key removal given a context.
    fn apply_rm(&mut self, key: K, clock: &VClock<A>) -> Result<()> {
        let map_clock = self.get_clock()?;
        if !(clock <= &map_clock) {
            let mut deferred = self.get_deferred()?;
            {
                let deferred_set = deferred.entry(clock.clone())
                    .or_insert_with(|| BTreeSet::new());
                deferred_set.insert(key.clone());
            }
            self.put_deferred(deferred)?;
        }

        let key_bytes = self.key_bytes(&key)?;
        if let Some(entry_bytes) = self.tree.del(&key_bytes)? {
            let mut entry: Entry<V, A> = bincode::deserialize(&entry_bytes)?;
            entry.clock.subtract(&clock);
            if !entry.clock.is_empty() {
                entry.val.truncate(&clock);
                let new_entry_bytes = bincode::serialize(&entry)?;
                self.tree.set(key_bytes, new_entry_bytes)?;
            }
        }
        Ok(())
    }

    fn get_clock(&self) -> Result<VClock<A>> {
        let clock_key = self.meta_key_bytes("clock".as_bytes().to_vec());
        let clock = if let Some(clock_bytes) = self.tree.get(&clock_key)? {
            bincode::deserialize(&clock_bytes)?
        } else {
            VClock::new()
        };
        Ok(clock)
    }

    fn put_clock(&self, clock: VClock<A>) -> Result<()> {
        let clock_key = self.meta_key_bytes("clock".as_bytes().to_vec());
        let clock_bytes = bincode::serialize(&clock)?;
        self.tree.set(clock_key, clock_bytes)?;
        Ok(())
    }

    fn get_deferred(&self) -> Result<HashMap<VClock<A>, BTreeSet<K>>> {
        let deferred_key = self.meta_key_bytes("deferred".as_bytes().to_vec());
        if let Some(deferred_bytes) = self.tree.get(&deferred_key)? {
            let deferred = bincode::deserialize(&deferred_bytes)?;
            Ok(deferred)
        } else {
            Ok(HashMap::new())
        }
    }

    fn put_deferred(&mut self, deferred: HashMap<VClock<A>, BTreeSet<K>>) -> Result<()> {
        let deferred_key = self.meta_key_bytes("deferred".as_bytes().to_vec());
        let deferred_bytes = bincode::serialize(&deferred)?;
        self.tree.set(deferred_key, deferred_bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crdts::{self, map, mvreg, MVReg};

    type TestActor = u8;
    type TestKey = u8;
    type TestVal = MVReg<u8, TestActor>;
    type TestMap =  Map<TestKey, crdts::Map<TestKey, TestVal, TestActor>, TestActor>;

    fn mk_tree() -> sled::Tree {
        let config = sled::ConfigBuilder::new().temporary(true).build();
        sled::Tree::start(config).unwrap()
    }
    
    #[test]
    fn test_op_exchange_converges_quickcheck1() {
        let op_actor1 = Op::Up {
            dot: Dot { actor: 0, counter: 3 },
            key: 9,
            op: map::Op::Up {
                dot: Dot { actor: 0, counter: 3 },
                key: 0,
                op: mvreg::Op::Put {
                    clock: Dot { actor: 0, counter: 3 }.into(),
                    val: 0
                }
            }
        };
        let op_1_actor2 = Op::Up {
            dot: Dot { actor: 1, counter: 1 },
            key: 9,
            op: map::Op::Rm {
                clock: Dot { actor: 1, counter: 1 }.into(),
                key: 0
            }
        };
        let op_2_actor2 = Op::Rm {
            clock: Dot { actor: 1, counter: 2 }.into(),
            key: 9
        };
        
        let mut m1: TestMap = Map::new(mk_tree());
        let mut m2: TestMap = Map::new(mk_tree());

        m1.apply(&op_actor1);
        m2.apply(&op_1_actor2);
        m2.apply(&op_2_actor2);

        // m1 <- m2
        m1.apply(&op_1_actor2);
        m1.apply(&op_2_actor2);

        // m2 <- m1
        m2.apply(&op_actor1);
        
        // m1 <- m2 == m2 <- m1
        assert_eq!(
            m1.iter().unwrap().map(|e| e.unwrap()).collect::<Vec<_>>(),
            m2.iter().unwrap().map(|e| e.unwrap()).collect::<Vec<_>>()
        );
    }
}
