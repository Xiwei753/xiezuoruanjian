//!   ：LWW manifest/state/local record 构造。
//!
//! 从 lww.rs 抽出的 manifest 相关：SYNC_MANIFEST_PATH 常量、lww_record_time。
//!
//!   ：local record / tombstone record / remote record 构造
//! 也收归本模块，`mod.rs` 只保留重试和一次 attempt 的编排。
//!
//!   `snapshot_local_records_read_only` 是真正的只读
//! local record 投影 helper（严格版），保留 per-file LWW（含真实 winner device_id）。
//! `snapshot_local_target_lifecycle` 和 LWW `execute_lww_sync_attempt` 都复用它。
//!
//!   ：只允许明确 tombstone / durable delete fact 产生 op="delete"。
//! known_files 中文件缺失但没有 tombstone 时，严格版返回 Err（Retry/本地状态错误），
//! 让同步停在非破坏性状态，不伪造 delete record 传播到远端；merge 路径改用宽松版
//! `snapshot_local_records_for_merge`，把缺失文件收集为待恢复事实，逐路径恢复或报错。

use crate::sync::path::ValidatedSyncPath;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{ManifestFileRecord, SyncManifest, SyncScope};
use crate::sync::SyncService;
use std::collections::HashMap;
use std::path::Path;

/// 同步清单文件路径——记录本地所有文件的哈希、操作类型和时间戳。
/// 这是 LWW 同步的唯一事实来源：三路比较的 base_hash 即从此文件读取。
pub const SYNC_MANIFEST_PATH: &str = "app-meta/sync/manifest.sync.json";

/// 获取 LWW 比较时间戳。
///
/// 对于 delete 操作，优先使用 `deleted_at_ms`（精确的删除时间），
/// 回退到 `updated_at_ms`（删除操作记录的更新时间）。
/// 对于 upsert 操作，直接使用 `updated_at_ms`。
pub(super) fn lww_record_time(record: &ManifestFileRecord) -> i64 {
    crate::sync::content_class::lww_effective_time(
        &record.op,
        record.updated_at_ms,
        record.deleted_at_ms,
    )
}

/// 本地文件快照：正常可判定的记录 + 缺失且无删除证据的文件。
#[derive(Debug, Clone)]
pub(crate) struct LocalSnapshot {
    /// 正常投影出的 `path → record`（含真实 tombstone 产生的 delete record）。
    pub records: HashMap<String, ManifestFileRecord>,
    /// known_files / 旧 manifest 中记录但磁盘缺失且无 tombstone 的文件。
    /// 这些文件既不能当成删除事实，也不能凭空生成 upsert；merge 需先尝试
    /// 从远端恢复，无法恢复时显式报错并保留基线。
    pub missing_without_delete_fact: Vec<MissingKnownFile>,
}

/// 磁盘缺失且无 tombstone 证据的已知文件。
#[derive(Debug, Clone)]
pub(crate) struct MissingKnownFile {
    /// 相对 sync_root 的路径。
    pub path: String,
    /// 已知基线哈希（known_files 或旧 manifest 记录中的哈希）。
    pub known_hash: String,
    /// 旧 manifest 中的对应记录（若有），保留真实 LWW 元数据供恢复复用。
    pub manifest_record: Option<ManifestFileRecord>,
    /// known_files 中记录的已知时间（若有），重建 known_files 时保留。
    pub known_files_updated_at: Option<i64>,
    /// 缺失来源，决定严格调用方给出的错误文案。
    pub cause: MissingCause,
}

/// 已知文件缺失来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingCause {
    /// known_files 中存在但磁盘缺失。
    KnownFile,
    /// 旧 manifest 中 upsert 记录存在但磁盘缺失。
    OldManifestUpsert,
}

