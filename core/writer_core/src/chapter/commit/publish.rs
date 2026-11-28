//! 章节提交的投影发布与 CAS 校验。

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::journal::{
    sha256_hex, write_manifest, AdditionalFilePlan, ChapterCommitManifest, ChapterCommitPhase,
};
use super::BODY_FILE;
use crate::chapter::Chapter;
use crate::error::{Error, Result};

pub(super) fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn mark_superseded(
    tx_dir: &Path,
    manifest: &mut ChapterCommitManifest,
    reason: String,
) -> Result<bool> {
    manifest.phase = ChapterCommitPhase::Superseded;
    manifest.superseded_reason = Some(reason);
    write_manifest(tx_dir, manifest)?;
    Ok(false)
}

/// 把 journal 中的 after 内容/附加文件发布到 live。
///
/// 当前正文或元数据既不是 before 也不是 after 时拒绝写入（返回 `false`），
/// 由调用方标记 Superseded 并保留冲突材料；绝不用旧 staging 覆盖新正文。
/// 缺失正文与 IO 错误必须区分：缺失表示章节已被删除，不能当作空正文复活。

/// 发布删除决议：把整章目录移动到 journal 预先固定的 trash 路径，并把
/// tombstone 写入 sync state。rename 与 state 写之间崩溃时，journal 仍在
/// Decided，恢复会按同一 trash_path 幂等补全。
pub(super) fn publish_deleted(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &mut ChapterCommitManifest,
) -> Result<bool> {
    let key = &manifest.key;
    let body_path = key.body_path(project_root);
    let meta_path = key.meta_path(project_root);
    let chapter_dir = body_path
        .parent()
        .ok_or_else(|| Error::Other("chapter delete has no parent directory".to_string()))?
        .to_path_buf();
    let Some(trash_path) = manifest.trash_path.as_ref().map(PathBuf::from) else {
        return mark_superseded(
            tx_dir,
            manifest,
            "chapter delete journal missing trash path".to_string(),
        );
    };

    if chapter_dir.exists() {
        let meta: Chapter = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;
        let body = fs::read_to_string(&body_path)?;
        let body_sha = sha256_hex(&body);
        let matches_before = meta.object_generation == manifest.before.object_generation
            && meta.revision == manifest.before.revision
            && body_sha == manifest.before.content_sha256;
        if !matches_before {
            return mark_superseded(
                tx_dir,
                manifest,
                "chapter changed after delete decision".to_string(),
            );
        }
        if let Some(parent) = trash_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&chapter_dir, &trash_path)?;
        if let Some(parent) = trash_path.parent() {
            crate::storage::sync_dir(parent)?;
        }
    } else if !trash_path.exists() {
        // live 与预先决定的 trash 同时不存在：这不是“删除已完成”的证据。
        // 保留 Decided journal 交给恢复/人工诊断，绝不能无 tombstone 直接确认。
        return Err(Error::Other(format!(
            "chapter delete lost both live directory and trash: tx={}, key={}/{}/{}",
            manifest.transaction_id,
            manifest.key.project_id,
            manifest.key.volume_id,
            manifest.key.chapter_id
        )));
    }

    // Issue #1062 1.4：app data root 只认请求写进 manifest 的真实路径；旧
    // journal 缺字段时，从同一 manifest 记录的真实 trash 路径反推
    //（`<app_data_root>/sync/trash/<token>`），不再用 `project_root.ancestors()`
    // 猜目录层级——独立 projects_root 会让猜测指到错误的工作区。
    let app_data_root = manifest
        .app_data_root
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            let trash = manifest.trash_path.as_deref().map(PathBuf::from)?;
            let trash_root = trash.parent()?;
            if trash_root.file_name().and_then(|name| name.to_str()) != Some("trash")
                || trash_root
                    .parent()?
                    .file_name()
                    .and_then(|name| name.to_str())
                    != Some("sync")
            {
                return None;
            }
            trash_root.parent()?.parent().map(Path::to_path_buf)
        })
        .ok_or_else(|| {
            Error::Other(format!(
                "chapter delete manifest lacks app_data_root/trash_path: tx={}",
                manifest.transaction_id
            ))
        })?;
    let rel_chapter_dir = chapter_dir
        .strip_prefix(project_root)
        .unwrap_or(&chapter_dir)
        .to_string_lossy()
        .replace('\\', "/");
    let rel_trash_path = trash_path
        .strip_prefix(&app_data_root)
        .unwrap_or(&trash_path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut state = crate::sync::SyncService::load_sync_state(project_root)?;
    crate::trash::generate_tombstones(
        &mut state,
        &trash_path,
        &rel_chapter_dir,
        &rel_trash_path,
    )?;
    crate::sync::SyncService::save_sync_state(project_root, &state)?;

    manifest.phase = ChapterCommitPhase::Applied;
    write_manifest(tx_dir, manifest)?;
    Ok(true)
}

