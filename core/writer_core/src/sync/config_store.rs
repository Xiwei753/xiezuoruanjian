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

        // But we DO want to sync app-meta/sync/trash!
        if rel_path.starts_with("app-meta/sync/trash/") {
            return false;
        }

        // Keep 'trash' pattern if it's anywhere else?
        // Let's just avoid a blanket "trash" ignore. We used to ignore "trash".
        // Instead of it from ignored_patterns, it won't.
        for pattern in ignored_patterns {
            if rel_path.contains(pattern) {
                return true;
            }
        }

        false
    }
}

impl crate::sync::SyncService {
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
    pub fn load_sync_state(workspace_path: &Path) -> crate::Result<SyncState> {
        let state_path = workspace_path.join("app-meta/sync/state.local.json");
        if !state_path.exists() {
            // Try to migrate if the old sync_state.json exists
            let old_path = workspace_path.join("app-meta/sync/sync_state.json");
            if old_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&old_path) {
                    if let Ok(mut state) = serde_json::from_str::<SyncState>(&content) {
                        if state.device_id.is_empty() {
                            state.device_id = uuid::Uuid::new_v4().to_string();
                        }
                        let _ = Self::save_sync_state(workspace_path, &state);
                        let _ = std::fs::remove_file(old_path);
                        return Ok(state);
                    }
                }
            }

            let default_state = SyncState { device_id: uuid::Uuid::new_v4().to_string(), ..Default::default() };
            return Ok(default_state);
        }

        let content = std::fs::read_to_string(state_path)?;
        let mut state: SyncState = serde_json::from_str(&content).unwrap_or_default();
        if state.device_id.is_empty() {
            state.device_id = uuid::Uuid::new_v4().to_string();
            let _ = Self::save_sync_state(workspace_path, &state);
        }
        Ok(state)
    }
}

impl crate::sync::SyncService {
    pub fn save_sync_state(workspace_path: &Path, state: &SyncState) -> crate::Result<()> {
        let state_path = workspace_path.join("app-meta/sync/state.local.json");
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
