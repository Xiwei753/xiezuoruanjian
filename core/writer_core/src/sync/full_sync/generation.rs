//! generation 原子发布 helpers。
//!
//! 包含 `GENERATION_SUBDIR` 常量、generation prefix 构造与路径判断、
//! generation 发布（upload + meta write）以及 staging 文件写入。
//!
//! Issue #761：当 `provider.capabilities().batch == true` 时走批量原子提交路径
//! （`commit_batch` 一次提交所有 mutation），否则降级为逐文件 `write()` 路径。

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

///   generation 原子发布 — 上传 staging 到新 generation prefix，
/// 写 `generation.meta.json`（complete=false → 内容 → complete=true）。
///
/// 步骤：
/// 1. 写 `meta(complete=false, lease_until=now+lease)` 到 `generation_prefix/generation.meta.json`。
/// 2. 上传 staging 内容到 generation prefix：
///    - 有 `merge_outcome`（已 merge）→ 直接用 outcome 的 upload paths / manifest
///      上传到新 generation prefix（不再做第二次 LWW 同步）；
///    - 无 `merge_outcome`（首次同步 / legacy）→ `run_single_target` 全量上传。
/// 3. 内容上传成功后写 `meta(complete=true)`。
///
/// meta 让 GC 能识别 incomplete generation（上传中，不删）和 complete generation
/// （可按保留期删）。`uploader_device_id` 用空字符串（诊断字段，不影响 GC 逻辑）。
///
/// Issue #761 评论 5828969186：`provider.capabilities().batch == true` 时**无条件**
/// 走 [`publish_generation_batch`] 批量原子提交路径（不再要求 `merge_outcome.is_some()`）。
/// 首次同步 / 远端没有 visible generation 时 `merge_outcome == None`，batch builder 从
/// staging 的本地 manifest 构造完整 generation，所有 upsert 都是 Put，delete 只留
/// manifest tombstone，manifest + meta(complete=true) 放进同一次 commit。
/// 逐文件 `run_single_target` 只在 `batch == false` 的 provider 上作为降级路径。
/// Git branch commit 本身就是一次可见，中间状态对其他设备不可见。
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // 10 个参数均为独立发布输入，打包会掩盖各自语义
pub(super) fn publish_generation(
    provider: &dyn crate::sync::provider::SyncProvider,
    sync_root: &Path,
    generation_prefix: &str,
    generation_id: &str,
    project_id: &str,
    scope: crate::sync::types::SyncScope,
    sync_policy: &SyncPolicy,
    force_sync: bool,
    merge_outcome: Option<&crate::sync::lww::LwwMergeOutcome>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::sync::types::SyncResult {
    use crate::sync::generation_gc::{
        GenerationMeta, GENERATION_META_FILENAME, GENERATION_UPLOAD_LEASE_MS,
    };
    use crate::sync::provider::model::WritePrecondition;

    // Issue #761 评论 5828969186 问题 2：batch 路径的入口条件只看 provider 能力。
    // 首次同步 / 远端无 visible generation（merge_outcome == None）同样是 generation 发布，
    // 必须一次 commit_batch 提交，不得退回 run_single_target() 的逐文件 Contents API。
    let caps = provider.capabilities();
    if caps.batch {
        return publish_generation_batch(
            provider,
            sync_root,
            generation_prefix,
            generation_id,
            project_id,
            scope,
            merge_outcome,
            cancellation_token,
        );
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    // 1. 写 meta(complete=false, lease)。
    let meta = GenerationMeta {
        generation_id: generation_id.to_string(),
        project_id: project_id.to_string(),
        created_at_ms: now_ms,
        uploader_device_id: String::new(),
        upload_lease_until_ms: now_ms + GENERATION_UPLOAD_LEASE_MS,
        complete: false,
    };
    let meta_path = format!("{generation_prefix}/{GENERATION_META_FILENAME}");
    let meta_content = match serde_json::to_vec(&meta) {
        Ok(c) => c,
        Err(e) => return super::transfer_helpers::sync_result_from_error(crate::Error::Json(e)),
    };
    if let Err(e) = provider.write(&meta_path, &meta_content, WritePrecondition::CreateNew) {
        return super::transfer_helpers::sync_result_from_provider_error(e);
    }

    // Issue #729：meta(complete=false) 写入后检查取消令牌。
    // 取消则不继续上传内容，返回 cancelled SyncResult。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] publish_generation: cancellation requested after meta(complete=false) — returning cancelled"
            );
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during generation publish".into(),
            ));
        }
    }

    // 2. 上传 staging 内容到 generation prefix。
    let content_result = if let Some(outcome) = merge_outcome {
        // 已 merge → 上传完整快照到新
        // generation prefix，不再做第二次 LWW 同步。
        // 关键：必须上传 merged_manifest 里所有 upsert 文件，不能只上传
        // outcome.remote_upload_paths（delta 动作）。NoOp/DownloadRemote/
        // LwwRemoteWinsDownload/pending_take_remote 成功下载的文件都不在
        // remote_upload_paths 里，但它们在 merged_manifest 中，新 generation
        // 必须包含这些文件对象，否则 catalog 指向一个 manifest 声称文件存在
        // 但实际对象不存在的 generation。
        match upload_complete_generation_snapshot(
            provider,
            sync_root,
            generation_prefix,
            &outcome.merged_manifest,
            scope,
            cancellation_token,
        ) {
            Ok(()) => {
                let mut r = crate::sync::types::SyncResult::success();
                r.uploaded_files = outcome.remote_upload_paths.clone();
                r.downloaded_files = outcome.downloaded_files.clone();
                r.local_deletes = outcome.remote_delete_paths.clone();
                r.remote_deletes = outcome.local_deletes.clone();
                r.overwritten_files = outcome.overwritten_files.clone();
                r.ignored_files = outcome.ignored_files.clone();
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
            Err(e) => super::transfer_helpers::sync_result_from_error(e),
        }
    } else {
        // 无 merge_outcome（首次同步 / legacy）→ run_single_target 全量上传。
        let generation_target = crate::sync::types::SyncTarget {
            scope,
            remote_prefix: generation_prefix.to_string(),
        };
        super::transfer_helpers::run_single_target(
            provider,
            sync_root,
            sync_policy,
            &generation_target,
            force_sync,
            cancellation_token,
        )
    };

    // Issue #729：upload_complete_generation_snapshot / run_single_target 返回后检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] publish_generation: cancellation requested after content upload — returning cancelled"
            );
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during generation publish".into(),
            ));
        }
    }

    let content_ok = matches!(
        content_result.status,
        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
    );
    if !content_ok {
        return content_result;
    }

    // Issue #729：meta(complete=true) 写入前检查取消令牌。
    // 取消则不写 complete 标记，返回 cancelled SyncResult。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] publish_generation: cancellation requested before meta(complete=true) — returning cancelled"
            );
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during generation publish".into(),
            ));
        }
    }

    // 3. 写 meta(complete=true)。
    let mut meta = meta;
    meta.complete = true;
    let meta_content = match serde_json::to_vec(&meta) {
        Ok(c) => c,
        Err(e) => return super::transfer_helpers::sync_result_from_error(crate::Error::Json(e)),
    };
    if let Err(e) = provider.write(&meta_path, &meta_content, WritePrecondition::Unconditional) {
        return super::transfer_helpers::sync_result_from_provider_error(e);
    }

    content_result
}

