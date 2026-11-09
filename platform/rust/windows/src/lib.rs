//! # Windows 平台适配层
//!
//! 提供 Windows 桌面端的平台初始化、目录解析、配置存储、同步传输和最终库组装。
//! 与 Linux 适配层保持相同的公开函数面，便于桌面 Qt 前端共用平台无关代码。
//!
//! ## 职责
//!
//! - 按 Windows 用户目录约定解析应用目录（`%APPDATA%` / `%LOCALAPPDATA%`）
//! - 构造 `PlatformInit` 并注入 Core
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输（尊重系统/环境代理）
//! - 组装最终 `cdylib`：包含通用核心、Windows 适配和 UniFFI 元数据

// 确保 writer_uniffi 被链接进 cdylib，保留最小成员标注。
#[allow(unused_imports)]
use writer_uniffi::WriterAppService;

mod dirs;
mod init;
mod network;
mod secure_storage;
mod services;
mod startup_diagnostics;
mod transport;

pub use dirs::{
    crash_diagnostics_dir, diagnostics_dir, diagnostics_export_dir, runtime_log_dir,
    startup_diagnostics_dir, sujian_home_dir, xdg_cache_dir, xdg_config_dir,
};
pub use init::{init_default_config_store, init_diagnostics, resolve_platform_init};
pub use network::{get_cached_network_state, refresh_network_state};
pub use services::create_platform_services;
pub use startup_diagnostics::{begin_startup_diagnostics, StartupDiagnostics};
#[cfg(feature = "github-api")]
pub use transport::{create_sync_transport, ReqwestSyncTransport};
