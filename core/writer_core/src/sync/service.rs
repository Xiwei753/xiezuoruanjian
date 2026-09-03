//! 同步服务层 — 编排 LWW 同步策略的统一入口。
//!
//! `SyncService` 是同步功能的业务编排层，提供：
//! - LWW 同步（`perform_lww_sync`）：基于 GitHub Contents API 的文件级 Last Writer Wins
//! - 诊断（`perform_sync_diagnostics`）：探测网络、认证、仓库和分支可用性
//! - 路径过滤（`is_blacklisted_path`/`is_whitelisted_path`）：见 `config_store` 模块
//!
//! ## 线程安全
//!
//! `SyncService` 的方法均为关联函数（无 `&self`），所有状态通过参数传递。
//! 调用方（`WriterAppService`）通过 `Mutex` 保证线程安全。

use crate::sync::lww;
use crate::sync::scanner;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncPlan;
use crate::sync::types::SyncResult;
use crate::sync::types::SyncScope;
use crate::sync::types::SyncStatus;
use std::path::Path;

/// 同步服务。
///
/// 封装同步的完整生命周期：配置加载 → dry-run → 执行。
/// `config` 为 `None` 表示未配置同步；`status` 跟踪最近一次同步结果。
pub struct SyncService {
    pub config: Option<SyncConfig>,
    pub status: SyncStatus,
}

impl SyncService {
    /// 干运行——构建同步计划但不执行文件传输。config.enabled=false 时返回空计划。
    /// `scope` 由调用方通过 `SyncTarget` 提供，`SyncConfig` 不再携带 scope（Issue #630）。
    pub fn perform_sync_dry_run(
        sync_root: &Path,
        config: &SyncConfig,
        scope: SyncScope,
    ) -> crate::Result<SyncPlan> {
        if !config.enabled {
            return Ok(SyncPlan::new());
        }
        scanner::build_sync_plan(sync_root, scope)
    }
}

impl SyncService {
    /// LWW 同步主入口——基于 Last-Writer-Wins 策略执行文件级同步。
    ///
    /// 通过 `SyncProvider` trait 与具体后端解耦：调用方传入已构造的 provider,
    /// engine 不直接依赖 `SyncConfig`/`SyncSecrets`/`SyncTransport`。
    /// `sync_policy` 携带 engine 决策所需的通用字段（enabled/interval 等）。
    /// `force_sync=true` 跳过 debounce。
    pub fn perform_lww_sync(
        sync_root: &Path,
        provider: &dyn crate::sync::provider::SyncProvider,
        sync_policy: &crate::sync::types::SyncPolicy,
        target: &crate::sync::types::SyncTarget,
        force_sync: bool,
    ) -> crate::Result<SyncResult> {
        lww::perform_lww_sync(sync_root, provider, sync_policy, target, force_sync)
    }
}

impl SyncService {
    pub fn new() -> Self {
        Self {
            config: None,
            status: SyncStatus::Idle,
        }
    }

    /// 扫描作品目录中所有可同步文件。
    pub fn scan_for_sync(
        sync_root: &Path,
        scope: crate::sync::types::SyncScope,
    ) -> crate::Result<Vec<crate::sync::types::SyncFileEntry>> {
        scanner::scan_for_sync(sync_root, scope)
    }

    /// 从作品目录构建同步计划（上传/下载/删除/冲突文件列表）。
    pub fn build_sync_plan(
        sync_root: &Path,
        scope: crate::sync::types::SyncScope,
    ) -> crate::Result<SyncPlan> {
        scanner::build_sync_plan(sync_root, scope)
    }

    /// 占位同步方法——当前返回 NotImplemented，实际同步通过 perform_lww_sync 执行。
    pub fn sync(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}
