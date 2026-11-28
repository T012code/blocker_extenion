//! Smart Clip's sync engine. Everything here is plain Rust with no Windows calls, so it
//! can be tested on any computer: signing in with an emailed code, the clips database,
//! images, and the live channel that brings new clips from the account's other PCs.

pub mod api;
pub mod auth;
pub mod content;
pub mod engine;
pub mod error;
pub mod image;
pub mod model;
pub mod realtime;

pub use engine::{Engine, Host, Status};
pub use error::Error;
pub use model::{Captured, Clip, ClipKind, ClipSummary, Config, StoredSession};
