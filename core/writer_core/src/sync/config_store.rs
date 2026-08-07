//! 同步路径过滤与状态持久化。
//!
//! ## 黑白名单契约
//!
//! `is_blacklisted_path` 和 `is_whitelisted_path` 构成同步路径过滤的两层模型：
//! 1. 黑名单排除不应同步的本地状态文件（设备专属配置、缓存、日志、临时文件等）
//! 2. 白名单包含应参与同步的工作区内容（项目、章节、设置、星图等）
//!
//! 黑名单优先：白名单检查前先排除黑名单路径，保证本地状态文件不会意外同步。
//!
//! ## 路径格式约定
//!
//! 所有路径为相对于工作区根目录的正斜杠分隔路径（与 Git/远程一致）。
//! Windows 反斜杠在 `normalize_rel_path` 中统一转换为正斜杠。
//!
//! ## 状态持久化
//!
//! 同步状态存储在 `app-meta/sync/state.local.json`，使用 atomic write（写临时文件后 rename）
//! 防止写入中断导致文件损坏。旧格式 `sync_state.json` 在首次加载时自动迁移并删除。

use crate::sync::types::SyncState;
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
    /// - `settings.local.json`：本地设置覆盖（设备专属）
    /// - `sync_secrets.local.json`：同步凭证（安全敏感，不落盘到远程）
    /// - `state.local.json` / `sync_state.json`：本地同步状态（设备专属）
    /// - `app-meta/ai/`：AI 缓存（设备专属，体积大）
    /// - `app-meta/stats/events.local/` / `cache/`：统计缓存（可重建）
    /// - `.git/`：Git 内部目录
    /// - `.tmp` / `.lock` 后缀：临时和锁文件
    /// - `app-meta/logs`：日志（设备专属）
    ///
    /// 特例：`app-meta/sync/trash/` 虽在 `app-meta/sync/` 下但**不**被黑名单排除，
    /// 因为回收站内容需要跨设备同步（在一端删除需在另一端也删除）。
    pub fn is_blacklisted_path(rel_path: &str) -> bool {
        let rel_path = normalize_rel_path(rel_path);
        let ignored_patterns = [
            "app-meta/settings/settings.local.json",
            "app-meta/sync/sync_secrets.local.json",
            "app-meta/sync/state.local.json",
            "app-meta/sync/sync_state.json",
            "app-meta/ai/",
            "app-meta/stats/events.local/",
            "app-meta/stats/cache/",
            "sqlite_cache",
            "tmp",
            "cache",
            "backups",
        ];

        if rel_path.ends_with(".tmp") || rel_path.ends_with(".lock") {
            return true;
        }

        if rel_path.starts_with("app-meta/logs") || rel_path.contains("/logs/") {
            return true;
        }

        if rel_path == ".git" || rel_path.starts_with(".git/") {
            return true;
        }

        // trash 目录需要跨设备同步（一端删除需在另一端生效），
        // 因此在黑名单检查中显式放行，覆盖后续 app-meta/sync/ 的黑名单规则。
        if rel_path.starts_with("app-meta/sync/trash/") {
            return false;
        }

        // trash 不在 ignored_patterns 中，因为回收站需要跨设备同步。
        // 旧版本曾 blanket ignore "trash"，现已移除该规则。
        for pattern in ignored_patterns {
            if rel_path.contains(pattern) {
                return true;
            }
        }

        false
    }
}

