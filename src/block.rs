extern crate ditto;
extern crate serde;
extern crate rmp_serde;

pub use self::ditto::json::Json;

use self::serde::{Serialize, Deserialize};

use db_error::{Result, DBErr};
use path;

pub trait Block<'a>: Serialize + Deserialize<'a> {
    fn crdt_merge(&mut self, other: &Self) -> Result<()>;
}

#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize)]
pub enum TreeEntryKind {
    Tree,
    Json,
    // TODO: expose datastructures from ditto
}

impl Eq for TreeEntryKind {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    entries: ditto::set::Set<TreeEntry>
}

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
pub struct TreeEntry {
    // TAI: we can store additional entry metadata here.
    //      auto updating metadata may be used as an index
    kind: TreeEntryKind,
    path_comp: path::PathComp
}

impl Eq for TreeEntry {}

impl<'a> Block<'a> for Tree {
    fn crdt_merge(&mut self, other: &Self) -> Result<()>{
        self.entries.merge(other.entries.state())
            .map_err(DBErr::CRDT)
    }
}

impl Tree {
    pub fn empty(site_id: &Option<ditto::dot::SiteId>) -> Result<Self>{
        let empty_set = ditto::set::Set::from_state(
            ditto::set::Set::new().state(), site_id.clone()
        ).map_err(DBErr::CRDT)?;

        Ok(Tree {
            entries: empty_set
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    
    pub fn add(&mut self, entry: &TreeEntry) -> Result<()>{
        // TAI: consider keeping entries sorted to make lookups faster
        {
            let matches: Vec<_> = self.entries
                .iter()
                .filter(|e| e.path_comp == entry.path_comp)
                .collect();
            if matches.len() > 0 {
                return Err(DBErr::Tree("Attempted to add an entry with an existing path_comp".into()));
            }
        }
        self.entries.insert(entry.clone())
            .map_err(DBErr::CRDT)?;
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &TreeEntry> {
        self.entries.iter()
    }

    pub fn rm(&mut self, entry: &TreeEntry) ->  Result<()>{
        match self.entries.remove(&entry) {
            Some(_) => Ok(()),
            None => Err(DBErr::NotFound)
        }
    }
}

impl TreeEntry {
    pub fn tree(comp: &path::PathComp) -> TreeEntry {
        TreeEntry {
            kind: TreeEntryKind::Tree,
            path_comp: comp.clone()
        }
    }
    
    pub fn json(comp: &path::PathComp) -> TreeEntry {
        TreeEntry {
            kind: TreeEntryKind::Json,
            path_comp: comp.clone()
        }
    }

    pub fn tree_from_str(comp: &str) -> TreeEntry {
        TreeEntry::tree(&path::PathComp::escape(&comp))
    }

    pub fn json_from_str(comp: &str) -> TreeEntry {
        TreeEntry::json(&path::PathComp::escape(&comp))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::{HashSet};
    
    #[test]
    fn empty() {
        {
            let tree = Tree::empty(&Some(1)).unwrap();
            assert_eq!(tree.len(), 0);
        }
        {
            let res = Tree::empty(&Some(0));
            assert!(res.is_err());
        }
        {
            let tree = Tree::empty(&None).unwrap();
            assert_eq!(tree.len(), 0);
        }
    }

    #[test]
    fn add() {
        let mut tree = Tree::empty(&Some(1)).unwrap();
        tree.add(&TreeEntry::tree_from_str("users")).unwrap();

        assert_eq!(tree.len(), 1);
        
        // should not be able to add path comp twice
        let res = tree.add(&TreeEntry::tree_from_str("users"));
        assert!(res.is_err());
        assert_eq!(tree.len(), 1);
        
        tree.add(&TreeEntry::tree_from_str("passwords")).unwrap();
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn iter() {
        let mut tree = Tree::empty(&Some(1)).unwrap();
        {
            let mut iter = tree.iter();
            assert_eq!(None, iter.next());
        }
        
        tree.add(&TreeEntry::tree_from_str("users")).unwrap();

        {
            let mut iter = tree.iter();
            let entry = iter.next().unwrap();
            assert_eq!(entry.kind, TreeEntryKind::Tree);
            assert_eq!(entry.path_comp, path::PathComp::escape("users"));
            assert_eq!(None, iter.next());
        }
        
        tree.add(&TreeEntry::tree_from_str("secrets")).unwrap();
        {
            let mut iter_vec: Vec<&str> = tree.iter().map(|e| e.path_comp.value()).collect();
            iter_vec.sort();
            assert_eq!(iter_vec, ["secrets", "users"]);
        }
    }

    #[test]
    fn rm() {
        let mut tree = Tree::empty(&Some(1)).unwrap();

        let res = tree.rm(&TreeEntry::tree_from_str("does not exist!!"));
        assert!(res.is_err());

        tree.add(&TreeEntry::tree_from_str("users")).unwrap();
        tree.add(&TreeEntry::tree_from_str("boo")).unwrap();

        assert_eq!(tree.len(), 2);

        tree.rm(&TreeEntry::tree_from_str("users")).unwrap();

        assert_eq!(tree.len(), 1);
        {
            let iter_vec: Vec<&TreeEntry> = tree.iter().collect();
            assert_eq!(iter_vec, [&TreeEntry::tree_from_str("boo")]);
        }
        tree.rm(&TreeEntry::tree_from_str("boo")).unwrap();
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn crdt_merge() {
        {
            let mut ta = Tree::empty(&Some(1)).unwrap();

            // empty U empty => empty
            ta.crdt_merge(&Tree::empty(&Some(2)).unwrap()).unwrap();
            assert_eq!(ta.len(), 0);

            // empty U [/bobby] => [/bobby]
            // [/bobby] U empty => [/bobby]
            let mut tb = Tree::empty(&Some(2)).unwrap();
            tb.add(&TreeEntry::tree_from_str("bobby")).unwrap();

            let mut tc = ta.clone();
            tc.crdt_merge(&tb).unwrap();

            assert_eq!(tc.len(), 1);
            {
                let expected: HashSet<TreeEntry> = [TreeEntry::tree_from_str("bobby")].iter().cloned().collect();
                assert_eq!(tc.entries.local_value(), expected);
            }

            let tb_frozen_values = tb.entries.local_value();
            assert_eq!(tb_frozen_values, tc.entries.local_value());

            tb.crdt_merge(&ta).unwrap();
            assert_eq!(tb_frozen_values, tb.entries.local_value());
        }

        {
            let mut ta = Tree::empty(&Some(1)).unwrap();
            let mut tb = Tree::empty(&Some(2)).unwrap();

            ta.add(&TreeEntry::tree_from_str("bobby")).unwrap();
            assert_eq!(ta.len(), 1);
            
            tb.add(&TreeEntry::tree_from_str("bobby")).unwrap();
            ta.crdt_merge(&tb).unwrap();

            assert_eq!(ta.len(), 1);
            assert_eq!(tb.len(), 1);
            assert_eq!(ta.entries.local_value(), tb.entries.local_value());
        }

        {
            let mut ta = Tree::empty(&Some(1)).unwrap();
            let mut tb = Tree::empty(&Some(2)).unwrap();

            ta.add(&TreeEntry::tree_from_str("users")).unwrap();
            tb.add(&TreeEntry::tree_from_str("passwords")).unwrap();
            ta.crdt_merge(&tb).unwrap();

            assert_eq!(ta.len(), 2);
            
            let expected: HashSet<TreeEntry> =
                [TreeEntry::tree_from_str("users"), TreeEntry::tree_from_str("passwords")].iter().cloned().collect();
            assert_eq!(ta.entries.local_value(), expected);
        }
    }

    #[test]
    fn serde() {
        let mut tree = Tree::empty(&Some(1)).unwrap();
        {
            let bytes: Vec<u8> = rmp_serde::to_vec(&tree).unwrap();
            let decoded_tree: Tree = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(tree, decoded_tree);
        }

        tree.add(&TreeEntry::tree_from_str("users")).unwrap();
        {
            let bytes: Vec<u8> = rmp_serde::to_vec(&tree).unwrap();
            let decoded_tree: Tree = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(decoded_tree.len(), 1);
            assert_eq!(tree, decoded_tree);
        }

        tree.add(&TreeEntry::tree_from_str("wifi_passwords")).unwrap();
        {
            let bytes: Vec<u8> = rmp_serde::to_vec(&tree).unwrap();
            let decoded_tree: Tree = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(decoded_tree.len(), 2);
            assert_eq!(tree, decoded_tree);
        }
    }
}

impl<'a> Block<'a> for Json {
    fn crdt_merge(&mut self, other: &Self) -> Result<()>{
        self.merge(other.state())
            .map_err(DBErr::CRDT)
    }
}
