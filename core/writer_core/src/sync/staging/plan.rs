//! 统一对象比较 → 一次不可变提交决议（Issue #1066）。
//!
//! 这是同步唯一的裁决点。输入只有版本事实：
//!
//! - **共同基线**：`state.local.json` 里已确认的内容摘要，或已确认删除事实；
//! - **本地**：本轮持久 mutation 账本给出的版本 / 显式删除，其次用已确认
//!   版本索引 + mtime 判断本地是否改动；
//! - **远端**：传输层写进 staging manifest 的固定 HEAD 索引记录（upsert /
//!   delete）。索引里记录的 hash 就是远端版本，缺少 blob 不等于没有版本。
//!
//! 裁决由 [`crate::sync::object_compare::compare_object`] 给出，没有业务分类、
//! 没有设备时钟、没有二次语义合并。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::sync::object_compare::{compare_object, ObjectConflictSide, ObjectSide, ObjectVerdict};
use crate::sync::types::{AppDataGroup, ManifestFileRecord, SyncManifest, SyncState};

use super::commit_plan::{
    BaselineUpdate, CommitAction, CommitPlan, PublishObject, StagingConflict,
};
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

    // 1. 已确认版本索引：基线与最后确认时间。
    let state = load_state(run, live_root)?;
    let known: HashMap<String, String> = state.known_files.clone();
    let known_updated_at: HashMap<String, i64> = state
        .known_files_updated_at
        .iter()
        .map(|(path, updated_at)| (path.clone(), *updated_at))
        .collect();
    let tombstones: HashSet<String> = state
        .tombstones
        .iter()
        .map(|tombstone| tombstone.original_path.clone())
        .collect();
    let deleted_facts: HashSet<String> = state.deleted_files.iter().cloned().collect();

    // 2. 本轮持久 mutation：决定本地版本与受影响集合。
    let pending = crate::sync::local_mutations::pending_records(
        live_root,
        run.scope(),
        group.as_ref(),
    )?;

    // 3. 远端固定 HEAD 的版本事实。
    let remote_records: HashMap<String, ManifestRecord> = match &group {
        Some(group) => read_staging_manifest_for_group(&staging_root, group)?,
        None => read_staging_manifest(&staging_root)?,
    }
    .unwrap_or_default()
    .into_iter()
    .collect();

    // 4. 参与本轮比较的路径：待同步 mutation ∪ 已确认基线 ∪ 远端版本。
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for path in pending
        .keys()
        .chain(known.keys())
        .chain(remote_records.keys())
    {
        if group.as_ref().map_or(true, |group| group.owns_path(path)) {
            paths.insert(path.to_string());
        }
    }
    paths.retain(|path| !is_engine_path(path, group.as_ref()));

    let mut final_records: BTreeMap<String, ManifestFileRecord> = read_local_records(&group, live_root)?
        .into_iter()
        .map(|(path, record)| (path, to_file_record(&record)))
        .collect();
    final_records.retain(|path, _| {
        !is_engine_path(path, group.as_ref())
            && group.as_ref().map_or(true, |group| group.owns_path(path))
    });

    let delete_intent = run.delete_intent();
    for path in paths {
        let rel = PathBuf::from(&path);
        let incoming_bytes = read_optional(staging_root.join(&rel))?;
        let remote_record = remote_records.get(&path);
        let pending_record = pending.get(&path);

        // 本地版本：删除意图 > 账本事实 > 已确认版本 + mtime > 显式删除。
        let local_side = if delete_intent {
            ObjectSide::Deleted
        } else if let Some(record) = pending_record {
            if record.is_delete() {
                ObjectSide::Deleted
            } else if record.content_hash.is_empty() {
                ObjectSide::Absent
            } else {
                ObjectSide::Present(record.content_hash.clone())
            }
        } else {
            local_side_from_disk(
                live_root,
                &rel,
                &path,
                &known,
                &known_updated_at,
                &tombstones,
                &deleted_facts,
            )?
        };

        // 远端版本：索引记录就是固定 HEAD 的版本，缺少 blob 仍是一个版本。
        let remote_side = match remote_record {
            Some(record) if record.op == "delete" => ObjectSide::Deleted,
            Some(record) if !record.content_hash.is_empty() => {
                ObjectSide::Present(record.content_hash.clone())
            }
            Some(_) => ObjectSide::Absent,
            None if known.contains_key(&path) && !remote_index_missing(&staging_root) => {
                // 远端索引是权威快照：基线对象不在索引里 = 远端删除。
                ObjectSide::Deleted
            }
            None => ObjectSide::Absent,
        };

        // 共同基线：内容摘要优先；其次是已确认删除。删除整个作品的意图下
        // 基线取远端版本：用户已明确删除，远端对象逐个传播删除，不再逐个
        // 产生"修改 vs 删除"冲突。
        let base_side = if delete_intent {
            remote_side.clone()
        } else if let Some(hash) = known.get(&path) {
            ObjectSide::Present(hash.clone())
        } else if tombstones.contains(&path) || deleted_facts.contains(&path) {
            ObjectSide::Deleted
        } else {
            ObjectSide::Absent
        };

        let verdict = compare_object(&base_side, &local_side, &remote_side);
        apply_verdict(
            &mut plan,
            &mut final_records,
            &state,
            &path,
            &rel,
            &verdict,
            local_side.hash(),
            &known,
            pending_record,
            remote_record,
            incoming_bytes.as_deref(),
        );
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

    Ok(plan)
}

