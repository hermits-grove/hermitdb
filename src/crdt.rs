use std::marker::PhantomData;
use std::fmt::Debug;

use bincode;
use sled;
use serde::Serialize;
use serde::de::DeserializeOwned;

use error::{self, Error, Result};
use crdts::traits::{Causal, CvRDT, CmRDT};
use crdts::vclock::{VClock, Actor};
use crdts;

/// Key Trait alias to reduce redundancy in type decl.
pub trait Key: Debug + Ord + Clone + Send + Serialize + DeserializeOwned {}
impl<T: Debug + Ord + Clone + Send + Serialize + DeserializeOwned> Key for T {}

/// Val Trait alias to reduce redundancy in type decl.
pub trait Val<A: Actor>
    : Debug + Default + Clone + Send + Serialize + DeserializeOwned
    + Causal<A> + CmRDT + CvRDT
{}

impl<A, T> Val<A> for T where
    A: Actor,
    T: Debug + Default + Clone + Send + Serialize + DeserializeOwned
    + Causal<A> + CmRDT + CvRDT
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

#[serde(bound(deserialize = ""))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Entry<V: Val<A>, A: Actor> {
    // The entry clock tells us which actors have last changed this entry.
    // This clock will tell us what to do with this entry in the case of a merge
    // where only one map has this entry.
    //
    // e.g. say replica A has key `"user_32"` but replica B does not. We need to
    // decide whether replica B has processed an `rm("user_32")` operation
    // and replica A has not or replica A has processed a update("key")
    // operation which has not been seen by replica B yet.
    //
    // This conflict can be resolved by comparing replica B's Map.clock to the
    // the "user_32" Entry clock in replica A.
    // If B's clock is >=  "user_32"'s clock, then we know that B has
    // seen this entry and removed it, otherwise B has not received the update
    // operation so we keep the key.
    clock: VClock<A>,

    // The nested CRDT
    val: V
}

pub struct Iter<'a, K: Key, V: Val<A>, A: Actor> {
    iter: sled::Iter<'a>,
    phantom_key: PhantomData<K>,
    phantom_val: PhantomData<V>,
    phantom_actor: PhantomData<A>
}

impl<'a, K: Key, V: Val<A>, A: Actor> Iterator for Iter<'a, K, V, A> {
    type Item = Result<(K, V)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Ok((k, v))) => {
                let res = bincode::deserialize(&k[KEY_PREFIX.len()..])
                    .and_then(|key: K| {
                        bincode::deserialize(&v)
                            .map(|val: Entry<V, A>| (key, val.val))
                    });

                Some(res.map_err(|e| Error::from(e)))
            },
            Some(Err(e)) => Some(Err(Error::from(e))),
            None => None
        }
    }
}

/// Operations which can be applied to the Map CRDT
#[serde(bound(deserialize = ""))]
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
        clock: VClock<A>,
        /// Key of the value to update
        key: K,
        /// The operation to apply on the value under `key`
        op: V::Op
    }
}

impl<K: Key + Debug, V: Val<A> + Debug, A: Actor> CmRDT for Map<K, V, A> {
    type Error = error::Error;
    type Op = Op<K, V, A>;

    fn apply(&mut self, op: &Self::Op) -> Result<()> {
        let mut map_clock = self.get_clock()?;
        match op.clone() {
            Op::Nop => {/* do nothing */},
            Op::Rm { clock, key } => {
                if !(map_clock >= clock) {
                    let key_bytes = self.key_bytes(&key)?;
                    let del_res = self.tree.del(&key_bytes)?;
                    if let Some(entry_bytes) = del_res {
                        let mut entry: Entry<V, A> = bincode::deserialize(&entry_bytes)?;
                        entry.clock.subtract(&clock);
                        if !entry.clock.is_empty() {
                            entry.val.truncate(&clock);
                            let new_entry_bytes = bincode::serialize(&entry)?;
                            self.tree.set(key_bytes, new_entry_bytes)?;
                        } else {
                            // the entry clock has been dominated by the
                            // remove op clock, so we remove (already did)
                        }
                    }
                    map_clock.merge(&clock);
                    self.put_clock(map_clock)?;
                    self.tree.flush()?;
                }
            },
            Op::Up { clock, key, op } => {
                if !(map_clock >= clock) {
                    let key_bytes = self.key_bytes(&key)?;
                    let entry_res = self.tree.del(&key_bytes)?;

                    let mut entry = if let Some(entry_bytes) = entry_res {
                        bincode::deserialize(&entry_bytes)?
                    } else {
                        Entry {
                            clock: clock.clone(),
                            val: V::default()
                        }
                    };

                    entry.clock.merge(&clock);
                    entry.val.apply(&op)
                        .map_err(|_| crdts::Error::NestedOpFailed)?;
                    let entry_bytes = bincode::serialize(&entry)?;
                    self.tree.set(key_bytes, entry_bytes)?;
                    map_clock.merge(&clock);
                    self.put_clock(map_clock)?;
                    self.tree.flush()?;
                }
            }
        }
        Ok(())
    }
}

