use std::collections::BTreeMap;

use sled;
use assert_matches::assert_matches;
use hermitdb::{
    data::{Prim, Data, Kind, Actor},
    crdts,
    memory_log,
    map,
    db,
    DB
};

fn mk_db(actor: Actor) -> DB<memory_log::Log<Actor, db::Map>> {
    let sled = sled::Config::new().temporary(true).open().unwrap();
    DB::new(memory_log::Log::new(actor), map::Map::new(sled))
}

#[test]
fn test_write_read_set() {
    let mut db = mk_db(1);

    let read_ctx = db.get(&("x".into(), Kind::Set)).unwrap();
    assert_matches!(read_ctx.val, None);

    assert_matches!(
        db.update(("x", Kind::Set), read_ctx.derive_add_ctx(1), |data, ctx| {
            let set = data.to_set().unwrap();
            set.add(57i64, ctx)
        }),
        Ok(())
    );

    assert_eq!(
        db.get(&("x".into(), Kind::Set)).unwrap().val
            .and_then(|val| val.to_set().ok())
            .map(|set| set.read().val),
        Some(vec![Prim::Int(57)].into_iter().collect())
    );
}

#[test]
fn test_iter() {
    let actor = 1;
    let mut db = mk_db(actor);

    let add_ctx = db.get(&("x".into(), Kind::Reg)).unwrap().derive_add_ctx(actor);
    db.update(("x", Kind::Reg), add_ctx, |data, ctx| {
        let reg = data.to_reg().unwrap();
        reg.write("x's val", ctx)
    }).unwrap();

    let add_ctx = db.get(&("y".into(), Kind::Reg)).unwrap().derive_add_ctx(actor);
    db.update(("y", Kind::Reg), add_ctx, |data, ctx| {
        let reg = data.to_reg().unwrap();
        reg.write("y's val", ctx)
    }).unwrap();

    let add_ctx = db.get(&("z".into(), Kind::Reg)).unwrap().derive_add_ctx(actor);
    db.update(("z", Kind::Reg), add_ctx, |data, ctx| {
        let reg = data.to_reg().unwrap();
        reg.write("z's val", ctx)
    }).unwrap();

    let items: BTreeMap<(String, Kind), crdts::ReadCtx<Data, Actor>> = db.iter().unwrap()
        .map(|opt| opt.unwrap())
        .collect();

    assert_eq!(items.len(), 3);
    assert_eq!(
        items.get(&("x".into(), Kind::Reg))
            .cloned()
            .and_then(|e| e.val.to_reg().ok())
            .map(|r| r.read().val),
        Some(vec!["x's val".into()])
    );
    assert_eq!(
        items.get(&("y".into(), Kind::Reg))
            .cloned()
            .and_then(|e| e.val.to_reg().ok())
            .map(|r| r.read().val),
        Some(vec!["y's val".into()])
    );
    assert_eq!(
        items.get(&("z".into(), Kind::Reg))
            .cloned()
            .and_then(|e| e.val.to_reg().ok())
            .map(|r| r.read().val),
        Some(vec!["z's val".into()])
    );
}

#[test]
fn test_sync() {
    let mut remote = memory_log::Log::new(0);
    let mut db_1 = mk_db(1);
    let mut db_2 = mk_db(2);

    let add_ctx = db_1.get(&("x".into(), Kind::Reg)).unwrap().derive_add_ctx(1);
    db_1.update(("x", Kind::Reg), add_ctx, |d, ctx| {
        let reg = d.to_reg().unwrap();
        reg.write("this is a reg for value 'x'", ctx)
    }).unwrap();

    let add_ctx = db_2.get(&("y".into(), Kind::Reg)).unwrap().derive_add_ctx(2);
    db_2.update(("y", Kind::Reg), add_ctx, |d, ctx| {
        let reg = d.to_reg().unwrap();
        reg.write("this is a reg for value 'y'", ctx)
    }).unwrap();

    db_1.sync(&mut remote).unwrap();
    db_2.sync(&mut remote).unwrap();
    db_1.sync(&mut remote).unwrap();

    assert_eq!(
        db_1.get(&("x".into(), Kind::Reg)).unwrap().val
            .and_then(|data| data.to_reg().ok())
            .map(|reg| reg.read().val),
        Some(vec!["this is a reg for value 'x'".into()])
    );
    assert_eq!(
        db_1.get(&("y".into(), Kind::Reg)).unwrap().val
            .and_then(|data| data.to_reg().ok())
            .map(|reg| reg.read().val),
        Some(vec!["this is a reg for value 'y'".into()])
    );
    assert_eq!(
        db_2.get(&("x".into(), Kind::Reg)).unwrap().val
            .and_then(|data| data.to_reg().ok())
            .map(|reg| reg.read().val),
        Some(vec!["this is a reg for value 'x'".into()])
    );
    assert_eq!(
        db_2.get(&("y".into(), Kind::Reg)).unwrap().val
            .and_then(|data| data.to_reg().ok())
            .map(|reg| reg.read().val),
        Some(vec!["this is a reg for value 'y'".into()])
    );
}
