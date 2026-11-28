//! Stable tree snapshot publication and legacy generation helpers.
//!
//! Issue #1012: the remote snapshot is the Git tree at a stable data path
//! (`projects/P` or `app/__groups__/<key>`), not a per-run generation
//! directory. `publish_tree_snapshot` replaces that tree in the round's single
//! commit. The `__generations__` helpers remain for reading and migrating old
//! repositories.

use std::path::Path;

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::types::SyncPolicy;
use crate::sync::SyncStatus;

// ── generation 原子发布 helpers ──

/// generation 原子发布 — 不可见 generation prefix 的子目录名。
///
/// LiveProject 先把完整 Project 上传到 `projects/P/__generations__/G/`，
/// CAS `targets.sync.json` 成功后才成为可见版本。`delete_all_remote_objects`
/// 跳过此子目录，不碰并发 Upsert 正在上传的 generation。
pub(super) const GENERATION_SUBDIR: &str = "__generations__";

/// 构造 generation 不可见 prefix。
///
/// `projects/P` + `G` → `projects/P/__generations__/G`。
///
///   防御性校验 `generation_id` 是合法单 path segment，
/// 不只依赖 catalog loader。非法 `generation_id`（空、`.`、`..`、含 `/`/`\`）→ `Err`。
pub(super) fn generation_remote_prefix(
    project_remote_prefix: &str,
    generation_id: &str,
) -> crate::error::Result<String> {
    crate::sync::target_lifecycle::validate_generation_id(generation_id)?;
    Ok(format!(
        "{}/{}/{}",
        project_remote_prefix, GENERATION_SUBDIR, generation_id
    ))
}

/// 判断远端相对路径是否落在 generation 不可见 prefix 下。
///
/// `delete_all_remote_objects` 用此跳过 `__generations__/` 下的对象，不碰并发
/// Upsert 正在上传的 generation。
pub(super) fn is_generation_path(rel_path: &str) -> bool {
    rel_path == GENERATION_SUBDIR || rel_path.starts_with(&format!("{}/", GENERATION_SUBDIR))
}