/// Key prefix is added to the front of all user added keys
const KEY_PREFIX: [u8; 1] = [1];

/// Meta prefix is added to the front of all housekeeping keys created by the database
const META_PREFIX: [u8; 1] = [0];

impl<K: Key + Debug, V: Val<A> + Debug, A: Actor> Map<K, V, A> {
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
    pub fn get(&self, key: &K) -> Result<Option<V>> {
        let key_bytes = self.key_bytes(&key)?;

        let val_opt = if let Some(val_bytes) = self.tree.get(&key_bytes)? {
            let entry: Entry<V, A> = bincode::deserialize(&val_bytes)?;
            Some(entry.val)
        } else {
            None
        };

        Ok(val_opt)
    }

    /// Update a value under some key, if the key is not present in the map,
    /// the updater will be given `None`, otherwise `Some(val)` is given.
    ///
    /// The updater must return Some(val) to have the updated val stored back in
    /// the Map. If None is returned, this entry is removed from the Map.
    pub fn update(
        &mut self,
        key: K,
        updater: impl FnOnce(V) -> Option<V::Op>,
        actor: A
    ) -> Result<Op<K, V, A>> {
        let mut clock = self.get_clock()?;
        clock.increment(actor.clone());

        let val_opt = self.get(&key)?;
        let val_exists = val_opt.is_some();
        let val = val_opt.unwrap_or(V::default());

        let op = if let Some(op) = updater(val) {
            Op::Up { clock, key, op }
        } else if val_exists {
            Op::Rm { clock, key }
        } else {
            Op::Nop
        };

        self.apply(&op)?;
        Ok(op)
    }

    /// Remove an entry from the Map
    pub fn rm(&mut self, key: K, actor: A) -> Result<Op<K, V, A>> {
        let mut clock = self.get_clock()?;
        clock.increment(actor.clone());
        let op = Op::Rm { clock, key };
        self.apply(&op)?;
        Ok(op)
    }