/// 宽松版 local record 投影：供 merge 路径使用。
///
/// 与 [`snapshot_local_records_read_only`] 同源，但 known file 缺失且无 tombstone
/// 时不返回 `Err`，而是收集到 [`LocalSnapshot::missing_without_delete_fact`]：
/// merge 会先尝试从远端 blob 恢复文件；无法恢复时显式报
/// `local_missing_without_tombstone`，绝不再伪造 delete record。
pub(crate) fn snapshot_local_records_for_merge(
    sync_root: &Path,
    scope: SyncScope,
    preferred_device_id: &str,
    app_group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::error::Result<LocalSnapshot> {
    build_local_snapshot(sync_root, scope, preferred_device_id, app_group)
}

///   真正的只读 local record 投影 helper（严格版）。
///
/// 读 old manifest + read-only SyncState → scan 当前文件，产出完整 LWW 投影：
///
/// - 当前 hash 与 old manifest record hash 相同 → 直接 clone old manifest record，
///   保留原 `updated_at` / `deleted_at` / `device_id` / `op`（**不**丢 winner device_id）；
/// - 当前 hash 改了或是新文件 → 用当前文件 mtime + 当前真实 device_id 生成新 upsert；
/// - known file 消失且有真实 tombstone → 用 tombstone 的 `deleted_at` + `deleted_by`/device_id
///   生成 delete record；
/// - known file 消失且无 tombstone → 返回 `Err`（本地状态错误），让同步停在非破坏性
///   状态。不再用 `now_ms` 伪造 delete record——本地缺文件可能是未下载完整、工作区
///   损坏、人工移动，不应直接变成远端删除命令。调用方据此返回 Retry。
///
/// 严格语义供生命周期决策与全量 materialize 使用：证据不完整时必须停下。
/// merge 路径用宽松版 [`snapshot_local_records_for_merge`] 做文件级恢复与隔离。
///
/// 这个 helper 是真正只读的：用 [`SyncService::load_sync_state_read_only`] 加载 state，
/// 不写文件、不删旧文件。
///
/// 返回 `HashMap<path, ManifestFileRecord>`，供 `build_sync_plan`（plan/dry-run 路径）
/// 与 LWW `execute_lww_sync_attempt` 同源复用。
pub fn snapshot_local_records_read_only(
    sync_root: &Path,
    scope: SyncScope,
    preferred_device_id: &str,
) -> crate::error::Result<HashMap<String, ManifestFileRecord>> {
    let snapshot = build_local_snapshot(sync_root, scope, preferred_device_id, None)?;
    if let Some(missing) = snapshot.missing_without_delete_fact.first() {
        let message = match missing.cause {
            MissingCause::KnownFile => format!(
                "snapshot_local_records_read_only: known file {} missing on disk \
                 without tombstone — cannot fabricate delete record; local state may be \
                 incomplete or corrupted, retry sync after restoring local state",
                missing.path
            ),
            MissingCause::OldManifestUpsert => format!(
                "snapshot_local_records_read_only: old manifest upsert for {} but file missing \
                 without tombstone — cannot fabricate current local fact",
                missing.path
            ),
        };
        return Err(crate::Error::Io(std::io::Error::other(message)));
    }
    Ok(snapshot.records)
}

/// Strict read-only projection for one App group.
pub fn snapshot_app_group_records_read_only(
    sync_root: &Path,
    group: &crate::sync::types::AppDataGroup,
    preferred_device_id: &str,
) -> crate::error::Result<HashMap<String, ManifestFileRecord>> {
    let snapshot =
        build_local_snapshot(sync_root, SyncScope::App, preferred_device_id, Some(group))?;
    if let Some(missing) = snapshot.missing_without_delete_fact.first() {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "snapshot_app_group_records_read_only: local file {} missing without tombstone",
            missing.path
        ))));
    }
    Ok(snapshot.records)
}

