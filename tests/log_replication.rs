use assert_matches::assert_matches;
use quickcheck::{quickcheck, Arbitrary, Gen, TestResult};
use git2;
use hermitdb::{
    crdts::{map, Map, Orswot, CmRDT},
    log::{LogReplicable, TaggedOp},
    memory_log,
    git_log,
    encrypted_git_log,
    crypto
};

type TActor = u8;
type TKey = u8;
type TVal = Orswot<u8, TActor>;
type TMap = Map<TKey, TVal, TActor>;
type TOp = map::Op<TKey, TVal, TActor>;

#[derive(Debug, Clone)]
struct OpVec(TActor, Vec<TOp>);

impl Arbitrary for OpVec {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let actor = TActor::arbitrary(g);
        let num_ops = u8::arbitrary(g);
        let mut map = TMap::new();
        let mut ops = Vec::with_capacity(num_ops as usize);
        for _ in 0..num_ops {
            let die_roll = u8::arbitrary(g);
            let key = TKey::arbitrary(g);
            let read_ctx = map.get(&key);
            let add_ctx = read_ctx.derive_add_ctx(actor.clone());
            let op = match die_roll % 3 {
                0 => {
                    // update Orswot
                    map.update(key, add_ctx, |set, ctx| {
                        let die_roll = u8::arbitrary(g);
                        let member = u8::arbitrary(g);
                        match die_roll % 2 {
                            0 => set.add(member, ctx),
                            _ => {
                                let rm_ctx = set.contains(&member).derive_rm_ctx();
                                set.remove(member, rm_ctx)
                            }
                        }
                    })
                },
                1 => {
                    // rm
                    let rm_ctx = map.get(&key).derive_rm_ctx();
                    map.rm(key, rm_ctx)
                },
                _ => {
                    // nop
                    map::Op::Nop
                }
            };
            map.apply(&op);
            ops.push(op);
        }
        OpVec(actor, ops)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let mut shrunk: Vec<Self> = Vec::new();
        for i in 0..self.1.len() {
            let mut vec = self.1.clone();
            vec.remove(i);
            shrunk.push(OpVec(self.0.clone(), vec))
        }
        Box::new(shrunk.into_iter())
    }    
}

fn p2p_converge<L: LogReplicable<TActor, TMap>>(
    mut a_log: L,
    mut b_log: L,
    mut remote: L::Remote,
    a_ops: Vec<TOp>,
    b_ops: Vec<TOp>
) -> TMap {
    let mut a_map = TMap::new();
    let mut b_map = TMap::new();

    for op in a_ops {
        let tagged_op = a_log.commit(op).unwrap();
        a_map.apply(tagged_op.op());
        assert_matches!(a_log.ack(&tagged_op), Ok(()));
    }

    for op in b_ops {
        let tagged_op = b_log.commit(op).unwrap();
        b_map.apply(tagged_op.op());
        assert_matches!(b_log.ack(&tagged_op), Ok(()));
    }

    assert_matches!(b_log.push(&mut remote), Ok(_));
    assert_matches!(a_log.push(&mut remote), Ok(_));

    assert_matches!(b_log.pull(&mut remote), Ok(_));
    assert_matches!(a_log.pull(&mut remote), Ok(_));

    while let Some(tagged_op) = a_log.next().unwrap() {
        a_map.apply(tagged_op.op());
        assert_matches!(a_log.ack(&tagged_op), Ok(()));
    }

    while let Some(tagged_op) = b_log.next().unwrap() {
        b_map.apply(tagged_op.op());
        assert_matches!(b_log.ack(&tagged_op), Ok(()));
    }

    assert_eq!(a_map, b_map);
    a_map
}

fn log_preserves_order(mut log: impl LogReplicable<TActor, TMap>, ops: Vec<TOp>) {
    for op in ops.iter() {
        assert_matches!(log.commit(op.clone()), Ok(_));
    }

    for op in ops.iter() {
        let tagged_op = log.next().unwrap().unwrap();
        assert_eq!(op, tagged_op.op());
        log.ack(&tagged_op).unwrap();
    }
    assert_matches!(log.next(), Ok(None));
}

