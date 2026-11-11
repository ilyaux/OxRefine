//! OxRefine engine: dictionary-encoded string table + OpenRefine-compatible operations.
pub mod cluster;
pub mod engine;
pub mod export;
pub mod double_metaphone;
pub mod grel;
pub mod import;
pub mod jobs;
pub mod metaphone3;
pub mod ops;
pub mod orproject;
pub mod project;
pub mod store;
pub mod table;

pub use project::{dispatch, dispatch_free, Project, Session};

pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
