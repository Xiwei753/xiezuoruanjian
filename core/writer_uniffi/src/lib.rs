//! # Writer UniFFI — 跨平台绑定入口与最终库组装
//!
//! 本 crate 是 UniFFI 对外的唯一入口，负责：
//! - 重新导出 `writer_core` 的稳定 API（包括 UniFFI scaffolding）
//! - 产生最终 `cdylib` 供 Android/HarmonyOS 等平台链接
//! - 提供 `uniffi-bindgen` 二进制用于生成 Kotlin/Swift 绑定
//!
//! ## 依赖方向
//!
//! ```text
//! 原生端 → writer_uniffi (cdylib) → writer_core + writer_platform_api
//! ```
//!
//! `writer_core` 自身是 `rlib`，不直接产生 `cdylib`。
//! UniFFI scaffolding（UDL + `include_scaffolding!`）保留在 `writer_core` 中，
//! 因为 UDL 中定义的类型实现位于 `writer_core`，UniFFI 要求 scaffolding 与类型定义在同一 crate。
//! 所有平台的最终库都通过 `writer_uniffi` 或平台 crate 组装。
//!
//! ## 导出边界
//!
//! 本 crate 只重新导出稳定 API，不导出 `writer_core` 的内部模块。
//! 平台端不应依赖 `chapter`、`editor`、`sync`、`storage` 等内部实现细节。

pub use writer_core::api::*;
pub use writer_core::app_config::AppConfig;
pub use writer_core::app_service::WriterAppService;
pub use writer_core::error::{Error, Result};
pub use writer_core::init_workspace;
pub use writer_core::open_workspace;
pub use writer_core::repair_workspace;
pub use writer_core::create_project_in_workspace;
pub use writer_core::load_workspace_summary;