pub(super) fn publish_decided(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &mut ChapterCommitManifest,
) -> Result<bool> {
    let key = &manifest.key;
    let body_path = key.body_path(project_root);
    let meta_path = key.meta_path(project_root);
    if manifest.created {
        let bytes = fs::read(tx_dir.join(BODY_FILE))?;
        if sha256_bytes(&bytes) != manifest.after.content_sha256 {
            return Err(Error::Other(format!(
                "chapter create staging digest mismatch: tx={}",
                manifest.transaction_id
            )));
        }
        let created_meta = manifest.created_meta.as_ref().ok_or_else(|| {
            Error::Other(format!(
                "chapter create journal missing meta: tx={}",
                manifest.transaction_id
            ))
        })?;
        // 先把 body/meta/additional 全部预检完，再统一发布。任何一处已被
        // 不同决议占用都只标 Superseded，不留下创建到一半的章节。
        if body_path.exists() {
            let existing = fs::read(&body_path)?;
            if sha256_bytes(&existing) != manifest.after.content_sha256 {
                return mark_superseded(
                    tx_dir,
                    manifest,
                    "chapter body appeared with different content during create".to_string(),
                );
            }
        }
        if meta_path.exists() {
            let existing: Chapter = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;
            if existing.object_generation != manifest.after.object_generation
                || existing.revision != manifest.after.revision
                || existing.content_sha256 != manifest.after.content_sha256
            {
                return mark_superseded(
                    tx_dir,
                    manifest,
                    "chapter meta appeared with different version during create".to_string(),
                );
            }
        }
        let mut staged_additional: Vec<(&AdditionalFilePlan, Vec<u8>)> = Vec::new();
        for plan in &manifest.additional_files {
            let staged = fs::read(tx_dir.join(&plan.staging_filename))?;
            if sha256_bytes(&staged) != plan.sha256 {
                return Err(Error::Other(format!(
                    "chapter create additional staging digest mismatch: {}",
                    plan.target_relative
                )));
            }
            let target = project_root.join(&plan.target_relative);
            match fs::read(&target) {
                Ok(existing) if sha256_bytes(&existing) == plan.sha256 => {
                    // 半创建恢复：已经是 after 的附加文件只确认，不重写。
                }
                Ok(_) => {
                    return mark_superseded(
                        tx_dir,
                        manifest,
                        format!(
                            "additional file appeared during create: {}",
                            plan.target_relative
                        ),
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    staged_additional.push((plan, staged));
                }
                Err(error) => return Err(error.into()),
            }
        }

        if !body_path.exists() {
            crate::storage::atomic_write_bytes(&body_path, &bytes)?;
            crate::storage::sync_dir(&body_path)?;
        }
        if !meta_path.exists() {
            let meta_json = serde_json::to_string_pretty(created_meta)?;
            crate::storage::atomic_write_bytes(&meta_path, meta_json.as_bytes())?;
            crate::storage::sync_dir(&meta_path)?;
        }
        for (plan, staged) in staged_additional {
            let target = project_root.join(&plan.target_relative);
            crate::storage::atomic_write_bytes(&target, &staged)?;
            crate::storage::sync_parent(&target)?;
        }
        manifest.phase = ChapterCommitPhase::Applied;
        write_manifest(tx_dir, manifest)?;
        return Ok(true);
    }
    let current = match fs::read_to_string(&body_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return mark_superseded(
                tx_dir,
                manifest,
                "chapter body missing before publish (deleted tombstone)".to_string(),
            );
        }
        Err(error) => return Err(error.into()),
    };
    let current_sha = sha256_hex(&current);
    let meta_str = match fs::read_to_string(&meta_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return mark_superseded(
                tx_dir,
                manifest,
                "chapter meta missing before publish (deleted tombstone)".to_string(),
            );
        }
        Err(error) => return Err(error.into()),
    };
    let mut meta: Chapter = serde_json::from_str(&meta_str)?;

    if meta.object_generation != manifest.before.object_generation
        && meta.object_generation != manifest.after.object_generation
    {
        return mark_superseded(
            tx_dir,
            manifest,
            format!(
                "chapter generation changed: current={}, before={}, after={}",
                meta.object_generation,
                manifest.before.object_generation,
                manifest.after.object_generation
            ),
        );
    }
    if meta.revision > manifest.after.revision
        || (meta.revision != manifest.before.revision && meta.revision != manifest.after.revision)
    {
        return mark_superseded(
            tx_dir,
            manifest,
            format!(
                "chapter revision changed after decision: current={}, before={}, after={}",
                meta.revision, manifest.before.revision, manifest.after.revision
            ),
        );
    }
    if current_sha == manifest.before.content_sha256
        && meta.content_sha256 != manifest.before.content_sha256
    {
        return mark_superseded(
            tx_dir,
            manifest,
            format!(
                "chapter meta digest changed after decision: current={}, before={}",
                meta.content_sha256, manifest.before.content_sha256
            ),
        );
    }

    let already_after = current_sha == manifest.after.content_sha256;
    if !already_after && current_sha != manifest.before.content_sha256 {
        return mark_superseded(
            tx_dir,
            manifest,
            format!(
                "current chapter changed after decision: current={current_sha}, before={}, after={}",
                manifest.before.content_sha256, manifest.after.content_sha256
            ),
        );
    }

    let after_bytes = if already_after {
        None
    } else {
        let bytes = fs::read(tx_dir.join(BODY_FILE))?;
        if sha256_bytes(&bytes) != manifest.after.content_sha256 {
            return Err(Error::Other(format!(
                "chapter commit staging digest mismatch: tx={}",
                manifest.transaction_id
            )));
        }
        Some(bytes)
    };
    let mut additional_writes: Vec<(&AdditionalFilePlan, Vec<u8>)> = Vec::new();
    for plan in &manifest.additional_files {
        let staged = fs::read(tx_dir.join(&plan.staging_filename))?;
        if sha256_bytes(&staged) != plan.sha256 {
            return Err(Error::Other(format!(
                "chapter commit additional staging digest mismatch: {}",
                plan.target_relative
            )));
        }
        let target = project_root.join(&plan.target_relative);
        match fs::read(&target) {
            Ok(existing) => {
                let existing_sha = sha256_bytes(&existing);
                if existing_sha == plan.sha256 {
                    continue;
                }
                if plan.before_sha256.as_deref() != Some(existing_sha.as_str()) {
                    return mark_superseded(
                        tx_dir,
                        manifest,
                        format!(
                            "additional file changed after decision: {} current={existing_sha}, before={:?}",
                            plan.target_relative, plan.before_sha256
                        ),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if plan.before_sha256.is_some() {
                    return mark_superseded(
                        tx_dir,
                        manifest,
                        format!(
                            "additional file deleted after decision: {}",
                            plan.target_relative
                        ),
                    );
                }
            }
            Err(error) => return Err(error.into()),
        }
        additional_writes.push((plan, staged));
    }

    if let Some(bytes) = after_bytes {
        crate::storage::atomic_write_bytes(&body_path, &bytes)?;
        crate::storage::sync_dir(&body_path)?;
    }
    for (plan, staged) in additional_writes {
        let target = project_root.join(&plan.target_relative);
        crate::storage::atomic_write_bytes(&target, &staged)?;
        crate::storage::sync_parent(&target)?;
    }

    if !already_after
        || meta.content_sha256 != manifest.after.content_sha256
        || meta.revision != manifest.after.revision
        || meta.updated_at != manifest.updated_at
        || manifest
            .meta_title
            .as_ref()
            .is_some_and(|title| title != &meta.title)
        || manifest.meta_order.is_some_and(|order| order != meta.order)
    {
        // 半提交恢复：即使正文已是 after，也要按决议补齐 meta 的
        // updated_at/revision/digest，不能留下正文新、meta 旧。
        meta.updated_at = manifest.updated_at.clone();
        meta.word_count = manifest.word_count;
        meta.hash = manifest.content_md5.clone();
        meta.content_sha256 = manifest.after.content_sha256.clone();
        meta.revision = manifest.after.revision;
        if let Some(title) = manifest.meta_title.as_ref() {
            meta.title = title.clone();
        }
        if let Some(order) = manifest.meta_order {
            meta.order = order;
        }
        if meta.object_generation.is_empty() {
            meta.object_generation = manifest.after.object_generation.clone();
        }
        let updated = serde_json::to_string_pretty(&meta)?;
        crate::storage::atomic_write_bytes(&meta_path, updated.as_bytes())?;
        crate::storage::sync_dir(&meta_path)?;
    }

    manifest.phase = ChapterCommitPhase::Applied;
    write_manifest(tx_dir, manifest)?;
    Ok(true)
}
