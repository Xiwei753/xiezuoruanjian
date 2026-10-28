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
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
    if provider.capabilities().batch {
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

    let snapshot = match merge_outcome {
        Some(outcome) => outcome.merged_manifest.clone(),
        None => match read_staging_generation_manifest(sync_root, app_group) {
            Ok(manifest) => manifest,
            Err(error) => return super::transfer_helpers::sync_result_from_error(error),
        },
    };
    if let Err(error) = upload_complete_tree_snapshot(
        provider,
        sync_root,
        tree_prefix,
        &snapshot,
        scope,
        app_group,
        cancellation_token,
    ) {
        return super::transfer_helpers::sync_result_from_error(error);
    }
    let mut result = crate::sync::types::SyncResult::success();
    if let Some(outcome) = merge_outcome {
        result.uploaded_files = outcome.remote_upload_paths.clone();
        result.downloaded_files = outcome.downloaded_files.clone();
        result.local_deletes = outcome.remote_delete_paths.clone();
        result.remote_deletes = outcome.local_deletes.clone();
        result.overwritten_files = outcome.overwritten_files.clone();
        result.ignored_files = outcome.ignored_files.clone();
    }
    if result.uploaded_files.is_empty()
        && result.downloaded_files.is_empty()
        && result.local_deletes.is_empty()
        && result.remote_deletes.is_empty()
    {
        result.status = SyncStatus::NoChanges;
    } else {
        result.status = SyncStatus::LatestWinsApplied;
    }
    result
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

    // manifest.sync.json 作为 Put 放进同一 batch。
    let manifest_remote_path = format!(
        "{generation_prefix}/{}",
        crate::sync::lww::SYNC_MANIFEST_PATH
    );
    let manifest_json = match serde_json::to_string(&manifest) {
        Ok(s) => s,
        Err(e) => {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Json(e));
        }
    };
    mutations.push(BatchMutation::Put {
        path: manifest_remote_path,
        content: manifest_json.into_bytes(),
    });

    // Remove paths that existed in the previous stable tree but are absent
    // from the merged snapshot, including old generation directories left by
    // the migration.
    let desired: std::collections::HashSet<String> = mutations
        .iter()
        .map(|mutation| mutation.path().to_string())
        .collect();
    let existing = match provider.list(generation_prefix) {
        Ok(entries) => entries,
        Err(error) => return super::transfer_helpers::sync_result_from_provider_error(error),
    };
    for entry in existing {
        let path = format!("{generation_prefix}/{}", entry.path);
        if !desired.contains(&path) {
            mutations.push(BatchMutation::Delete { path });
        }
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
    let commit_message = format!("sujian: publish tree {generation_prefix}");
    if let Err(error) = provider.commit_batch(&mutations, &commit_message) {
        return super::transfer_helpers::sync_result_from_provider_error(error);
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
    if r.uploaded_files.is_empty()
        && r.downloaded_files.is_empty()
        && r.local_deletes.is_empty()
        && r.remote_deletes.is_empty()
    {
        r.status = SyncStatus::NoChanges;
    } else {
        r.status = SyncStatus::LatestWinsApplied;
    }
    r
}

/// 读取 staging 的本地 `manifest.sync.json`（无 merge_outcome 的首次同步用）。
///
/// 首次同步 / 远端无 visible generation 时没有 merge 快照可用，generation 的完整
/// 文件集合来自 staging 自己的 manifest（prepare 阶段由本地扫描 + tombstone 生成）。
fn read_staging_generation_manifest(
    sync_root: &Path,
    app_group: Option<&crate::sync::types::AppDataGroup>,
) -> crate::error::Result<crate::sync::types::SyncManifest> {
    let manifest_rel_path = app_group
        .map(crate::sync::types::AppDataGroup::local_manifest_path)
        .unwrap_or_else(|| crate::sync::lww::SYNC_MANIFEST_PATH.to_string());
    let manifest_path = sync_root.join(manifest_rel_path);
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

/// 把完整 merged manifest 快照上传到新 generation prefix。
///
/// 新 generation 是不可变完整快照。本函数遍历 `merged_manifest.files`，
/// 对每个 `op=upsert` 的 record：
/// 1. 路径必须仍在同步白名单（`SyncService::is_whitelisted_path`）。
/// 2. `staging_root/path` 必须存在。
/// 3. 重新算 md5 hash，必须 == `record.content_hash`（不一致则 `Err`）。
/// 4. 上传到新 generation prefix（`CreateNew` 或 `Unconditional`）。
///
/// `op=delete` 的 record 只保留 tombstone（manifest 里有记录），不上传物理文件。
///
/// 上传规则保证新 generation 的 manifest 与实际对象一致：manifest 声称存在的
/// 文件对象一定已上传到 generation prefix。原先 `upload_merged_outcome_to_generation`
/// 只上传 `remote_upload_paths`（delta 动作），NoOp/DownloadRemote/
/// LwwRemoteWinsDownload/pending_take_remote 成功下载的文件都不会进新 generation，
/// 导致 catalog 指向一个 manifest 声称文件存在但实际对象不存在的 generation。
///
/// Issue #761：本函数仅在 `capabilities().batch == false` 时由
/// [`publish_tree_snapshot`] 调用（降级路径）。`batch == true` 时走
/// [`publish_snapshot_batch`]。
fn upload_complete_tree_snapshot(
    provider: &dyn crate::sync::provider::SyncProvider,
    staging_root: &Path,
    generation_prefix: &str,
    merged_manifest: &crate::sync::types::SyncManifest,
    scope: crate::sync::types::SyncScope,
    app_group: Option<&crate::sync::types::AppDataGroup>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::error::Result<()> {
    use crate::sync::provider::model::WritePrecondition;

    // 遍历 merged_manifest.files，上传所有 upsert 文件到新 generation prefix。
    for record in &merged_manifest.files {
        // op=delete 只保留 tombstone（manifest 里有记录），不上传物理文件。
        if record.op == "delete" {
            continue;
        }
        if record.op != "upsert" {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_tree_snapshot: unknown op={} for path={}",
                record.op, record.path
            ))));
        }

        // 1. 路径白名单检查。
        let whitelisted = app_group.map_or_else(
            || crate::sync::SyncService::is_whitelisted_path(&record.path, scope),
            |group| crate::sync::SyncService::is_whitelisted_app_group_path(&record.path, group),
        );
        if !whitelisted {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_tree_snapshot: path {} not whitelisted for scope {:?}",
                record.path, scope
            ))));
        }

        // 2. staging 文件必须存在。
        let local_full = staging_root.join(&record.path);
        if !local_full.exists() {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_tree_snapshot: staging file missing for path={}",
                record.path
            ))));
        }

        // 3. 读取 staging 文件，计算 md5 hash，必须 == record.content_hash。
        let content = std::fs::read(&local_full).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_tree_snapshot: read {}: {e}",
                record.path
            )))
        })?;
        let actual_hash = crate::sync::hash::content_md5(&content);
        if actual_hash != record.content_hash {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_tree_snapshot: hash mismatch for path={} expected={} actual={}",
                record.path, record.content_hash, actual_hash
            ))));
        }

        // 4. 上传到新 generation prefix。
        let remote_path = format!("{generation_prefix}/{}", record.path);
        let precondition = WritePrecondition::Unconditional;
        provider.write(&remote_path, &content, precondition)?;

        // Issue #729：每次 provider.write 返回后检查取消令牌。
        // 取消则立即停止上传，返回 cancelled 错误。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] upload_complete_tree_snapshot: cancellation requested after write {} — returning cancelled",
                    record.path
                );
                return Err(crate::Error::Other(
                    "sync cancelled during generation upload".into(),
                ));
            }
        }
    }

    // 上传 manifest 到 generation prefix。
    let manifest_remote_path = format!(
        "{generation_prefix}/{}",
        crate::sync::lww::SYNC_MANIFEST_PATH
    );
    let manifest_json = serde_json::to_string(merged_manifest).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "upload_complete_tree_snapshot: serialize manifest: {e}"
        )))
    })?;
    let manifest_precondition = WritePrecondition::Unconditional;
    provider.write(
        &manifest_remote_path,
        manifest_json.as_bytes(),
        manifest_precondition,
    )?;

    {
        let mut desired: std::collections::HashSet<String> = merged_manifest
            .files
            .iter()
            .filter(|record| record.op == "upsert")
            .map(|record| format!("{generation_prefix}/{}", record.path))
            .collect();
        desired.insert(manifest_remote_path);
        for entry in provider.list(generation_prefix)? {
            let path = format!("{generation_prefix}/{}", entry.path);
            if !desired.contains(&path) {
                provider.delete(
                    &path,
                    crate::sync::provider::model::DeletePrecondition::Unconditional,
                )?;
            }
        }
    }

    Ok(())
}

/// 把单个远端对象内容写入 staging（创建父目录 + 写文件）。
pub(super) fn write_staging_file(staging: &Path, rel: &str, content: &[u8]) -> std::io::Result<()> {
    let dest = staging.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, content)
}
