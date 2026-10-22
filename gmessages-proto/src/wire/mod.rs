//! Wire-level protobuf messages, as described by the specification.
//!
//! Field numbers are what matters on the wire; names are this crate's own. Every type
//! encodes to binary protobuf (via [`prost::Message`]) and to pblite (via
//! [`PbLite`](crate::codec::PbLite)). Applications normally use the higher-level types of
//! [`crate::types`] instead.

#![allow(missing_docs)]

use crate::codec::{
    B64Msg, B64Str, Bool, Bytes, Enum, Fx64, I32, I64, Msg, Msgs, Opt, Rep, Str, U64,
};
use crate::message;

mod actions;
mod common;
mod data;
mod pairing;
mod relay;

pub use actions::*;
pub use common::*;
pub use data::*;
pub use pairing::*;
pub use relay::*;

// Silence "unused import" for kinds only some submodules use.
#[allow(dead_code)]
type _Kinds = (
    B64Msg<Empty>,
    B64Str,
    Bool,
    Bytes,
    Enum,
    Fx64,
    I32,
    I64,
    Msg<Empty>,
    Opt<Str>,
    Rep<Str>,
    Str,
    U64,
);
