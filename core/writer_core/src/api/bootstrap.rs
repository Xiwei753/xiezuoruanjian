//! 应用服务启动入口。
//!
//! 提供从平台初始化信息构造 `WriterAppService` 的若干变体：
//! - `open_app_service`：仅传入根目录，不注入平台服务
//! - `open_app_service_with_init`：注入 `PlatformInitDto`（含网络状态）
//! - `open_app_service_with_secure_storage`：同时注入安全存储 callback
//!
//! 这些函数是平台客户端打开 Core 的统一入口，UniFFI 通过 `#[::uniffi::export]`
//! 暴露给各平台绑定层。

use std::path::Path;
use std::sync::Arc;

use writer_platform_api::{
    get_platform_services_resolver, ConfigStore, FileConfigStore, NetworkState, PlatformInit,
    PlatformServices, SecureStorage,
};

use super::secure_storage_bridge::{wrap_secure_storage, SecureStorageProvider};
use crate::api::error::WriterError;
use crate::api::types::PlatformInitDto;
use crate::app_service::WriterAppService;

/// #644 评论 5495945801 问题4：启动时恢复待处理的删除事务。
///
/// 在创建 `WriterAppService` 之前调用，确保崩溃前的删除事务被完成。
/// 恢复失败返回 Err（用 `?` 严格返回），让调用方决定。
fn recover_storage_transactions(app_data_root: &Path) -> std::result::Result<(), WriterError> {
    crate::storage::journal::project_delete::recover_pending_delete_transactions(app_data_root)?;
    Ok(())
}

/// #645 评论 5504296097 第2点：应用打开 workspace 时初始化唯一 Git repo。
///
/// 本地 Git 仓库的生命周期独立于 SyncProvider — 只要 workspace 被打开，
/// Git 历史层就存在，不依赖有没有启用远端同步。
///
/// `git_metadata_root`：Android 私有 Git metadata 根目录；`None` 用标准布局。
///
/// 返回对应的 `GitRepoLayout`，供调用方注入到 `WriterCoreApi`。
fn ensure_workspace_git(
    app_data_root: &Path,
    git_metadata_root: Option<&Path>,
) -> std::result::Result<crate::storage::git_repo_layout::GitRepoLayout, WriterError> {
    let layout = match git_metadata_root {
        Some(root) => crate::storage::git_repo_layout::GitRepoLayout::with_external_git_dir(
            app_data_root.to_path_buf(),
            root.join("workspace"),
        ),
        None => crate::storage::git_repo_layout::GitRepoLayout::new(app_data_root.to_path_buf()),
    };
    crate::storage::workspace_git::ensure_workspace_repo(&layout)?;
    // #645 评论 5504296097 问题4：bootstrap 初始化后实际调用 recover_workspace_crash，
    // 确保打开 workspace 时自动恢复 HEAD/index 损坏。
    match crate::storage::workspace_git::recover_workspace_crash(&layout) {
        Ok(result) => {
            if result.head_was_recovered || result.index_corrupted {
                log::info!(
                    "ensure_workspace_git: recovery performed (head={}, index={})",
                    result.head_was_recovered,
                    result.index_corrupted
                );
            }
        }
        Err(e) => {
            log::warn!(
                "ensure_workspace_git: recover_workspace_crash failed: {}",
                e
            );
        }
    }
    Ok(layout)
}

/// 仅凭根目录打开服务，不注入平台能力。
pub fn open_app_service(
    app_data_root: String,
    projects_root: String,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    crate::storage::git_runtime::ensure_initialized()?;
    // #645 评论 5504296097 第2点：应用打开时初始化 workspace Git。
    let layout = ensure_workspace_git(Path::new(&app_data_root), None)?;
    // #644 评论 5495945801 问题4：在创建服务之前先恢复待处理的删除事务。
    recover_storage_transactions(Path::new(&app_data_root))?;
    let service = Arc::new(WriterAppService::new(app_data_root, projects_root));
    // #645 评论 5504296097 问题3：注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
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
    crate::storage::git_runtime::ensure_initialized()?;
    // #644 评论 5491531984 问题5：git_metadata_root 不在 PlatformInit 中，
    // 从 PlatformInitDto 直接提取后传给 WriterCoreApi。
    let git_metadata_root = init
        .git_metadata_root
        .as_ref()
        .map(std::path::PathBuf::from);
    // #645 评论 5504296097 第2点：应用打开时初始化 workspace Git。
    let layout = ensure_workspace_git(Path::new(&app_data_root), git_metadata_root.as_deref())?;
    // #644 评论 5495945801 问题4：在创建服务之前先恢复待处理的删除事务。
    recover_storage_transactions(Path::new(&app_data_root))?;
    let platform_init: PlatformInit = init.clone().into();
    let network_state: NetworkState = init.into();

    let services = if let Some(resolver) = get_platform_services_resolver() {
        resolver.resolve(&platform_init, &network_state)
    } else {
        let config_dir = platform_init.app_data_dir.join("config");
        let config_store: Option<Box<dyn ConfigStore>> =
            Some(Box::new(FileConfigStore::new(config_dir)));

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
    // #645 评论 5504296097 问题3：注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
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
    crate::storage::git_runtime::ensure_initialized()?;
    // #644 评论 5491531984 问题5：git_metadata_root 不在 PlatformInit 中，
    // 从 PlatformInitDto 直接提取后传给 WriterCoreApi。
    let git_metadata_root = init
        .git_metadata_root
        .as_ref()
        .map(std::path::PathBuf::from);
    // #645 评论 5504296097 第2点：应用打开时初始化 workspace Git。
    let layout = ensure_workspace_git(Path::new(&app_data_root), git_metadata_root.as_deref())?;
    // #644 评论 5495945801 问题4：在创建服务之前先恢复待处理的删除事务。
    recover_storage_transactions(Path::new(&app_data_root))?;
    let platform_init: PlatformInit = init.clone().into();
    let network_state: NetworkState = init.into();

    let secure_storage_impl: Option<Box<dyn SecureStorage>> =
        secure_storage.map(wrap_secure_storage);

    let config_dir = platform_init.app_data_dir.join("config");
    let config_store: Option<Box<dyn ConfigStore>> =
        Some(Box::new(FileConfigStore::new(config_dir)));

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
    // #645 评论 5504296097 问题3：注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service_with_secure_storage: {e}");
    }
    Ok(service)
}
