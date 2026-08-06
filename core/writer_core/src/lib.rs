//! # Writer Core - 核心业务逻辑层
//!
//! 本 crate 是整个应用的**唯一事实来源**（Single Source of Truth）。
//! 所有文件 I/O、项目管理、同步、格式化、设置规则都在此实现。
//!
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::field_reassign_with_default,
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity,
        deprecated
    )
)]
//! ## 架构约束
//!
//! - **严格排除 UI 逻辑**：不允许出现动画、窗口管理、输入法、平台特定代码。
//! - **客户端只做展示**：平台客户端通过 `writer_uniffi` 或 `writer_platform_api` 调用 Core。
//! - **正文永远是纯文本**：所有文件读写都是纯文本，不使用 HTML。
//! - **平台能力注入**：目录、设备 ID、密钥存储、网络状态由 `writer_platform_api` 契约注入，Core 不自行猜测平台。
//!
//! ## 模块分层
//!
//! | 模块 | 职责 | 边界 |
//! |------|------|------|
//! | `facade` | Core 内部统一入口，聚合所有子模块 | 不直接作为平台稳定边界 |
//! | `api` | 跨平台稳定 API 层，提供 DTO / Error / Service | `writer_uniffi` 和平台适配层的底座 |
//! | `workspace` | 工作区创建、验证、最近编辑 | 不处理项目内容 |
//! | `project` | 作品 CRUD、统计、排序、删除 | 删除走 `delete_guard` |
//! | `volume` | 卷 CRUD、排序、删除 | 删除走 `delete_guard` |
//! | `chapter` | 章节 CRUD、内容读写、验证保存 | 正文永远是纯文本 |
//! | `settings` | 本地设置 & 可同步设置 | 分为 LocalSettings / SyncableSettings |
//! | `sync` | 同步配置、密钥、状态、诊断、实际同步（合并了 sync） | 唯一同步模块 |

//! | `starmap` | 星图（元数据、图、布局）- 正式图谱路线 | 唯一推荐图谱接口 |
//! | `writing_stats` | 写作统计（事件记录、聚合、查询） | 按设备/项目/章节统计 |
//! | `error` | 统一错误类型 | 所有模块共享 |
//! | `storage` | 原子替换文件写入（写临时文件 + fsync 临时文件 + rename） | 防止半写入；耐久性受文件系统语义影响 |
//! | `delete_guard` | 删除安全守卫（验证 ID、防止误删工作区根） | 所有删除操作必须经过此模块 |
//!
//! ## 调用链路示例
//!
//! ```text
//! 客户端 → facade::WriterCore::create_chapter()
//!         → chapter::create_chapter()
//!         → storage::atomic_write_string()
//! ```

pub mod action_registry;
pub mod api;
pub(crate) mod delete_guard;
// Always export these for UniFFI
pub mod ai_service;
pub use crate::ai_service::{AiAction, AiActionResponse, AiActionType};
pub mod app_config;
pub mod settings_registry;

pub mod chapter;
pub mod editor;
pub mod error;
pub mod history;
pub mod index;
pub mod platform_interaction;

#[cfg(feature = "harmony-ffi")]
pub mod ffi;
pub mod layout_policy;
pub mod project;
pub mod screen_policy;
pub mod search;
pub mod settings;
pub mod settings_presentation;
pub mod starmap;
pub mod storage;
pub mod sync;
pub mod trash;
pub mod volume;
pub mod workspace;
pub mod writing_stats;

pub use api::*;
pub use error::{Error, Result};

#[cfg(test)]
pub mod action_ops_tests;
#[cfg(test)]
pub mod chapter_tests;
#[cfg(test)]
pub mod dto_contract_tests;
pub mod facade;
#[cfg(test)]
pub mod fixture_tests;
#[cfg(test)]
pub mod index_tests;
#[cfg(test)]
pub mod project_tests;
#[cfg(test)]
pub mod settings_tests;
#[cfg(test)]
pub mod sync_api_tests;
#[cfg(test)]
pub mod trash_tests;
#[cfg(test)]
pub mod volume_tests;
#[cfg(test)]
pub mod workspace_tests;
#[cfg(test)]
pub mod writing_stats_tests;

use std::path::Path;

pub fn init_workspace(path: String) -> std::result::Result<bool, WriterError> {
    let p = Path::new(&path);
    crate::workspace::create_workspace(p).map_err(WriterError::from)?;

    Ok(true)
}

pub fn open_workspace(
    path: String,
) -> std::result::Result<std::sync::Arc<WriterAppService>, WriterError> {
    let p = Path::new(&path);
    if !crate::workspace::validate_workspace(p).map_err(WriterError::from)? {
        return Err(WriterError::InvalidWorkspace);
    }
    let service = std::sync::Arc::new(WriterAppService::new(path));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_workspace: {e}");
    }
    Ok(service)
}

pub fn open_workspace_with_platform_services(
    path: String,
    services: writer_platform_api::PlatformServices,
) -> std::result::Result<std::sync::Arc<WriterAppService>, WriterError> {
    let p = Path::new(&path);
    if !crate::workspace::validate_workspace(p).map_err(WriterError::from)? {
        return Err(WriterError::InvalidWorkspace);
    }
    let service = std::sync::Arc::new(WriterAppService::with_platform_services(path, services));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_workspace_with_platform_services: {e}");
    }
    Ok(service)
}