/// 共享实现：宽松投影 + 缺失文件收集。
#[allow(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    clippy::cognitive_complexity
)]
fn build_local_snapshot(
    sync_root: &Path,
    scope: SyncScope,
    preferred_device_id: &str,
    app_group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::error::Result<LocalSnapshot> {
    // 1. 读 old manifest（manifest.sync.json）→ HashMap<path, ManifestFileRecord>。
    //   manifest 存在但损坏 → 返回 Err（不静默 fallback）。
    // 调用方（compute_local_project_lifecycle_candidate）据此返回 Retry，
    // 避免在无可靠本地 LWW 证据时做破坏性 DeleteLocalProject 决策。
    // manifest 不存在 → 空 HashMap（首次同步，正常）。
    let manifest_rel_path = app_group
        .map(crate::sync::types::AppDataGroup::local_manifest_path)
        .unwrap_or_else(|| SYNC_MANIFEST_PATH.to_string());
    let manifest_path = sync_root.join(&manifest_rel_path);
    let legacy_manifest_path = sync_root.join(SYNC_MANIFEST_PATH);
    let source_manifest_path = if manifest_path.exists() {
        manifest_path.as_path()
    } else if app_group.is_some() && legacy_manifest_path.exists() {
        // Local v1 state is read-only migration input. The first successful group
        // merge writes its own manifest and leaves the aggregate manifest intact.
        legacy_manifest_path.as_path()
    } else {
        manifest_path.as_path()
    };
    let old_manifest_records: HashMap<String, ManifestFileRecord> = if source_manifest_path.exists()
    {
        let content = std::fs::read(source_manifest_path).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "snapshot_local_records_read_only: manifest read failed at {}: {e}",
                source_manifest_path.display()
            )))
        })?;
        serde_json::from_slice::<SyncManifest>(&content)
            .map_err(|e| {
                crate::Error::Io(std::io::Error::other(format!(
                    "snapshot_local_records_read_only: manifest corrupt at {}: {e}",
                    source_manifest_path.display()
                )))
            })?
            .files
            .into_iter()
            .filter(|record| app_group.map_or(true, |group| group.owns_path(&record.path)))
            .map(|r| (r.path.clone(), r))
            .collect()
    } else {
        HashMap::new()
    };

    // 2. 读 SyncState（read-only，不写文件）。
    let preferred = if preferred_device_id.is_empty() {
        None
    } else {
        Some(preferred_device_id)
    };
    let state = match app_group {
        Some(group) => {
            SyncService::load_sync_state_read_only_for_group(sync_root, group, preferred)?
        }
        None => SyncService::load_sync_state_read_only(sync_root, preferred)?,
    };

    // 3. scan 当前文件。
    let local_entries = match app_group {
        Some(group) => crate::sync::scanner::scan_for_sync_group(sync_root, scope, Some(group))?,
        None => crate::sync::scanner::scan_for_sync(sync_root, scope)?,
    };

    let mut records = HashMap::new();

    for entry in &local_entries {
        if entry.sync_kind == crate::sync::types::SyncKind::Upload
            && entry.relative_path != SYNC_MANIFEST_PATH
        {
            let path = entry.relative_path.clone();
            let local_hash = entry.file_hash.clone();

            // 当前 hash 与 old manifest record hash 相同
            // → 直接 clone old manifest record，保留原 device_id。
            if let Some(old_rec) = old_manifest_records.get(&path) {
                if old_rec.content_hash == local_hash && old_rec.op == "upsert" {
                    records.insert(path.clone(), old_rec.clone());
                    continue;
                }
            }

            // 当前 hash 改了或是新文件 → 用当前文件 mtime + 当前真实 device_id。
            let updated_at_ms = if let Some(known_hash) = state.known_files.get(&path) {
                if *known_hash == local_hash {
                    state
                        .known_files_updated_at
                        .get(&path)
                        .cloned()
                        .unwrap_or(0)
                } else {
                    read_mtime_ms(sync_root, &path)?
                }
            } else {
                read_mtime_ms(sync_root, &path)?
            };

            records.insert(
                path.clone(),
                ManifestFileRecord {
                    path,
                    content_hash: local_hash,
                    updated_at_ms,
                    deleted_at_ms: None,
                    device_id: state.device_id.clone(),
                    op: "upsert".to_string(),
                    schema_version: 1,
                },
            );
        }
    }

    // 4. known file 消失 → 有 tombstone 用真实删除时间；
    //    无 tombstone → 收集为 missing_without_delete_fact（不伪造 delete record）。
    //    只允许明确 tombstone / durable delete fact 产生 op="delete"。
    let mut missing_without_delete_fact: Vec<MissingKnownFile> = Vec::new();
    for path in state.known_files.keys() {
        if records.contains_key(path) {
            continue;
        }
        if !SyncService::is_whitelisted_path(path, scope)
            || SyncService::is_blacklisted_path(path, scope)
        {
            continue;
        }
        if !sync_root.join(path).exists() {
            // 有真实 tombstone → 用 tombstone 的 deleted_at + deleted_by/device_id。
            if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path) {
                let deleted_at_ms = tombstone.deleted_at * 1000;
                records.insert(
                    path.clone(),
                    ManifestFileRecord {
                        path: path.clone(),
                        content_hash: String::new(),
                        updated_at_ms: deleted_at_ms,
                        deleted_at_ms: Some(deleted_at_ms),
                        device_id: if tombstone.deleted_by.is_empty() {
                            state.device_id.clone()
                        } else {
                            tombstone.deleted_by.clone()
                        },
                        op: "delete".to_string(),
                        schema_version: 1,
                    },
                );
            } else {
                // 无 tombstone → 收集缺失事实（不伪造 delete record）。
                // 本地 known_files 中记录了此文件但磁盘上不存在，且没有 tombstone
                // 证明它被合法删除。可能是未下载完整、工作区损坏、人工移动。
                // 伪造 delete record 会让本地删除错误地传播到远端（破坏性）。
                // merge 会先尝试从远端 blob 恢复；无法恢复时显式报错。
                missing_without_delete_fact.push(MissingKnownFile {
                    path: path.clone(),
                    known_hash: state.known_files.get(path).cloned().unwrap_or_default(),
                    manifest_record: old_manifest_records.get(path).cloned(),
                    known_files_updated_at: state.known_files_updated_at.get(path).cloned(),
                    cause: MissingCause::KnownFile,
                });
            }
        }
    }

    // 5.   ：manifest 中有 upsert record 但文件不在磁盘上
    //    且不在 known_files → 不再把旧 upsert 冒充当前本地事实。
    //    - 文件实际存在 → 说明被 whitelisted/blacklisted 跳过，不管（上面 step 3 已处理）；
    //    - 文件不存在 + 有 tombstone（state.tombstones 中 original_path 匹配）
    //      → 生成 Delete record（用 tombstone 的 deleted_at*1000 作为 deleted_at_ms
    //      和 updated_at_ms，device_id 用 tombstone.deleted_by 或 state.device_id）；
    //    - 文件不存在 + 无 tombstone → 返回 Err（不能伪造当前本地事实）。
    for (path, old_rec) in &old_manifest_records {
        if records.contains_key(path) {
            continue;
        }
        if old_rec.op != "upsert" {
            continue;
        }
        // 文件实际存在 → 上面 step 3 已处理（whitelisted 之外或被跳过的）。
        if sync_root.join(path).exists() {
            continue;
        }
        // 文件不存在 → 查找 tombstone。
        if let Some(tombstone) = state.tombstones.iter().find(|t| t.original_path == *path) {
            let deleted_at_ms = tombstone.deleted_at * 1000;
            records.insert(
                path.clone(),
                ManifestFileRecord {
                    path: path.clone(),
                    content_hash: String::new(),
                    updated_at_ms: deleted_at_ms,
                    deleted_at_ms: Some(deleted_at_ms),
                    device_id: if tombstone.deleted_by.is_empty() {
                        state.device_id.clone()
                    } else {
                        tombstone.deleted_by.clone()
                    },
                    op: "delete".to_string(),
                    schema_version: 1,
                },
            );
        } else {
            // 无 tombstone → 收集缺失事实（不能把旧 upsert 冒充当前本地事实）。
            if missing_without_delete_fact.iter().any(|m| m.path == *path) {
                continue;
            }
            missing_without_delete_fact.push(MissingKnownFile {
                path: path.clone(),
                known_hash: old_rec.content_hash.clone(),
                manifest_record: Some(old_rec.clone()),
                known_files_updated_at: state.known_files_updated_at.get(path).cloned(),
                cause: MissingCause::OldManifestUpsert,
            });
        }
    }

    Ok(LocalSnapshot {
        records,
        missing_without_delete_fact,
    })
}

