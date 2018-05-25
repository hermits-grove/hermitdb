extern crate ditto;

use std;

use db_error::{Result, DBErr};

pub trait Blockable {
    fn blocks(&self) -> Vec<(String, Block)>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Prim {
    U64(u64),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>)
}

impl Eq for Prim {}

impl std::hash::Hash for Prim {
    fn hash<H: std::hash::Hasher>(&self, mut state: &mut H) {
        match self {   
            Prim::U64(v) => v.hash(&mut state),
            Prim::F64(v) => v.to_bits().hash(&mut state),
            Prim::Str(v) => v.hash(&mut state),
            Prim::Bytes(v) => v.hash(&mut state),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Block {
    Val(ditto::Register<Prim>),
    Set(ditto::Set<Prim>),
    Map(ditto::Map<Prim, Prim>),
    List(ditto::List<Prim>)
}

impl Blockable for Block {
    fn blocks(&self) -> Vec<(String, Block)> {
        vec![(String::new(), self.clone())]
    }
}

impl Block {
    pub fn merge(&mut self, other: &Self) -> Result<()> {
        match (self, other) {
            (Block::Val(x), Block::Val(y)) => Ok(x.merge(&y)),
            (Block::Set(x), Block::Set(y)) => Ok(x.merge(&y)),
            (Block::Map(x), Block::Map(y)) => Ok(x.merge(&y)),
            (Block::List(x), Block::List(y)) => Ok(x.merge(&y)),
            (_, _) => Err(DBErr::BlockTypeConflict)
        }
    }

    pub fn to_val(&self) -> Result<ditto::Register<Prim>> {
        match self {
            Block::Val(v) => Ok(v.clone()),
            Block::Set(_) => Err(DBErr::State("Expected Val got Set".into())),
            Block::Map(_) => Err(DBErr::State("Expected Val got Map".into())),
            Block::List(_) => Err(DBErr::State("Expected Val got List".into()))
        }
    }

    pub fn to_set(&self) -> Result<ditto::Set<Prim>> {
        match self {
            Block::Val(_) => Err(DBErr::State("Expected Set got Val".into())),
            Block::Set(s) => Ok(s.clone()),
            Block::Map(_) => Err(DBErr::State("Expected Set got Map".into())),
            Block::List(_) => Err(DBErr::State("Expected Set got List".into()))
        }
    }

    pub fn to_map(&self) -> Result<ditto::Map<Prim, Prim>> {
        match self {
            Block::Val(_) => Err(DBErr::State("Expected Map got Val".into())),
            Block::Set(_) => Err(DBErr::State("Expected Map got Set".into())),
            Block::Map(m) => Ok(m.clone()),
            Block::List(_) => Err(DBErr::State("Expected Map got List".into()))
        }
    }

    pub fn to_list(&self) -> Result<ditto::List<Prim>> {
        match self {
            Block::Val(_) => Err(DBErr::State("Expected List got Val".into())),
            Block::Set(_) => Err(DBErr::State("Expected List got Set".into())),
            Block::Map(_) => Err(DBErr::State("Expected List got Map".into())),
            Block::List(l) => Ok(l.clone())
        }
    }
}

impl Prim {
    pub fn to_u64(&self) -> Result<u64>{
        match self {
            Prim::U64(v) => Ok(*v),
            Prim::F64(_) => Err(DBErr::State("Expected U64 got F64".into())),
            Prim::Str(_) => Err(DBErr::State("Expected U64 got Str".into())),
            Prim::Bytes(_) => Err(DBErr::State("Expected U64 got Bytes".into()))
        }
    }

    pub fn to_f64(&self) -> Result<f64>{
        match self {
            Prim::U64(_) => Err(DBErr::State("Expected F64 got U64".into())),
            Prim::F64(v) => Ok(*v),
            Prim::Str(_) => Err(DBErr::State("Expected F64 got Str".into())),
            Prim::Bytes(_) => Err(DBErr::State("Expected F64 got Bytes".into()))
        }
    }

    pub fn to_string(&self) -> Result<String>{
        match self {
            Prim::U64(_) => Err(DBErr::State("Expected Str got U64".into())),
            Prim::F64(_) => Err(DBErr::State("Expected Str got F64".into())),
            Prim::Str(v) => Ok(v.clone()),
            Prim::Bytes(_) => Err(DBErr::State("Expected Str got Bytes".into()))
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Prim::U64(_) => Err(DBErr::State("Expected Bytes got U64".into())),
            Prim::F64(_) => Err(DBErr::State("Expected Bytes got F64".into())),
            Prim::Str(_) => Err(DBErr::State("Expected Bytes got Str".into())),
            Prim::Bytes(v) => Ok(v.clone())
        }
    }
}

impl<'a> From<&'a str> for Prim {
    fn from(v: &'a str) -> Self {
        Prim::Str(v.to_string())
    }
}

impl From<String> for Prim {
    fn from(v: String) -> Self {
        Prim::Str(v)
    }
}

impl From<f64> for Prim {
    fn from(v: f64) -> Self {
        Prim::F64(v)
    }
}

impl From<u64> for Prim {
    fn from(v: u64) -> Self {
        Prim::U64(v)
    }
}

impl From<Vec<u8>> for Prim {
    fn from(v: Vec<u8>) -> Self {
        Prim::Bytes(v)
    }
}
