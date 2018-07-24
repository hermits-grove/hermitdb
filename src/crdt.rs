use serde::Serialize;
use serde::de::DeserializeOwned;

use error::Result;
use crdts::traits::{Causal, CvRDT, CmRDT};
use crdts::vclock::{VClock, Actor};
use std::collections::BTreeMap;

/// Key Trait alias to reduce redundancy in type decl.
pub trait Key: Ord + Clone + Send + Serialize + DeserializeOwned {}
impl<T: Ord + Clone + Send + Serialize + DeserializeOwned> Key for T {}

/// Val Trait alias to reduce redundancy in type decl.
pub trait Val<A: Actor>
    : Default + Clone + Send + Serialize + DeserializeOwned
    + Causal<A> + CmRDT + CvRDT
{}

impl<A, T> Val<A> for T where
    A: Actor,
    T: Default + Clone + Send + Serialize + DeserializeOwned
    + Causal<A> + CmRDT + CvRDT
{}

trait StorageLayer<K, V> {
    fn put(&mut self, key: K, val: V) -> Result<()>;
    fn get(&self, key: &K) -> Result<Option<V>>;
    fn rm(&mut self, key: &K) -> Result<Option<V>>;
    fn len(&self) -> Result<usize>;
}

struct MemoryMap<K, V>(BTreeMap<K, V>);

impl<K, V> StorageLayer<K, V> for MemoryMap<K, V> {
    fn put(&mut self, key: K, val: V) -> Result<()> {
        Ok(self.0.insert(key, val));
    }

    fn get(&self, key: &K) -> Result<Option<V>> {
        Ok(self.0.get(key))
    }