/// Publish a complete, stable tree snapshot at `tree_prefix`.
///
/// This is the Issue #1012 publication path: the remote tree at a stable path
/// (for example `projects/P` or `app/__groups__/<key>`) is replaced by the
/// merged manifest in the same commit that updates the catalog.  No generation
/// UUID, no `generation.meta.json`, and no per-target push.
///
/// The snapshot and its v2 manifest root must land in one atomic commit.
/// Providers without `batch + atomic_write` are rejected explicitly instead of
/// falling back to a per-file publish that leaves content and root out of sync.
#[allow(clippy::too_many_arguments)]
pub(super) fn publish_tree_snapshot(
    provider: &dyn crate::sync::provider::SyncProvider,
    sync_root: &Path,
    tree_prefix: &str,
    scope: crate::sync::types::SyncScope,
    app_group: Option<&crate::sync::types::AppDataGroup>,
    _sync_policy: &SyncPolicy,
    _force_sync: bool,
    merge_outcome: Option<&crate::sync::lww::LwwMergeOutcome>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::sync::types::SyncResult {
    let capabilities = provider.capabilities();
    if capabilities.batch && capabilities.atomic_write {
        return publish_snapshot_batch(
            provider,
            sync_root,
            tree_prefix,
            scope,
            app_group,
            merge_outcome,
            cancellation_token,
        );
    }

    // 没有原子多对象提交能力时不再保留逐文件发布兜底：正文与清单根分开写
    // 会留下"正文已变、根未变"的中间版本。显式拒绝，不执行任何 provider 写。
    log::warn!(
        "[sync] publish_tree_snapshot: refusing non-atomic publish for {tree_prefix} \
         (batch={}, atomic_write={})",
        capabilities.batch,
        capabilities.atomic_write
    );
    super::transfer_helpers::sync_result_from_error(crate::Error::Other(
        "provider lacks atomic multi-object publish; refusing to publish content and manifest \
         separately"
            .to_string(),
    ))
}

/// Build one atomic batch that replaces the complete tree at `generation_prefix`.
///
/// The manifest comes from the merged LWW outcome, or from staging for a first
/// sync. Unchanged remote files are referenced with `ReuseVersion`; locally
/// changed files are read from staging and emitted as `Put`. Paths that existed
/// in the previous stable tree but are absent from the merged manifest are
/// deleted, so the commit is a complete tree replacement rather than a delta on
/// top of stale objects.
///
/// The batch includes the manifest and is committed once. The caller's provider
/// round buffer defers the actual commit/push until all targets are complete.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
fn publish_snapshot_batch(
    provider: &dyn crate::sync::provider::SyncProvider,
    sync_root: &Path,
    generation_prefix: &str,
    scope: crate::sync::types::SyncScope,
    app_group: Option<&crate::sync::types::AppDataGroup>,
    merge_outcome: Option<&crate::sync::lww::LwwMergeOutcome>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::sync::types::SyncResult {
    use crate::sync::provider::model::{BatchMutation, RemoteVersion};

    // 取消令牌检查（与逐文件路径对齐）。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during tree publish".into(),
            ));
        }
    }

    // 稀疏轮只发布 affected：不加载完整 manifest、不 list 全树，
    // 业务 Put/Delete 与清单变化 objects/indexes/root 一次提交。
    // 冲突或 delta 视图不可用时返回 None，回退到下面的完整入口。
    if let Some(result) = try_publish_sparse_batch(
        provider,
        sync_root,
        generation_prefix,
        app_group,
        merge_outcome,
        cancellation_token,
    ) {
        return result;
    }

    // 本次 tree 的完整 manifest：merge 后的快照，或首次同步时 staging 的本地 manifest。
    let manifest: crate::sync::types::SyncManifest = match merge_outcome {
        Some(outcome) => outcome.merged_manifest.clone(),
        None => match read_staging_generation_manifest(sync_root, app_group) {
            Ok(manifest) => manifest,
            Err(e) => return super::transfer_helpers::sync_result_from_error(e),
        },
    };

    let remote_upload_paths: std::collections::HashSet<&str> = merge_outcome
        .map(|outcome| {
            outcome
                .remote_upload_paths
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();

    // 固定基线的远端 tree：用来消除同内容 Put / 同 OID ReuseVersion /
    // 不存在的 Delete。只有实际差异才进入 batch；全无差异时根本不发布。
    let existing = match provider.list(generation_prefix) {
        Ok(entries) => entries
            .into_iter()
            .map(|entry| (format!("{generation_prefix}/{}", entry.path), entry.version))
            .collect::<std::collections::HashMap<_, _>>(),
        Err(error) => return super::transfer_helpers::sync_result_from_provider_error(error),
    };

    let mut mutations: Vec<BatchMutation> = Vec::new();
    let mut uploaded_files: Vec<String> = Vec::new();

    for record in &manifest.files {
        // op=delete 只保留 tombstone（manifest 里有记录），不上传物理文件。
        if record.op == "delete" {
            continue;
        }
        if record.op != "upsert" {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Io(
                std::io::Error::other(format!(
                    "publish_snapshot_batch: unknown op={} for path={}",
                    record.op, record.path
                )),
            ));
        }

        // 路径白名单检查。
        let whitelisted = app_group.map_or_else(
            || crate::sync::SyncService::is_whitelisted_path(&record.path, scope),
            |group| crate::sync::SyncService::is_whitelisted_app_group_path(&record.path, group),
        );
        if !whitelisted {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Io(
                std::io::Error::other(format!(
                    "publish_snapshot_batch: path {} not whitelisted for scope {:?}",
                    record.path, scope
                )),
            ));
        }

        let remote_path = format!("{generation_prefix}/{}", record.path);
        let needs_upload = remote_upload_paths.contains(record.path.as_str());
        let reusable_blob =
            merge_outcome.and_then(|outcome| outcome.remote_tree_files.get(&record.path));

        if !needs_upload {
            if let Some(blob_sha) = reusable_blob {
                // 无本地修改，复用旧 visible generation 的 blob → ReuseVersion。
                // 冲突路径也走这里：用远端 blob SHA，不读本地冲突正文。
                // 远端该路径已经是同一 blob 时不再重复写入 batch。
                if existing.get(&remote_path).map(RemoteVersion::as_str) == Some(blob_sha.as_str())
                {
                    continue;
                }
                mutations.push(BatchMutation::ReuseVersion {
                    path: remote_path,
                    version: RemoteVersion(blob_sha.clone()),
                });
                continue;
            }
            if let Some(outcome) = merge_outcome {
                // Issue #761 Part 4：unresolved BothChanged conflict 且远端无 blob 可复用。
                // 不生成内容不完整/哈希不一致的 generation，整个 target 返回 PartialConflict。
                log::warn!(
                    "[sync] publish_snapshot_batch: unresolved conflict path={} has no remote blob to reuse — returning PartialConflict",
                    record.path
                );
                let mut r = crate::sync::types::SyncResult::success();
                r.status = SyncStatus::PartialConflict;
                r.error = Some(format!(
                    "unresolved conflict path {} has no remote blob to reuse; \
                     generation publish skipped to avoid publishing incomplete content",
                    record.path
                ));
                r.conflicts = outcome.conflicts.clone();
                r.downloaded_files = outcome.downloaded_files.clone();
                r.remote_deletes = outcome.local_deletes.clone();
                r.overwritten_files = outcome.overwritten_files.clone();
                r.ignored_files = outcome.ignored_files.clone();
                return r;
            }
            // merge_outcome == None（首次同步 / 远端无 visible generation）：
            // 没有旧 blob 可复用，落到下面的 Put 分支上传完整内容。
        }

        // 本地有修改需上传（或首次同步的所有 upsert）→ Put。
        match build_put_mutation(sync_root, generation_prefix, record) {
            Ok(mutation) => {
                mutations.push(mutation);
                uploaded_files.push(record.path.clone());
            }
            Err(e) => return super::transfer_helpers::sync_result_from_error(e),
        }
    }

    // 清单权威是 v2 存储：内容寻址对象先写，索引次之，根最后（根是提交点）。
    // 远端语义未变化时不重写清单，避免 generation 串号制造空提交。
    //
    // 稀疏轮（merge 已持久化 `ManifestDelta`）只发布 delta 涉及的
    // objects/indexes 与根，不再用 `plan_write_all` 构造完整可达集合；
    // 首次同步/迁移/索引损坏（delta 为 None 或本地无 root）仍走完整入口。
    use crate::sync::lww::{ManifestStorageKind, ManifestStore};
    let manifest_store = ManifestStore::new(sync_root, &manifest_rel_path(app_group));
    let delta_manifest_files =
        match merge_outcome.and_then(|outcome| outcome.manifest_delta.as_ref()) {
            Some(delta) => {
                match crate::sync::lww::local_manifest_storage_for_delta(&manifest_store, delta) {
                    Ok(files) => Some(files),
                    Err(error) => {
                        log::warn!(
                            "[sync] publish_snapshot_batch: delta manifest view unavailable for \
                             {generation_prefix}: {error} — falling back to the full manifest"
                        );
                        None
                    }
                }
            }
            None => None,
        };
    // 只有 delta 视图真正可用时才按增量期望树处理；回退完整清单时
    // desired/删除语义也一并回到完整入口，保持旧行为不变。
    let delta_mode = delta_manifest_files.is_some();
    let manifest_files = match delta_manifest_files {
        Some(files) => files,
        None => match crate::sync::lww::local_manifest_storage(&manifest_store, Some(&manifest)) {
            Ok(files) => files,
            Err(error) => return super::transfer_helpers::sync_result_from_error(error),
        },
    };
    let mut manifest_remote_paths: Vec<String> = Vec::with_capacity(manifest_files.len());
    let mut root_mutation: Option<BatchMutation> = None;
    for file in &manifest_files {
        let remote_rel = match crate::sync::lww::remote_rel_for(&manifest_store, &file.rel) {
            Ok(rel) => rel,
            Err(error) => return super::transfer_helpers::sync_result_from_error(error),
        };
        let remote_path = format!("{generation_prefix}/{remote_rel}");
        manifest_remote_paths.push(remote_path.clone());
        match file.kind {
            // 内容寻址对象：远端已有同名对象直接复用，不再上传正文。
            ManifestStorageKind::Object => {
                if existing.contains_key(&remote_path) {
                    continue;
                }
            }
            // 索引/根：只在远端字节不同时才写；根必须是 mutations 的最后一项。
            ManifestStorageKind::Index | ManifestStorageKind::Root => {
                let bytes = match file.read_bytes(sync_root) {
                    Ok(bytes) => bytes,
                    Err(error) => return super::transfer_helpers::sync_result_from_error(error),
                };
                let unchanged = match provider.read(&remote_path) {
                    Ok(Some(object)) => object.content == bytes,
                    Ok(None) => false,
                    Err(error) => {
                        return super::transfer_helpers::sync_result_from_provider_error(error)
                    }
                };
                if unchanged {
                    continue;
                }
                let mutation = BatchMutation::Put {
                    path: remote_path,
                    content: bytes,
                };
                if file.kind == ManifestStorageKind::Root {
                    root_mutation = Some(mutation);
                } else {
                    mutations.push(mutation);
                }
                continue;
            }
        }
        let bytes = match file.read_bytes(sync_root) {
            Ok(bytes) => bytes,
            Err(error) => return super::transfer_helpers::sync_result_from_error(error),
        };
        mutations.push(BatchMutation::Put {
            path: remote_path,
            content: bytes,
        });
    }

    // Remove paths that existed in the previous stable tree but are absent
    // from the merged snapshot, including old generation directories left by
    // the migration.
    //
    // 完整模式：期望树来自"合并后的 manifest"，不是"本轮 mutation 列表"：
    // 没有进入 batch 的路径同样属于期望树（它们已在基线 tree 里，只是不需要
    // 重写）。delta 模式：不遍历完整 manifest，baseline tree 中除本轮已确认
    // 的远端删除（`remote_delete_paths`）外全部保留；未变化业务路径不产生
    // Delete，旧清单对象/索引也保留给上一代根。
    let mut desired: std::collections::HashSet<String> = if delta_mode {
        let mut desired: std::collections::HashSet<String> = existing.keys().cloned().collect();
        if let Some(outcome) = merge_outcome {
            for rel in &outcome.remote_delete_paths {
                desired.remove(&format!("{generation_prefix}/{rel}"));
            }
        }
        desired
    } else {
        manifest
            .files
            .iter()
            .filter(|record| record.op != "delete")
            .map(|record| format!("{generation_prefix}/{}", record.path))
            .collect()
    };
    if delta_mode {
        // 本轮真正 Put/Reuse 的业务路径（新增路径不在 baseline tree 里）必须
        // 留在期望树中；删除路径不重新加入。
        for mutation in &mutations {
            match mutation {
                BatchMutation::Put { path, .. } | BatchMutation::ReuseVersion { path, .. } => {
                    desired.insert(path.clone());
                }
                BatchMutation::Delete { .. } | BatchMutation::DeleteIfMatch { .. } => {}
                BatchMutation::CheckObject { .. } | BatchMutation::CheckPrefix { .. } => {}
            }
        }
    }
    desired.extend(manifest_remote_paths);
    for path in existing.keys() {
        if !desired.contains(path) {
            mutations.push(BatchMutation::Delete { path: path.clone() });
        }
    }
    // 根是提交点：所有业务 Put/Reuse 与旧路径 Delete 之后才写根。
    if let Some(root) = root_mutation {
        mutations.push(root);
    }

    // 取消令牌检查（commit_batch 前）。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during tree publish".into(),
            ));
        }
    }

    // PreconditionFailed 表示远端在本轮规划后又发生了变化。不能在这里重放
    // 同一批 mutations：provider 可能已重新 fetch 到新 HEAD，重放会绕过 LWW。
    // 将错误交给完整同步入口，由它重新 Prepare → 读取 catalog → LWW → 生成新 batch。
    // 整批都被判定为 no-op 时连只读前置条件都不再需要：固定基线的 tree 已经
    // 就是目标 tree，直接返回，不创建提交。
    if !mutations.is_empty() {
        let commit_message = format!("sujian: publish tree {generation_prefix}");
        if let Err(error) = provider.commit_batch(&mutations, &commit_message) {
            return super::transfer_helpers::sync_result_from_provider_error(error);
        }
    }
    let mut r = crate::sync::types::SyncResult::success();
    r.uploaded_files = uploaded_files;
    if let Some(outcome) = merge_outcome {
        r.downloaded_files = outcome.downloaded_files.clone();
        r.local_deletes = outcome.remote_delete_paths.clone();
        r.remote_deletes = outcome.local_deletes.clone();
        r.overwritten_files = outcome.overwritten_files.clone();
        r.ignored_files = outcome.ignored_files.clone();
    }
    let auto_adjudicated =
        merge_outcome.is_some_and(|outcome| !outcome.auto_adjudicated_files.is_empty());
    if r.uploaded_files.is_empty()
        && r.downloaded_files.is_empty()
        && r.local_deletes.is_empty()
        && r.remote_deletes.is_empty()
    {
        r.status = SyncStatus::NoChanges;
    } else if auto_adjudicated {
        r.status = SyncStatus::LatestWinsApplied;
    } else {
        r.status = SyncStatus::Success;
    }
    r
}