pub fn open_workspace_with_init(
    path: String,
    init: crate::api::types::PlatformInitDto,
) -> std::result::Result<std::sync::Arc<WriterAppService>, WriterError> {
    let p = Path::new(&path);
    if !crate::workspace::validate_workspace(p).map_err(WriterError::from)? {
        return Err(WriterError::InvalidWorkspace);
    }

    let platform_init: writer_platform_api::PlatformInit = init.clone().into();
    let network_state: writer_platform_api::NetworkState = init.into();

    let services = if let Some(resolver) = writer_platform_api::get_platform_services_resolver() {
        resolver.resolve(&platform_init, &network_state)
    } else {
        let config_dir = platform_init.app_data_dir.join("config");
        let config_store: Option<Box<dyn writer_platform_api::ConfigStore>> = Some(Box::new(
            writer_platform_api::FileConfigStore::new(config_dir),
        ));

        writer_platform_api::PlatformServices {
            init: platform_init,
            config_store,
            secure_storage: None,
            network_state: Some(network_state),
            sync_transport_factory: None,
        }
    };

    let service = std::sync::Arc::new(WriterAppService::with_platform_services(path, services));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_workspace_with_init: {e}");
    }
    Ok(service)
}

pub fn repair_workspace(path: String) -> std::result::Result<bool, WriterError> {
    let p = Path::new(&path);
    crate::workspace::create_workspace(p).map_err(WriterError::from)?;
    Ok(true)
}

pub fn create_project_in_workspace(
    workspace: String,
    title: String,
) -> std::result::Result<ProjectDto, WriterError> {
    let p = Path::new(&workspace);
    let project = crate::project::create_project(p, &title).map_err(WriterError::from)?;
    Ok(project.into())
}

pub fn load_workspace_summary(
    path: String,
) -> std::result::Result<WorkspaceSummaryDto, WriterError> {
    let p = Path::new(&path);
    let is_valid = crate::workspace::validate_workspace(p).unwrap_or(false);

    let projects = if is_valid {
        crate::project::list_projects(p)
            .map(|v| v.into_iter().map(Into::into).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let recent_edits = if is_valid {
        crate::workspace::get_recent_edits(p)
            .map(|v| v.into_iter().map(Into::into).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(WorkspaceSummaryDto {
        path,
        is_valid,
        projects,
        recent_edits,
    })
}

uniffi::include_scaffolding!("api");
pub mod app_service;
pub use app_service::WriterAppService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureStorageError {
    KeystoreKeyInvalidated,
    KeystoreError,
    StorageError,
    MigrationError { reason: String },
}

impl std::fmt::Display for SecureStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecureStorageError::KeystoreKeyInvalidated => write!(f, "Keystore key invalidated"),
            SecureStorageError::KeystoreError => write!(f, "Keystore error"),
            SecureStorageError::StorageError => write!(f, "Storage error"),
            SecureStorageError::MigrationError { reason } => write!(f, "Migration error: {reason}"),
        }
    }
}

impl std::error::Error for SecureStorageError {}

#[uniffi::export(callback_interface)]
pub trait SecureStorageProvider: Send + Sync {
    fn get_secret(&self, key: String) -> std::result::Result<Option<Vec<u8>>, SecureStorageError>;
    fn set_secret(
        &self,
        key: String,
        value: Vec<u8>,
    ) -> std::result::Result<(), SecureStorageError>;
    fn delete_secret(&self, key: String) -> std::result::Result<(), SecureStorageError>;
}

struct CallbackSecureStorage(Box<dyn SecureStorageProvider>);

impl writer_platform_api::SecureStorage for CallbackSecureStorage {
    fn get_secret(&self, key: &str) -> std::result::Result<Option<Vec<u8>>, String> {
        self.0
            .get_secret(key.to_string())
            .map_err(|e| e.to_string())
    }

    fn set_secret(&self, key: &str, value: &[u8]) -> std::result::Result<(), String> {
        self.0
            .set_secret(key.to_string(), value.to_vec())
            .map_err(|e| e.to_string())
    }

    fn delete_secret(&self, key: &str) -> std::result::Result<(), String> {
        self.0
            .delete_secret(key.to_string())
            .map_err(|e| e.to_string())
    }
}

#[::uniffi::export]
pub fn open_workspace_with_secure_storage(
    path: String,
    init: crate::api::types::PlatformInitDto,
    secure_storage: Option<Box<dyn SecureStorageProvider>>,
) -> std::result::Result<std::sync::Arc<WriterAppService>, WriterError> {
    let p = Path::new(&path);
    if !crate::workspace::validate_workspace(p).map_err(WriterError::from)? {
        return Err(WriterError::InvalidWorkspace);
    }

    let platform_init: writer_platform_api::PlatformInit = init.clone().into();
    let network_state: writer_platform_api::NetworkState = init.into();

    let secure_storage_impl: Option<Box<dyn writer_platform_api::SecureStorage>> = secure_storage
        .map(|p| Box::new(CallbackSecureStorage(p)) as Box<dyn writer_platform_api::SecureStorage>);

    let config_dir = platform_init.app_data_dir.join("config");
    let config_store: Option<Box<dyn writer_platform_api::ConfigStore>> = Some(Box::new(
        writer_platform_api::FileConfigStore::new(config_dir),
    ));

    let services = if let Some(resolver) = writer_platform_api::get_platform_services_resolver() {
        let mut resolved = resolver.resolve(&platform_init, &network_state);
        if secure_storage_impl.is_some() {
            resolved.secure_storage = secure_storage_impl;
        }
        resolved
    } else {
        writer_platform_api::PlatformServices {
            init: platform_init,
            config_store,
            secure_storage: secure_storage_impl,
            network_state: Some(network_state),
            sync_transport_factory: None,
        }
    };

    let service = std::sync::Arc::new(WriterAppService::with_platform_services(path, services));
    if let Err(e) = service.rebuild_search_index(None) {
        log::warn!("Failed to rebuild search index on open_workspace_with_secure_storage: {e}");
    }
    Ok(service)
}