/// Issue #761 Part 3/4：批量原子发布 generation。
///
/// 当 `provider.capabilities().batch == true` 时由 [`publish_generation`] 调用
/// （不论 `merge_outcome` 是否存在）。构造一组 [`BatchMutation`]，一次 `commit_batch`
/// 提交所有 mutation + manifest + meta(complete=true)。
///
/// ## manifest 来源
///
/// - 有 `merge_outcome` → 用 `outcome.merged_manifest`（已 merge 的完整快照）；
/// - 无 `merge_outcome`（首次同步 / 远端无 visible generation）→ 读 staging 的
///   `manifest.sync.json`，`upsert` 记录全部作为 Put（没有旧 generation 的 blob 可复用），
///   `delete` 记录只留 manifest tombstone，不建物理 blob。
///
/// ## mutation 构造规则
///
/// 遍历 manifest.files：
/// - `op == "delete"`（remote delete/tombstone）→ 跳过，不生成 mutation（新 generation
///   不建物理 blob）；
/// - `op == "upsert"`：
///   - 有 merge_outcome 且路径属于 `remote_upload_paths`（本地有修改需上传）→ 读取 staging
///     当前内容，生成 `BatchMutation::Put { path, content }`；
///   - 有 merge_outcome、路径不属于 `remote_upload_paths`，且 `remote_tree_files` 有该路径的
///     blob SHA（无本地修改，复用旧 visible generation 的 blob）→ 生成
///     `BatchMutation::ReuseVersion { path, version }`；
///   - 有 merge_outcome、路径不属于 `remote_upload_paths`，且 `remote_tree_files` 没有该路径的
///     blob SHA（unresolved BothChanged conflict，远端无 blob 可复用）→ 返回 `PartialConflict`
///     错误，不执行 commit_batch（Issue #761 Part 4）；
///   - 无 merge_outcome（首次同步）→ 读取 staging 当前内容，生成 `BatchMutation::Put`。
///
/// `manifest.sync.json` 和 `generation.meta.json`（complete=true）作为
/// `BatchMutation::Put` 放进同一个 batch。
///
/// Git branch commit 本身已经是一次可见，所以不需要先写 `complete=false`、逐文件上传、
/// 最后再改 `complete=true`。直接在同一 commit 里写 `complete=true`。
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn publish_generation_batch(
    provider: &dyn crate::sync::provider::SyncProvider,
    sync_root: &Path,
    generation_prefix: &str,
    generation_id: &str,
    project_id: &str,
    scope: crate::sync::types::SyncScope,
    merge_outcome: Option<&crate::sync::lww::LwwMergeOutcome>,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::sync::types::SyncResult {
    use crate::sync::generation_gc::{
        GenerationMeta, GENERATION_META_FILENAME, GENERATION_UPLOAD_LEASE_MS,
    };
    use crate::sync::provider::model::{BatchMutation, RemoteVersion};

    // 取消令牌检查（与逐文件路径对齐）。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during generation publish".into(),
            ));
        }
    }

    // 本次 generation 的完整 manifest：merge 后的快照，或首次同步时 staging 的本地 manifest。
    let manifest: crate::sync::types::SyncManifest = match merge_outcome {
        Some(outcome) => outcome.merged_manifest.clone(),
        None => match read_staging_generation_manifest(sync_root) {
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
                    "publish_generation_batch: unknown op={} for path={}",
                    record.op, record.path
                )),
            ));
        }

        // 路径白名单检查。
        if !crate::sync::SyncService::is_whitelisted_path(&record.path, scope) {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Io(
                std::io::Error::other(format!(
                    "publish_generation_batch: path {} not whitelisted for scope {:?}",
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
                    "[sync] publish_generation_batch: unresolved conflict path={} has no remote blob to reuse — returning PartialConflict",
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

    // generation.meta.json（complete=true）作为 Put 放进同一 batch。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let meta = GenerationMeta {
        generation_id: generation_id.to_string(),
        project_id: project_id.to_string(),
        created_at_ms: now_ms,
        uploader_device_id: String::new(),
        upload_lease_until_ms: now_ms + GENERATION_UPLOAD_LEASE_MS,
        complete: true,
    };
    let meta_path = format!("{generation_prefix}/{GENERATION_META_FILENAME}");
    let meta_content = match serde_json::to_vec(&meta) {
        Ok(c) => c,
        Err(e) => {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Json(e));
        }
    };
    mutations.push(BatchMutation::Put {
        path: meta_path,
        content: meta_content,
    });

    // 取消令牌检查（commit_batch 前）。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return super::transfer_helpers::sync_result_from_error(crate::Error::Other(
                "sync cancelled during generation publish".into(),
            ));
        }
    }

    // 一次 commit_batch 提交所有 mutation。
    let commit_message = format!("WriterApp publish generation {generation_id}");
    match provider.commit_batch(&mutations, &commit_message) {
        Ok(_result) => {
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
        Err(e) => super::transfer_helpers::sync_result_from_provider_error(e),
    }
}