/// 从磁盘判定本地版本。
///
/// 已确认版本未过期（mtime 未前进）直接复用基线；mtime 前进才读一次内容。
/// 文件缺失只有存在显式删除事实时才是删除；否则是本轮没有本地事实。
fn local_side_from_disk(
    live_root: &Path,
    rel: &Path,
    path: &str,
    known: &HashMap<String, String>,
    known_updated_at: &HashMap<String, i64>,
    tombstones: &HashSet<String>,
    deleted_facts: &HashSet<String>,
) -> Result<ObjectSide> {
    let full = live_root.join(rel);
    if full.is_file() {
        if let Some(base_hash) = known.get(path) {
            let confirmed_at = known_updated_at.get(path).copied().unwrap_or(0);
            let mtime = std::fs::metadata(&full)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
                .unwrap_or(0);
            if mtime <= confirmed_at {
                return Ok(ObjectSide::Present(base_hash.clone()));
            }
        }
        let bytes = std::fs::read(&full)?;
        return Ok(ObjectSide::Present(crate::sync::hash::content_md5(&bytes)));
    }
    if tombstones.contains(path) || deleted_facts.contains(path) {
        return Ok(ObjectSide::Deleted);
    }
    Ok(ObjectSide::Absent)
}

#[allow(clippy::too_many_arguments)]
fn apply_verdict(
    plan: &mut CommitPlan,
    final_records: &mut BTreeMap<String, ManifestFileRecord>,
    state: &SyncState,
    path: &str,
    rel: &Path,
    verdict: &ObjectVerdict,
    local_hash: Option<&str>,
    known: &HashMap<String, String>,
    pending: Option<&crate::sync::local_mutations::LocalMutationRecord>,
    remote_meta: Option<&ManifestRecord>,
    incoming_bytes: Option<&[u8]>,
) {
    match verdict {
        ObjectVerdict::NoOp => {
            plan.noop.push(rel.to_path_buf());
            match remote_meta {
                Some(record) if record.op == "delete" => {
                    final_records.insert(path.to_string(), to_file_record(record));
                }
                Some(record) => {
                    final_records.insert(path.to_string(), to_file_record(record));
                    // 双方内容已一致但基线还停在旧版本：本轮直接确认。
                    if known.get(path) != Some(&record.content_hash) {
                        plan.confirmations.push(BaselineUpdate {
                            rel_path: rel.to_path_buf(),
                            content_hash: Some(record.content_hash.clone()),
                        });
                    }
                }
                None => {
                    if let Some(hash) = local_hash {
                        final_records.insert(path.to_string(), upsert_record(state, path, hash));
                    }
                }
            }
        }
        ObjectVerdict::Unparticipated => {
            plan.noop.push(rel.to_path_buf());
        }
        ObjectVerdict::PublishUpsert => {
            let Some(hash) = local_hash else {
                return;
            };
            plan.uploads.push(PublishObject {
                rel_path: rel.to_path_buf(),
                base_hash: known.get(path).cloned(),
                final_hash: hash.to_string(),
                mutation_seq: pending.map(|record| record.seq).unwrap_or(0),
                transaction_id: pending
                    .map(|record| record.transaction_id.clone())
                    .unwrap_or_default(),
            });
            final_records.insert(path.to_string(), upsert_record(state, path, hash));
        }
        ObjectVerdict::PublishDelete => {
            plan.publish_deletes.push(rel.to_path_buf());
            final_records.insert(path.to_string(), delete_record(state, path));
        }
        ObjectVerdict::ApplyRemoteUpsert => {
            let Some(record) = remote_meta.filter(|record| record.op != "delete") else {
                return;
            };
            // 只有确实需要落盘时才有 blob；缺少 blob 时保留远端版本事实，
            // 不能把已知版本降级成"不存在"。
            let Some(bytes) = incoming_bytes else {
                plan.noop.push(rel.to_path_buf());
                final_records.insert(path.to_string(), to_file_record(record));
                return;
            };
            plan.content_actions.push(CommitAction::Apply {
                rel_path: rel.to_path_buf(),
                content: bytes.to_vec(),
            });
            final_records.insert(path.to_string(), to_file_record(record));
            plan.confirmations.push(BaselineUpdate {
                rel_path: rel.to_path_buf(),
                content_hash: Some(record.content_hash.clone()),
            });
        }
        ObjectVerdict::ApplyRemoteDelete => {
            plan.content_actions.push(CommitAction::Delete {
                rel_path: rel.to_path_buf(),
            });
            final_records.insert(path.to_string(), delete_record(state, path));
            plan.confirmations.push(BaselineUpdate {
                rel_path: rel.to_path_buf(),
                content_hash: None,
            });
        }
        ObjectVerdict::ConfirmDelete => {
            plan.noop.push(rel.to_path_buf());
            final_records.remove(path);
            plan.confirmations.push(BaselineUpdate {
                rel_path: rel.to_path_buf(),
                content_hash: None,
            });
        }
        ObjectVerdict::Conflict(side) => {
            plan.conflict.push(StagingConflict {
                rel_path: rel.to_path_buf(),
                base_hash: known.get(path).cloned().unwrap_or_default(),
                local_hash: local_hash.unwrap_or_default().to_string(),
                incoming_hash: remote_meta
                    .map(|record| record.content_hash.clone())
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
            // 未决冲突：本地索引原样保留远端版本事实（远端 blob 没有变化），
            // 不能发布本地一侧的旧记录冒充远端。
            if let Some(record) = remote_meta {
                final_records.insert(path.to_string(), to_file_record(record));
            }
        }
    }
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

fn upsert_record(state: &SyncState, path: &str, hash: &str) -> ManifestFileRecord {
    ManifestFileRecord {
        path: path.to_string(),
        content_hash: hash.to_string(),
        updated_at_ms: chrono::Utc::now().timestamp_millis(),
        deleted_at_ms: None,
        device_id: state.device_id.clone(),
        op: "upsert".to_string(),
        schema_version: 1,
    }
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
