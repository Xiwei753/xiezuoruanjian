//! 统一对象比较 → 一次不可变提交决议（Issue #1066）。
//!
//! 这是同步唯一的裁决点。输入只有对象事实：
//!
//! - **共同基线**：`state.local.json` 里已确认的内容摘要（没有基线 = 从未同步）；
//! - **本地**：扫描得到的当前内容摘要，或显式删除事实（tombstone）；
//! - **远端**：传输层写进 staging manifest 的远端索引记录（upsert / delete），
//!   以及已下载的 blob。
//!
//! 裁决由 [`crate::sync::object_compare::compare_object`] 给出，没有业务分类、
//! 没有设备时钟、没有二次语义合并。结果落成一个 [`CommitPlan`]：远端内容走
//! `content_actions`，本地上传/删除走 `uploads`/`publish_deletes`，未决冲突走
//! `conflict`，最终对象索引走 `engine_state_actions`。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::sync::object_compare::{compare_object, ObjectConflictSide, ObjectSide, ObjectVerdict};
use crate::sync::types::{AppDataGroup, ManifestFileRecord, SyncManifest, SyncState};

use super::commit_plan::{CommitAction, CommitPlan, StagingConflict};
use super::resolve::{
    read_live_manifest, read_live_manifest_for_group, read_staging_manifest,
    read_staging_manifest_for_group, ManifestRecord,
};
use super::run::StagingRun;

/// 计算本轮提交决议。
pub(crate) fn compute(run: &StagingRun, live_root: &Path) -> Result<CommitPlan> {
    let group = run.app_group().cloned();
    let staging_root = run.staging_root();
    let mut plan = CommitPlan {
        object_engine: true,
        ..CommitPlan::default()
    };

    // 1. 共同基线：只信已确认状态，不用文件 mtime / 设备时钟。
    let state = load_state(run, live_root)?;
    let known: HashMap<String, String> = state
        .known_files
        .iter()
        .map(|(path, hash)| (path.clone(), hash.clone()))
        .collect();
    let tombstones: HashSet<String> = state
        .tombstones
        .iter()
        .map(|tombstone| tombstone.original_path.clone())
        .collect();

    // 2. 远端事实：传输层写进 staging manifest 的索引记录。
    let remote_records: HashMap<String, ManifestRecord> = match &group {
        Some(group) => read_staging_manifest_for_group(&staging_root, group)?,
        None => read_staging_manifest(&staging_root)?,
    }
    .unwrap_or_default()
    .into_iter()
    .collect();

    // 3. 本机事实：扫描同步范围内的全部白名单对象（含本轮新建的对象）。
    let entries =
        crate::sync::scanner::scan_for_sync_group(live_root, run.scope(), group.as_ref())?;
    let mut local_hashes: HashMap<String, String> = HashMap::new();
    for entry in &entries {
        if entry.sync_kind == crate::sync::types::SyncKind::Upload {
            local_hashes.insert(entry.relative_path.clone(), entry.file_hash.clone());
        }
    }

    // 4. 参与本轮比较的路径：本机对象 ∪ 远端索引 ∪ 已确认基线。
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for group_entry in &group {
        for path in known.keys() {
            if group_entry.owns_path(path) {
                paths.insert(path.to_string());
            }
        }
    }
    if group.is_none() {
        paths.extend(known.keys().cloned());
    }
    paths.extend(local_hashes.keys().cloned());
    paths.extend(remote_records.keys().cloned());
    // 引擎状态/配置永不参与对象裁决。
    paths.retain(|path| !is_engine_path(path, group.as_ref()));

    let mut final_records: BTreeMap<String, ManifestFileRecord> = read_local_records(&group, live_root)?
        .into_iter()
        .map(|(path, record)| (path, to_file_record(&record)))
        .collect();
    // 清单只记录本 target 的业务对象：引擎状态与其它组路径不进入本组索引。
    final_records.retain(|path, _| {
        !is_engine_path(path, group.as_ref())
            && group.as_ref().map_or(true, |group| group.owns_path(path))
    });

    for path in paths {
        let rel = PathBuf::from(&path);
        let local_bytes = read_optional(live_root.join(&rel))?;
        let incoming_bytes = read_optional(staging_root.join(&rel))?;

        let local_hash = local_bytes
            .as_deref()
            .map(crate::sync::hash::content_md5);
        let local_side = match local_hash.as_deref() {
            Some(hash) => ObjectSide::Present(hash.to_string()),
            None if tombstones.contains(&path) => ObjectSide::Deleted,
            None => ObjectSide::Absent,
        };
        let remote_side = match remote_records.get(&path) {
            Some(record) if record.op == "delete" => ObjectSide::Deleted,
            Some(record) => match incoming_bytes.as_deref() {
                Some(bytes) => ObjectSide::Present(crate::sync::hash::content_md5(bytes)),
                // 索引声明该版本但本轮没有下载：内容已与基线或本地一致时，
                // 它仍是权威远端事实；否则是本轮未参与，不是删除。
                None if !record.content_hash.is_empty()
                    && (known.get(&path) == Some(&record.content_hash)
                        || local_hash.as_deref() == Some(record.content_hash.as_str())) =>
                {
                    ObjectSide::Present(record.content_hash.clone())
                }
                None => ObjectSide::Absent,
            },
            // 旧生命周期路径（restore/replace）只把内容写进 staging、没有索引记录：
            // staging 里的字节就是远端事实。
            None if incoming_bytes.is_some() => ObjectSide::Present(
                crate::sync::hash::content_md5(incoming_bytes.as_deref().unwrap_or_default()),
            ),
            None if known.contains_key(&path) && !remote_index_missing(&staging_root) => {
                // 远端索引是权威快照：基线对象不在索引里 = 远端删除。
                ObjectSide::Deleted
            }
            None => ObjectSide::Absent,
        };

        let base = known.get(&path).map(String::as_str);
        let verdict = compare_object(base, &local_side, &remote_side);
        apply_verdict(
            &mut plan,
            &mut final_records,
            &state,
            &path,
            &rel,
            &verdict,
            local_bytes.as_deref(),
            incoming_bytes.as_deref(),
            remote_records.get(&path),
        )?;
    }

    // 5. 最终对象索引：只写与本机事实不同的内容寻址对象/索引/根，
    //    与正文、state 在同一个 SaveTransaction 里原子提交。
    let manifest_rel = manifest_rel(group.as_ref());
    let manifest = SyncManifest {
        files: final_records.into_values().collect(),
    };
    let store = crate::sync::lww::ManifestStore::new(live_root, &manifest_rel.to_string_lossy());
    let write_plan = store.plan_sync_manifest(&manifest, "")?;
    for (rel, bytes) in write_plan.writes {
        plan.engine_state_actions.push(CommitAction::Apply {
            rel_path: PathBuf::from(rel),
            content: bytes,
        });
    }
    for rel in write_plan.deletes {
        plan.engine_state_actions.push(CommitAction::Delete {
            rel_path: PathBuf::from(rel),
        });
    }

    // 旧单文件清单已被 v2 存储取代：存储写完后在同一事务里删除它。
    if live_root.join(&manifest_rel).exists() {
        plan.engine_state_actions.push(CommitAction::Delete {
            rel_path: manifest_rel,
        });
    }

    // 上传/删除清单交给提交阶段冻结成发布决议；本函数只做只读比较。
    run.record_publish_lists(plan.uploads.clone(), plan.publish_deletes.clone());
    Ok(plan)
}

