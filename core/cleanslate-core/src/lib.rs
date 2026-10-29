//! CleanSlate engine.
//!
//! This crate holds all the business logic of CleanSlate. It contains no FFI
//! code: the `cleanslate-ffi` crate exposes it to C#, and `cleanslate-cli`
//! drives it from the command line.

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
