//! 同步路径过滤与状态持久化。
//!
//! ## 黑白名单契约
//!
//! `is_blacklisted_path` 和 `is_whitelisted_path` 构成同步路径过滤的两层模型：
//! 1. 黑名单排除不应同步的本地状态文件（设备专属配置、缓存、日志、临时文件等）
//! 2. 白名单包含应参与同步的作品内容（project.json、volumes、characters 等）
//!
//! 黑名单优先：白名单检查前先排除黑名单路径，保证本地状态文件不会意外同步。
//!
//! ## 路径格式约定
//!
//! 所有路径为相对于作品目录（同步根）的正斜杠分隔路径（与 Git/远程一致）。
//! Windows 反斜杠在 `normalize_rel_path` 中统一转换为正斜杠。
//!
//! ## 状态持久化
//!
//! 同步状态存储在 `app-meta/sync/state.local.json`，使用 atomic write（写临时文件后 rename）
//! 防止写入中断导致文件损坏。旧格式 `sync_state.json` 在首次加载时自动迁移并删除。

use crate::sync::types::{SyncScope, SyncState};
use std::path::Path;

/// Normalize a relative path to use forward slashes.
///
/// On Windows, local paths may contain backslashes. All sync path matching
/// logic expects forward-slash-separated paths (matching the remote/Git convention).
/// This function ensures consistent comparison regardless of platform.
fn normalize_rel_path(rel_path: &str) -> std::borrow::Cow<'_, str> {
    if rel_path.contains('\\') {
        std::borrow::Cow::Owned(rel_path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(rel_path)
    }
}

impl crate::sync::SyncService {
    /// 判断路径是否在同步黑名单中。
    ///
    /// 黑名单排除设备专属或临时性文件，这些文件不应参与跨设备同步：
    /// - `state.local.json` / `sync_state.json`：本地同步状态（设备专属）
    /// - `.git/`：Git 内部目录
    /// - `.tmp` / `.lock` 后缀：临时和锁文件
    /// - `app-meta/logs`：日志（设备专属）
    /// - `sqlite_cache` / `tmp` / `cache` / `backups`：缓存与备份（可重建）
    ///
    /// 同步根是单个作品目录（Issue #600）：设置、凭证、统计、最近编辑、
    /// 设备信息、星图、日志等应用级数据位于 `app_data_root`，不在任何作品
    /// 仓库内，因此不需要（也不可能）出现在黑名单中。
    pub fn is_blacklisted_path(rel_path: &str, scope: SyncScope) -> bool {
        let rel_path = normalize_rel_path(rel_path);

        // 通用排除：.git / .tmp / .lock
        if rel_path == ".git" || rel_path.starts_with(".git/") {
            return true;
        }
        if rel_path.ends_with(".tmp") || rel_path.ends_with(".lock") {
            return true;
        }

        match scope {
            SyncScope::Project => Self::is_project_blacklisted_path(&rel_path),
            SyncScope::App => Self::is_app_blacklisted_path(&rel_path),
        }
    }

    /// 作品级黑名单：排除作品仓库内的本地状态/临时文件。
    fn is_project_blacklisted_path(rel_path: &str) -> bool {
        let ignored_patterns = [
            "app-meta/sync/state.local.json",
            "app-meta/sync/sync_state.json",
            "sqlite_cache",
            "tmp",
            "cache",
            "backups",
        ];

        if rel_path.starts_with("app-meta/logs") || rel_path.contains("/logs/") {
            return true;
        }

        for pattern in ignored_patterns {
            if rel_path.contains(pattern) {
                return true;
            }
        }

        false
    }

