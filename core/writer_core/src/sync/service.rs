//! 同步服务层 — 扫描与计划入口。
//!
//! 生产同步入口是 `WriterCoreApi::perform_full_sync`（对象版本比较 → 增量传输
//! → 一次提交 → 一次发布 → 精确确认）。本模块只保留：
//! - 扫描与 dry-run 计划（与正式入口使用同一只读对象比较口径）
//! - 路径过滤（`is_blacklisted_path`/`is_whitelisted_path`）：见 `config_store`
//!
//! ## 线程安全
//!
//! `SyncService` 的方法均为关联函数（无 `&self`），所有状态通过参数传递。
//! 调用方（`WriterAppService`）通过 `Mutex` 保证线程安全。

use crate::sync::scanner;
use crate::sync::types::SyncConfig;
use crate::sync::types::SyncPlan;
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
    /// `scope` 由调用方通过 `SyncTarget` 提供，`SyncConfig` 不再携带 scope。
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

    /// Build a dry-run plan limited to one App data group when the target is grouped.
    pub fn build_sync_plan_for_target(
        sync_root: &Path,
        target: &crate::sync::types::SyncTarget,
    ) -> crate::Result<SyncPlan> {
        scanner::build_sync_plan_for_target(sync_root, target)
    }

}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}
