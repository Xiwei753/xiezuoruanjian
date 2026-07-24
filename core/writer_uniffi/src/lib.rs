//! # Writer UniFFI — 跨平台绑定入口
//!
//! 本 crate 是 UniFFI 绑定的中间层，负责：
//! - 重新导出 `writer_core` 的稳定 API（包括 UniFFI scaffolding）
//! - 提供 `uniffi-bindgen` 二进制用于生成 Kotlin/Swift 绑定
//!
//! ## 依赖方向
//!
//! ```text
//! 原生端 → 平台 crate (cdylib) → writer_uniffi (rlib) → writer_core + writer_platform_api
//! ```
//!
//! `writer_uniffi` 自身是 `rlib`，不直接产生 `cdylib`。
//! 最终 `cdylib` 由平台 crate（`writer-platform-android`、`writer-platform-linux`）组装，
//! 每个平台 crate 包含通用核心、平台适配和 UniFFI 元数据。
//!
//! UniFFI scaffolding（UDL + `include_scaffolding!`）保留在 `writer_core` 中，
//! 因为 UDL 中定义的类型实现位于 `writer_core`，UniFFI 要求 scaffolding 与类型定义在同一 crate。
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
pub use writer_core::open_workspace_with_init;
pub use writer_core::open_workspace_with_secure_storage;
pub use writer_core::open_workspace_with_platform_services;
pub use writer_core::SecureStorageProvider;
pub use writer_core::repair_workspace;
pub use writer_core::create_project_in_workspace;
pub use writer_core::load_workspace_summary;
pub use writer_platform_api::PlatformServices;
pub use writer_platform_api::SyncTransportFactory;