/// 本地清单相对路径：App group 用自己的分组清单，其它 target 用聚合清单。
fn manifest_rel_path(app_group: Option<&crate::sync::types::AppDataGroup>) -> String {
    app_group
        .map(crate::sync::types::AppDataGroup::local_manifest_path)
        .unwrap_or_else(|| crate::sync::lww::SYNC_MANIFEST_PATH.to_string())
}

/// 读取 staging 的本地权威清单（无 merge_outcome 的首次同步用）。
///
/// 首次同步 / 远端无 visible generation 时没有 merge 快照可用，generation 的完整
/// 文件集合来自 staging 自己的清单：优先读 staging 的 v2 存储（root + 两级索引 +
/// 内容寻址对象，严格校验），root 不存在时回退 prepare 物化的旧单文件只读视图。
fn read_staging_generation_manifest(
    sync_root: &Path,
    app_group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::error::Result<crate::sync::types::SyncManifest> {
    let manifest_rel = manifest_rel_path(app_group);
    let store = crate::sync::lww::ManifestStore::new(sync_root, &manifest_rel);
    if let Some(manifest) = store.load_all()? {
        return Ok(manifest);
    }
    let manifest_path = sync_root.join(&manifest_rel);
    let bytes = std::fs::read(&manifest_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "publish_snapshot_batch: read local manifest {}: {e}",
            manifest_path.display()
        )))
    })?;
    serde_json::from_slice(&bytes).map_err(crate::Error::Json)
}

