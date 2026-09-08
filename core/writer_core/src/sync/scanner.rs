//! # 同步扫描与计划构建
//!
//! 扫描作品目录文件系统，构建上传/删除计划。与 `SyncService` 配合使用。
//!
//! ## 扫描逻辑
//!
//! - 遍历作品目录所有文件（只显式跳过 `.git/` 和 `.git`）
//! - 白名单路径标记为 `Upload`，其余为 `Ignore`
//! - 黑名单路径直接忽略（不进入上传计划）
//!
//! #645 评论 5504296097 问题3：严格扫描 — WalkDir / metadata / mtime / hash
//! 错误不再被 `filter_map(Result::ok)` / `unwrap_or(now)` / `unwrap_or_default()`
//! 静默吞掉。白名单（需要同步）文件任一 I/O 失败都 `Err` 向上传递；非白名单
//! （ignored）文件 metadata/hash 失败仍收集为 `Ignore`（下游只用其路径）。
//!
//! ## 计划构建
//!
//! - **首次同步**（`known_files` 为空）：所有白名单文件上传
//! - **增量同步**：仅上传 hash 变化的文件 + 新增文件
//! - **远端删除**：本地已不存在但远端仍有的文件
//! - **墓碑清理**：超过 `purge_after` 时间的本地 trash 文件

use crate::sync::types::{SyncFileEntry, SyncKind, SyncPlan, SyncScope};
use crate::sync::SyncService;
use std::path::Path;

#[allow(clippy::cast_possible_wrap)]
/// 扫描作品目录所有文件，生成 `SyncFileEntry` 列表。
///
/// `.git/` 目录被显式跳过。`modified_time` 使用 Unix epoch 秒。
///
/// #645 评论 5504296097 问题3 修复：严格扫描，不再吞 I/O / mtime / hash 错误。
/// - WalkDir 迭代错误全部 `Err` 向上传递（不再 `filter_map(Result::ok)`）；
/// - 白名单（需要同步）文件的 metadata / modified / hash 任一失败都 `Err`
///   （下游 `lww::manifest` / `lww::attempt` 依赖真实 hash，空 hash 会让 manifest
///   记算错误）；
/// - 非 白名单（ignored）文件 metadata / hash 失败不 `Err`（仍收集为
///   `SyncKind::Ignore`，hash/mtime 用 fallback 值，因为下游只用 ignored 文件的
///   路径，不用其 hash/mtime）。
pub(crate) fn scan_for_sync(
    sync_root: &Path,
    scope: SyncScope,
) -> crate::Result<Vec<SyncFileEntry>> {
    let mut entries = Vec::new();

    for item in walkdir::WalkDir::new(sync_root).into_iter() {
        let entry = item.map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "scan_for_sync: walkdir error under {}: {e}",
                sync_root.display()
            )))
        })?;
        if !entry.file_type().is_file() {
            continue;
        }

        let absolute_path = entry.path().to_path_buf();

        let rel_path = match absolute_path.strip_prefix(sync_root) {
            Ok(p) => p.to_string_lossy().replace("\\", "/"),
            Err(_) => continue,
        };

        // 只显式跳过 .git/ 和 .git（其余路径错误不跳过）。
        if rel_path.starts_with(".git/") || rel_path == ".git" {
            continue;
        }

        let is_whitelisted = SyncService::is_whitelisted_path(&rel_path, scope);
        let sync_kind = if is_whitelisted {
            SyncKind::Upload
        } else {
            SyncKind::Ignore
        };

        if is_whitelisted {
            // 白名单文件：metadata / modified / hash 任一失败都 Err（不静默吞错误）。
            let metadata = entry.metadata().map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "scan_for_sync: metadata failed for {}: {e}",
                    absolute_path.display()
                )))
            })?;
            let modified = metadata.modified().map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "scan_for_sync: mtime failed for {}: {e}",
                    absolute_path.display()
                )))
            })?;
            let modified_time = modified
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "scan_for_sync: mtime before epoch for {}: {e}",
                        absolute_path.display()
                    )))
                })?
                .as_secs();
            let modified_time = modified_time as i64;
            let file_hash = SyncService::compute_file_hash(&absolute_path).map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "scan_for_sync: hash failed for {}: {e}",
                    absolute_path.display()
                )))
            })?;

            entries.push(SyncFileEntry {
                relative_path: rel_path,
                absolute_path: absolute_path.to_string_lossy().into_owned(),
                file_hash,
                modified_time,
                sync_kind,
            });
        } else {
            // 非 白名单（ignored）文件：metadata / hash 失败不 Err
            // （下游只用 ignored 文件的路径，不用其 hash/mtime）。
            // 仍尝试取 mtime/hash 以便诊断，失败则用 fallback 值。
            let modified_time = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let file_hash = SyncService::compute_file_hash(&absolute_path).unwrap_or_default();

            entries.push(SyncFileEntry {
                relative_path: rel_path,
                absolute_path: absolute_path.to_string_lossy().into_owned(),
                file_hash,
                modified_time,
                sync_kind,
            });
        }
    }

    Ok(entries)
}

