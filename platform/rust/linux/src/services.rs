//! Linux 平台服务组装。
//!
//! 聚合目录解析、平台初始化、配置存储、安全存储、网络探测与同步传输，
//! 构造 `PlatformServices` 注入 Core。

use writer_platform_api::{
    FileConfigStore, PlatformServices, SecureStorage, SyncTransport, TransportError,
};

use super::dirs::xdg_config_dir;
use super::init::resolve_platform_init;
use super::network::{cache_network_state, detect_network_state};
use super::secure_storage::create_secure_storage;
#[cfg(feature = "github-api")]
use super::transport::ReqwestSyncTransport;

pub fn create_platform_services() -> PlatformServices {
    let init = resolve_platform_init();
    let config_dir = xdg_config_dir();
    let config_store: Option<Box<dyn writer_platform_api::ConfigStore>> =
        Some(Box::new(FileConfigStore::new(config_dir)));

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

    let secure_storage: Option<Box<dyn SecureStorage>> = create_secure_storage();

    let network_state = detect_network_state();
    cache_network_state(&network_state);

    PlatformServices {
        init,
        config_store,
        secure_storage,
        network_state: Some(network_state),
        sync_transport_factory,
    }
}
