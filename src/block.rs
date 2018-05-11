extern crate ditto;
extern crate serde;
extern crate bincode;

use self::serde::{Serialize, Deserialize};

use db_error::{DBErr};
use path;

pub trait Block<'a>: Serialize + Deserialize<'a> {
    fn merge(&mut self, other: &Self) -> Result<(), DBErr>;
}


#[derive(Clone, Debug, PartialEq, Hash, Serialize, Deserialize)]
pub enum TreeEntryKind {
    Tree,
    // TODO: expose datastructures from ditto
}

impl Eq for TreeEntryKind {}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeBlock {
    entries: ditto::set::Set<TreeEntry>
}

#[derive(Debug, Clone, PartialEq, Hash, Serialize, Deserialize)]
pub struct TreeEntry {
    // TAI: we can store additional entry metadata here.
    //      auto updating metadata may be used as an index
    kind: TreeEntryKind,
    path_comp: path::PathComp<'static>
}

impl Eq for TreeEntry {}

impl<'a> Block<'a> for TreeBlock {
    fn merge(&mut self, other: &Self) -> Result<(), DBErr>{
        self.entries.merge(other.entries.state())
            .map_err(DBErr::CRDT)
    }
}

impl TreeBlock {
    pub fn empty(site_id: Option<ditto::dot::SiteId>) -> Result<Self, DBErr> {
        let empty_set = ditto::set::Set::from_state(
            ditto::set::Set::new().state(), site_id
        ).map_err(DBErr::CRDT)?;

        Ok(TreeBlock {
            entries: empty_set
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    
    pub fn add(&mut self, entry: TreeEntry) -> Result<(), DBErr> {
        // TAI: consider keeping entries sorted
        //      to make lookups faster
        if self.entries.contains(&entry) {
            return Err(DBErr::Tree(String::from("Attempted to add an entry which already exists")));
        }
        self.entries.insert(entry)
            .map_err(DBErr::CRDT)?;
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &TreeEntry> {
        self.entries.iter()
    }

    pub fn rm(&mut self, entry: &TreeEntry) ->  Result<(), DBErr> {
        match self.entries.remove(&entry) {
            Some(_) => Ok(()),
            None => Err(DBErr::NotFound)
        }
    }
}

impl TreeEntry {
    fn tree(comp: &str) -> TreeEntry {
        TreeEntry {
            kind: TreeEntryKind::Tree,
            path_comp: path::PathComp::escape(&comp)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::{HashSet};
    
    #[test]
    fn empty() {
        {
            let tree = TreeBlock::empty(Some(1)).unwrap();
            assert_eq!(tree.len(), 0);
        }
        {
            let res = TreeBlock::empty(Some(0));
            assert!(res.is_err());
        }
        {
            let tree = TreeBlock::empty(None).unwrap();
            assert_eq!(tree.len(), 0);
        }
    }

    #[test]
    fn add() {
        let mut tree = TreeBlock::empty(Some(1)).unwrap();
        tree.add(TreeEntry::tree("users")).unwrap();

        assert_eq!(tree.len(), 1);
        
        // should not be able to add path comp twice
        let res = tree.add(TreeEntry::tree("users"));
        assert!(res.is_err());
        assert_eq!(tree.len(), 1);
        
        tree.add(TreeEntry::tree("passwords")).unwrap();
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn iter() {
        let mut tree = TreeBlock::empty(Some(1)).unwrap();
        {
            let mut iter = tree.iter();
            assert_eq!(None, iter.next());
        }
        
        tree.add(TreeEntry::tree("users")).unwrap();

        {
            let mut iter = tree.iter();
            let entry = iter.next().unwrap();
            assert_eq!(entry.kind, TreeEntryKind::Tree);
            assert_eq!(entry.path_comp, path::PathComp::escape("users"));
            assert_eq!(None, iter.next());
        }
        
        tree.add(TreeEntry::tree("secrets")).unwrap();
        {
            let mut iter_vec: Vec<&str> = tree.iter().map(|e| e.path_comp.value()).collect();
            iter_vec.sort();
            assert_eq!(iter_vec, ["secrets", "users"]);
        }
    }

    #[test]
    fn rm() {
        let mut tree = TreeBlock::empty(Some(1)).unwrap();

        let res = tree.rm(&TreeEntry::tree("does not exist!!"));
        assert!(res.is_err());

        tree.add(TreeEntry::tree("users")).unwrap();
        tree.add(TreeEntry::tree("boo")).unwrap();

        assert_eq!(tree.len(), 2);

        tree.rm(&TreeEntry::tree("users")).unwrap();

        assert_eq!(tree.len(), 1);
        {
            let iter_vec: Vec<&TreeEntry> = tree.iter().collect();
            assert_eq!(iter_vec, [&TreeEntry::tree("boo")]);
        }
        tree.rm(&TreeEntry::tree("boo")).unwrap();
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn merge() {
        {
            let mut ta = TreeBlock::empty(Some(1)).unwrap();

            // empty U empty => empty
            ta.merge(&TreeBlock::empty(Some(2)).unwrap()).unwrap();
            assert_eq!(ta.len(), 0);

            // empty U [/bobby] => [/bobby]
            // [/bobby] U empty => [/bobby]
            let mut tb = TreeBlock::empty(Some(2)).unwrap();
            tb.add(TreeEntry::tree("bobby")).unwrap();

            let mut tc = ta.clone();
            tc.merge(&tb).unwrap();

            assert_eq!(tc.len(), 1);
            {
                let expected: HashSet<TreeEntry> = [TreeEntry::tree("bobby")].iter().cloned().collect();
                assert_eq!(tc.entries.local_value(), expected);
            }

            let tb_frozen_values = tb.entries.local_value();
            assert_eq!(tb_frozen_values, tc.entries.local_value());

            tb.merge(&ta).unwrap();
            assert_eq!(tb_frozen_values, tb.entries.local_value());
        }

        {
            let mut ta = TreeBlock::empty(Some(1)).unwrap();
            let mut tb = TreeBlock::empty(Some(2)).unwrap();

            ta.add(TreeEntry::tree("bobby")).unwrap();
            assert_eq!(ta.len(), 1);
            
            tb.add(TreeEntry::tree("bobby")).unwrap();
            ta.merge(&tb).unwrap();

            assert_eq!(ta.len(), 1);
            assert_eq!(tb.len(), 1);
            assert_eq!(ta.entries.local_value(), tb.entries.local_value());
        }

        {
            let mut ta = TreeBlock::empty(Some(1)).unwrap();
            let mut tb = TreeBlock::empty(Some(2)).unwrap();

            ta.add(TreeEntry::tree("users")).unwrap();
            tb.add(TreeEntry::tree("passwords")).unwrap();
            ta.merge(&tb).unwrap();

            assert_eq!(ta.len(), 2);
            
            let expected: HashSet<TreeEntry> =
                [TreeEntry::tree("users"), TreeEntry::tree("passwords")].iter().cloned().collect();
            assert_eq!(ta.entries.local_value(), expected);
        }
    }

    #[test]
    fn serde() {
        let mut tree = TreeBlock::empty(Some(1)).unwrap();
        {
            let bytes: Vec<u8> = bincode::serialize(&tree).unwrap();
            let decoded_tree: TreeBlock = bincode::deserialize(&bytes[..]).unwrap();
            assert_eq!(tree, decoded_tree);
        }

        tree.add(TreeEntry::tree("users")).unwrap();
        {
            let bytes: Vec<u8> = bincode::serialize(&tree).unwrap();
            let decoded_tree: TreeBlock = bincode::deserialize(&bytes[..]).unwrap();
            assert_eq!(decoded_tree.len(), 1);
            assert_eq!(tree, decoded_tree);
        }

        tree.add(TreeEntry::tree("wifi_passwords")).unwrap();
        {
            let bytes: Vec<u8> = bincode::serialize(&tree).unwrap();
            let decoded_tree: TreeBlock = bincode::deserialize(&bytes[..]).unwrap();
            assert_eq!(decoded_tree.len(), 2);
            assert_eq!(tree, decoded_tree);
        }
    }
}