/// 读取 staging 的本地 `manifest.sync.json`（无 merge_outcome 的首次同步用）。
///
/// 首次同步 / 远端无 visible generation 时没有 merge 快照可用，generation 的完整
/// 文件集合来自 staging 自己的 manifest（prepare 阶段由本地扫描 + tombstone 生成）。
fn read_staging_generation_manifest(
    sync_root: &Path,
) -> crate::error::Result<crate::sync::types::SyncManifest> {
    let manifest_path = sync_root.join(crate::sync::lww::SYNC_MANIFEST_PATH);
    let bytes = std::fs::read(&manifest_path).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "publish_generation_batch: read local manifest {}: {e}",
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
            "publish_generation_batch: read {}: {e}",
            record.path
        )))
    })?;
    let actual_hash = crate::sync::hash::content_md5(&content);
    if actual_hash != record.content_hash {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "publish_generation_batch: hash mismatch for path={} expected={} actual={}",
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
/// Issue #761：本函数仅在 `capabilities().batch == false` 时由 [`publish_generation`]
/// 调用（降级路径）。`batch == true` 时走 [`publish_generation_batch`]。
fn upload_complete_generation_snapshot(
    provider: &dyn crate::sync::provider::SyncProvider,
    staging_root: &Path,
    generation_prefix: &str,
    merged_manifest: &crate::sync::types::SyncManifest,
    scope: crate::sync::types::SyncScope,
    cancellation_token: Option<&SyncCancellationToken>,
) -> crate::error::Result<()> {
    use crate::sync::provider::model::WritePrecondition;

    let caps = provider.capabilities();

    // 遍历 merged_manifest.files，上传所有 upsert 文件到新 generation prefix。
    for record in &merged_manifest.files {
        // op=delete 只保留 tombstone（manifest 里有记录），不上传物理文件。
        if record.op == "delete" {
            continue;
        }
        if record.op != "upsert" {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: unknown op={} for path={}",
                record.op, record.path
            ))));
        }

        // 1. 路径白名单检查。
        if !crate::sync::SyncService::is_whitelisted_path(&record.path, scope) {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: path {} not whitelisted for scope {:?}",
                record.path, scope
            ))));
        }

        // 2. staging 文件必须存在。
        let local_full = staging_root.join(&record.path);
        if !local_full.exists() {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: staging file missing for path={}",
                record.path
            ))));
        }

        // 3. 读取 staging 文件，计算 md5 hash，必须 == record.content_hash。
        let content = std::fs::read(&local_full).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: read {}: {e}",
                record.path
            )))
        })?;
        let actual_hash = crate::sync::hash::content_md5(&content);
        if actual_hash != record.content_hash {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: hash mismatch for path={} expected={} actual={}",
                record.path, record.content_hash, actual_hash
            ))));
        }

        // 4. 上传到新 generation prefix。
        let remote_path = format!("{generation_prefix}/{}", record.path);
        let precondition = if caps.conditional_write {
            WritePrecondition::CreateNew
        } else {
            WritePrecondition::Unconditional
        };
        provider.write(&remote_path, &content, precondition)?;

        // Issue #729：每次 provider.write 返回后检查取消令牌。
        // 取消则立即停止上传，返回 cancelled 错误。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] upload_complete_generation_snapshot: cancellation requested after write {} — returning cancelled",
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
            "upload_complete_generation_snapshot: serialize manifest: {e}"
        )))
    })?;
    let manifest_precondition = if caps.conditional_write {
        WritePrecondition::CreateNew
    } else {
        WritePrecondition::Unconditional
    };
    provider.write(
        &manifest_remote_path,
        manifest_json.as_bytes(),
        manifest_precondition,
    )?;

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