    fn rm(&mut self, key: &K) -> Result<Option<V>> {
        Ok(self.0.remove(key))
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

#[serde(bound(deserialize = ""))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map<K: Key, V: Val<A>, A: Actor, S>
    where S: StorageLayer<K, Entry<V, A>> {
    // This clock stores the current version of the Map, it should
    // be greator or equal to all Entry.clock's in the Map.
    clock: VClock<A>,
    entries: S
}

#[serde(bound(deserialize = ""))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Entry<V: Val<A>, A: Actor> {
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

impl<K: Key, V: Val<A>, A: Actor, S: StorageLayer<K, Entry<V, A>>> CmRDT for Map<K, V, A, S> {
    type Op = Op<K, V, A>;

    fn apply(&mut self, op: &Self::Op) -> Result<()> {
        match op.clone() {
            Op::Nop => {/* do nothing */},
            Op::Rm { clock, key } => {
                if !(self.clock >= clock) {
                    if let Some(mut entry) = self.entries.rm(&key)? {
                        entry.clock.subtract(&clock);
                        if !entry.clock.is_empty() {
                            entry.val.truncate(&clock);
                            self.entries.put(key, entry)?;
                        } else {
                            // the entries clock has been dominated by the
                            // remove op clock, so we remove (already did)
                        }
                    }
                    self.clock.merge(&clock);
                }
            },
            Op::Up { clock, key, op } => {
                if !(self.clock >= clock) {
                    let mut entry = self.entries.rm(&key)
                        ?.unwrap_or_else(|| Entry {
                            clock: clock.clone(),
                            val: V::default()
                        });

                    entry.clock.merge(&clock);
                    entry.val.apply(&op)?;
                    self.entries.put(key.clone(), entry);
                    self.clock.merge(&clock);
                }
            }
        }
        Ok(())
    }
}

impl<K: Key, V: Val<A>, A: Actor, S: StorageLayer<K, Entry<V, A>>> Map<K, V, A, S> {
    /// Constructs an empty Map
    pub fn new(storage: S) -> Map<K, V, A, S> {
        Map {
            clock: VClock::new(),
            entries: storage
         }
    }

    /// Returns the number of entries in the Map
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get a reference to a value stored under a key
    pub fn get(&self, key: &K) -> Result<Option<&V>> {
        self.entries.get(&key)
            ?.map(|map_entry| &map_entry.val)
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
        let mut clock = self.clock.clone();
        clock.increment(actor.clone());

        let val_opt = self.entries.get(&key)
            ?.map(|entry| entry.val.clone());
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
        op
    }

    /// Remove an entry from the Map
    pub fn rm(&mut self, key: K, actor: A) -> Result<Op<K, V, A>> {
        let mut clock = self.clock.clone();
        clock.increment(actor.clone());
        let op = Op::Rm { clock, key };
        self.apply(&op)?;
        op
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::{Arbitrary, Gen, TestResult};

    use lwwreg::LWWReg;

    type TActor = u8;
    type TKey = u8;
    type TVal = LWWReg<u8, (u64, TActor)>;
    type TOp = Op<TKey, Map<TKey, TVal, TActor>, TActor>;
    type TMap =  Map<TKey, crdts::Map<TKey, TVal, TActor>, TActor, MemoryMap<TKey, Entry<TVal, TActor>>>;

    // We can't impl on types outside this module ie. '(u8, Vec<_>)' so we wrap.
    #[derive(Debug, Clone)]
    struct OpVec(TActor, Vec<TOp>);

    impl<K, V, A> Arbitrary for Op<K, V, A> where
        K: Key + Arbitrary,
        V: Val<A> + Arbitrary,
        A: Actor + Arbitrary,
        V::Op: Arbitrary
    {
        fn arbitrary<G: Gen>(_g: &mut G) -> Self {
            /// we don't generate arbitrary Op's in isolation
            /// since they are highly likely to conflict with
            /// other ops, insted we generate OpVec's.
            unimplemented!();
        }

        fn shrink(&self) -> Box<Iterator<Item = Op<K, V, A>>> {
            let mut shrunk: Vec<Op<K, V, A>> = Vec::new();

            match self.clone() {
                Op::Nop => {/* shrink to nothing */},
                Op::Rm { clock, key } => {
                    shrunk.extend(
                        clock.shrink()
                            .map(|c| Op::Rm {
                                clock: c,
                                key: key.clone()
                            })
                            .collect::<Vec<Self>>()
                    );

                    shrunk.extend(
                        key.shrink()
                            .map(|k| Op::Rm {
                                clock: clock.clone(),
                                key: k.clone()
                            })
                            .collect::<Vec<Self>>()
                    );

                    shrunk.push(Op::Nop);
                },
                Op::Up { clock, key, op } => {
                    shrunk.extend(
                        clock.shrink()
                            .map(|c| Op::Up {
                                clock: c,
                                key: key.clone(),
                                op: op.clone()
                            })
                            .collect::<Vec<Self>>()
                    );
                    shrunk.extend(
                        key.shrink()
                            .map(|k| Op::Up {
                                clock: clock.clone(),
                                key: k,
                                op: op.clone() })
                            .collect::<Vec<Self>>()
                    );
                    shrunk.extend(
                        op.shrink()
                            .map(|o| Op::Up {
                                clock: clock.clone(),
                                key: key.clone(),
                                op: o
                            })
                            .collect::<Vec<Self>>()
                    );
                    shrunk.push(Op::Nop);
                }
            }

            Box::new(shrunk.into_iter())
        }
    }

    impl Arbitrary for OpVec {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let actor = TActor::arbitrary(g);
            let num_ops: u8 = g.gen_range(0, 50);

            let mut map = TMap::new(MemoryMap(BTreeMap::new()));
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

            for i in 0..self.1.len() {
                for shrunk_op in self.1[i].shrink() {
                    let mut vec = self.1.clone();
                    vec[i] = shrunk_op;
                    shrunk.push(OpVec(self.0, vec));
                }
            }
            Box::new(shrunk.into_iter())
        }
    }

    #[test]
    fn test_new() {
        let m: TMap = Map::new(MemoryMap(BTreeMap::new()));
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_get() {
        let mut m: TMap = Map::new(MemoryMap(BTreeMap::new()));

        assert_eq!(m.get(&0).unwrap(), None);

        m.clock.increment(1);
        m.entries.put(0, Entry {
            clock: m.clock.clone(),
            val: Map::default()
        }).unwrap();

        assert_eq!(m.get(&0), Some(&Map::new()));
    }

    #[test]
    fn test_update() {
        let mut m: TMap = Map::new(MemoryMap(BTreeMap::new()));

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
            m.get(&101).unwrap().get(&110),
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
            m.get(&101).unwrap().get(&110),
            Some(&LWWReg { val: 6, dot: (2, 1) })
        );

        // Returning None from the closure should remove the element
        m.update(101, |_| None, 1).unwrap();

        assert_eq!(m.get(&101), None);
    }

    #[test]
    fn test_remove() {
        let mut m: TMap = Map::new(MemoryMap(BTreeMap::new()));

        m.update(101, |mut m| Some(m.update(110, |r| Some(r), 1)), 1).unwrap();
        let mut inner_map = crdts::Map::new(MemoryMap(BTreeMap::new()));
        inner_map.update(110, |r| Some(r), 1).unwrap();
        assert_eq!(m.get(&101).unwrap(), Some(&inner_map));
        assert_eq!(m.len(), 1);

        m.rm(101, 1).unwrap();
        assert_eq!(m.get(&101).unwrap(), None);
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_reset_remove_semantics() {
        let mut m1 = TMap::new(MemoryMap(BTreeMap::new()));
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

        let mut m2 = TMap::new(MemoryMap(BTreeMap::new()));
        m2.apply(&op1).unwrap();

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

        assert_eq!(m1, m2);

        let inner_map = m1.get(&101).unwrap().unwrap();
        assert_matches!(inner_map.get(&220), Ok(Some(&LWWReg { val: 5, dot: (0, 37) })));
        assert_matches!(inner_map.get(&110), Ok(None));
        assert_eq!(inner_map.len(), 1);
    }

    #[test]
    fn test_updating_with_current_clock_should_be_a_nop() {
        let mut m1: TMap = Map::new(MemoryMap(BTreeMap::new()));

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
        }).unwrap();

        assert!(res.is_ok());

        // the update should have been ignored
        assert_eq!(m1, Map::new());
    }

    #[test]
    fn test_concurrent_update_and_remove() {
        let mut m1 = TMap::new(MemoryMap(BTreeMap::new()));
        let mut m2 = TMap::new(MemoryMap(BTreeMap::new()));

        let op1 = m1.rm(102, 75).unwrap();
        // TAI: try with an update instead of a Nop
        let op2 = m2.update(102, |_| Some(Op::Nop), 61).unwrap();

        assert!(m1.apply(&op2).is_ok());
        assert!(m2.apply(&op1).is_ok());

        assert_eq!(m1, m2);

        // we bias towards adds
        assert!(m1.get(&102).is_some());
    }

    #[test]
    fn test_order_of_remove_and_update_does_not_matter() {
        let mut m1 = TMap::new(MemoryMap(BTreeMap::new()));
        let mut m2 = TMap::new(MemoryMap(BTreeMap::new()));

        let op1 = m1.update(0, |_| Some(Op::Nop), 35).unwrap();
        let op2 = m2.rm(1, 47).unwrap();

        assert!(m1.apply(&op2).is_ok());
        assert!(m2.apply(&op1).is_ok());

        assert_eq!(m1, m2);
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

            let mut m1: TMap = Map::new(MemoryMap(BTreeMap::new()));
            let mut m2: TMap = Map::new(MemoryMap(BTreeMap::new()));

            apply_ops(&mut m1, &ops1.1);
            apply_ops(&mut m2, &ops2.1);

            apply_ops(&mut m1, &ops2.1);
            apply_ops(&mut m2, &ops1.1);

            TestResult::from_bool(m1 == m2)
        }

        fn prop_truncate_with_empty_vclock_is_nop(ops: OpVec) -> bool {
            let mut m: TMap = Map::new(MemoryMap(BTreeMap::new()));
            apply_ops(&mut m, &ops.1);

            let m_snapshot = m.clone();
            m.truncate(&VClock::new());

            m == m_snapshot
        }

        fn prop_associative(
            ops1: OpVec,
            ops2: OpVec,
            ops3: OpVec
        ) -> TestResult {
            if ops1.0 == ops2.0 || ops1.0 == ops3.0 || ops2.0 == ops3.0 {
                return TestResult::discard();
            }

            let mut m1: TMap = Map::new();
            let mut m2: TMap = Map::new();

            apply_ops(&mut m1, &ops1.1);
            apply_ops(&mut m2, &ops2.1);

            // (m1 ^ m2) ^ m3
            apply_ops(&mut m1, &ops2.1);
            apply_ops(&mut m1, &ops3.1);
            

            // m1 ^ (m2 ^ m3)
            apply_ops(&mut m2, &ops3.1);
            apply_ops(&mut m3, &ops1.1);

            // (m1 ^ m2) ^ m3 = m1 ^ (m2 ^ m3)
            TestResult::from_bool(m1 == m2)
        }

        fn prop_idempotent(ops: OpVec) -> bool {
            let mut m: TMap = Map::new(MemoryMap(BTreeMap::new()));
            let mut m_clone: TMap = Map::new(MemoryMap(BTreeMap::new()));
            apply_ops(&mut m, &ops.1);
            apply_ops(&mut m_clone, &ops.1);

            apply_ops(&mut m, &ops.1); // apply ops once more

            // m ^ m = m
            m == m_clone
        }
    }
}
