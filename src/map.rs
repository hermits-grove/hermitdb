use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::marker::PhantomData;

use bincode;
use crdts::ctx::{AddCtx, ReadCtx, RmCtx};
use crdts::{Actor, CmRDT, CvRDT, Dot, ResetRemove, VClock};
use serde_derive::{Deserialize, Serialize};
use sled;

use crate::error::{Error, Result};

/// Key Trait alias to reduce redundancy in type decl.
pub trait Key: Debug + Ord + Clone + Send {}
impl<T: Debug + Ord + Clone + Send> Key for T {}

/// Val Trait alias to reduce redundancy in type decl.
pub trait Val<A: Actor>: Debug + Default + Clone + Send + ResetRemove<A> + CmRDT + CvRDT {}

impl<A: Actor, T> Val<A> for T where T: Debug + Default + Clone + Send + ResetRemove<A> + CmRDT + CvRDT {}

#[derive(Debug)]
pub struct Map<K: Key, V: Val<A>, A: Actor> {
    // This clock stores the current version of the Map, it should
    // be greator or equal to all Entry clock's in the Map.
    sled: sled::Db,
    phantom_key: PhantomData<K>,
    phantom_val: PhantomData<V>,
    phantom_actor: PhantomData<A>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<V: Val<A>, A: Actor> {
    // The entry clock tells us which actors edited this entry.
    pub clock: VClock<A>,

    // The nested CRDT
    pub val: V,
}

pub struct Iter<K: Key, V: Val<A>, A: Actor> {
    iter: sled::Iter,
    clock: VClock<A>,
    phantom_key: PhantomData<K>,
    phantom_val: PhantomData<V>,
    phantom_actor: PhantomData<A>,
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
        key: K,
    },
    /// Update an entry in the map
    Up {
        /// Update context
        dot: Dot<A>,
        /// Key of the value to update
        key: K,
        /// The operation to apply on the value under `key`
        op: V::Op,
    },
}

impl<K, V, A> Iterator for Iter<K, V, A>
where
    K: Key + serde::de::DeserializeOwned,
    A: Actor + serde::de::DeserializeOwned,
    V: Val<A> + serde::de::DeserializeOwned,
{
    type Item = Result<(K, ReadCtx<V, A>)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Ok((k, v))) => {
                let res = bincode::deserialize(&k[KEY_PREFIX.len()..]).and_then(|key: K| {
                    let entry: Entry<V, A> = bincode::deserialize(&v)?;
                    Ok((
                        key,
                        ReadCtx {
                            add_clock: self.clock.clone(),
                            rm_clock: entry.clock,
                            val: entry.val,
                        },
                    ))
                });

                Some(res.map_err(Error::from))
            }
            Some(Err(e)) => Some(Err(Error::from(e))),
            None => None,
        }
    }
}

impl<K, V, A> CmRDT for Map<K, V, A>
where
    K: Key + Debug + serde::Serialize + serde::de::DeserializeOwned,
    A: Actor + Debug + serde::Serialize + serde::de::DeserializeOwned,
    V: Val<A> + Debug + serde::Serialize + serde::de::DeserializeOwned,
{
    type Op = Op<K, V, A>;
    type Validation = std::convert::Infallible;

    fn validate_op(&self, _op: &Self::Op) -> std::result::Result<(), Self::Validation> {
        Ok(())
    }

    fn apply(&mut self, op: Self::Op) {
        match op {
            Op::Nop => { /* do nothing */ }
            Op::Rm { clock, key } => {
                self.apply_rm(key, &clock).unwrap();
                self.sled.flush().unwrap();
            }
            Op::Up { dot, key, op } => {
                let mut map_clock = self.get_clock().unwrap();
                if map_clock.get(&dot.actor) >= dot.counter {
                    // we've seen this op already
                    return;
                }

                let key_bytes = self.key_bytes(&key).unwrap();

                let mut entry = match self.sled.get(&key_bytes).unwrap() {
                    Some(bytes) => bincode::deserialize(&bytes).unwrap(),
                    None => Entry {
                        clock: VClock::new(),
                        val: V::default(),
                    },
                };

                entry.clock.apply(dot.clone());
                entry.val.apply(op);
                let entry_bytes = bincode::serialize(&entry).unwrap();
                self.sled.insert(key_bytes, entry_bytes).unwrap();

                map_clock.apply(dot);
                self.put_clock(map_clock).unwrap();
                self.apply_deferred().unwrap();
                self.sled.flush().unwrap();
            }
        }
    }
}

