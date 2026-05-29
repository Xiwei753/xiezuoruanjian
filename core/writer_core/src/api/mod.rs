//! Stable Core API layer for platform bindings.
//!
//! This module is the reusable boundary for UniFFI, Linux bindings, and future
//! platform frontends. It wraps the internal facade and exposes stable DTOs and
//! API errors without depending on any platform UI or binding implementation.

pub mod error;
pub mod service;
pub mod types;

pub use error::WriterError;
pub use service::WriterCoreApi;
pub use types::*;