#[allow(clippy::too_many_arguments)]
fn apply_verdict(
    plan: &mut CommitPlan,
    final_records: &mut BTreeMap<String, ManifestFileRecord>,
    state: &SyncState,
    path: &str,
    rel: &Path,
    verdict: &ObjectVerdict,
    local_bytes: Option<&[u8]>,
    incoming_bytes: Option<&[u8]>,
    remote_meta: Option<&ManifestRecord>,
) -> Result<()> {
    match verdict {
        ObjectVerdict::NoOp | ObjectVerdict::Unparticipated => {
            plan.noop.push(rel.to_path_buf());
            // 内容已一致：优先沿用远端索引的版本事实，避免每轮重写时间戳。
            if let Some(record) = remote_meta.filter(|record| record.op != "delete") {
                final_records.insert(path.to_string(), to_file_record(record));
            } else if let Some(bytes) = local_bytes {
                let current = final_records.get(path);
                let hash = crate::sync::hash::content_md5(bytes);
                if current.map(|record| record.content_hash.as_str()) != Some(hash.as_str()) {
                    upsert_record(final_records, state, path, bytes);
                }
            }
        }
        ObjectVerdict::PublishUpsert => {
            plan.uploads.push(rel.to_path_buf());
            if let Some(bytes) = local_bytes {
                let current = final_records.get(path);
                let hash = crate::sync::hash::content_md5(bytes);
                if current.map(|record| record.content_hash.as_str()) != Some(hash.as_str()) {
                    upsert_record(final_records, state, path, bytes);
                }
            }
        }
        ObjectVerdict::PublishDelete => {
            plan.publish_deletes.push(rel.to_path_buf());
            final_records.insert(path.to_string(), delete_record(state, path));
        }
        ObjectVerdict::ApplyRemoteUpsert => {
            let Some(bytes) = incoming_bytes else {
                // 没有下载到内容：本轮不参与，保持基线。
                plan.noop.push(rel.to_path_buf());
                return Ok(());
            };
            plan.content_actions.push(CommitAction::Apply {
                rel_path: rel.to_path_buf(),
                content: bytes.to_vec(),
            });
            final_records.insert(
                path.to_string(),
                remote_record(path, bytes, remote_meta, state),
            );
        }
        ObjectVerdict::ApplyRemoteDelete => {
            plan.content_actions.push(CommitAction::Delete {
                rel_path: rel.to_path_buf(),
            });
            final_records.insert(
                path.to_string(),
                remote_meta
                    .map(|record| to_file_record(record))
                    .unwrap_or_else(|| delete_record(state, path)),
            );
        }
        ObjectVerdict::ConfirmDelete => {
            plan.noop.push(rel.to_path_buf());
            final_records.remove(path);
        }
        ObjectVerdict::Conflict(side) => {
            plan.conflict.push(StagingConflict {
                rel_path: rel.to_path_buf(),
                base_hash: state.known_files.get(path).cloned().unwrap_or_default(),
                local_hash: local_bytes
                    .map(crate::sync::hash::content_md5)
                    .unwrap_or_default(),
                incoming_hash: incoming_bytes
                    .map(crate::sync::hash::content_md5)
                    .unwrap_or_default(),
                kind: match side {
                    ObjectConflictSide::BothChanged | ObjectConflictSide::LocalDeleted => {
                        crate::sync::types::SyncConflictKind::BothChanged
                    }
                    ObjectConflictSide::RemoteDeleted => {
                        crate::sync::types::SyncConflictKind::RemoteDeleted
                    }
                },
                remote_snapshot_path: None,
                remote_graph_snapshot_dir: None,
            });
            // 未决冲突保留共同基线：最终索引不变。
        }
    }
    Ok(())
}

