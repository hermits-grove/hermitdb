use std::cmp;
use crdts::{self, CvRDT, CmRDT, Causal};
use error::{Error, Result};

pub type Actor = u128;

#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Prim {
    Nil,
    Float(f64),
    Int(i64),
    Str(String),
    Blob(Vec<u8>)
}

impl Eq for Prim {}

impl Ord for Prim {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match (self, other) {
            (Prim::Int(a), Prim::Int(b)) => a.cmp(&b),
            (Prim::Str(a), Prim::Str(b)) => a.cmp(&b),
            (Prim::Blob(a), Prim::Blob(b)) => a.cmp(&b),
            // TAI: we panic when we compare with NaN. is this expected behaviour?
            (a, b) => a.partial_cmp(&b).unwrap()
        }
    }
}

impl Default for Prim {
    fn default() -> Self {
        Prim::Nil
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Nil,
    Reg,
    Set,
    Map,
    Float,
    Int,
    Str,
    Blob
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Data {
    Nil,
    Reg(crdts::MVReg<Prim, Actor>),
    Set(crdts::Orswot<Prim, Actor>),
    Map(crdts::Map<(Vec<u8>, Kind), Box<Data>, Actor>)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    Reg(crdts::mvreg::Op<Prim, Actor>),
    Set(crdts::orswot::Op<Prim, Actor>),
    Map(crdts::map::Op<(Vec<u8>, Kind), Box<Data>, Actor>)
}

impl Default for Data {
    fn default() -> Self {
        Data::Nil
    }
}

impl CvRDT for Data {
    type Error = Error;

    fn merge(&mut self, other: &Self) -> Result<()> {
        if &mut Data::Nil == self {
            *self = other.clone();
            return Ok(());
        }

        // compute kinds here in case the match falls to error case.
        // (the match will consume self and other)
        let kind = self.kind();
        let other_kind = other.kind();
        match (self, other) {
            (_, Data::Nil) => Ok(()),
            (Data::Reg(a), Data::Reg(b)) => {
                a.merge(b)?;
                Ok(())
            },
            (Data::Set(a), Data::Set(b)) => {
                a.merge(&b)?;
                Ok(())
            },
            (Data::Map(a), Data::Map(b)) => {
                a.merge(&b)?;
                Ok(())
            },
            _ => {
                return Err(Error::UnexpectedKind(kind, other_kind));
            }
        }
    }
}

impl CvRDT for Box<Data> {
    type Error = Error;
        
    fn merge(&mut self, other: &Self) -> Result<()> {
        Data::merge(self, other)
    }
}

impl CmRDT for Data {
    type Error = Error;
    type Op = Op;

    fn apply(&mut self, op: &Self::Op) -> Result<()> {
        if &mut Data::Nil == self {
            *self = op.kind().default_data();
        }
        let kind = self.kind();
        let op_kind = op.kind();
        match (self, op) {
            (Data::Reg(crdt), Op::Reg(op)) => crdt.apply(op).map_err(|e| e.into()),
            (Data::Set(crdt), Op::Set(op)) => crdt.apply(op).map_err(|e| e.into()),
            (Data::Map(crdt), Op::Map(op)) => crdt.apply(op).map_err(|e| e.into()),
            _ => Err(Error::UnexpectedKind(kind, op_kind))
        }   
    }
}

impl CmRDT for Box<Data> {
    type Error = Error;
    type Op = Box<Op>;

    fn apply(&mut self, op: &Self::Op) -> Result<()> {
        Data::apply(self, op)
    }
}

impl Causal<Actor> for Data {
    fn truncate(&mut self, clock: &crdts::VClock<Actor>) {
        match self {
            Data::Nil => (),
            Data::Reg(causal) => causal.truncate(&clock),
            Data::Set(causal) => causal.truncate(&clock),
            Data::Map(causal) => causal.truncate(&clock)
        }
    }
}

impl Causal<Actor> for Box<Data> {
    fn truncate(&mut self, clock: &crdts::VClock<Actor>) {
        Data::truncate(self, clock)
    }
}

impl Data {
    pub fn kind(&self) -> Kind {
        match self {
            Data::Nil => Kind::Nil,
            Data::Reg(_) => Kind::Reg,
            Data::Set(_) => Kind::Set,
            Data::Map(_) => Kind::Map
        }
    }

    pub fn nil(self) -> Result<()> {
        match self {
            Data::Nil => Ok(()),
            other => Err(Error::UnexpectedKind(Kind::Nil, other.kind()))
        }
    }
    pub fn reg(self) -> Result<crdts::MVReg<Prim, Actor>> {
        match self {
            Data::Nil => Ok(crdts::MVReg::default()),
            Data::Reg(r) => Ok(r),
            other => Err(Error::UnexpectedKind(Kind::Reg, other.kind()))
        }
    }

    pub fn set(self) -> Result<crdts::Orswot<Prim, Actor>> {
        match self {
            Data::Nil => Ok(crdts::Orswot::default()),
            Data::Set(s) => Ok(s),
            other => Err(Error::UnexpectedKind(Kind::Set, other.kind()))
        }
    }

    pub fn map(self) -> Result<crdts::Map<(Vec<u8>, Kind), Box<Data>, Actor>> {
        match self {
            Data::Nil => Ok(crdts::Map::default()),
            Data::Map(m) => Ok(m),
            other => Err(Error::UnexpectedKind(Kind::Map, other.kind()))
        }
    }
}

impl Prim {
    pub fn kind(&self) -> Kind {
        match self {
            Prim::Nil => Kind::Nil,
            Prim::Float(_) => Kind::Float,
            Prim::Int(_) => Kind::Int,
            Prim::Str(_) => Kind::Str,
            Prim::Blob(_) => Kind::Blob
        }
    }

    pub fn nil(self) -> Result<()> {
        match self {
            Prim::Nil => Ok(()),
            other => Err(Error::UnexpectedKind(Kind::Nil, other.kind()))
        }
    }

    pub fn float(self) -> Result<f64> {
        match self {
            Prim::Float(p) => Ok(p),
            other => Err(Error::UnexpectedKind(Kind::Float, other.kind()))
        }
    }

    pub fn int(self) -> Result<i64> {
        match self {
            Prim::Int(p) => Ok(p),
            other => Err(Error::UnexpectedKind(Kind::Int, other.kind()))
        }
    }

    pub fn str(self) -> Result<String> {
        match self {
            Prim::Str(p) => Ok(p),
            other => Err(Error::UnexpectedKind(Kind::Str, other.kind()))
        }
    }

    pub fn blob(self) -> Result<Vec<u8>> {
        match self {
            Prim::Blob(p) => Ok(p),
            other => Err(Error::UnexpectedKind(Kind::Blob, other.kind()))
        }
    }
}

impl Op {
    pub fn kind(&self) -> Kind {
        match self {
            Op::Reg(_) => Kind::Reg,
            Op::Set(_) => Kind::Set,
            Op::Map(_) => Kind::Map
        }
    }
}

impl Kind {
    pub fn default_data(&self) -> Data {
        match self {
            Kind::Nil => Data::Nil,
            Kind::Reg => Data::Reg(crdts::MVReg::default()),
            Kind::Set => Data::Set(crdts::Orswot::default()),
            Kind::Map => Data::Map(crdts::Map::default()),

            // TAI: does it make sense to implement these prim kinds as Reg(<prim>::default())?
            Kind::Float => panic!("attempted to call default_data on Kind::Float"),
            Kind::Int => panic!("attempted to call default_data on Kind::Int"),
            Kind::Str => panic!("attempted to call default_data on Kind::Str"),
            Kind::Blob => panic!("attempted to call default_data on Kind::Blob")
        }
    }
}

impl From<f64> for Prim {
    fn from(p: f64) -> Self {
        Prim::Float(p)
    }
}

impl From<i64> for Prim {
    fn from(p: i64) -> Self {
        Prim::Int(p)
    }
}

impl From<String> for Prim {
    fn from(p: String) -> Self {
        Prim::Str(p)
    }
}

impl<'a> From<&'a str> for Prim {
    fn from(p: &'a str) -> Self {
        Prim::from(p.to_string())
    }
}

impl From<Vec<u8>> for Prim {
    fn from(p: Vec<u8>) -> Self {
        Prim::Blob(p)
    }
}

impl From<crdts::mvreg::Op<Prim, Actor>> for Op {
    fn from(op: crdts::mvreg::Op<Prim, Actor>) -> Self {
        Op::Reg(op)
    }
}

impl From<crdts::orswot::Op<Prim, Actor>> for Op {
    fn from(op: crdts::orswot::Op<Prim, Actor>) -> Self {
        Op::Set(op)
    }
}

impl From<crdts::map::Op<(Vec<u8>, Kind), Box<Data>, Actor>> for Op {
    fn from(op: crdts::map::Op<(Vec<u8>, Kind), Box<Data>, Actor>) -> Self {
        Op::Map(op)
    }
}