/// 读取 staging 文件并构造 `BatchMutation::Put`（校验 md5 与 manifest record 一致）。
///
/// 读不到文件或哈希不一致时返回 `Err`：宁可不发布，也不发布内容与 manifest 不符的
/// generation。
/// 稀疏发布：只处理 `affected_paths` + 本轮确认的删除 + 清单 delta。
///
/// 返回 `None` 表示不适用（无 delta、delta 视图不可用、存在冲突需要完整入口
/// 的 PartialConflict 语义等），调用方回退完整发布。不调用 `provider.list`，
/// 不遍历完整 `manifest.files`，不受远端对象总数影响。
fn try_publish_sparse_batch(
    provider: &dyn crate::sync::provider::SyncProvider,
    sync_root: &Path,
    generation_prefix: &str,
    app_group: Option<&crate::sync::types::AppDataGroup>,
    merge_outcome: Option<&crate::sync::lww::LwwMergeOutcome>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> Option<crate::sync::types::SyncResult> {
    use crate::sync::provider::model::{BatchMutation, RemoteVersion};

    let outcome = merge_outcome?;
    let delta = outcome.manifest_delta.as_ref()?;
    // 冲突路径需要完整入口的 PartialConflict 判定（不能发布不完整树）。
    if !outcome.conflicts.is_empty() {
        return None;
    }
    let store = crate::sync::lww::ManifestStore::new(sync_root, &manifest_rel_path(app_group));
    let manifest_files = match crate::sync::lww::local_manifest_storage_for_delta(&store, delta) {
        Ok(files) => files,
        Err(error) => {
            log::warn!(
                "[sync] sparse publish: delta manifest view unavailable for {generation_prefix}:                  {error} — falling back to the full publish path"
            );
            return None;
        }
    };

    let mut mutations: Vec<BatchMutation> = Vec::new();
    let mut uploaded_files: Vec<String> = Vec::new();
    for path in &outcome.remote_upload_paths {
        let Some(record) = delta.upserts.get(path) else {
            // delta 里没有该业务记录说明本轮记录不完整，交回完整入口。
            return None;
        };
        match build_put_mutation(sync_root, generation_prefix, record) {
            Ok(mutation) => {
                mutations.push(mutation);
                uploaded_files.push(path.clone());
            }
            Err(error) => {
                return Some(super::transfer_helpers::sync_result_from_error(error));
            }
        }
    }
    for path in &outcome.remote_delete_paths {
        let remote_path = format!("{generation_prefix}/{path}");
        let mutation = match outcome.remote_tree_files.get(path) {
            Some(version) => BatchMutation::DeleteIfMatch {
                path: remote_path,
                version: RemoteVersion(version.clone()),
            },
            None => BatchMutation::Delete { path: remote_path },
        };
        mutations.push(mutation);
    }

    let mut root_mutation: Option<BatchMutation> = None;
    for file in &manifest_files {
        let remote_rel = match crate::sync::lww::remote_rel_for(&store, &file.rel) {
            Ok(rel) => rel,
            Err(error) => return Some(super::transfer_helpers::sync_result_from_error(error)),
        };
        let remote_path = format!("{generation_prefix}/{remote_rel}");
        let bytes = match file.read_bytes(sync_root) {
            Ok(bytes) => bytes,
            Err(error) => return Some(super::transfer_helpers::sync_result_from_error(error)),
        };
        let mutation = BatchMutation::Put {
            path: remote_path,
            content: bytes,
        };
        if file.kind == crate::sync::lww::ManifestStorageKind::Root {
            root_mutation = Some(mutation);
        } else {
            mutations.push(mutation);
        }
    }
    // 旧单文件清单在稀疏轮只按远端已知版本删除；旧分片目录留给完整同步/GC。
    if let Some(version) = outcome
        .remote_tree_files
        .get(crate::sync::lww::SYNC_MANIFEST_PATH)
    {
        mutations.push(BatchMutation::DeleteIfMatch {
            path: format!(
                "{generation_prefix}/{}",
                crate::sync::lww::SYNC_MANIFEST_PATH
            ),
            version: RemoteVersion(version.clone()),
        });
    }
    if let Some(root) = root_mutation {
        mutations.push(root);
    }

    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Some(super::transfer_helpers::sync_result_from_error(
                crate::Error::Other("sync cancelled during sparse tree publish".into()),
            ));
        }
    }

    if !mutations.is_empty() {
        let message = format!("sujian: publish sparse tree {generation_prefix}");
        if let Err(error) = provider.commit_batch(&mutations, &message) {
            return Some(super::transfer_helpers::sync_result_from_provider_error(
                error,
            ));
        }
    }

    let mut result = crate::sync::types::SyncResult::success();
    result.uploaded_files = uploaded_files;
    result.downloaded_files = outcome.downloaded_files.clone();
    result.local_deletes = outcome.remote_delete_paths.clone();
    result.remote_deletes = outcome.local_deletes.clone();
    result.overwritten_files = outcome.overwritten_files.clone();
    result.ignored_files = outcome.ignored_files.clone();
    result.status = if mutations.is_empty() {
        SyncStatus::NoChanges
    } else {
        SyncStatus::Success
    };
    Some(result)
}