/// 基于扫描结果和同步状态构建 `SyncPlan`。
///
/// - 首次同步：所有白名单文件上传
/// - 增量同步：hash 变化或新增的文件上传；本地已删除的远端文件标记删除
/// - 墓碑清理：`purge_after <= now` 的 trash 文件标记本地删除
///
/// #645 评论 5504296097 问题1：upload/delete 动作**直接**从
/// `snapshot_local_records_read_only` 的 records 推导，保持 `build_sync_plan`
/// 与 LWW `execute_lww_sync_attempt` 同一 source of truth（per-file 真实 winner
/// device_id + 真实删除时间）。
///
/// - `record.op == "upsert"` → 决定 upload（仍结合 `state.known_files` hash 判断
///   是否需要上传，但动作**来源**是 snapshot records）
/// - `record.op == "delete"` → 决定 remote delete
///
/// `scan_for_sync` 仍用于收集 `ignored_files`（黑名单/非白名单路径）；
/// `state.tombstones` 仍用于 `files_to_delete_local`（墓碑清理/purge）。
/// 但 state/scan 不再单独解释同步动作。
///
/// #645 评论 5504296097 问题1 修复：snapshot 失败（known file 消失且无 tombstone、
/// manifest 损坏等）不再 fallback 到 entries-only，直接返回 Err。fallback 会让
/// build_sync_plan 与 execute_lww_sync_attempt 使用两套 local record 规则，
/// 且会静默吞掉"无法可靠确认本地状态"的错误。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn build_sync_plan(sync_root: &Path, scope: SyncScope) -> crate::Result<SyncPlan> {
    let mut plan = SyncPlan::new();

    // scan_for_sync 仍用于收集 ignored_files（黑名单/非白名单路径）。
    let entries = scan_for_sync(sync_root, scope)?;
    // #645 评论 5504296097 问题5：build_sync_plan 是 plan/dry-run helper，
    // 用 read-only state loader，不写文件（旧 state 迁移/device_id 补写只在内存）。
    // #645 评论 5504296097 问题1 修复：state 加载失败 → 直接返回 Err，
    // 不再 unwrap_or_default() 把损坏的 state.local.json 当成"首次同步"。
    let state = SyncService::load_sync_state_read_only(sync_root, None)?;
    let is_first_sync = state.known_files.is_empty();

    // #645 评论 5504296097 问题1 修复：用 snapshot_local_records_read_only 获取只读 records，
    // 与 LWW execute attempt 同源。snapshot 失败直接返回 Err（不再 fallback）。
    // upload/delete 动作直接从 snapshot_records 推导，不再用 state.known_files /
    // scan / local_files 重新推一遍。
    let snapshot_records =
        crate::sync::lww::snapshot_local_records_read_only(sync_root, scope, "")?;

    // ignored_files：黑名单路径 + 非 Upload/ConflictCandidate kind。
    // snapshot_records 只含白名单且非黑名单的路径，所以这些辅助信息仍需从 entries 收集。
    for entry in entries {
        if SyncService::is_blacklisted_path(&entry.relative_path, scope) {
            plan.ignored_files.push(entry.relative_path.clone());
            continue;
        }
        if entry.sync_kind != SyncKind::Upload && entry.sync_kind != SyncKind::ConflictCandidate {
            plan.ignored_files.push(entry.relative_path.clone());
        }
    }

    // upload/delete 动作直接从 snapshot_records 推导（与 LWW execute attempt 同源）。
    // 遍历 HashMap 顺序不确定，排序输出以保证 plan 的确定性（测试/UI 稳定）。
    let mut sorted_paths: Vec<&String> = snapshot_records.keys().collect();
    sorted_paths.sort();
    for path in sorted_paths {
        let record = &snapshot_records[path];
        if record.op == "upsert" {
            // 动作来源是 snapshot records；用 state.known_files 做 hash 比较优化，
            // 避免无变化时重复上传。
            if is_first_sync {
                plan.files_to_upload.push(path.clone());
            } else if let Some(known_hash) = state.known_files.get(path) {
                if *known_hash != record.content_hash {
                    plan.files_to_upload.push(path.clone());
                }
            } else {
                // known_files 没有但 snapshot 有 upsert → 新文件需上传。
                plan.files_to_upload.push(path.clone());
            }
        } else if record.op == "delete" {
            // #645 评论 5504296097 问题1 修复：remote delete 直接从 snapshot 的
            // delete op 推导，不再遍历 state.known_files.keys() 推断。
            // 这修复了"old manifest upsert + known_files 无 + tombstone 有 +
            // 磁盘无"场景：统一 snapshot 产出 Delete(real deleted_at/device)，
            // build_sync_plan 现在能正确生成 remote delete，与 LWW execute attempt 一致。
            plan.files_to_delete_remote.push(path.clone());
        }
    }

    // 墓碑清理/purge：保留 state.tombstones 用于 files_to_delete_local。
    let now = chrono::Utc::now().timestamp();
    for t in &state.tombstones {
        if t.purge_after <= now {
            plan.files_to_delete_local.push(t.trash_path.clone());
        }
    }

    Ok(plan)
}
