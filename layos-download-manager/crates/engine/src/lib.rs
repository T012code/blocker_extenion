//! Download engine for Layó's Download Manager.
//!
//! Splits each file into segments fetched over several connections at once, keeps
//! progress in SQLite so downloads survive restarts, and reports progress as events.

pub mod category;
mod engine;
pub mod error;
pub mod filename;
pub mod http;
pub mod limiter;
pub mod model;
pub mod runner;
pub mod store;
mod transfer;

pub use category::Category;
pub use engine::{Engine, EngineConfig, MAX_CONNECTIONS, SECTION_PRESETS};
pub use error::{Error, Result};
pub use model::*;
pub use runner::{Job, Reporter, Runner};
