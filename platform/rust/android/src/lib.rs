//! # Android 平台适配层
//!
//! 提供 Android 端的平台初始化、配置存储、同步传输和最终库组装。
//!
//! ## 职责
//!
//! - 接收 Kotlin 层传入的 Context 目录信息构造 `PlatformInit`
//! - 使用 `writer_platform_api::FileConfigStore` 提供配置存储
//! - 通过 `ReqwestSyncTransport` 提供同步 HTTP 传输
//! - 安全存储由 Kotlin 侧通过 UniFFI callback interface 实现 Android Keystore，
//!   Rust Core 只消费 `SecureStorage` 接口
//! - 组装最终 `cdylib`：包含通用核心、Android 适配和 UniFFI 元数据
//!
//! ## 依赖方向
//!
//! ```text
//! Kotlin/Compose → writer-platform-android (cdylib) → writer_uniffi → writer_core + writer_platform_api
//! ```

// 确保 writer_uniffi 被链接进 cdylib，保留最小成员标注。
#[allow(unused_imports)]
use writer_uniffi::WriterAppService;

mod init;
mod services;
mod transport;

pub use init::*;
pub use services::*;
pub use transport::*;
