//! # 平台能力契约 (Platform API)
//!
//! 本 crate 定义平台能力契约与初始化参数，是通用业务核心与平台适配层之间的稳定边界。
//!
//! ## 职责
//!
//! - 定义 `PlatformInit`：平台启动时注入的初始化上下文
//! - 定义 `PlatformPaths`：应用数据目录、缓存目录、日志目录等路径
//! - 定义 `ConfigStore`：配置存储契约
//! - 定义 `SecureStorage`：安全存储契约（令牌、凭据）
//! - 定义 `NetworkState`：网络状态信息
//! - 定义 `SyncTransport`：同步传输契约（HTTP 执行与同步协议分离）
//!
//! ## 依赖方向
//!
//! ```text
//! writer_platform_api <- writer_core
//! writer_platform_api <- platform/rust/<target>
//! ```
//!
//! `writer_platform_api` 不依赖 `writer_core` 或任何平台 crate。

mod config_store;
mod network_state;
mod platform_init;
mod platform_paths;
mod secure_storage;
mod sync_transport;

pub use config_store::{ConfigStore, FileConfigStore};
pub use network_state::NetworkState;
pub use platform_init::{PlatformInit, PlatformKind, PlatformPaths};
pub use secure_storage::SecureStorage;
pub use sync_transport::{HttpRequest, HttpResponse, SyncTransport, TransportError};
