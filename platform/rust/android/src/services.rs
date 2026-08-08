//! Android 平台服务解析与注册。
//!
//! 提供 `PlatformServicesResolver` 实现、自动注册（通过 `ctor` 在库加载时
//! 注册到 `writer_platform_api` 全局 resolver）、服务组装以及默认配置存储初始化。

use std::path::PathBuf;
use std::sync::OnceLock;

use writer_platform_api::{
    register_platform_services_resolver, FileConfigStore, NetworkState, PlatformInit,
    PlatformServices, PlatformServicesResolver, SyncTransport, TransportError,
};

#[cfg(feature = "github-api")]
use super::transport::ReqwestSyncTransport;

struct AndroidPlatformServicesResolver;

impl PlatformServicesResolver for AndroidPlatformServicesResolver {
    fn resolve(&self, init: &PlatformInit, network_state: &NetworkState) -> PlatformServices {
        create_platform_services(
            init.clone(),
            network_state.is_connected,
            network_state.is_metered,
        )
    }
}

static ANDROID_RESOLVER_REGISTERED: OnceLock<()> = OnceLock::new();

pub fn ensure_android_resolver_registered() {
    ANDROID_RESOLVER_REGISTERED.get_or_init(|| {
        register_platform_services_resolver(Box::new(AndroidPlatformServicesResolver));
    });
}

#[::ctor::ctor]
fn auto_register_android_resolver() {
    ensure_android_resolver_registered();
}

pub fn create_platform_services(
    platform_init: PlatformInit,
    is_connected: bool,
    is_metered: bool,
) -> PlatformServices {
    let config_dir = platform_init.app_data_dir.join("config");

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

    PlatformServices {
        init: platform_init,
        config_store,
        secure_storage: None,
        network_state: Some(NetworkState {
            is_connected,
            is_metered,
            proxy_host: None,
            proxy_port: None,
        }),
        sync_transport_factory,
    }
}

pub fn init_default_config_store(config_dir: PathBuf) {
    let store = FileConfigStore::new(config_dir);
    writer_core::app_config::set_default_config_store(Box::new(store));
}
