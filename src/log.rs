extern crate crdts;

use std::fmt::Debug;

use crdts::{CmRDT, Actor};
use error::Result;

pub trait TaggedOp<C: CmRDT> {
    type ID: Eq;

    fn id(&self) -> Self::ID;
    fn op(&self) -> &C::Op;
}

pub trait LogReplicable<A: Actor, C: CmRDT> {
    type Op: Debug + TaggedOp<C>;

    fn next(&self) -> Result<Option<Self::Op>>;
    fn ack(&mut self, op: &Self::Op) -> Result<()>;
    fn commit(&mut self, op: C::Op) -> Result<Self::Op>;
    fn pull(&mut self, other: &Self) -> Result<()>;
    fn push(&self, other: &mut Self) -> Result<()>;
}
