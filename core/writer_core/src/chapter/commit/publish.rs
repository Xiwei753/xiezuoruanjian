//! 章节提交的投影发布与 CAS 校验。

use std::fs;
use std::path::Path;

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
pub(super) fn publish_decided(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &mut ChapterCommitManifest,
) -> Result<bool> {
    let key = &manifest.key;
    let body_path = key.body_path(project_root);
    let meta_path = key.meta_path(project_root);
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
    if meta.revision > manifest.after.revision {
        return mark_superseded(
            tx_dir,
            manifest,
            format!(
                "chapter revision advanced past decision: current={}, after={}",
                meta.revision, manifest.after.revision
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

    if !already_after || meta.content_sha256 != manifest.after.content_sha256 {
        if !already_after {
            meta.updated_at = manifest.updated_at.clone();
        }
        meta.word_count = manifest.word_count;
        meta.hash = manifest.content_md5.clone();
        meta.content_sha256 = manifest.after.content_sha256.clone();
        meta.revision = manifest.after.revision;
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