impl crate::sync::SyncService {
    /// 判断路径是否在同步白名单中。
    ///
    /// 白名单定义工作区中应参与同步的内容路径。黑名单优先——先排除黑名单再检查白名单。
    ///
    /// 同步语义按路径类型不同：
    /// - `workspace_manifest.json`：工作区元数据，全量覆盖
    /// - `settings.sync.json`：跨设备设置，语义合并（非三路比较）
    /// - `chapter.md`：章节正文，LWW 或三路比较
    /// - `characters/` / `outline/` / `graphs/`：结构化内容，全量同步
    /// - `app-meta/sync/trash/`：回收站，跨设备同步删除操作
    /// - `app-meta/sync/tombstones.json`：墓碑记录，防止已删除文件复活
    pub fn is_whitelisted_path(rel_path: &str) -> bool {
        let rel_path = normalize_rel_path(rel_path);
        if Self::is_blacklisted_path(&rel_path) {
            return false;
        }

        if rel_path == "workspace_manifest.json" {
            return true;
        }
        if rel_path == "app-meta/settings/settings.sync.json" {
            return true;
        }
        if rel_path == "app-meta/sync/manifest.sync.json" {
            return true;
        }

        if rel_path.starts_with("projects/") {
            if rel_path.ends_with("/project.json") {
                return true;
            }
            if rel_path.contains("/volumes/") && rel_path.ends_with("/volume.json") {
                return true;
            }
            if rel_path.contains("/chapters/") && rel_path.ends_with("/chapter.md") {
                return true;
            }
            if rel_path.contains("/chapters/") && rel_path.ends_with("/chapter.meta.json") {
                return true;
            }
            if rel_path.contains("/characters/") {
                return true;
            }
            if rel_path.contains("/outline/") {
                return true;
            }
            if rel_path.contains("/graphs/") {
                return true;
            }
            return false;
        }

        if rel_path.starts_with("app-meta/graphs/") {
            return true;
        }

        if rel_path.starts_with("app-meta/starmaps/") {
            return true;
        }

        if rel_path.starts_with("app-meta/ai/") {
            return true;
        }

        if rel_path.starts_with("app-meta/proofreading/") {
            return true;
        }

        if rel_path.starts_with("app-meta/stats/daily/") {
            return true;
        }

        // 最近编辑记录
        if rel_path.starts_with("app-meta/recent/") {
            return true;
        }

        // 设备信息
        if rel_path.starts_with("app-meta/device/") {
            return true;
        }

        if rel_path.starts_with("app-meta/sync/trash/") {
            return true;
        }

        if rel_path == "app-meta/sync/tombstones.json" {
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
    pub fn load_sync_state(workspace_path: &Path) -> crate::Result<SyncState> {
        Self::load_sync_state_with_preferred_device_id(workspace_path, None)
    }

    /// 加载同步状态，优先使用平台注入的 device_id。
    ///
    /// 当 `preferred_device_id` 为 `Some` 且 state 中 device_id 为空时，
    /// 使用平台注入值而非随机生成。这保证同一设备在不同工作区使用相同的 device_id。
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_sync_state_with_preferred_device_id(
        workspace_path: &Path,
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

        let state_path = workspace_path.join("sync/state.local.json");
        if !state_path.exists() {
            let old_path = workspace_path.join("sync/sync_state.json");
            if old_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&old_path) {
                    if let Ok(mut state) = serde_json::from_str::<SyncState>(&content) {
                        state.device_id = resolve_device_id(&state.device_id);
                        let _ = Self::save_sync_state(workspace_path, &state);
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
            let _ = Self::save_sync_state(workspace_path, &state);
        }
        Ok(state)
    }
}

impl crate::sync::SyncService {
    /// 保存同步状态，使用 atomic write 防止写入中断导致文件损坏。
    ///
    /// 写入流程：序列化 → 写入 `.tmp` 临时文件 → rename 为最终文件。
    /// rename 在同一文件系统上是原子操作，保证读端不会看到部分写入的状态。
    pub fn save_sync_state(workspace_path: &Path, state: &SyncState) -> crate::Result<()> {
        let state_path = workspace_path.join("sync/state.local.json");
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
    pub fn get_sync_ignored_paths(workspace_path: &Path) -> crate::Result<Vec<String>> {
        let plan = Self::build_sync_plan_from_workspace(workspace_path)?;
        Ok(plan.ignored_files)
    }
}
