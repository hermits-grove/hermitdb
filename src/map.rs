use std::marker::PhantomData;
use std::fmt::Debug;
use std::collections::{HashMap, BTreeSet};

use bincode;
use sled;
use serde::Serialize;
use serde::de::DeserializeOwned;

use error::{self, Error, Result};
use crdts::traits::{Causal, CvRDT, CmRDT};
use crdts::vclock::{VClock, Dot, Actor};
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
    // The entry clock tells us which actors edited this entry.
    pub clock: VClock<A>,

    // The nested CRDT
    pub val: V
}

pub struct Iter<'a, K: Key, V: Val<A>, A: Actor> {
    iter: sled::Iter<'a>,
    phantom_key: PhantomData<K>,
    phantom_val: PhantomData<V>,
    phantom_actor: PhantomData<A>
}

impl<'a, K: Key, V: Val<A>, A: Actor> Iterator for Iter<'a, K, V, A> {
    type Item = Result<(K, Entry<V, A>)>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.iter.next() {
            Some(Ok((k, v))) => {
                let res = bincode::deserialize(&k[KEY_PREFIX.len()..])
                    .and_then(|key: K| {
                        bincode::deserialize(&v)
                            .map(|entry: Entry<V, A>| (key, entry))
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
        context: VClock<A>,
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

impl<K: Key + Debug, V: Val<A> + Debug, A: Actor> CmRDT for Map<K, V, A> {
    type Error = error::Error;
    type Op = Op<K, V, A>;

    fn apply(&mut self, op: &Self::Op) -> Result<()> {
        match op.clone()  {
            Op::Nop => {/* do nothing */},
            Op::Rm { context, key } => {
                self.apply_rm(key, &context)?;
                self.tree.flush()?;
            },
            Op::Up { dot: Dot { actor, counter }, key, op } => {
                let mut map_clock = self.get_clock()?;
                if map_clock.get(&actor) >= counter {
                    // we've seen this op already
                    return Ok(())
                }

                let key_bytes = self.key_bytes(&key)?;
                let mut entry = if let Some(bytes) = self.tree.get(&key_bytes)? {
                    bincode::deserialize(&bytes)?
                } else {
                    Entry {
                        clock: VClock::new(),
                        val: V::default()
                    }
                };

                entry.clock.witness(actor.clone(), counter).unwrap();
                entry.val.apply(&op).map_err(|_| crdts::Error::NestedOpFailed)?;
                let entry_bytes = bincode::serialize(&entry)?;
                self.tree.set(key_bytes, entry_bytes)?;

                map_clock.witness(actor, counter).unwrap();
                self.put_clock(map_clock)?;
                self.apply_deferred()?;
                self.tree.flush()?;
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

    pub fn dot(&self, actor: A) -> Result<Dot<A>> {
        let counter = self.get_clock()?.get(&actor) + 1;
        Ok(Dot { actor, counter })
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
    pub fn get(&self, key: &K) -> Result<Option<Entry<V, A>>> {
        let key_bytes = self.key_bytes(&key)?;

        let val_opt = if let Some(val_bytes) = self.tree.get(&key_bytes)? {
            let entry: Entry<V, A> = bincode::deserialize(&val_bytes)?;
            Some(entry)
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
    pub fn update<U, O>(&self, key: K, dot: Dot<A>, updater: U) -> Result<Op<K, V, A>>
        where U: FnOnce(V, Dot<A>) -> O,
              O: Into<V::Op>
    {
        let val = self.get(&key)
            ?.map(|entry| entry.val)
            .unwrap_or_else(|| V::default());

        let op = updater(val, dot.clone()).into();
        Ok(Op::Up { dot, key, op })
    }

    /// Remove an entry from the Map
    pub fn rm(&self, key: K, context: VClock<A>) -> Op<K, V, A> {
        Op::Rm { context, key }
    }

    pub fn iter<'a>(&'a self) -> Iter<'a, K, V, A> {
        Iter {
            iter: self.tree.scan(&KEY_PREFIX),
            phantom_key: PhantomData,
            phantom_val: PhantomData,
            phantom_actor: PhantomData
        }
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
    fn apply_rm(&mut self, key: K, context: &VClock<A>) -> Result<()> {
        let map_clock = self.get_clock()?;
        if !context.dominating_vclock(&map_clock).is_empty() {
            let mut deferred = self.get_deferred()?;
            {
                let deferred_set = deferred.entry(context.clone())
                    .or_insert_with(|| BTreeSet::new());
                deferred_set.insert(key.clone());
            }
            self.put_deferred(deferred)?;
        }

        let key_bytes = self.key_bytes(&key)?;
        if let Some(entry_bytes) = self.tree.del(&key_bytes)? {
            let mut entry: Entry<V, A> = bincode::deserialize(&entry_bytes)?;
            let dom_clock = entry.clock.dominating_vclock(&context);
            if !dom_clock.is_empty() {
                entry.clock = dom_clock;
                entry.val.truncate(&context);
                let new_entry_bytes = bincode::serialize(&entry)?;
                self.tree.set(key_bytes, new_entry_bytes)?;
            }
        }
        Ok(())
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
mod tests {
    use super::*;

    use quickcheck::{Arbitrary, Gen, TestResult};

    use crdts::mvreg::MVReg;
    use crdts::{mvreg, map};

    type TActor = u8;
    type TKey = u8;
    type TVal = MVReg<u8, TActor>;
    type InnerMap = crdts::Map<TKey, TVal, TActor>;
    type TOp = Op<TKey, crdts::Map<TKey, TVal, TActor>, TActor>;
    type TMap =  Map<TKey, InnerMap, TActor>;

    #[derive(Debug, Clone)]
    struct OpVec(TActor, Vec<TOp>);

    fn mk_tree() -> sled::Tree {
        let config = sled::ConfigBuilder::new().temporary(true).build();
        sled::Tree::start(config).unwrap()
    }

    impl PartialEq for TMap {
        fn eq(&self, other: &Self) -> bool {
            let self_collected: Vec<_>  = self.iter().map(|r| r.unwrap()).collect();
            let other_collected: Vec<_>  = self.iter().map(|r| r.unwrap()).collect();
            self.get_clock().unwrap() == other.get_clock().unwrap()
                && self.get_deferred().unwrap() == other.get_deferred().unwrap()
                &&  self_collected == other_collected
        }
    }
    impl Eq for TMap {}
    
    impl Arbitrary for OpVec {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let actor = TActor::arbitrary(g);
            let num_ops: u8 = g.gen_range(0, 50);
            let mut ops = Vec::with_capacity(num_ops as usize);
            for i in 0..num_ops {
                let die_roll: u8 = g.gen();
                let die_roll_inner: u8 = g.gen();
                let context: VClock<_> = Dot { actor, counter: i as u64 }.into();
                // context = context.into_iter().filter(|(a, _)| a != &actor).collect();
                // context.witness(actor.clone(), i as u64).unwrap();
                let op = match die_roll % 3 {
                    0 => {
                        let dot = Dot { actor, counter: context.get(&actor) };
                        Op::Up {
                            dot: dot.clone(),
                            key: g.gen(),
                            op: match die_roll_inner % 3 {
                                0 => map::Op::Up {
                                    dot: dot.clone(),
                                    key: g.gen(),
                                    op: mvreg::Op::Put {
                                        context,
                                        val: g.gen()
                                    }
                                },
                                1 => map::Op::Rm {
                                    context,
                                    key: g.gen()
                                },
                                _ => map::Op::Nop
                            }
                        }
                    },
                    1 => Op::Rm { context, key: g.gen() },
                    _ => Op::Nop
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
    fn test_update() {
        let mut m: TMap = Map::new(mk_tree());

        // constructs a default value if does not exist
        let op = m.update(101, m.dot(1).unwrap(), |map, dot| {
            map.update(110, dot, |reg, dot| reg.set(2, dot))
        }).unwrap();

        assert_eq!(
            op,
            Op::Up {
                dot: Dot { actor: 1, counter: 1 },
                key: 101,
                op: map::Op::Up {
                    dot: Dot { actor: 1, counter: 1 },
                    key: 110,
                    op: mvreg::Op::Put {
                        context: Dot { actor: 1, counter: 1 }.into(),
                        val: 2
                    }
                }
            }
        );

        m.apply(&op).unwrap();

        {
            let val = m.get(&101).unwrap().unwrap().val;
            let reg_state = val.get(&110).unwrap().0.get().0;
            assert_eq!(reg_state, vec![&2]);
        }

        // the map should give the latest val to the closure
        let op2 = m.update(101, m.dot(1).unwrap(), |map, dot| {
            map.update(110, dot, |reg, dot| {
                assert_eq!(reg.get().0, vec![&2]);
                reg.set(6, dot)
            })
        }).unwrap();
        m.apply(&op2).unwrap();

        {
            let val = m.get(&101).unwrap().unwrap().val;
            let reg_state = val.get(&110).unwrap().0.get().0;
            assert_eq!(reg_state, vec![&6]);
        }
    }

    #[test]
    fn test_remove() {
        let mut m: TMap = Map::new(mk_tree());

        let op = m.update(
            101,
            m.dot(1).unwrap(),
            |m, dot| m.update(
                110,
                dot,
                |r, dot| r.set(0, dot)
            )
        ).unwrap();
        let mut inner_map: map::Map<TKey, TVal, TActor> = map::Map::new();
        let inner_op = inner_map.update(110, m.dot(1).unwrap(), |r, dot| r.set(0, dot));
        inner_map.apply(&inner_op).unwrap();

        m.apply(&op).unwrap();

        let rm_op = {
            let entry = m.get(&101).unwrap().unwrap();
            assert_eq!(entry.val, inner_map);
            m.rm(101, entry.clock)
        };
        m.apply(&rm_op).unwrap();
        assert_eq!(m.get(&101).unwrap(), None);
    }

    #[test]
    fn test_reset_remove_semantics() {
        let mut m1 = TMap::new(mk_tree());
        let mut m2 = TMap::new(mk_tree());
        let op1 = m1.update(101, m1.dot(74).unwrap(), |map, dot| {
            map.update(110, dot, |reg, dot| {
                reg.set(32, dot)
            })
        }).unwrap();
        m1.apply(&op1).unwrap();
        m2.apply(&op1).unwrap();
        
        let entry = m1.get(&101).unwrap().unwrap();

        let op2 = m1.rm(101, entry.clock);
        m1.apply(&op2).unwrap();

        let op3 = m2.update(101, m2.dot(37).unwrap(), |map, dot| {
            map.update(220, dot, |reg, dot| {
                reg.set(5, dot)
            })
        }).unwrap();
        m2.apply(&op3).unwrap();

        
        m1.apply(&op3).unwrap();
        m2.apply(&op2).unwrap();
        assert_eq!(m1, m2);

        let inner_map = m1.get(&101).unwrap().unwrap().val;
        assert_eq!(inner_map.get(&220).unwrap().0.get().0, vec![&5]);
        assert_eq!(inner_map.get(&110), None);
        assert_eq!(inner_map.len(), 1);
    }
    
    #[test]
    fn test_updating_with_current_clock_should_be_a_nop() {
        let mut m1: TMap = Map::new(mk_tree());

        let res = m1.apply(&Op::Up {
            dot: Dot { actor: 1, counter: 0 },
            key: 0,
            op: map::Op::Up {
                dot: Dot { actor: 1, counter: 0 },
                key: 1,
                op: mvreg::Op::Put {
                    context: VClock::new(),
                    val: 235
                }
            }
        });

        assert!(res.is_ok());

        // the update should have been ignored
        assert_eq!(m1, Map::new(mk_tree()));
    }

    #[test]
    fn test_concurrent_update_and_remove_has_an_add_bias() {
        let mut m1 = TMap::new(mk_tree());
        let mut m2 = TMap::new(mk_tree());

        let op1 = Op::Rm {
            context: Dot { actor: 1, counter: 1 }.into(),
            key: 102
        };
        let op2 = m2.update(102, m2.dot(2).unwrap(), |_, _| map::Op::Nop).unwrap();

        m1.apply(&op1).unwrap();
        m2.apply(&op2).unwrap();

        m1.apply(&op2).unwrap();
        m2.apply(&op1).unwrap();

        assert_eq!(m1, m2);

        // we bias towards adds
        assert!(m1.get(&102).unwrap().is_some());
    }

    #[test]
    fn test_op_deferred_remove() {
        let mut m1 = TMap::new(mk_tree());
        let mut m2 = TMap::new(mk_tree());
        let mut m3 = TMap::new(mk_tree());

        let m1_up1 = m1.update(0, m1.dot(1).unwrap(), |map, dot| map.update(0, dot, |reg, dot| {
            reg.set(0, dot)
        })).unwrap();
        m1.apply(&m1_up1).unwrap();

        let m1_up2 = m1.update(
            1,
            m1.dot(1).unwrap(),
            |map, dot| map.update(1, dot, |reg, dot| reg.set(1, dot))
        ).unwrap();
        m1.apply(&m1_up2).unwrap();

        assert!(m2.apply(&m1_up1).is_ok());
        assert!(m2.apply(&m1_up2).is_ok());

        let entry = m2.get(&0).unwrap().unwrap();
        let m2_rm = m2.rm(0, entry.clock);
        m2.apply(&m2_rm).unwrap();
        
        assert_eq!(m2.get(&0).unwrap(), None);
        assert!(m3.apply(&m2_rm).is_ok());
        assert!(m3.apply(&m1_up1).is_ok());
        assert!(m3.apply(&m1_up2).is_ok());
        assert!(m1.apply(&m2_rm).is_ok());

        assert_eq!(m2.get(&0).unwrap(), None);
        let val = m3.get(&1).unwrap().unwrap().val;
        let reg_state = val.get(&1).unwrap().0.get().0;
        assert_eq!(reg_state, vec![&1]);

        assert_eq!(m2, m3);
        assert_eq!(m1, m2);
        assert_eq!(m1, m3);
    }

    #[test]
    fn test_op_exchange_converges_quickcheck1() {
        let ops1 = vec![
            Op::Up {
                dot: Dot { actor: 0, counter: 3 },
                key: 9,
                op: map::Op::Up {
                    dot: Dot { actor: 0, counter: 3 },
                    key: 0,
                    op: mvreg::Op::Put {
                        context: Dot { actor: 0, counter: 3 }.into(),
                        val: 0
                    }
                }
            }
        ];
        let ops2 = vec![
            Op::Up {
                dot: Dot { actor: 1, counter: 1 },
                key: 9,
                op: map::Op::Rm {
                    context: Dot { actor: 1, counter: 1 }.into(),
                    key: 0
                }
            },
            Op::Rm {
                context: Dot { actor: 1, counter: 2 }.into(),
                key: 9
            }
        ];

        let mut m1: TMap = Map::new(mk_tree());
        let mut m2: TMap = Map::new(mk_tree());

        apply_ops(&mut m1, &ops1);
        apply_ops(&mut m2, &ops2);

        // m1 <- m2
        apply_ops(&mut m1, &ops2);

        // m2 <- m1
        apply_ops(&mut m2, &ops1);
        
        // m1 <- m2 == m2 <- m1
        assert_eq!(m1, m2);
            
    }

    fn apply_ops(map: &mut TMap, ops: &[TOp]) {
        for op in ops.iter() {
            map.apply(op).unwrap()
        }
    }

    quickcheck! {
        fn prop_op_exchange_converges(ops1: OpVec, ops2: OpVec) -> TestResult {
            if ops1.0 == ops2.0 {
                return TestResult::discard();
            }

            let mut m1: TMap = Map::new(mk_tree());
            let mut m2: TMap = Map::new(mk_tree());

            apply_ops(&mut m1, &ops1.1);
            apply_ops(&mut m2, &ops2.1);

            // m1 <- m2
            apply_ops(&mut m1, &ops2.1);

            // m2 <- m1
            apply_ops(&mut m2, &ops1.1);

            // m1 <- m2 == m2 <- m1
            assert_eq!(m1, m2);
            TestResult::from_bool(true)
        }

        fn prop_op_exchange_associative(ops1: OpVec, ops2: OpVec, ops3: OpVec) -> TestResult {
            if ops1.0 == ops2.0 || ops1.0 == ops3.0 || ops2.0 == ops3.0 {
                return TestResult::discard();
            }

            let mut m1: TMap = Map::new(mk_tree());
            let mut m2: TMap = Map::new(mk_tree());
            let mut m3: TMap = Map::new(mk_tree());

            apply_ops(&mut m1, &ops1.1);
            apply_ops(&mut m2, &ops2.1);
            apply_ops(&mut m3, &ops3.1);

            // (m1 <- m2) <- m3
            apply_ops(&mut m1, &ops2.1);
            apply_ops(&mut m1, &ops3.1);

            // (m2 <- m3) <- m1
            apply_ops(&mut m2, &ops3.1);
            apply_ops(&mut m2, &ops1.1);

            // (m1 <- m2) <- m3 = (m2 <- m3) <- m1
            TestResult::from_bool(m1 == m2)
        }

        fn prop_op_idempotent(ops: OpVec) -> bool {
            let mut m = TMap::new(mk_tree());
            let mut m_control = TMap::new(mk_tree());

            apply_ops(&mut m, &ops.1);
            apply_ops(&mut m_control, &ops.1);
            
            apply_ops(&mut m, &ops.1);

            m == m_control
        }
    }
}
