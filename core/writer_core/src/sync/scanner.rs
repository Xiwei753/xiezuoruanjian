//! # 同步扫描与计划构建
//!
//! 扫描作品目录文件系统，构建上传/删除计划。与 `SyncService` 配合使用。
//!
//! ## 扫描逻辑
//!
//! - 遍历作品目录所有文件（跳过 `.git/`）
//! - 白名单路径标记为 `Upload`，其余为 `Ignore`
//! - 黑名单路径直接忽略（不进入上传计划）
//!
//! ## 计划构建
//!
//! - **首次同步**（`known_files` 为空）：所有白名单文件上传
//! - **增量同步**：仅上传 hash 变化的文件 + 新增文件
//! - **远端删除**：本地已不存在但远端仍有的文件
//! - **墓碑清理**：超过 `purge_after` 时间的本地 trash 文件

use crate::sync::types::{SyncFileEntry, SyncKind, SyncPlan};
use crate::sync::SyncService;
use std::path::Path;

#[allow(clippy::cast_possible_wrap)]
/// 扫描作品目录所有文件，生成 `SyncFileEntry` 列表。
///
/// `.git/` 目录被排除。`modified_time` 使用 Unix epoch 秒；
/// 文件 hash 为空字符串表示计算失败（扫描不因单个文件失败而中断）。
pub(crate) fn scan_for_sync(sync_root: &Path) -> crate::Result<Vec<SyncFileEntry>> {
    let mut entries = Vec::new();

    for entry in walkdir::WalkDir::new(sync_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let absolute_path = entry.path().to_path_buf();

        let rel_path = match absolute_path.strip_prefix(sync_root) {
            Ok(p) => p.to_string_lossy().replace("\\", "/"),
            Err(_) => continue,
        };

        if rel_path.starts_with(".git/") || rel_path == ".git" {
            continue;
        }

        let modified_time = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::SystemTime::now())
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let modified_time = modified_time as i64;

        let file_hash = SyncService::compute_file_hash(&absolute_path).unwrap_or_default();

        let sync_kind = if SyncService::is_whitelisted_path(&rel_path) {
            SyncKind::Upload
        } else {
            SyncKind::Ignore
        };

        entries.push(SyncFileEntry {
            relative_path: rel_path,
            absolute_path: absolute_path.to_string_lossy().into_owned(),
            file_hash,
            modified_time,
            sync_kind,
        });
    }

    Ok(entries)
}

/// 基于扫描结果和同步状态构建 `SyncPlan`。
///
/// - 首次同步：所有白名单文件上传
/// - 增量同步：hash 变化或新增的文件上传；本地已删除的远端文件标记删除
/// - 墓碑清理：`purge_after <= now` 的 trash 文件标记本地删除
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn build_sync_plan(sync_root: &Path) -> crate::Result<SyncPlan> {
    let mut plan = SyncPlan::new();

    let entries = scan_for_sync(sync_root)?;
    let state = SyncService::load_sync_state(sync_root).unwrap_or_default();
    let is_first_sync = state.known_files.is_empty();

    let mut local_files = std::collections::HashSet::new();

    for entry in entries {
        if SyncService::is_blacklisted_path(&entry.relative_path) {
            plan.ignored_files.push(entry.relative_path.clone());
            continue;
        }

        if entry.sync_kind == SyncKind::Upload || entry.sync_kind == SyncKind::ConflictCandidate {
            local_files.insert(entry.relative_path.clone());
            let known_hash_opt = state.known_files.get(&entry.relative_path);
            if is_first_sync {
                plan.files_to_upload.push(entry.relative_path.clone());
            } else if let Some(kh) = known_hash_opt {
                if *kh != entry.file_hash {
                    plan.files_to_upload.push(entry.relative_path.clone());
                }
            } else {
                plan.files_to_upload.push(entry.relative_path.clone());
            }
        } else {
            plan.ignored_files.push(entry.relative_path.clone());
        }
    }

    if !is_first_sync {
        for known_path in state.known_files.keys() {
            if !local_files.contains(known_path) {
                plan.files_to_delete_remote.push(known_path.clone());
            }
        }
    }

    let now = chrono::Utc::now().timestamp();
    for t in &state.tombstones {
        if t.purge_after <= now {
            plan.files_to_delete_local.push(t.trash_path.clone());
        }
    }

    Ok(plan)
}