    pub fn iter<'a>(&'a self) -> Iter<'a, K, V, A> {
        Iter {
            iter: self.tree.scan(&KEY_PREFIX),
            phantom_key: PhantomData,
            phantom_val: PhantomData,
            phantom_actor: PhantomData
        }
    }

    fn get_clock(&self) -> Result<crdts::VClock<A>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::{Arbitrary, Gen, TestResult};

    use crdts::lwwreg::LWWReg;

    type TActor = u8;
    type TKey = u8;
    type TVal = LWWReg<u8, (u64, TActor)>;
    type InnerMap = crdts::Map<TKey, TVal, TActor>;
    type TOp = Op<TKey, crdts::Map<TKey, TVal, TActor>, TActor>;
    type TMap =  Map<TKey, InnerMap, TActor>;

    // We can't impl on types outside this module ie. '(u8, Vec<_>)' so we wrap.
    #[derive(Debug, Clone)]
    struct OpVec(TActor, Vec<TOp>);

    fn mk_tree() -> sled::Tree {
        let config = sled::ConfigBuilder::new().temporary(true).build();
        sled::Tree::start(config).unwrap()
    }

    impl Arbitrary for OpVec {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let actor = TActor::arbitrary(g);
            let num_ops: u8 = g.gen_range(0, 50);

            let mut map = TMap::new(mk_tree());
            let mut ops = Vec::with_capacity(num_ops as usize);
            for _ in 0..num_ops {
                let die_roll: u8 = g.gen();
                let key = g.gen();
                let op = match die_roll % 3 {
                    0 => {
                        // update inner map
                        map.update(key, |mut inner_map| {
                            let die_roll: u8 = g.gen();
                            let inner_key = g.gen();
                            match die_roll % 4 {
                                0 => {
                                    // update key inner rm
                                    let op = inner_map
                                        .update(inner_key, |_| None, actor.clone());
                                    Some(op)
                                },
                                1 => {
                                    // update key and val update
                                    let op = inner_map.update(inner_key, |mut reg| {
                                        reg.update(
                                            g.gen(),
                                            (g.gen(), actor.clone())
                                        ).unwrap();
                                        Some(reg)
                                    }, actor.clone());
                                    Some(op)
                                },
                                2 => {
                                    // inner rm
                                    let op = inner_map.rm(inner_key, actor.clone());
                                    Some(op)
                                },
                                _ => {
                                    // rm
                                    None
                                }
                            }
                        }, actor.clone()).unwrap()
                    },
                    1 => {
                        // rm
                        map.rm(key, actor.clone()).unwrap()
                    },
                    _ => {
                        // nop
                        Op::Nop
                    }
                };
                ops.push(op);
            }
            OpVec(actor, ops)
        }

        fn shrink(&self) -> Box<Iterator<Item = Self>> {
            let mut shrunk: Vec<Self> = Vec::new();
            for i in 0..self.1.len() {
                let mut vec = self.1.clone();
                vec.remove(i);
                shrunk.push(OpVec(self.0.clone(), vec))
            }
            Box::new(shrunk.into_iter())
        }
    }

    #[test]
    fn test_new() {
        let m: TMap = Map::new(mk_tree());
        assert_eq!(m.get(&0).unwrap(), None);
    }

    #[test]
    fn test_update() {
        let mut m: TMap = Map::new(mk_tree());

        // constructs a default value if does not exist
        m.update(
            101,
            |mut map| {
                Some(map.update(110, |mut reg| {
                    let new_val = (reg.val + 1) * 2;
                    let new_dot = (reg.dot.0 + 1, 1);
                    assert!(reg.update(new_val, new_dot).is_ok());
                    Some(reg)
                }, 1))
            },
            1
        ).unwrap();

        assert_eq!(
            m.get(&101).unwrap().unwrap().get(&110),
            Some(&LWWReg { val: 2, dot: (1, 1) })
        );

        // the map should give the latest val to the closure
        m.update(
            101,
            |mut map| {
                Some(map.update(110, |mut reg| {
                    assert_eq!(reg, LWWReg { val: 2, dot: (1, 1) });
                    let new_val = (reg.val + 1) * 2;
                    let new_dot = (reg.dot.0 + 1, 1);
                    assert!(reg.update(new_val, new_dot).is_ok());
                    Some(reg)
                }, 1))
            },
            1
        ).unwrap();

        assert_eq!(
            m.get(&101).unwrap().unwrap().get(&110),
            Some(&LWWReg { val: 6, dot: (2, 1) })
        );

        // Returning None from the closure should remove the element
        m.update(101, |_| None, 1).unwrap();

        assert_matches!(m.get(&101), Ok(None));
    }

    #[test]
    fn test_key_bytes() {
        let m: TMap = Map::new(mk_tree());
        let bytes = m.key_bytes(&101).unwrap();

        assert_eq!(bytes, vec![KEY_PREFIX[0], 101]);
                   
        let meta_bytes = m.meta_key_bytes(vec![101]);

        assert_eq!(meta_bytes, vec![META_PREFIX[0], 101]);
    }

    #[test]
    fn test_remove() {
        let mut m: TMap = Map::new(mk_tree());

        m.update(101, |mut m| Some(m.update(110, |r| Some(r), 1)), 1).unwrap();

        let mut inner_map: InnerMap = crdts::Map::new();
        inner_map.update(110, |r| Some(r), 1);
        assert_eq!(m.get(&101).unwrap(), Some(inner_map));

        m.rm(101, 1).unwrap();
        assert_eq!(m.get(&101).unwrap(), None);
    }

    #[test]
    fn test_reset_remove_semantics() {
        let mut m1 = TMap::new(mk_tree());
        let m1_op1 = m1.update(
            101,
            |mut map| {
                let op = map.update(
                    110,
                    |mut reg| {
                        reg.update(32, (0, 74)).unwrap();
                        Some(reg)
                    },
                    74
                );
                Some(op)
            },
            74
        ).unwrap();

        let mut m2 = TMap::new(mk_tree());
        m2.apply(&m1_op1).unwrap();

        let m1_op2 = m1.rm(101, 74).unwrap();

        let m2_op1 = m2.update(
            101,
            |mut map| {
                let op = map.update(
                    220,
                    |mut reg| {
                        reg.update(5, (0, 37)).unwrap();
                        Some(reg)
                    },
                    37
                );
                Some(op)
            },
            37
        ).unwrap();

        m1.apply(&m2_op1).unwrap();
        m2.apply(&m1_op2).unwrap();

        let inner_map = m1.get(&101).unwrap().unwrap();
        assert_matches!(inner_map.get(&220), Some(&LWWReg { val: 5, dot: (0, 37) }));
        assert_matches!(inner_map.get(&110), None);
        assert_eq!(inner_map.len(), 1);
    }

    #[test]
    fn test_updating_with_current_clock_should_be_a_nop() {
        let mut m1 = TMap::new(mk_tree());

        let res = m1.apply(&Op::Up {
            clock: VClock::new(),
            key: 0,
            op: crdts::map::Op::Up {
                clock: VClock::new(),
                key: 1,
                op: LWWReg {
                    val: 0,
                    dot: (0, 0)
                }
            }
        });

        assert!(res.is_ok());
        assert_eq!(m1.get(&0).unwrap(), None);
    }

    #[test]
    fn test_concurrent_add_and_remove_biases_towards_add() {
        let mut m1 = TMap::new(mk_tree());
        let mut m2 = TMap::new(mk_tree());

        let op1 = m1.rm(102, 75).unwrap();
        let op2 = m2.update(102, |_| Some(crdts::map::Op::Nop), 61).unwrap();

        assert!(m1.apply(&op2).is_ok());
        assert!(m2.apply(&op1).is_ok());

        // we bias towards adds
        assert_matches!(m1.get(&102).unwrap(), Some(_));
    }

    #[test]
    fn test_order_of_remove_and_update_does_not_matter() {
        let mut m1 = TMap::new(mk_tree());
        let mut m2 = TMap::new(mk_tree());

        let op1 = m1.update(0, |_| Some(crdts::map::Op::Nop), 35).unwrap();
        let op2 = m2.rm(1, 47).unwrap();

        assert!(m1.apply(&op2).is_ok());
        assert!(m2.apply(&op1).is_ok());

        assert_eq!(m1.get(&0).unwrap(), m2.get(&0).unwrap());
        assert_eq!(m1.get(&1).unwrap(), m2.get(&1).unwrap());
    }

    fn apply_ops(map: &mut TMap, ops: &[TOp]) {
        for op in ops.iter() {
            map.apply(op).unwrap()
        }
    }

    quickcheck! {
        fn prop_exchange_ops_converges(ops1: OpVec, ops2: OpVec) -> TestResult {
            if ops1.0 == ops2.0 {
                return TestResult::discard();
            }

            let mut m1: TMap = Map::new(mk_tree());
            let mut m2: TMap = Map::new(mk_tree());

            apply_ops(&mut m1, &ops1.1);
            apply_ops(&mut m2, &ops2.1);

            apply_ops(&mut m1, &ops2.1);
            apply_ops(&mut m2, &ops1.1);

            let m1_state: Vec<(u8, InnerMap)> = m1
                .iter()
                .map(|v| v.unwrap())
                .collect();
            let m2_state: Vec<(u8, InnerMap)> = m2
                .iter()
                .map(|v| v.unwrap())
                .collect();

            assert_eq!(m1_state, m2_state);
            TestResult::from_bool(true)
        }

        fn prop_associative(
            ops1: OpVec,
            ops2: OpVec,
            ops3: OpVec
        ) -> TestResult {
            if ops1.0 == ops2.0 || ops1.0 == ops3.0 || ops2.0 == ops3.0 {
                return TestResult::discard();
            }

            let mut m1: TMap = Map::new(mk_tree());
            let mut m2: TMap = Map::new(mk_tree());

            apply_ops(&mut m1, &ops1.1);
            apply_ops(&mut m2, &ops2.1);

            // (m1 ^ m2) ^ m3
            apply_ops(&mut m1, &ops2.1);
            apply_ops(&mut m1, &ops3.1);
            

            // m1 ^ (m2 ^ m3)
            apply_ops(&mut m2, &ops3.1);
            apply_ops(&mut m2, &ops1.1);

            // (m1 ^ m2) ^ m3 = m1 ^ (m2 ^ m3)
            let m1_state: Vec<(u8, InnerMap)> = m1
                .iter()
                .map(|v| v.unwrap())
                .collect();
            let m2_state: Vec<(u8, InnerMap)> = m2
                .iter()
                .map(|v| v.unwrap())
                .collect();

            assert_eq!(m1_state, m2_state);
            TestResult::from_bool(true)
        }

        fn prop_idempotent(ops: OpVec) -> bool {
            let mut m: TMap = Map::new(mk_tree());
            let mut m_clone: TMap = Map::new(mk_tree());
            apply_ops(&mut m, &ops.1);
            apply_ops(&mut m_clone, &ops.1);

            apply_ops(&mut m, &ops.1); // apply ops once more

            // m ^ m = m
            let m_state: Vec<(u8, InnerMap)> = m
                .iter()
                .map(|v| v.unwrap())
                .collect();

            let m_clone_state: Vec<(u8, InnerMap)> = m_clone
                .iter()
                .map(|v| v.unwrap())
                .collect();

            assert_eq!(m_state, m_clone_state);
            true
        }
    }
}
