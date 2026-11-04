//! Tools for local COFF development, independent of a particular CLI.
//!
//! Inspection and argument packing are portable. Execution requires Windows x64.
//! The runtime invokes native code; see [`runtime::execute`] for its safety contract.

pub mod args;
pub mod bundle;
pub mod cna;
pub mod coff;
mod comdat;
pub mod downloads;
pub mod inspect;
pub mod library;
pub mod link;
pub mod manifest;
pub mod runtime;
pub mod standalone;
pub mod suite;
pub mod tool;

/// Maximum accepted object, argument, or bundle payload size (64 MiB).
pub const MAX_INPUT: usize = 64 * 1024 * 1024;

/// Read a bounded input without trusting its reported file length.
pub fn read_input(path: &std::path::Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_INPUT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "input exceeds 64 MiB",
        ));
    }
    Ok(bytes)
}
