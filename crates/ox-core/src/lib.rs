//! OxRefine engine: dictionary-encoded string table + OpenRefine-compatible operations.
pub mod cluster;
pub mod engine;
pub mod grel;
pub mod ops;
pub mod project;
pub mod table;

pub use project::{dispatch, Project};

pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
