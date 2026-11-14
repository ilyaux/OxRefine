//! OxRefine engine: dictionary-encoded string table + OpenRefine-compatible operations.
pub mod cluster;
pub mod commons;
mod cluster_hints;
pub mod cross;
pub mod database;
mod database_profiles;
pub mod engine;
pub mod export;
pub mod double_metaphone;
pub mod grel;
pub mod import;
pub mod jobs;
pub mod memory;
pub mod metaphone3;
pub mod ops;
pub mod orproject;
pub mod ppm;
pub mod project;
pub mod recon;
pub mod recon_values;
pub mod reshape;
pub mod store;
pub mod spill;
pub mod storage;
pub mod table;
mod transform_preview;
pub mod unpack;

pub use project::{dispatch, dispatch_free, Project, Session};

pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
