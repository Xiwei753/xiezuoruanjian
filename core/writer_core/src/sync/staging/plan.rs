//! 统一对象比较 → 一次不可变提交决议（Issue #1066）。
//!
//! 这是同步唯一的裁决点。输入只有版本事实：
//!
//! - **共同基线**：`state.local.json` 里已确认的内容摘要 / 已确认删除，或从未同步；
//! - **本地**：本轮持久 mutation 账本给出的版本 / 显式删除，其次按磁盘当前内容；
//! - **远端**：传输层写进 staging manifest 的固定 HEAD 索引记录（upsert / delete），
//!   远端完整树里没有的对象就是已删除，不用 marker 反推。
//!
//! 裁决由 [`crate::sync::object_compare::compare_object`] 给出，没有业务分类、
//! 没有设备时钟、没有二次语义合并。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::sync::object_compare::{compare_object, ObjectConflictSide, ObjectSide, ObjectVerdict};
use crate::sync::types::{AppDataGroup, ManifestFileRecord, SyncManifest, SyncState};

use super::commit_plan::{BaselineUpdate, CommitAction, CommitPlan, PublishObject, StagingConflict};
use super::resolve::{
    read_live_manifest, read_live_manifest_for_group, read_staging_manifest,
    read_staging_manifest_for_group, ManifestRecord,
};
use super::run::StagingRun;