    /// 应用级黑名单：排除不应跨设备同步的本地状态/作品/临时数据。
    /// 作品目录（`作品/`、`projects/`）整体忽略——每个作品是独立 Git 仓库。
    fn is_app_blacklisted_path(rel_path: &str) -> bool {
        // 作品目录整体忽略（每个作品是独立 Git 仓库）
        if rel_path == "作品" || rel_path.starts_with("作品/") {
            return true;
        }
        if rel_path == "projects" || rel_path.starts_with("projects/") {
            return true;
        }

        // 日志/导出/备份
        if rel_path.starts_with("日志/") || rel_path.contains("/日志/") {
            return true;
        }
        if rel_path.starts_with("log/") || rel_path.contains("/log/") {
            return true;
        }
        if rel_path.starts_with("导出/") || rel_path.contains("/导出/") {
            return true;
        }
        if rel_path.starts_with("exports/") || rel_path.contains("/exports/") {
            return true;
        }
        if rel_path.starts_with("备份/") || rel_path.contains("/备份/") {
            return true;
        }
        if rel_path.starts_with("backups/") || rel_path.contains("/backups/") {
            return true;
        }

        // 本地设置/最近编辑（设备专属）
        if rel_path == "settings.local.json" || rel_path.ends_with("/settings.local.json") {
            return true;
        }
        if rel_path == "recent_edits.json" || rel_path.ends_with("/recent_edits.json") {
            return true;
        }

        // 含 secret 的路径
        if rel_path.contains("secret") {
            return true;
        }

        // 设备信息
        if rel_path.starts_with("device/") || rel_path.contains("/device/") {
            return true;
        }

        // 统计本地状态
        if rel_path.starts_with("app-meta/stats/") || rel_path.contains("/app-meta/stats/") {
            return true;
        }
        if rel_path.starts_with("app-meta/transactions/")
            || rel_path.contains("/app-meta/transactions/")
        {
            return true;
        }

        // 应用级同步自身的本地状态
        if rel_path == "app-meta/sync/state.local.json"
            || rel_path == "app-meta/sync/sync_state.json"
            || rel_path.starts_with("app-meta/sync/secrets")
        {
            return true;
        }

        // 日志目录
        if rel_path.starts_with("app-meta/logs") || rel_path.contains("/logs/") {
            return true;
        }

        // 缓存/临时
        if rel_path.contains("sqlite_cache")
            || rel_path.contains("cache")
            || rel_path == "tmp"
            || rel_path.contains("/tmp")
        {
            return true;
        }

        false
    }
}

impl crate::sync::SyncService {
    /// 判断路径是否在同步白名单中。
    ///
    /// 同步根是单个作品目录（Issue #600），`rel_path` 相对 `project_root`。
    /// 黑名单优先——先排除黑名单再检查白名单。
    ///
    /// 同步语义按路径类型不同：
    /// - `chapter.md`：章节正文，LWW 或三路比较
    /// - `project.json` / `volume.json` / `chapter.meta.json`：结构化元数据
    /// - `characters/`：角色数据
    /// - `app-meta/sync/manifest.sync.json`：同步清单
    ///
    /// 应用级数据（设置、同步配置与凭证、统计、最近编辑、设备信息、星图、
    /// 日志、回收站）位于 `app_data_root`，不在任何作品仓库内，不参与作品同步。
    pub fn is_whitelisted_path(rel_path: &str, scope: SyncScope) -> bool {
        let rel_path = normalize_rel_path(rel_path);
        if Self::is_blacklisted_path(&rel_path, scope) {
            return false;
        }

        match scope {
            SyncScope::Project => Self::is_project_whitelisted_path(&rel_path),
            SyncScope::App => Self::is_app_whitelisted_path(&rel_path),
        }
    }

    /// 作品级白名单：作品正文/元数据/角色/同步清单。
    fn is_project_whitelisted_path(rel_path: &str) -> bool {
        // 作品自身内容
        if rel_path == "project.json" {
            return true;
        }
        if rel_path.starts_with("volumes/") {
            if rel_path.ends_with("/volume.json")
                || rel_path.ends_with("/chapter.md")
                || rel_path.ends_with("/chapter.meta.json")
            {
                return true;
            }
            return false;
        }
        if rel_path.starts_with("characters/") {
            return true;
        }

        // 作品自己的同步元数据
        if rel_path == "app-meta/sync/manifest.sync.json" {
            return true;
        }

        false
    }