quickcheck! {
    fn prop_replication_converges_memory(a_ops: OpVec, b_ops: OpVec) -> TestResult {
        let (actor1, a_ops) = (a_ops.0, a_ops.1);
        let (actor2, b_ops) = (b_ops.0, b_ops.1);

        if actor1 == actor2 {
            return TestResult::discard();
        }

        let a_log = memory_log::Log::new(actor1);
        let b_log = memory_log::Log::new(actor2);
        let remote = memory_log::Log::new(actor1);

        p2p_converge(a_log, b_log, remote, a_ops, b_ops);
        TestResult::from_bool(true)
    }

    fn prop_replication_converge_git(a_ops: OpVec, b_ops: OpVec) -> TestResult {
        let (actor1, a_ops) = (a_ops.0, a_ops.1);
        let (actor2, b_ops) = (b_ops.0, b_ops.1);

        if actor1 == actor2 {
            return TestResult::discard();
        }

        let a_log_dir = tempfile::tempdir().unwrap();
        let b_log_dir = tempfile::tempdir().unwrap();
        let remote_dir = tempfile::tempdir().unwrap();
        
        let a_log_git = git2::Repository::init_bare(
            &a_log_dir.path()
        ).unwrap();

        let b_log_git = git2::Repository::init_bare(
            &b_log_dir.path()
        ).unwrap();

        let _remote_git = git2::Repository::init_bare(
            &remote_dir.path()
        ).unwrap();
        
        let a_log = git_log::Log::new(actor1, a_log_git);
        let b_log = git_log::Log::new(actor2, b_log_git);

        let remote = git_log::Remote::no_auth(
            "remote".into(),
            remote_dir.path().to_str().unwrap().to_string()
        );

        p2p_converge(a_log, b_log, remote, a_ops, b_ops);
        TestResult::from_bool(true)
    }

    fn prop_replication_converge_encrypted_git(a_ops: OpVec, b_ops: OpVec) -> TestResult {
        let (actor1, a_ops) = (a_ops.0, a_ops.1);
        let (actor2, b_ops) = (b_ops.0, b_ops.1);

        if actor1 == actor2 {
            return TestResult::discard();
        }

        let root_key = crypto::KDF {
            pbkdf2_iters: 1,
            salt: [0u8; 256 / 8]
        }.derive_root("password".as_bytes());

        let a_log_dir = tempfile::tempdir().unwrap();
        let b_log_dir = tempfile::tempdir().unwrap();
        let remote_dir = tempfile::tempdir().unwrap();
        
        let a_log_git = git2::Repository::init_bare(
            &a_log_dir.path()
        ).unwrap();

        let b_log_git = git2::Repository::init_bare(
            &b_log_dir.path()
        ).unwrap();

        let _remote_git = git2::Repository::init_bare(
            &remote_dir.path()
        ).unwrap();
        
        let a_log = encrypted_git_log::Log::new(
            actor1,
            a_log_git,
            root_key.derive_child("git_log".as_bytes())
        );

        let b_log = encrypted_git_log::Log::new(
            actor2,
            b_log_git,
            root_key.derive_child("git_log".as_bytes())
        );

        let remote = git_log::Remote::no_auth(
            "remote".into(),
            remote_dir.path().to_str().unwrap().to_string()
        );

        p2p_converge(a_log, b_log, remote, a_ops, b_ops);
        TestResult::from_bool(true)
    }

    fn prop_log_preserves_order_memory(ops: OpVec) -> bool {
        let log: memory_log::Log<u8, TMap> = memory_log::Log::new(ops.0);
        log_preserves_order(log, ops.1);
        true
    }

    fn prop_log_preserves_order_git(ops: OpVec) -> bool {
        let OpVec(actor, ops) = ops;
        let log_dir = tempfile::tempdir().unwrap();
        let log_path = log_dir.path();
        let log_git = git2::Repository::init_bare(&log_path).unwrap();
        
        let log = git_log::Log::new(actor, log_git);
        
        log_preserves_order(log, ops);

        true
    }

    fn prop_log_preserves_order_encrypted_git(ops: OpVec) -> bool {
        let OpVec(actor, ops) = ops;
        let log_dir = tempfile::tempdir().unwrap();
        let log_path = log_dir.path();
        let log_git = git2::Repository::init_bare(&log_path).unwrap();

        let root_key = crypto::KDF {
            pbkdf2_iters: 1,
            salt: [0u8; 256 / 8]
        }.derive_root("password".as_bytes());
        
        let log = encrypted_git_log::Log::new(
            actor,
            log_git,
            root_key.derive_child("log".as_bytes())
        );
        
        log_preserves_order(log, ops);

        true
    }
}
