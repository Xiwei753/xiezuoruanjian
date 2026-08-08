//! Stable Core API layer for platform bindings.
//!
//! This module is the reusable boundary for UniFFI, Linux bindings, and future
//! platform frontends. It wraps the internal facade and exposes stable DTOs and
//! API errors without depending on any platform UI or binding implementation.

pub mod bootstrap;
pub mod chapter_api;
pub mod envelope;
pub mod error;
pub mod secure_storage_bridge;
pub mod service;
pub mod settings_api;
pub mod sync_api;
pub mod types;

pub use bootstrap::*;
pub use envelope::{ChangedEntityDto, ResultEnvelope};
pub use error::WriterError;
pub use secure_storage_bridge::*;
pub use service::WriterCoreApi;
pub use types::*;