/// 计算本轮提交决议。
pub(crate) fn compute(run: &StagingRun, live_root: &Path) -> Result<CommitPlan> {
    let group = run.app_group().cloned();
    let staging_root = run.staging_root();
    let mut plan = CommitPlan::default();

    // 1. 远端固定 HEAD 的版本事实。
    let remote_records: HashMap<String, ManifestRecord> = match &group {
        Some(group) => read_staging_manifest_for_group(&staging_root, group)?,
        None => read_staging_manifest(&staging_root)?,
    }
    .unwrap_or_default()
    .into_iter()
    .collect();
    let remote_index_missing = remote_index_missing(&staging_root);

    // 2. 已确认版本索引（唯一权威状态始终是 live root；删除作品在 Prepare
    //    阶段已把回收区的已确认版本写成 live 的初始索引）。
    let mut state = load_state(run, live_root)?;

    // 3. 第一次建立索引：把本地已有对象按磁盘内容导入一次持久 mutation 账本，
    //    正常同步之后只按账本与远端版本差异迭代。
    if !run.delete_intent()
        && state.known_files.is_empty()
        && state.tombstones.is_empty()
        && state.deleted_files.is_empty()
        && remote_records.is_empty()
    {
        crate::sync::local_mutations::rebuild_pending_from_disk(
            live_root,
            run.scope(),
            group.as_ref(),
        )?;
        state = load_state(run, live_root)?;
    }

    let known: HashMap<String, String> = state.known_files.clone();
    let tombstones: HashSet<String> = state
        .tombstones
        .iter()
        .map(|tombstone| tombstone.original_path.clone())
        .collect();
    let deleted_facts: HashSet<String> = state.deleted_files.iter().cloned().collect();
    let pending_take_remote: HashSet<String> = state.pending_take_remote.iter().cloned().collect();

    // 4. 本轮持久 mutation：决定本地版本与受影响集合。
    let pending = crate::sync::local_mutations::pending_records(
        live_root,
        run.scope(),
        group.as_ref(),
    )?;

    let local_manifest_records = read_local_records(&group, live_root)?;

    // 5. 参与本轮比较的路径：待同步 ledger ∪ 已确认基线 ∪ 远端变化 ∪
    //    本地索引独有对象。没有版本差异的 known 对象不再逐轮重读。
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut insert = |path: &str| {
        if group.as_ref().map_or(true, |group| group.owns_path(path))
            && !is_engine_path(path, group.as_ref())
        {
            paths.insert(path.to_string());
        }
    };
    for path in pending
        .keys()
        .chain(tombstones.iter())
        .chain(pending_take_remote.iter())
        .chain(local_manifest_records.keys())
    {
        insert(path);
    }
    for (path, record) in &remote_records {
        let changed = known.get(path).map_or(true, |base| {
            record.op == "delete" || record.content_hash != *base
        });
        if changed {
            insert(path);
        }
    }
    if !remote_index_missing {
        // 远端索引是完整树：已确认对象不在索引里就是远端删除。
        for path in known.keys() {
            if !remote_records.contains_key(path) {
                insert(path);
            }
        }
    }
    if run.delete_intent() {
        for path in known.keys().chain(local_manifest_records.keys()) {
            insert(path);
        }
        for entry in crate::sync::scanner::scan_for_sync_group(
            live_root,
            run.scope(),
            group.as_ref(),
        )? {
            insert(&entry.relative_path);
        }
    }

    // 最终远端索引只从固定 HEAD 的远端记录出发，再叠加本轮真实决议；
    // 不从本地清单反推远端状态（复审 #7239824124 第 1 项）。
    let mut final_records: BTreeMap<String, ManifestFileRecord> = remote_records
        .iter()
        .map(|(path, record)| (path.clone(), to_file_record(record)))
        .collect();
    final_records.retain(|path, _| !is_engine_path(path, group.as_ref()));

    let delete_intent = run.delete_intent();
    for path in paths {
        let path = crate::sync::path::ValidatedSyncPath::new(&path)?
            .as_str()
            .to_string();
        let rel = PathBuf::from(&path);
        let incoming_bytes = read_optional(staging_root.join(&rel))?;
        let remote_record = remote_records.get(&path);
        let pending_record = pending.get(&path);

        // 本地版本：删除意图 > 账本事实 > 磁盘当前内容 > 显式删除。
        let local_side = if delete_intent {
            let full = live_root.join(&rel);
            if full.is_file() {
                ObjectSide::Present(crate::sync::hash::content_md5(&std::fs::read(&full)?))
            } else {
                ObjectSide::Deleted
            }
        } else if let Some(record) = pending_record {
            if record.is_delete() {
                ObjectSide::Deleted
            } else if record.content_hash.is_empty() {
                ObjectSide::Absent
            } else {
                // 账本记录的是业务提交内容；磁盘被外部改写时以磁盘为准，
                // 但待发布对象必须真实存在，读取失败就是本轮读失败。
                let full = live_root.join(&rel);
                let bytes = std::fs::read(&full).map_err(|error| {
                    crate::Error::Other(format!(
                        "pending object {} cannot be read: {error}",
                        full.display()
                    ))
                })?;
                ObjectSide::Present(crate::sync::hash::content_md5(&bytes))
            }
        } else {
            local_side_from_disk(live_root, &rel, &tombstones, &deleted_facts)?
        };

        // 远端版本：索引记录就是固定 HEAD 的版本；完整树里缺少的已确认
        // 对象是远端删除，从未同步过的对象才算"远端没有事实"。
        let remote_side = match remote_record {
            Some(record) if record.op == "delete" => ObjectSide::Deleted,
            Some(record) if !record.content_hash.is_empty() => {
                ObjectSide::Present(record.content_hash.clone())
            }
            Some(_) => ObjectSide::Absent,
            None if known.contains_key(&path) => ObjectSide::Deleted,
            None => ObjectSide::Absent,
        };

        // 共同基线只取已确认版本；tombstone 是本地待发布删除，不是基线。
        let base_side = if let Some(hash) = known.get(&path) {
            ObjectSide::Present(hash.clone())
        } else if deleted_facts.contains(&path) {
            ObjectSide::Deleted
        } else {
            ObjectSide::Absent
        };

        // 旧冲突遗留的"下一轮取远端"：远端版本在传输阶段已强制下载，
        // 本轮直接采用远端；远端已删除时同样应用删除。
        let verdict = if pending_take_remote.contains(&path) {
            match &remote_side {
                ObjectSide::Present(_) => ObjectVerdict::ApplyRemoteUpsert,
                ObjectSide::Deleted => ObjectVerdict::ApplyRemoteDelete,
                ObjectSide::Absent => compare_object(&base_side, &local_side, &remote_side),
            }
        } else {
            compare_object(&base_side, &local_side, &remote_side)
        };
        apply_verdict(
            &mut plan,
            &mut final_records,
            &state,
            &path,
            &rel,
            &verdict,
            live_root,
            &local_side,
            &known,
            pending_record,
            remote_record,
            incoming_bytes.as_deref(),
        )?;
    }

    // 6. 最终对象索引：只写与本机事实不同的内容寻址对象/索引/根，
    //    与正文、state 在同一个 SaveTransaction 里原子提交。
    let manifest_rel = manifest_rel(group.as_ref());
    let manifest = SyncManifest {
        files: final_records.into_values().collect(),
    };
    plan.remote_manifest = Some(manifest.clone());
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

/// 从磁盘判定本地版本：文件存在就是当前内容，缺失只有显式删除事实才是删除。
fn local_side_from_disk(
    live_root: &Path,
    rel: &Path,
    tombstones: &HashSet<String>,
    deleted_facts: &HashSet<String>,
) -> Result<ObjectSide> {
    let full = live_root.join(rel);
    if full.is_file() {
        let bytes = std::fs::read(&full)?;
        return Ok(ObjectSide::Present(crate::sync::hash::content_md5(&bytes)));
    }
    let path = rel.to_string_lossy().replace('\\', "/");
    if tombstones.contains(&path) || deleted_facts.contains(&path) {
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
    live_root: &Path,
    local_side: &ObjectSide,
    known: &HashMap<String, String>,
    pending: Option<&crate::sync::local_mutations::LocalMutationRecord>,
    remote_meta: Option<&ManifestRecord>,
    incoming_bytes: Option<&[u8]>,
) -> Result<()> {
    match verdict {
        ObjectVerdict::NoOp => {
            plan.noop.push(rel.to_path_buf());
            match remote_meta {
                Some(record) => {
                    final_records.insert(path.to_string(), to_file_record(record));
                    // 双方内容已一致但基线还停在旧版本：本轮直接确认。
                    if record.op != "delete" && known.get(path) != Some(&record.content_hash) {
                        plan.confirmations.push(BaselineUpdate {
                            rel_path: rel.to_path_buf(),
                            content_hash: Some(record.content_hash.clone()),
                        });
                    }
                }
                // 远端索引里没有该对象时不能凭空声明它存在：只有本轮真正
                // 上传/应用的对象才进入远端清单。
                None => {}
            }
        }
        ObjectVerdict::Unparticipated => {
            plan.noop.push(rel.to_path_buf());
        }
        ObjectVerdict::PublishUpsert => {
            let ObjectSide::Present(_) = local_side else {
                return Err(crate::Error::Other(format!(
                    "publish upsert without local content for {}",
                    rel.display()
                )));
            };
            let full = live_root.join(rel);
            let content = std::fs::read(&full).map_err(|error| {
                crate::Error::Other(format!(
                    "local object {} cannot be read: {error}",
                    full.display()
                ))
            })?;
            let final_hash = crate::sync::hash::content_md5(&content);
            plan.uploads.push(PublishObject {
                rel_path: rel.to_path_buf(),
                content,
                base_hash: known.get(path).cloned(),
                final_hash: final_hash.clone(),
                mutation_seq: pending.map(|record| record.seq).unwrap_or(0),
                transaction_id: pending
                    .map(|record| record.transaction_id.clone())
                    .unwrap_or_default(),
            });
            final_records.insert(path.to_string(), upsert_record(state, path, &final_hash));
        }
        ObjectVerdict::PublishDelete => {
            plan.publish_deletes.push(rel.to_path_buf());
            final_records.insert(path.to_string(), delete_record(state, path));
        }
        ObjectVerdict::ApplyRemoteUpsert => {
            let record = remote_meta
                .filter(|record| record.op != "delete")
                .ok_or_else(|| crate::Error::Other(format!(
                    "remote index lost upsert record for {path}"
                )))?;
            let bytes = incoming_bytes.ok_or_else(|| {
                crate::Error::Other(format!(
                    "remote object blob missing for {path}; fixed HEAD content was not downloaded"
                ))
            })?;
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
            // 只有本地确实存在对象时才需要落盘删除；远端删除事实本身仍然确认。
            if !matches!(local_side, ObjectSide::Absent) {
                plan.content_actions.push(CommitAction::Delete {
                    rel_path: rel.to_path_buf(),
                });
            }
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
                local_hash: local_side.hash().unwrap_or_default().to_string(),
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
            match remote_meta {
                Some(record) => {
                    final_records.insert(path.to_string(), to_file_record(record));
                }
                // 远端已删除、本地另有修改：远端索引保持"没有该对象"，
                // 不能拿本地旧清单把已删除对象重新写回远端。
                None => {
                    final_records.remove(path);
                }
            }
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
