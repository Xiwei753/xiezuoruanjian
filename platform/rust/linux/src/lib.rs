//! # Linux 平台适配层
//!
//! 提供 Linux 桌面端的平台初始化、目录解析、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 按 XDG Base Directory 规范解析应用目录
//! - 构造 `PlatformInit` 并注入 Core
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输（尊重系统/环境代理）
//! - 组装最终 `cdylib`：包含通用核心、Linux 适配和 UniFFI 元数据
//!
//! ## 依赖方向
//!
//! ```text
//! Linux Qt 应用 → writer-platform-linux (cdylib) → writer_uniffi → writer_core + writer_platform_api
//! ```

// 确保 writer_uniffi 被链接进 cdylib，保留最小成员标注。
#[allow(unused_imports)]
use writer_uniffi::WriterAppService;

mod dirs;
mod init;
mod network;
mod secure_storage;
mod services;
mod transport;

pub use dirs::{xdg_cache_dir, xdg_config_dir};
pub use init::{init_default_config_store, resolve_platform_init};
pub use network::{get_cached_network_state, refresh_network_state};
pub use services::create_platform_services;
#[cfg(feature = "github-api")]
pub use transport::{create_sync_transport, ReqwestSyncTransport};
