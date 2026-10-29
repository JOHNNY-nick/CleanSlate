//! CleanSlate engine.
//!
//! This crate holds all the business logic of CleanSlate. It contains no FFI
//! code: the `cleanslate-ffi` crate exposes it to C#, and `cleanslate-cli`
//! drives it from the command line.

pub mod engine;
pub mod error;
pub mod fs;
pub mod model;
pub mod registry;

pub use engine::{Engine, EngineConfig};
pub use error::{CoreError, CoreResult};

/// Engine version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_package() {
        assert_eq!(VERSION, "0.1.0");
    }
}
