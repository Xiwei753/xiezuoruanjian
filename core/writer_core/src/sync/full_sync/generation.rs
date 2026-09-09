//! generation 原子发布 helpers。
//!
//! 包含 `GENERATION_SUBDIR` 常量、generation prefix 构造与路径判断、
//! generation 发布（upload + meta write）以及 staging 文件写入。

use std::path::Path;

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
#[allow(clippy::too_many_arguments)] // 9 个参数均为独立发布输入，打包会掩盖各自语义
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
) -> crate::sync::types::SyncResult {
    use crate::sync::generation_gc::{
        GenerationMeta, GENERATION_META_FILENAME, GENERATION_UPLOAD_LEASE_MS,
    };
    use crate::sync::provider::model::WritePrecondition;

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
        )
    };
    let content_ok = matches!(
        content_result.status,
        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
    );
    if !content_ok {
        return content_result;
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
fn upload_complete_generation_snapshot(
    provider: &dyn crate::sync::provider::SyncProvider,
    staging_root: &Path,
    generation_prefix: &str,
    merged_manifest: &crate::sync::types::SyncManifest,
    scope: crate::sync::types::SyncScope,
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
        let actual_hash = format!("{:x}", md5::compute(&content));
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