fn build_put_mutation(
    sync_root: &Path,
    generation_prefix: &str,
    record: &crate::sync::types::ManifestFileRecord,
) -> crate::error::Result<crate::sync::provider::model::BatchMutation> {
    let local_full = sync_root.join(&record.path);
    let content = std::fs::read(&local_full).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "publish_snapshot_batch: read {}: {e}",
            record.path
        )))
    })?;
    let actual_hash = crate::sync::hash::content_md5(&content);
    if actual_hash != record.content_hash {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "publish_snapshot_batch: hash mismatch for path={} expected={} actual={}",
            record.path, record.content_hash, actual_hash
        ))));
    }
    Ok(crate::sync::provider::model::BatchMutation::Put {
        path: format!("{generation_prefix}/{}", record.path),
        content,
    })
}

/// 把单个远端对象内容写入 staging（创建父目录 + 写文件）。
///
/// staging 条目可能与 live 共享 inode（seed 用硬链接），因此必须先 unlink
/// 再新建：就地截断会把 live 内容一起改掉。
pub(super) fn write_staging_file(staging: &Path, rel: &str, content: &[u8]) -> std::io::Result<()> {
    let dest = staging.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::symlink_metadata(&dest) {
        Ok(_) => std::fs::remove_file(&dest)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std::fs::write(&dest, content)
}
