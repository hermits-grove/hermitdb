extern crate hermitdb;
extern crate tempfile;

#[macro_use]
extern crate assert_matches;

use std::collections::BTreeMap;

use hermitdb::data::{Prim, Kind, Actor};
use hermitdb::{memory_log, map, sled, db, DB};

fn mk_db(actor: Actor) -> DB<memory_log::Log<Actor, db::Map>> {
    let config = sled::ConfigBuilder::new().temporary(true).build();
    let tree = sled::Tree::start(config).unwrap();
    let log = memory_log::Log::new(actor);
    let map = map::Map::new(tree);
    DB::new(log, map)
}

#[test]
fn test_write_read_set() {
    let mut db = mk_db(1);

    assert_matches!(db.get(&("x".into(), Kind::Set)), Ok(None));

    let dot = db.dot(1).unwrap();
    assert_matches!(
        db.update(("x", Kind::Set), dot, |data, dot| {
            let set = data.set().unwrap();
            set.add(57.18, dot)
        }),
        Ok(())
    );

    assert_eq!(
        db.get(&("x".into(), Kind::Set)).unwrap()
            .and_then(|entry| entry.val.set().ok())
            .map(|set| set.value()),
        Some(vec![Prim::Float(57.18)])
    );
}

#[test]
fn test_iter() {
    let actor = 1;
    let mut db = mk_db(actor);

    let dot1 = db.dot(actor).unwrap();
    db.update(("x", Kind::Reg), dot1.clone(), |data, d| {
        let reg = data.reg().unwrap();
        reg.set("x's val", d)
    }).unwrap();

    let dot2 = db.dot(actor).unwrap();
    db.update(("y", Kind::Reg), dot2.clone(), |data, d| {
        let reg = data.reg().unwrap();
        reg.set("y's val", d)
    }).unwrap();

    let dot3 = db.dot(actor).unwrap();
    db.update(("z", Kind::Reg), dot3.clone(), |data, d| {
        let reg = data.reg().unwrap();
        reg.set("z's val", d)
    }).unwrap();

    let items: BTreeMap<(String, Kind), db::Entry> = db.iter()
        .map(|opt| opt.unwrap())
        .collect();

    assert_eq!(items.len(), 3);
    assert_eq!(
        items.get(&("x".into(), Kind::Reg))
            .cloned()
            .and_then(|e| e.val.reg().ok())
            .map(|r| r.get_owned()),
        Some((vec!["x's val".into()], dot1.into()))
    );
    assert_eq!(
        items.get(&("y".into(), Kind::Reg))
            .cloned()
            .and_then(|e| e.val.reg().ok())
            .map(|r| r.get_owned()),
        Some((vec!["y's val".into()], dot2.into()))
    );
    assert_eq!(
        items.get(&("z".into(), Kind::Reg))
            .cloned()
            .and_then(|e| e.val.reg().ok())
            .map(|r| r.get_owned()),
        Some((vec!["z's val".into()], dot3.into()))
    );
}

#[test]
fn test_sync() {
    let mut remote = memory_log::Log::new(0);
    let mut db_1 = mk_db(1);
    let mut db_2 = mk_db(2);

    let dot_1 = db_1.dot(1).unwrap();
    db_1.update(("x", Kind::Reg), dot_1, |d, dot| {
        let reg = d.reg().unwrap();
        reg.set("this is a reg for value 'x'", dot)
    }).unwrap();

    let dot_2 = db_2.dot(2).unwrap();
    db_2.update(("y", Kind::Reg), dot_2, |d, dot| {
        let reg = d.reg().unwrap();
        reg.set("this is a reg for value 'y'", dot)
    }).unwrap();

    db_1.sync(&mut remote).unwrap();
    db_2.sync(&mut remote).unwrap();
    db_1.sync(&mut remote).unwrap();

    assert_eq!(
        db_1.get(&("x".into(), Kind::Reg)).unwrap()
            .and_then(|entry| entry.val.reg().ok())
            .map(|reg| reg.get_owned().0),
        Some(vec!["this is a reg for value 'x'".into()])
    );
    assert_eq!(
        db_1.get(&("y".into(), Kind::Reg)).unwrap()
            .and_then(|entry| entry.val.reg().ok())
            .map(|reg| reg.get_owned().0),
        Some(vec!["this is a reg for value 'y'".into()])
    );
    assert_eq!(
        db_2.get(&("x".into(), Kind::Reg)).unwrap()
            .and_then(|entry| entry.val.reg().ok())
            .map(|reg| reg.get_owned().0),
        Some(vec!["this is a reg for value 'x'".into()])
    );
    assert_eq!(
        db_2.get(&("y".into(), Kind::Reg)).unwrap()
            .and_then(|entry| entry.val.reg().ok())
            .map(|reg| reg.get_owned().0),
        Some(vec!["this is a reg for value 'y'".into()])
    );
}
