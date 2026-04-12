const _: () = assert!(std::mem::size_of::<usize>() == 8, "Only 64-bit architecture supported");

pub mod engine;
pub mod error;
pub mod ingress;
pub mod matcher;
pub mod wal;

pub use engine::{Engine, EngineConfig};
pub use error::Error;