/// 构建远端文件记录。
///
/// 从远端 manifest 和 tree 构建 `path → ManifestFileRecord` 映射。
/// 远端 tree 中存在但 manifest 中无记录的文件（首次同步或 manifest 损失），
/// 通过 `provider.read` 读取远端正文算 MD5 作为 `content_hash` 补充记录。
///
/// **不变量**：返回的 `ManifestFileRecord.content_hash` 始终是 MD5（32位 hex）。
/// - manifest 明确给出的 `op == "upsert"` 记录，若 `content_hash` 已是 MD5 直接保留；
///   若不是 MD5（旧版本可能写入 40 位 Git blob SHA），读远端正文算 MD5 替换。
/// - tree 有文件但 manifest 无记录的 fallback 记录，读远端正文算 MD5 作为 `content_hash`，
///   不再把 40 位 Git blob SHA 塞进 `content_hash`。tree SHA 只留在 `remote_tree_files` 里
///   （由调用方持有），不写进 `content_hash`。
/// - 远端正文读不到（`provider.read` 返回 `None`）时返回 `Err`，不伪造内容哈希。
///
/// 所有远端路径在进入同步逻辑前必须通过 [`ValidatedSyncPath`] 验证；
/// 非法路径（绝对路径、`..` 穿越、Windows prefix 等）直接返回错误，
/// 不做"清洗后继续用"。
pub(super) fn build_remote_records(
    remote_manifest: SyncManifest,
    remote_tree_files: &HashMap<String, String>,
    scope: SyncScope,
    app_group: Option<&crate::sync::types::AppDataGroup>,
    provider: &dyn SyncProvider,
    source_remote_prefix: &str,
) -> crate::Result<HashMap<String, ManifestFileRecord>> {
    let mut remote_records = HashMap::new();
    // 两个来源（remote_manifest.files / remote_tree_files）按同一顺序处理：
    //   ValidatedSyncPath::new → 用 normalized 判断 SYNC_MANIFEST_PATH
    //   → is_whitelisted_path / is_blacklisted_path → 插入 map。
    // manifest 和 tree 两个远端来源都先标准化再判断 SYNC_MANIFEST_PATH，
    // 共用同一条边界：远端给出 `app-meta\sync\manifest.sync.json`（反斜杠形式）
    // 时标准化后正好是保留的 manifest 路径，会被这里 continue 跳过，
    // 不会作为普通 ManifestFileRecord 插进 remote_records。
    // 用 ValidatedSyncPath::as_str() 的标准化路径写回 record.path 和 map key，
    // 避免 `volumes\a\chapter.md` 与 `volumes/a/chapter.md` 成为两份逻辑路径。
    for mut rec in remote_manifest.files {
        let validated = ValidatedSyncPath::new(&rec.path)?;
        let normalized = validated.as_str();
        if normalized == SYNC_MANIFEST_PATH {
            continue;
        }
        let whitelisted = app_group.map_or_else(
            || SyncService::is_whitelisted_path(normalized, scope),
            |group| SyncService::is_whitelisted_app_group_path(normalized, group),
        );
        if !whitelisted || SyncService::is_blacklisted_path(normalized, scope) {
            continue;
        }
        rec.path = normalized.to_string();
        // manifest 明确给出的 upsert 记录：若 content_hash 不是 MD5（旧版本可能写入
        // 40 位 Git blob SHA），读远端正文算 MD5 替换，保证 content_hash 始终是 MD5。
        // 远端文件不存在时返回 Err，不伪造内容哈希。
        if rec.op == "upsert" && !crate::sync::hash::is_md5_content_hash(&rec.content_hash) {
            let remote_path = format!("{}/{}", source_remote_prefix, normalized);
            let remote_obj = provider.read(&remote_path)?;
            match remote_obj {
                Some(obj) => {
                    rec.content_hash = crate::sync::hash::content_md5(&obj.content);
                }
                None => {
                    return Err(crate::Error::Other(format!(
                        "build_remote_records: manifest upsert for {} but remote object missing \
                         at {} — cannot fabricate content hash",
                        normalized, remote_path
                    )));
                }
            }
        }
        remote_records.insert(normalized.to_string(), rec);
    }

    for (path, sha) in remote_tree_files {
        let validated = ValidatedSyncPath::new(path)?;
        let normalized = validated.as_str();
        // 用标准化后的路径判断，避免反斜杠形式和 / 形式指向同一逻辑路径时
        // tree 记录覆盖 manifest 的 LWW 元数据。
        if normalized == SYNC_MANIFEST_PATH || remote_records.contains_key(normalized) {
            continue;
        }
        let whitelisted = app_group.map_or_else(
            || SyncService::is_whitelisted_path(normalized, scope),
            |group| SyncService::is_whitelisted_app_group_path(normalized, group),
        );
        if !whitelisted || SyncService::is_blacklisted_path(normalized, scope) {
            continue;
        }
        // tree 有文件但 manifest 无记录：读远端正文算 MD5 作为 content_hash，
        // 不再用 40 位 Git blob SHA（sha）作为 content_hash。tree SHA 只留在
        // remote_tree_files 里（由调用方持有），不写进 content_hash。
        // 远端文件不存在时返回 Err，不伪造内容哈希。
        let remote_path = format!("{}/{}", source_remote_prefix, normalized);
        let remote_obj = provider.read(&remote_path)?;
        let content_hash = match remote_obj {
            Some(obj) => crate::sync::hash::content_md5(&obj.content),
            None => {
                return Err(crate::Error::Other(format!(
                    "build_remote_records: tree entry for {} but remote object missing at {} \
                     — cannot fabricate content hash",
                    normalized, remote_path
                )));
            }
        };
        let _ = sha; // tree SHA 不写进 content_hash，仅留在 remote_tree_files 里。
        remote_records.insert(
            normalized.to_string(),
            ManifestFileRecord {
                path: normalized.to_string(),
                content_hash,
                updated_at_ms: 0,
                deleted_at_ms: None,
                device_id: "remote".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            },
        );
    }

    Ok(remote_records)
}

/// 读取文件 mtime，失败时返回 `Err`。
///
/// 不再 `unwrap_or(fallback_ms)` — 二次 metadata 失败（竞态删/权限变）伪造
/// "当前时间"的 LWW 记录会让本地文件错误地胜过远端。metadata / modified /
/// epoch 转换任一失败直接 `Err`，调用方应走 Retry。
fn read_mtime_ms(sync_root: &Path, path: &str) -> crate::error::Result<i64> {
    let metadata = std::fs::metadata(sync_root.join(path)).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_mtime_ms: metadata failed for {path}: {e}"
        )))
    })?;
    let modified = metadata.modified().map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "read_mtime_ms: modified failed for {path}: {e}"
        )))
    })?;
    let duration = modified
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "read_mtime_ms: mtime before epoch for {path}: {e}"
            )))
        })?;
    Ok(i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests;
