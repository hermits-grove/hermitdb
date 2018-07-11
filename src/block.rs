extern crate crdts;

use std;

use error::{Result, Error};

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Prim {
    Bool(bool),
    U64(u64),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>)
}

impl std::hash::Hash for Prim {
    fn hash<H: std::hash::Hasher>(&self, mut state: &mut H) {
        match self {
            Prim::Bool(v) => v.hash(&mut state),
            Prim::U64(v) => v.hash(&mut state),
            Prim::F64(v) => v.to_bits().hash(&mut state),
            Prim::Str(v) => v.hash(&mut state),
            Prim::Bytes(v) => v.hash(&mut state),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Block {
    // TAI: should the (i64, i32) timestamp be (u64, u32) ??
    VClock(crdts::VClock<u128>),
    Reg(crdts::LWWReg<Prim, ((i64, i32), u128)>),
    Map(crdts::Map<Vec<u8>, Block, u128>)
}

impl crdts::ComposableCrdt<u128> for Block {
    fn truncate(&mut self, clock: crdts::VClock<u128>) {
        match self {
            Block::VClock(current_clock) =>
                current_clock.truncate(&clock),
            Block::Reg(lwwreg) =>
                lwwreg.truncate(clock),
            Block::Map(map) =>
                map.truncate(clock)
        }
    }
    
    fn merge(&mut self, other: &Self) -> crdts::Result<()> {
        match (self, other) {
            (Block::VClock(x), Block::VClock(y)) =>
                Ok(x.merge(&y)),
            (Block::Reg(x), Block::Reg(y)) =>
                crdts::ComposableCrdt::<()>::merge(x, &y),
            (Block::Map(x), Block::Map(y)) =>
                x.merge(&y),
            (_, _) =>
                Err(crdts::Error::MergeConflict)
        }
    }
}

impl Block {
    pub fn to_vclock(&self) -> Result<crdts::VClock<u128>> {
        match self {
            Block::VClock(v) => Ok(v.clone()),
            _ => Err(Error::State("Expected VClock".into()))
        }
    }

    pub fn to_reg(&self) -> Result<crdts::LWWReg<Prim, ((i64, i32), u128)>> {
        match self {
            Block::Reg(v) => Ok(v.clone()),
            _ => Err(Error::State("Expected Reg".into()))
        }
    }

    pub fn to_map(&self) -> Result<crdts::Map<Vec<u8>, Block, u128>> {
        match self {
            Block::Map(m) => Ok(m.clone()),
            _ => Err(Error::State("Expected Map".into()))
        }
    }
}

impl Prim {
    pub fn to_bool(&self) -> Result<bool>{
        match self {
            Prim::Bool(v) => Ok(*v),
            _ => Err(Error::State("Expected Bool".into()))
        }
    }

    pub fn to_u64(&self) -> Result<u64>{
        match self {
            Prim::U64(v) => Ok(*v),
            _ => Err(Error::State("Expected U64".into()))
        }
    }

    pub fn to_f64(&self) -> Result<f64>{
        match self {
            Prim::F64(v) => Ok(*v),
            _ => Err(Error::State("Expected F64".into()))
        }
    }

    pub fn to_string(&self) -> Result<String>{
        match self {
            Prim::Str(v) => Ok(v.clone()),
            _ => Err(Error::State("Expected Str".into()))
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        match self {
            Prim::Bytes(v) => Ok(v.clone()),
            _ => Err(Error::State("Expected Bytes".into()))
        }
    }
}

impl<'a> From<bool> for Prim {
    fn from(v: bool) -> Self {
        Prim::Bool(v)
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