/// Key prefix is added to the front of all user added keys
const KEY_PREFIX: [u8; 1] = [1];

/// Meta prefix is added to the front of all housekeeping keys created by the database
const META_PREFIX: [u8; 1] = [0];

impl<K, V, A> Map<K, V, A>
where
    K: Key + Debug + serde::Serialize + serde::de::DeserializeOwned,
    A: Actor + serde::Serialize + serde::de::DeserializeOwned,
    V: Val<A> + Debug + serde::Serialize + serde::de::DeserializeOwned,
{
    /// Constructs an empty Map
    pub fn new(sled: sled::Db) -> Map<K, V, A> {
        Map {
            sled,
            phantom_key: PhantomData,
            phantom_val: PhantomData,
            phantom_actor: PhantomData,
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
        let key_bytes = self.key_bytes(key)?;

        let entry_opt = if let Some(val_bytes) = self.sled.get(&key_bytes)? {
            let entry: Entry<V, A> = bincode::deserialize(&val_bytes)?;
            Some(entry)
        } else {
            None
        };

        Ok(ReadCtx {
            add_clock: self.get_clock()?,
            rm_clock: entry_opt
                .clone()
                .map(|map_entry| map_entry.clock)
                .unwrap_or_else(VClock::new),
            val: entry_opt.map(|map_entry| map_entry.val),
        })
    }

    /// Update a value under some key, if the key is not present in the map,
    /// the updater will be given `None`, otherwise `Some(val)` is given.
    ///
    /// The updater must return Some(val) to have the updated val stored back in
    /// the Map. If None is returned, this entry is removed from the Map.
    pub fn update<F, O, I>(&self, key: I, ctx: AddCtx<A>, f: F) -> Result<Op<K, V, A>>
    where
        F: FnOnce(&V, AddCtx<A>) -> O,
        O: Into<V::Op>,
        I: Into<K>,
    {
        let key = key.into();
        let dot = ctx.dot.clone();
        let op = if let Some(data) = self.get(&key)?.val {
            f(&data, ctx).into()
        } else {
            f(&V::default(), ctx).into()
        };
        Ok(Op::Up {
            dot,
            key,
            op,
        })
    }

    /// Remove an entry from the Map
    pub fn rm(&self, key: impl Into<K>, ctx: RmCtx<A>) -> Op<K, V, A> {
        Op::Rm {
            clock: ctx.clock,
            key: key.into(),
        }
    }

    pub fn iter(&self) -> Result<Iter<K, V, A>> {
        Ok(Iter {
            iter: self.sled.range(KEY_PREFIX..),
            clock: self.get_clock()?,
            phantom_key: PhantomData,
            phantom_val: PhantomData,
            phantom_actor: PhantomData,
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
        use std::cmp::Ordering;
        let map_clock = self.get_clock()?;
        // Defer the remove if clock is not causally preceded by map_clock
        // (i.e., clock is concurrent with or causally after map_clock)
        let should_defer = match clock.partial_cmp(&map_clock) {
            Some(Ordering::Less) | Some(Ordering::Equal) => false,
            Some(Ordering::Greater) | None => true,
        };
        if should_defer {
            let mut deferred = self.get_deferred()?;
            let deferred_set = deferred.entry(clock.clone()).or_insert_with(BTreeSet::new);
            deferred_set.insert(key.clone());
            self.put_deferred(deferred)?;
        }

        let key_bytes = self.key_bytes(&key)?;
        if let Some(entry_bytes) = self.sled.remove(&key_bytes)? {
            let mut entry: Entry<V, A> = bincode::deserialize(&entry_bytes)?;
            entry.clock = entry.clock.clone_without(clock);
            if !entry.clock.is_empty() {
                entry.val.reset_remove(clock);
                let new_entry_bytes = bincode::serialize(&entry)?;
                self.sled.insert(key_bytes, new_entry_bytes)?;
            }
        }
        Ok(())
    }

    fn get_clock(&self) -> Result<VClock<A>> {
        let clock_key = self.meta_key_bytes(b"clock".to_vec());
        let clock = if let Some(clock_bytes) = self.sled.get(&clock_key)? {
            bincode::deserialize(&clock_bytes)?
        } else {
            VClock::new()
        };
        Ok(clock)
    }

    fn put_clock(&self, clock: VClock<A>) -> Result<()> {
        let clock_key = self.meta_key_bytes(b"clock".to_vec());
        let clock_bytes = bincode::serialize(&clock)?;
        self.sled.insert(clock_key, clock_bytes)?;
        Ok(())
    }

    fn get_deferred(&self) -> Result<HashMap<VClock<A>, BTreeSet<K>>> {
        let deferred_key = self.meta_key_bytes(b"deferred".to_vec());
        if let Some(deferred_bytes) = self.sled.get(&deferred_key)? {
            let deferred = bincode::deserialize(&deferred_bytes)?;
            Ok(deferred)
        } else {
            Ok(HashMap::new())
        }
    }

    fn put_deferred(&mut self, deferred: HashMap<VClock<A>, BTreeSet<K>>) -> Result<()> {
        let deferred_key = self.meta_key_bytes(b"deferred".to_vec());
        let deferred_bytes = bincode::serialize(&deferred)?;
        self.sled.insert(deferred_key, deferred_bytes)?;
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
    type TestMap = Map<TestKey, crdts::Map<TestKey, TestVal, TestActor>, TestActor>;

    fn mk_map() -> TestMap {
        let sled = sled::Config::new().temporary(true).open().unwrap();
        Map::new(sled)
    }

    #[test]
    fn test_op_exchange_converges_quickcheck1() {
        let op_actor1 = Op::Up {
            dot: Dot {
                actor: 0,
                counter: 3,
            },
            key: 9,
            op: map::Op::Up {
                dot: Dot {
                    actor: 0,
                    counter: 3,
                },
                key: 0,
                op: mvreg::Op::Put {
                    clock: Dot {
                        actor: 0,
                        counter: 3,
                    }
                    .into(),
                    val: 0,
                },
            },
        };
        let op_1_actor2 = Op::Up {
            dot: Dot {
                actor: 1,
                counter: 1,
            },
            key: 9,
            op: map::Op::Rm {
                clock: Dot {
                    actor: 1,
                    counter: 1,
                }
                .into(),
                keyset: std::iter::once(0).collect(),
            },
        };
        let op_2_actor2 = Op::Rm {
            clock: Dot {
                actor: 1,
                counter: 2,
            }
            .into(),
            key: 9,
        };

        let mut m1: TestMap = mk_map();
        let mut m2: TestMap = mk_map();

        m1.apply(op_actor1.clone());
        m2.apply(op_1_actor2.clone());
        m2.apply(op_2_actor2.clone());

        // m1 <- m2
        m1.apply(op_1_actor2);
        m1.apply(op_2_actor2);

        // m2 <- m1
        m2.apply(op_actor1);

        // m1 <- m2 == m2 <- m1
        assert_eq!(
            m1.iter().unwrap().map(|e| e.unwrap()).collect::<Vec<_>>(),
            m2.iter().unwrap().map(|e| e.unwrap()).collect::<Vec<_>>()
        );
    }
}