    /// 应用级白名单：设置/全局星图/主题调色板/应用级同步清单。
    fn is_app_whitelisted_path(rel_path: &str) -> bool {
        // 可同步设置
        if rel_path == "settings.sync.json" {
            return true;
        }

        // 全局星图
        if rel_path.starts_with("starmaps/") {
            return true;
        }

        // 主题调色板
        if rel_path.starts_with("themes/palettes/") {
            return true;
        }

        // 应用级同步清单
        if rel_path == "app-meta/sync/manifest.sync.json" {
            return true;
        }

        false
    }
}

impl crate::sync::SyncService {
    /// 加载同步状态，支持从旧格式自动迁移。
    ///
    /// 查找顺序：`state.local.json` → 旧格式 `sync_state.json`（自动迁移后删除）→ 创建默认状态。
    /// 迁移时若 `device_id` 为空则生成新 UUID，保证每台设备有唯一标识。
    /// `device_id` 用于 LWW 同步中区分不同设备的写入。
    pub fn load_sync_state(sync_root: &Path) -> crate::Result<SyncState> {
        Self::load_sync_state_with_preferred_device_id(sync_root, None)
    }

    /// 加载同步状态，优先使用平台注入的 device_id。
    ///
    /// 当 `preferred_device_id` 为 `Some` 且 state 中 device_id 为空时，
    /// 使用平台注入值而非随机生成。这保证同一设备在不同数据根下使用相同的 device_id。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_sync_state_with_preferred_device_id(
        sync_root: &Path,
        preferred_device_id: Option<&str>,
    ) -> crate::Result<SyncState> {
        let resolve_device_id = |existing: &str| -> String {
            if !existing.is_empty() {
                return existing.to_string();
            }
            if let Some(id) = preferred_device_id {
                if !id.is_empty() {
                    return id.to_string();
                }
            }
            uuid::Uuid::new_v4().to_string()
        };

        let state_path = sync_root.join("app-meta/sync/state.local.json");
        if !state_path.exists() {
            let old_path = sync_root.join("app-meta/sync/sync_state.json");
            if old_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&old_path) {
                    if let Ok(mut state) = serde_json::from_str::<SyncState>(&content) {
                        state.device_id = resolve_device_id(&state.device_id);
                        let _ = Self::save_sync_state(sync_root, &state);
                        let _ = std::fs::remove_file(old_path);
                        return Ok(state);
                    }
                }
            }

            let default_state = SyncState {
                device_id: resolve_device_id(""),
                ..Default::default()
            };
            return Ok(default_state);
        }

        let content = std::fs::read_to_string(state_path)?;
        let mut state: SyncState = serde_json::from_str(&content).unwrap_or_default();
        let new_device_id = resolve_device_id(&state.device_id);
        if new_device_id != state.device_id {
            state.device_id = new_device_id;
            let _ = Self::save_sync_state(sync_root, &state);
        }
        Ok(state)
    }
}

impl crate::sync::SyncService {
    /// 保存同步状态，使用 atomic write 防止写入中断导致文件损坏。
    ///
    /// 写入流程：序列化 → 写入 `.tmp` 临时文件 → rename 为最终文件。
    /// rename 在同一文件系统上是原子操作，保证读端不会看到部分写入的状态。
    pub fn save_sync_state(sync_root: &Path, state: &SyncState) -> crate::Result<()> {
        let state_path = sync_root.join("app-meta/sync/state.local.json");
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(state)
            .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;

        let tmp_path = state_path.with_extension("tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(tmp_path, state_path)?;

        Ok(())
    }
}

impl crate::sync::SyncService {
    pub fn get_sync_ignored_paths(
        sync_root: &Path,
        scope: crate::sync::types::SyncScope,
    ) -> crate::Result<Vec<String>> {
        let plan = Self::build_sync_plan(sync_root, scope)?;
        Ok(plan.ignored_files)
    }
}
