#[cfg(feature = "sdk-basic")]
mod basic;

pub mod taproot_pubkey_gen;
pub mod validation;

#[cfg(feature = "sdk-basic")]
pub use basic::*;
