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
use crate::api::service::WriterCoreApi;
use crate::api::types::PlatformInitDto;
use crate::app_service::WriterAppService;

///   启动时恢复待处理的删除事务。
///
/// 在创建 `WriterAppService` 之前调用，确保崩溃前的删除事务被完成。
/// 恢复失败返回 Err（用 `?` 严格返回），让调用方决定。
///
/// recover 返回 `Vec<RecoveredProjectDelete>`，
/// 每个含待补 history 的 change-set。bootstrap 用 layout 调
/// `record_workspace_change_set` 写本地 history，成功后调
/// `ack_project_delete_history` 推进 journal 到 `HistoryRecorded` → `Completed`
/// 并清 journal。history 失败时 log::warn 并保留 journal，下次启动 recover 补记。
fn recover_storage_transactions(
    app_data_root: &Path,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
) -> std::result::Result<(), WriterError> {
    // 恢复项目删除事务（project_delete journal）。
    let recovered_list =
        crate::storage::journal::project_delete::recover_pending_delete_transactions(
            app_data_root,
        )?;
    for rec in &recovered_list {
        match crate::storage::workspace_git::record_workspace_change_set(
            layout,
            &rec.changes,
            "recover_project_delete",
        ) {
            Ok(result) => {
                if result.oid.is_some() {
                    log::debug!(
                        "recover_storage_transactions: history committed for {} ({} staged)",
                        rec.journal_token,
                        result.staged_count
                    );
                }
                // history 成功，ack 推进 journal。
                if let Err(e) = crate::storage::journal::project_delete::ack_project_delete_history(
                    app_data_root,
                    &rec.journal_token,
                ) {
                    log::warn!(
                        "recover_storage_transactions: ack failed for {}: {} — journal retained",
                        rec.journal_token,
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "recover_storage_transactions: record_workspace_change_set failed for {}: {} \
                     — journal retained, history will be补 on next startup",
                    rec.journal_token,
                    e
                );
            }
        }
    }

    // 恢复统一 workspace 变更事务（workspace_change journal）。
    recover_workspace_change_transactions(app_data_root, layout)?;
    Ok(())
}

/// 恢复统一 workspace 变更事务（workspace_change journal）。
///
/// 扫描未完成 journal，如果本地删除已完成但 history 没写进去就补 history，
/// 只有 history 成功后才清 journal。
fn recover_workspace_change_transactions(
    app_data_root: &Path,
    layout: &crate::storage::git_repo_layout::GitRepoLayout,
) -> std::result::Result<(), WriterError> {
    let recovered_changes =
        crate::storage::journal::workspace_change::recover_unfinished(app_data_root)?;
    for rec in &recovered_changes {
        let message = match rec.op_type {
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::DeleteProject => {
                "recover_delete_project"
            }
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::DeleteVolume => {
                "recover_delete_volume"
            }
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::DeleteChapter => {
                "recover_delete_chapter"
            }
        };
        match crate::storage::workspace_git::record_workspace_change_set(
            layout,
            &rec.changes,
            message,
        ) {
            Ok(result) => {
                if result.oid.is_some() {
                    log::debug!(
                        "recover_workspace_change_transactions: history committed for {} \
                         ({} staged)",
                        rec.journal_token,
                        result.staged_count
                    );
                }
                // history 成功，推进 journal 到 HistoryRecorded 并清理。
                let journal = crate::storage::journal::workspace_change::WorkspaceChangeJournal {
                    token: rec.journal_token.clone(),
                    change_set: rec.changes.clone(),
                    device_id: String::new(),
                    op_type: rec.op_type.clone(),
                    created_at: 0,
                    phase:
                        crate::storage::journal::workspace_change::WorkspaceChangePhase::LocalApplied,
                };
                if let Err(e) = journal.mark_history_recorded(app_data_root) {
                    log::warn!(
                        "recover_workspace_change_transactions: mark_history_recorded failed \
                         for {}: {} — journal retained",
                        rec.journal_token,
                        e
                    );
                } else if let Err(e) = journal.clear_journal(app_data_root) {
                    log::warn!(
                        "recover_workspace_change_transactions: clear_journal failed for {}: {}",
                        rec.journal_token,
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "recover_workspace_change_transactions: history failed for {}: {} \
                     — journal retained, history will be补 on next startup",
                    rec.journal_token,
                    e
                );
            }
        }
    }
    Ok(())
}

/// 应用打开 workspace 时初始化唯一 Git repo。
///
/// 本地 Git 仓库的生命周期独立于 SyncProvider — 只要 workspace 被打开，
/// Git 历史层就存在，不依赖有没有启用远端同步。
///
/// 返回对应的 `GitRepoLayout`，供调用方注入到 `WriterCoreApi`。
fn ensure_workspace_git(
    app_data_root: &Path,
) -> std::result::Result<crate::storage::git_repo_layout::GitRepoLayout, WriterError> {
    let layout = crate::storage::git_repo_layout::GitRepoLayout::new(app_data_root.to_path_buf());
    crate::storage::workspace_git::ensure_workspace_repo(&layout)?;
    // bootstrap 初始化后实际调用 recover_workspace_crash，
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

/// 统一 workspace bootstrap 入口。
///
/// 封装 `ensure_workspace_git` + `recover_storage_transactions`，返回初始化好的
/// `GitRepoLayout`。`open_app_service*` 和 Linux_Qt 的 `create_core_api` 都调用
/// 这一入口，不允许桌面端自己再拼一套初始化。
///
/// - 确保 workspace Git 仓库存在（`.git`）
/// - 恢复 HEAD/index 损坏
/// - 恢复未完成的删除事务
///
/// 返回的 layout 供调用方注入到 `WriterCoreApi` / `WriterAppService`。
pub fn bootstrap_workspace(
    app_data_root: &Path,
) -> std::result::Result<crate::storage::git_repo_layout::GitRepoLayout, WriterError> {
    let layout = ensure_workspace_git(app_data_root)?;
    recover_storage_transactions(app_data_root, &layout)?;
    Ok(layout)
}

/// 统一构造已 bootstrap 的 `WriterCoreApi`。
///
/// 供 Linux_Qt 等需要直接使用 `WriterCoreApi`（而非 `WriterAppService`）的
/// 平台端调用。内部走 `bootstrap_workspace` 确保 `.git` 存在、删除事务已恢复，
/// 并注入正确的 `GitRepoLayout`，不裸构造未 bootstrap 的 API。
pub fn bootstrap_core_api<P1: AsRef<Path>, P2: AsRef<Path>>(
    app_data_root: P1,
    projects_root: P2,
    sync_transport_factory: Option<writer_platform_api::SyncTransportFactory>,
    secure_storage: Option<std::sync::Arc<dyn writer_platform_api::SecureStorage>>,
) -> std::result::Result<WriterCoreApi, WriterError> {
    let app_data_root_path = app_data_root.as_ref();
    let layout = bootstrap_workspace(app_data_root_path)?;
    let api = WriterCoreApi::with_platform_services(
        app_data_root,
        projects_root,
        sync_transport_factory,
        secure_storage,
    );
    api.set_workspace_git_layout(layout);
    Ok(api)
}

/// 仅凭根目录打开服务，不注入平台能力。
pub fn open_app_service(
    app_data_root: String,
    projects_root: String,
) -> std::result::Result<Arc<WriterAppService>, WriterError> {
    crate::storage::git_runtime::ensure_initialized()?;
    // 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务。
    let layout = bootstrap_workspace(Path::new(&app_data_root))?;
    let service = Arc::new(WriterAppService::new(app_data_root, projects_root));
    // 注入 bootstrap 计算的 layout 到 API 层。
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
    // 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务。
    let layout = bootstrap_workspace(Path::new(&app_data_root))?;
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
    // 注入 bootstrap 计算的 layout 到 API 层。
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
    // 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务。
    let layout = bootstrap_workspace(Path::new(&app_data_root))?;
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
    // 注入 bootstrap 计算的 layout 到 API 层。
    service.set_workspace_git_layout(layout);
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_app_service_with_secure_storage: {e}");
    }
    Ok(service)
}
