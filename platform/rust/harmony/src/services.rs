//! HarmonyOS 平台服务组装。
//!
//! 聚合平台初始化、配置存储、网络状态与同步传输工厂，
//! 构造 `PlatformServices` 注入 Core。

use writer_platform_api::{FileConfigStore, NetworkState, PlatformInit, PlatformServices};

#[cfg(feature = "github-api")]
use writer_platform_api::{SyncTransport, TransportError};

#[cfg(feature = "github-api")]
use super::transport::ReqwestSyncTransport;

/// 组装 Harmony 平台的 `PlatformServices`。
///
/// 包含：
/// - `PlatformInit`：Harmony 平台初始化上下文
/// - `ConfigStore`：基于 `FileConfigStore` 的配置存储
/// - `NetworkState`：网络连接状态；`None` 表示平台查询失败/未知，不注入伪造的在线状态
/// - `SyncTransportFactory`：基于 `ReqwestSyncTransport` 的同步传输工厂（仅 `github-api` feature）
///
/// `is_connected` / `is_metered` 为 `Option<bool>`：`None` 是「未知」，不是「离线」。
/// 平台查询失败时不能默认成「在线」，因此未知状态完全不写 `network_state`，
/// 由后续 `writer_core_update_network_state` 在真实查询成功后再更新。
pub fn create_platform_services(
    platform_init: PlatformInit,
    is_connected: Option<bool>,
    is_metered: Option<bool>,
) -> PlatformServices {
    let config_dir = platform_init.app_data_dir.join("config");

    let config_store: Option<Box<dyn writer_platform_api::ConfigStore>> =
        Some(Box::new(FileConfigStore::new(config_dir)));

    let network_state = is_connected.map(|connected| NetworkState {
        is_connected: connected,
        // 计费状态未知时按非计费处理（不阻断同步），连通性未知才整体标记未知。
        is_metered: is_metered.unwrap_or(false),
        proxy_host: None,
        proxy_port: None,
    });
    if network_state.is_none() {
        log::warn!(
            "harmony: network state unknown (hasDefaultNet query failed); \
             not injecting a fabricated online state"
        );
    }

    #[cfg(feature = "github-api")]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = {
        let factory: writer_platform_api::SyncTransportFactory =
            std::sync::Arc::new(|| -> Result<Box<dyn SyncTransport>, TransportError> {
                ReqwestSyncTransport::new().map(|t| Box::new(t) as Box<dyn SyncTransport>)
            });
        Some(factory)
    };
    #[cfg(not(feature = "github-api"))]
    let sync_transport_factory: Option<writer_platform_api::SyncTransportFactory> = None;

    PlatformServices {
        init: platform_init,
        config_store,
        secure_storage: None,
        network_state,
        sync_transport_factory,
    }
}
