//! 应用服务启动入口。
//!
//! 提供从平台初始化信息构造 `WriterAppService` 的若干变体：
//! - `open_app_service`：仅传入根目录，不注入平台服务
//! - `open_app_service_with_init`：注入 `PlatformInitDto`（含网络状态）
//! - `open_app_service_with_secure_storage`：同时注入安全存储 callback
//!
//! 这些函数是平台客户端打开 Core 的统一入口，UniFFI 通过 `#[::uniffi::export]`
//! 暴露给各平台绑定层。

use std::sync::Arc;

use writer_platform_api::{
    get_platform_services_resolver, ConfigStore, FileConfigStore, NetworkState, PlatformInit,
    PlatformServices, SecureStorage,
};

use super::secure_storage::{wrap_secure_storage, SecureStorageProvider};
use crate::api::types::PlatformInitDto;
use crate::api::error::WriterError;
use crate::app_service::WriterAppService;

/// 仅凭根目录打开服务，不注入平台能力。
pub fn open_app_service(
    app_data_root: String,
    projects_root: String,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    let service = Arc::new(WriterAppService::new(app_data_root, projects_root));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service: {e}");
    }
    Ok(service)
}

/// 注入平台初始化信息打开服务。
pub fn open_app_service_with_init(
    app_data_root: String,
    projects_root: String,
    init: PlatformInitDto,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    let platform_init: PlatformInit = init.clone().into();
    let network_state: NetworkState = init.into();

    let services = if let Some(resolver) = get_platform_services_resolver() {
        resolver.resolve(&platform_init, &network_state)
    } else {
        let config_dir = platform_init.app_data_dir.join("config");
        let config_store: Option<Box<dyn ConfigStore>> = Some(Box::new(FileConfigStore::new(
            config_dir,
        )));

        PlatformServices {
            init: platform_init,
            config_store,
            secure_storage: None,
            network_state: Some(network_state),
            sync_transport_factory: None,
        }
    };

    let service = Arc::new(WriterAppService::with_platform_services(
        app_data_root,
        projects_root,
        services,
    ));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service_with_init: {e}");
    }
    Ok(service)
}

/// 注入平台初始化信息与安全存储 callback 打开服务。
#[::uniffi::export]
pub fn open_app_service_with_secure_storage(
    app_data_root: String,
    projects_root: String,
    init: PlatformInitDto,
    secure_storage: Option<Box<dyn SecureStorageProvider>>,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    let platform_init: PlatformInit = init.clone().into();
    let network_state: NetworkState = init.into();

    let secure_storage_impl: Option<Box<dyn SecureStorage>> = secure_storage
        .map(wrap_secure_storage);

    let config_dir = platform_init.app_data_dir.join("config");
    let config_store: Option<Box<dyn ConfigStore>> = Some(Box::new(FileConfigStore::new(
        config_dir,
    )));

    let services = if let Some(resolver) = get_platform_services_resolver() {
        let mut resolved = resolver.resolve(&platform_init, &network_state);
        if secure_storage_impl.is_some() {
            resolved.secure_storage = secure_storage_impl;
        }
        resolved
    } else {
        PlatformServices {
            init: platform_init,
            config_store,
            secure_storage: secure_storage_impl,
            network_state: Some(network_state),
            sync_transport_factory: None,
        }
    };

    let service = Arc::new(WriterAppService::with_platform_services(
        app_data_root,
        projects_root,
        services,
    ));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service_with_secure_storage: {e}");
    }
    Ok(service)
}
