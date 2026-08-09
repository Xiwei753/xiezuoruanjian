//! # API DTO 模块 — Core 内部类型与跨语言边界的类型映射
//!
//! 每个 `*_dto` 类型对应一个 Core 内部类型，提供 `From` 双向转换。
//! DTO 使用 serde JSON 序列化，是 FFI/HTTP 边界的唯一合法数据格式。
//! 内部类型的业务不变量在 DTO 层不强制执行（DTO 是薄传输层），
//! 转换回内部类型时由 Core 验证。

mod action;
mod editor;
mod platform;
mod platform_interaction;
mod project;
mod recent_edits;
pub mod screen_policy;
mod settings;
mod starmap;
mod stats;
mod sync;
mod volume;
#[cfg(test)]
mod settings_tests;
#[cfg(test)]
mod sync_tests;

#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod project_tests;
#[cfg(test)]
mod recent_edits_tests;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod volume_tests;

pub use action::*;
pub use editor::*;
pub use platform::*;
pub use platform_interaction::*;
pub use project::*;
pub use recent_edits::*;
pub use screen_policy::*;
pub use settings::*;
pub use starmap::*;
pub use stats::*;
pub use sync::*;
pub use volume::*;