fn remote_index_missing(staging_root: &Path) -> bool {
    staging_root
        .join(super::download::REMOTE_INDEX_MISSING_MARKER)
        .exists()
}

fn is_engine_path(path: &str, group: Option<&AppDataGroup>) -> bool {
    if let Some(group) = group {
        if path == group.local_manifest_path()
            || path == group.local_state_path()
            || path == group.local_conflicts_path()
        {
            return true;
        }
    }
    path.starts_with("app-meta/")
        || path == crate::sync::lww::SYNC_MANIFEST_PATH
        || path.starts_with(".git/")
}

fn manifest_rel(group: Option<&AppDataGroup>) -> PathBuf {
    PathBuf::from(
        group
            .map(AppDataGroup::local_manifest_path)
            .unwrap_or_else(|| crate::sync::lww::SYNC_MANIFEST_PATH.to_string()),
    )
}

fn load_state(run: &StagingRun, live_root: &Path) -> Result<SyncState> {
    match run.app_group() {
        Some(group) => crate::sync::SyncService::load_sync_state_read_only_for_group(
            live_root, group, None,
        ),
        None => crate::sync::SyncService::load_sync_state_read_only(live_root, None),
    }
}

fn read_local_records(
    group: &Option<AppDataGroup>,
    live_root: &Path,
) -> Result<HashMap<String, ManifestRecord>> {
    let records = match group {
        Some(group) => read_live_manifest_for_group(live_root, group)?,
        None => read_live_manifest(live_root)?,
    };
    Ok(records.unwrap_or_default())
}

fn read_optional(path: impl AsRef<Path>) -> Result<Option<Vec<u8>>> {
    let path = path.as_ref();
    if path.is_file() {
        Ok(Some(std::fs::read(path)?))
    } else {
        Ok(None)
    }
}

fn to_file_record(record: &ManifestRecord) -> ManifestFileRecord {
    ManifestFileRecord {
        path: record.path.clone(),
        content_hash: record.content_hash.clone(),
        updated_at_ms: record.updated_at_ms,
        deleted_at_ms: record.deleted_at_ms,
        device_id: record.device_id.clone(),
        op: record.op.clone(),
        schema_version: 1,
    }
}

fn remote_record(
    path: &str,
    bytes: &[u8],
    remote_meta: Option<&ManifestRecord>,
    state: &SyncState,
) -> ManifestFileRecord {
    ManifestFileRecord {
        path: path.to_string(),
        content_hash: crate::sync::hash::content_md5(bytes),
        updated_at_ms: remote_meta.map(|record| record.updated_at_ms).unwrap_or(0),
        deleted_at_ms: None,
        device_id: remote_meta
            .map(|record| record.device_id.clone())
            .filter(|device_id| !device_id.is_empty())
            .unwrap_or_else(|| state.device_id.clone()),
        op: "upsert".to_string(),
        schema_version: 1,
    }
}

fn upsert_record(
    records: &mut BTreeMap<String, ManifestFileRecord>,
    state: &SyncState,
    path: &str,
    bytes: &[u8],
) {
    let hash = crate::sync::hash::content_md5(bytes);
    let updated_at_ms = chrono::Utc::now().timestamp_millis();
    records.insert(
        path.to_string(),
        ManifestFileRecord {
            path: path.to_string(),
            content_hash: hash,
            updated_at_ms,
            deleted_at_ms: None,
            device_id: state.device_id.clone(),
            op: "upsert".to_string(),
            schema_version: 1,
        },
    );
}

fn delete_record(state: &SyncState, path: &str) -> ManifestFileRecord {
    let deleted_at_ms = state
        .tombstones
        .iter()
        .find(|tombstone| tombstone.original_path == path)
        .map(|tombstone| tombstone.deleted_at.saturating_mul(1000))
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    ManifestFileRecord {
        path: path.to_string(),
        content_hash: String::new(),
        updated_at_ms: deleted_at_ms,
        deleted_at_ms: Some(deleted_at_ms),
        device_id: state.device_id.clone(),
        op: "delete".to_string(),
        schema_version: 1,
    }
}
