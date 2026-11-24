//! 章节提交 journal：durable 决议、投影发布与崩溃恢复。
//!
//! 本模块只由 [`super`] 调用；所有路径都在 project root 的
//! `app-meta/chapter-commits/` 下。journal 是本地提交权威，`chapter.md`、
//! `chapter.meta.json` 与附加文件是可幂等重放的投影。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::*;
use crate::chapter::calculate_word_count;

use super::publish::publish_decided;

/// journal 阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChapterCommitPhase {
    /// 材料已落盘，尚未做提交决议；崩溃后可直接丢弃。
    Prepared,
    /// 决议已 durable；崩溃后按 before/after 摘要裁决。
    Decided,
    /// 正文/元数据/附加文件投影已发布。
    Applied,
    /// 所有投影完成，可清理。
    Finished,
    /// 决议期间发现更新版本，保留材料但绝不覆盖。
    Superseded,
}

/// journal 中的一个附加文件投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AdditionalFilePlan {
    pub(super) target_relative: String,
    pub(super) staging_filename: String,
    /// 提交决议时的目标文件摘要；`None` 表示目标原本不存在。
    #[serde(default)]
    pub(super) before_sha256: Option<String>,
    pub(super) sha256: String,
}

/// durable commit decision。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChapterCommitManifest {
    pub(super) schema_version: u32,
    pub(super) transaction_id: String,
    /// project root 的绝对路径；journal 本身在 project root 内，恢复时以它
    /// 为准校验，防止 journal 被移动到另一个项目后误写。
    pub(super) project_root: String,
    pub(super) key: ChapterKey,
    pub(super) mutation: ChapterMutationKind,
    pub(super) source: ChapterCommitSource,
    pub(super) created_at_ms: i64,
    pub(super) occurred_at_ms: i64,
    pub(super) event_ids: Vec<String>,
    #[serde(default)]
    pub(super) proof_events: Vec<super::ChapterProofProjection>,
    pub(super) expected: Option<ChapterVersion>,
    pub(super) before: ChapterVersion,
    pub(super) after: ChapterVersion,
    pub(super) body_changed: bool,
    pub(super) content_len: usize,
    pub(super) content_md5: String,
    pub(super) word_count: u32,
    pub(super) updated_at: String,
    pub(super) additional_files: Vec<AdditionalFilePlan>,
    pub(super) phase: ChapterCommitPhase,
    pub(super) pending_projections: Vec<String>,
    /// 恢复时发现当前正文既不是 before 也不是 after，记录原因。
    #[serde(default)]
    pub(super) superseded_reason: Option<String>,
}

/// 一次待恢复 journal 的可读结果。
#[derive(Debug, Clone)]
pub struct ChapterCommitRecovery {
    pub transaction_id: String,
    pub key: ChapterKey,
    pub after: ChapterVersion,
    pub pending_projections: Vec<String>,
    pub superseded: bool,
    pub conflict_dir: Option<String>,
}

pub(super) fn sha256_hex(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub(super) fn md5_hex(text: &str) -> String {
    format!("{:x}", md5::compute(text.as_bytes()))
}

pub(super) fn journal_root(project_root: &Path) -> PathBuf {
    project_root.join(JOURNAL_DIR)
}

fn pending_marker(project_root: &Path) -> PathBuf {
    journal_root(project_root).join(PENDING_MARKER)
}

fn project_id(project_root: &Path) -> String {
    project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

pub(super) fn chapter_relative_path(key: &ChapterKey) -> String {
    format!(
        "volumes/{}/chapters/{}/{}",
        key.volume_id, key.chapter_id, BODY_FILE
    )
}

fn ensure_journal_dir(project_root: &Path) -> Result<()> {
    let root = journal_root(project_root);
    fs::create_dir_all(&root)?;
    Ok(())
}

/// 写入 pending 探针。只在第一次写 journal 时调用，内容为空。
pub(super) fn mark_pending(project_root: &Path) -> Result<()> {
    ensure_journal_dir(project_root)?;
    let marker = pending_marker(project_root);
    if !marker.exists() {
        crate::storage::atomic_write_bytes(&marker, b"1")?;
    }
    Ok(())
}

/// 没有未完成 journal 时移除探针。
fn clear_pending_if_idle(project_root: &Path) -> Result<()> {
    let root = journal_root(project_root);
    let marker = pending_marker(project_root);
    if !marker.exists() {
        return Ok(());
    }
    let mut unfinished = false;
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("manifest.json");
            if !manifest_path.exists() {
                unfinished = true;
                break;
            }
            match fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<ChapterCommitManifest>(&raw).ok())
            {
                Some(manifest)
                    if matches!(
                        manifest.phase,
                        ChapterCommitPhase::Finished
                            | ChapterCommitPhase::Superseded
                            | ChapterCommitPhase::Prepared
                    ) => {}
                _ => {
                    unfinished = true;
                    break;
                }
            }
        }
    }
    if !unfinished {
        let _ = fs::remove_file(marker);
    }
    Ok(())
}

pub(super) fn read_current(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<(Chapter, String, ChapterVersion)> {
    let meta_path = key.meta_path(project_root);
    let body_path = key.body_path(project_root);
    if !meta_path.exists() || !body_path.exists() {
        return Err(Error::ChapterNotFound);
    }
    let meta: Chapter = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;
    let content = fs::read_to_string(&body_path)?;
    let version = ChapterVersion {
        object_generation: meta.object_generation.clone(),
        revision: meta.revision,
        content_sha256: sha256_hex(&content),
    };
    Ok((meta, content, version))
}

/// 读取当前章节版本。普通读取不承担 journal 恢复；恢复入口必须持有
/// workspace guard 并调用 [`read_chapter_state_recovering`]。
pub fn read_chapter_version(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterVersion> {
    read_chapter_state(project_root, volume_id, chapter_id).map(|(_, _, version)| version)
}

/// 读取正文、元数据和同一时刻的持久化版本。不会修改磁盘。
pub fn read_chapter_state(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
) -> Result<(Chapter, String, ChapterVersion)> {
    let key = ChapterKey::new(project_id(project_root), volume_id, chapter_id);
    read_current(project_root, &key)
}

/// 持有 workspace guard 的读取入口：先按 journal 决议恢复半提交，再读取。
pub fn read_chapter_state_recovering(
    project_root: &Path,
    guard: &crate::storage::WorkspaceWriteLock,
    volume_id: &str,
    chapter_id: &str,
) -> Result<(Chapter, String, ChapterVersion)> {
    recover_pending_for_read(guard, project_root)?;
    read_chapter_state(project_root, volume_id, chapter_id)
}

/// 持有 workspace guard 的版本读取。
pub fn read_chapter_version_recovering(
    project_root: &Path,
    guard: &crate::storage::WorkspaceWriteLock,
    volume_id: &str,
    chapter_id: &str,
) -> Result<ChapterVersion> {
    read_chapter_state_recovering(project_root, guard, volume_id, chapter_id)
        .map(|(_, _, version)| version)
}

/// 读取路径的恢复入口：调用方必须持有覆盖该项目的 workspace guard。
fn recover_pending_for_read(
    guard: &crate::storage::WorkspaceWriteLock,
    project_root: &Path,
) -> Result<()> {
    if pending_marker(project_root).exists() {
        let _ = recover_pending_chapter_commits(project_root)?;
    }
    Ok(())
}

/// 完整版本 CAS：generation、revision、正文摘要三项必须同时匹配。
///
/// 任一字段为空都视为旧客户端的不完整 base，直接拒绝；旧数据必须先经过
/// [`read_current_migrated`] 在锁内补全稳定 generation/digest。
fn expected_matches(expected: &ChapterVersion, current: &ChapterVersion) -> bool {
    !expected.object_generation.is_empty()
        && !expected.content_sha256.is_empty()
        && expected.object_generation == current.object_generation
        && expected.revision == current.revision
        && expected.content_sha256 == current.content_sha256
}

/// 在锁内一次性补齐旧 meta 的稳定 generation/revision/digest。
fn read_current_migrated(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<(Chapter, String, ChapterVersion)> {
    let (mut meta, content, mut version) = read_current(project_root, key)?;
    let mut changed = false;
    if meta.object_generation.is_empty() {
        meta.object_generation = Uuid::new_v4().to_string();
        changed = true;
    }
    if meta.content_sha256.is_empty() {
        meta.content_sha256 = sha256_hex(&content);
        changed = true;
    }
    // 旧章节没有 revision 字段；非空正文从 1 开始，空正文保持 0。
    if meta.revision == 0 && !content.is_empty() {
        meta.revision = 1;
        changed = true;
    }
    if changed {
        version = ChapterVersion {
            object_generation: meta.object_generation.clone(),
            revision: meta.revision,
            content_sha256: meta.content_sha256.clone(),
        };
        let json = serde_json::to_string_pretty(&meta)?;
        crate::storage::atomic_write_bytes(&key.meta_path(project_root), json.as_bytes())?;
        crate::storage::sync_dir(&key.meta_path(project_root))?;
    }
    Ok((meta, content, version))
}

fn write_conflict_material(
    project_root: &Path,
    request: &ChapterCommitRequest,
    current: &str,
    actual: &ChapterVersion,
    transaction_id: &str,
) -> Result<String> {
    let root = journal_root(project_root)
        .join(CONFLICT_DIR)
        .join(transaction_id);
    fs::create_dir_all(&root)?;
    if let Some(base) = request.base_content.as_deref() {
        crate::storage::atomic_write_bytes(&root.join("base.md"), base.as_bytes())?;
    }
    crate::storage::atomic_write_bytes(&root.join("local.md"), current.as_bytes())?;
    let incoming = request.content.clone();
    crate::storage::atomic_write_bytes(&root.join("incoming.md"), incoming.as_bytes())?;
    if let Some(expected) = request.expected.as_ref() {
        crate::storage::atomic_write_bytes(
            &root.join("expected.json"),
            serde_json::to_string_pretty(expected)?.as_bytes(),
        )?;
    }
    crate::storage::atomic_write_bytes(
        &root.join("actual.json"),
        serde_json::to_string_pretty(actual)?.as_bytes(),
    )?;
    crate::storage::sync_dir(&root)?;
    Ok(root.to_string_lossy().to_string())
}

fn ensure_empty_overwrite_allowed(
    chapter_id: &str,
    old_content: &str,
    content: &str,
    allow_empty_overwrite: bool,
) -> Result<()> {
    if allow_empty_overwrite || old_content.is_empty() || !content.is_empty() {
        return Ok(());
    }
    let reason = "new_content_empty_without_allow_empty_overwrite".to_string();
    log::warn!(
        "blocked_empty_overwrite chapter_id={} old_len={} new_len={} reason={}",
        chapter_id,
        old_content.len(),
        content.len(),
        reason
    );
    Err(Error::EmptyOverwriteBlocked {
        chapter_id: chapter_id.to_string(),
        old_len: old_content.len(),
        new_len: content.len(),
        reason,
    })
}

fn additional_file_changed(project_root: &Path, relative_path: &str, content: &str) -> bool {
    let path = project_root.join(relative_path);
    match fs::read(&path) {
        Ok(existing) => existing != content.as_bytes(),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "chapter additional file read failed, rewriting: path={} error={}",
                    path.display(),
                    error
                );
            }
            true
        }
    }
}

pub(super) fn write_manifest(tx_dir: &Path, manifest: &ChapterCommitManifest) -> Result<()> {
    let json = serde_json::to_string_pretty(manifest)?;
    crate::storage::atomic_write_bytes(&tx_dir.join("manifest.json"), json.as_bytes())?;
    crate::storage::sync_dir(tx_dir)?;
    Ok(())
}

fn remove_transaction_dir(tx_dir: &Path) -> Result<()> {
    // 只允许删除 journal root 下一级的目录，避免路径计算错误误删。
    let parent = tx_dir.parent().ok_or_else(|| {
        Error::Other(format!(
            "chapter commit journal has no parent: {}",
            tx_dir.display()
        ))
    })?;
    if parent.file_name().and_then(|name| name.to_str()) != Some("chapter-commits") {
        return Err(Error::Other(format!(
            "refuse to remove non-journal directory: {}",
            tx_dir.display()
        )));
    }
    fs::remove_dir_all(tx_dir)?;
    Ok(())
}

/// 把 journal 中的 after 内容/附加文件发布到 live。
///
/// 当前正文既不是 before 也不是 after 时拒绝写入（返回 `false`），由调用方
/// 标记 Superseded 并保留冲突材料；绝不用旧 staging 覆盖新正文。
/// 章节提交唯一实现。调用方必须传入已持有的 workspace guard；`None` 仅用于
/// 测试/纯局部导入等没有跨进程锁的场景。
#[allow(clippy::too_many_lines)]
pub(crate) fn commit_chapter_with_guard(
    project_root: &Path,
    guard: Option<&crate::storage::WorkspaceWriteLock>,
    request: ChapterCommitRequest,
) -> Result<ChapterCommitOutcome> {
    if let Some(guard) = guard {
        recover_pending_for_read(guard, project_root)?;
    }

    let key = request.key.clone();
    let (mut meta, current, before) = read_current_migrated(project_root, &key)?;
    ensure_empty_overwrite_allowed(
        &key.chapter_id,
        &current,
        &request.content,
        request.allow_empty_overwrite,
    )?;

    let transaction_id = request
        .transaction_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let content_changed = current != request.content;

    if let Some(expected) = request.expected.as_ref() {
        if !expected_matches(expected, &before) {
            let conflict_dir = write_conflict_material(
                project_root,
                &request,
                &current,
                &before,
                &transaction_id,
            )?;
            return Ok(ChapterCommitOutcome::Conflict(ChapterConflict {
                key,
                expected: Some(expected.clone()),
                actual: before,
                conflict_dir,
            }));
        }
    }

    let changed_additional = request
        .additional_files
        .iter()
        .filter(|(relative_path, content)| {
            additional_file_changed(project_root, relative_path, content)
        })
        .cloned()
        .collect::<Vec<_>>();

    if !content_changed && changed_additional.is_empty() {
        // no-op：不创建 journal、不推进 revision/updated_at，返回磁盘真实现状。
        let meta_hash = md5_hex(&serde_json::to_string_pretty(&meta)?);
        return Ok(ChapterCommitOutcome::Committed(ChapterCommitReceipt {
            transaction_id,
            key,
            before: before.clone(),
            after: before,
            content_changed: false,
            content_len: current.len(),
            content_hash: md5_hex(&current),
            meta_hash,
            updated_at: meta.updated_at.clone(),
            word_count: meta.word_count,
            written_files: Vec::new(),
            pending_projections: Vec::new(),
        }));
    }

    let generation = if before.object_generation.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        before.object_generation.clone()
    };
    let after = ChapterVersion {
        object_generation: generation,
        revision: if content_changed {
            before.revision.saturating_add(1)
        } else {
            before.revision
        },
        content_sha256: sha256_hex(&request.content),
    };
    let word_count = calculate_word_count(&request.content);
    let updated_at = if content_changed {
        Utc::now().to_rfc3339()
    } else {
        meta.updated_at.clone()
    };
    meta.word_count = word_count;
    meta.hash = md5_hex(&request.content);
    meta.content_sha256 = after.content_sha256.clone();
    meta.revision = after.revision;
    if meta.object_generation.is_empty() {
        meta.object_generation = after.object_generation.clone();
    }
    if content_changed {
        meta.updated_at = updated_at.clone();
    }

    ensure_journal_dir(project_root)?;
    mark_pending(project_root)?;
    let tx_dir = journal_root(project_root).join(&transaction_id);
    fs::create_dir_all(&tx_dir)?;

    // 1) 先把所有提交材料 durable 落盘（未决议，崩溃可丢弃）。
    let after_content_path = tx_dir.join(BODY_FILE);
    crate::storage::atomic_write_bytes(&after_content_path, request.content.as_bytes())?;
    let mut additional_plans = Vec::new();
    for (index, (target_relative, content)) in changed_additional.iter().enumerate() {
        let staging_filename = format!("additional_{index}");
        crate::storage::atomic_write_bytes(&tx_dir.join(&staging_filename), content.as_bytes())?;
        let target = project_root.join(target_relative);
        let before_sha256 = match fs::read(&target) {
            Ok(existing) => Some(sha256_hex(&String::from_utf8_lossy(&existing))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        additional_plans.push(AdditionalFilePlan {
            target_relative: target_relative.clone(),
            staging_filename,
            before_sha256,
            sha256: sha256_hex(content),
        });
    }
    crate::storage::sync_dir(&tx_dir)?;

    let mut manifest = ChapterCommitManifest {
        schema_version: 1,
        transaction_id: transaction_id.clone(),
        project_root: project_root.to_string_lossy().to_string(),
        key: key.clone(),
        mutation: request.mutation,
        source: request.source,
        created_at_ms: Utc::now().timestamp_millis(),
        occurred_at_ms: Utc::now().timestamp_millis(),
        event_ids: request.event_ids.clone(),
        proof_events: request.proof_events.clone(),
        expected: request.expected.clone(),
        before: before.clone(),
        after: after.clone(),
        body_changed: content_changed,
        content_len: request.content.len(),
        content_md5: meta.hash.clone(),
        word_count,
        updated_at: updated_at.clone(),
        additional_files: additional_plans,
        phase: ChapterCommitPhase::Prepared,
        pending_projections: request.projection_ids.clone(),
        superseded_reason: None,
    };
    write_manifest(&tx_dir, &manifest)?;

    // 2) 决议 durable。只有这一步成功后才允许触碰 live。
    manifest.phase = ChapterCommitPhase::Decided;
    write_manifest(&tx_dir, &manifest)?;

    // 3) 发布正文/元数据/附加文件投影。
    let published = publish_decided(project_root, &tx_dir, &mut manifest)?;
    if !published {
        let conflict_dir =
            write_conflict_material(project_root, &request, &current, &before, &transaction_id)?;
        clear_pending_if_idle(project_root)?;
        return Ok(ChapterCommitOutcome::Conflict(ChapterConflict {
            key,
            expected: request.expected,
            actual: before,
            conflict_dir,
        }));
    }

    // 4) 无滞后投影时立即清理；有投影则保留 journal，由调用方确认后清除。
    let mut pending = manifest.pending_projections.clone();
    if pending.is_empty() {
        manifest.phase = ChapterCommitPhase::Finished;
        write_manifest(&tx_dir, &manifest)?;
        let _ = remove_transaction_dir(&tx_dir);
        clear_pending_if_idle(project_root)?;
    } else {
        // 去重，事件 ID 重发不会制造第二份 pending。
        pending.sort();
        pending.dedup();
        manifest.pending_projections = pending.clone();
        write_manifest(&tx_dir, &manifest)?;
    }

    let written_files = {
        let mut files = Vec::new();
        if content_changed {
            files.push(chapter_relative_path(&key));
            files.push(format!(
                "volumes/{}/chapters/{}/{}",
                key.volume_id, key.chapter_id, META_FILE
            ));
        }
        for plan in &manifest.additional_files {
            files.push(plan.target_relative.clone());
        }
        files
    };
    let meta_hash = md5_hex(&fs::read_to_string(key.meta_path(project_root))?);

    Ok(ChapterCommitOutcome::Committed(ChapterCommitReceipt {
        transaction_id,
        key,
        before,
        after,
        content_changed,
        content_len: request.content.len(),
        content_hash: manifest.content_md5,
        meta_hash,
        updated_at: manifest.updated_at,
        word_count,
        written_files,
        pending_projections: pending,
    }))
}

/// 确认一个投影完成。所有投影确认后 journal 进入 Finished 并清理。
///
/// 当前编辑保存路径在 receipt 返回后补写统计/证明；平台端重发同一批事件时
/// 可调用本函数推进 journal。
pub fn mark_chapter_commit_projection(
    project_root: &Path,
    transaction_id: &str,
    projection_id: &str,
) -> Result<bool> {
    let tx_dir = journal_root(project_root).join(transaction_id);
    let manifest_path = tx_dir.join("manifest.json");
    if !manifest_path.exists() {
        return Ok(false);
    }
    let mut manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    manifest
        .pending_projections
        .retain(|id| id != projection_id);
    if manifest.pending_projections.is_empty()
        && matches!(
            manifest.phase,
            ChapterCommitPhase::Applied | ChapterCommitPhase::Decided
        )
    {
        if manifest.phase == ChapterCommitPhase::Decided {
            let published = publish_decided(project_root, &tx_dir, &mut manifest)?;
            if !published {
                clear_pending_if_idle(project_root)?;
                return Ok(false);
            }
        }
        manifest.phase = ChapterCommitPhase::Finished;
    }
    write_manifest(&tx_dir, &manifest)?;
    if manifest.phase == ChapterCommitPhase::Finished {
        let _ = remove_transaction_dir(&tx_dir);
        clear_pending_if_idle(project_root)?;
        return Ok(true);
    }
    Ok(false)
}

/// 保留 Superseded 事务的真实材料、pending 事件和冲突记录。
fn preserve_superseded_conflict(
    project_root: &Path,
    tx_dir: &Path,
    manifest: &ChapterCommitManifest,
) -> Result<String> {
    let conflict_dir = journal_root(project_root)
        .join(CONFLICT_DIR)
        .join(&manifest.transaction_id);
    fs::create_dir_all(&conflict_dir)?;
    let _ = fs::copy(
        tx_dir.join("manifest.json"),
        conflict_dir.join("manifest.json"),
    );
    if tx_dir.join(BODY_FILE).exists() {
        let _ = fs::copy(tx_dir.join(BODY_FILE), conflict_dir.join(BODY_FILE));
    }
    for plan in &manifest.additional_files {
        let staged = tx_dir.join(&plan.staging_filename);
        if staged.exists() {
            let _ = fs::copy(&staged, conflict_dir.join(&plan.staging_filename));
        }
    }
    let actual = read_current(project_root, &manifest.key)
        .ok()
        .map(|(_, _, version)| version);
    let record = serde_json::json!({
        "transactionId": manifest.transaction_id,
        "key": manifest.key,
        "before": manifest.before,
        "after": manifest.after,
        "actual": actual,
        "reason": manifest.superseded_reason,
        "pendingProjections": manifest.pending_projections,
    });
    crate::storage::atomic_write_bytes(
        &conflict_dir.join("conflict.json"),
        serde_json::to_string_pretty(&record)?.as_bytes(),
    )?;
    crate::storage::sync_dir(&conflict_dir)?;
    Ok(conflict_dir.to_string_lossy().to_string())
}

/// 读取 journal 中尚未确认的证明事件（供投影 worker 使用）。
pub fn pending_chapter_proof_events(
    project_root: &Path,
    transaction_id: &str,
) -> Result<Vec<super::ChapterProofProjection>> {
    let manifest_path = journal_root(project_root)
        .join(transaction_id)
        .join("manifest.json");
    if !manifest_path.exists() {
        return Ok(Vec::new());
    }
    let manifest: ChapterCommitManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    Ok(manifest.proof_events)
}

/// 扫描并恢复未完成 journal。按 `created_at_ms` 顺序处理；只处理未完成事务，
/// 不遍历作品正文。调用方应持有工作区写锁（或传入 app_data_root）。
pub fn recover_pending_chapter_commits(project_root: &Path) -> Result<Vec<ChapterCommitRecovery>> {
    let root = journal_root(project_root);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        if dir.file_name().and_then(|name| name.to_str()) == Some(CONFLICT_DIR) {
            continue;
        }
        let manifest_path = dir.join("manifest.json");
        if !manifest_path.exists() {
            // 材料都还没落盘完的目录：没有决议，删除自己的未发布材料。
            let _ = fs::remove_dir_all(&dir);
            continue;
        }
        match serde_json::from_str::<ChapterCommitManifest>(&fs::read_to_string(&manifest_path)?) {
            Ok(manifest) => entries.push((dir, manifest)),
            Err(error) => {
                log::warn!(
                    "chapter commit journal manifest parse failed, preserving: {}: {}",
                    manifest_path.display(),
                    error
                );
            }
        }
    }
    entries.sort_by_key(|(_, manifest)| manifest.created_at_ms);

    let mut recovered = Vec::new();
    for (tx_dir, mut manifest) in entries {
        match manifest.phase {
            ChapterCommitPhase::Prepared => {
                // 未决议：live 从未被触碰。保留目录供人工诊断，不能删除后
                // 继续写 Finished manifest；pending 探针会由空闲清理移除。
                continue;
            }
            ChapterCommitPhase::Decided => {
                let published = publish_decided(project_root, &tx_dir, &mut manifest)?;
                if !published {
                    let conflict_dir =
                        preserve_superseded_conflict(project_root, &tx_dir, &manifest)?;
                    recovered.push(ChapterCommitRecovery {
                        transaction_id: manifest.transaction_id.clone(),
                        key: manifest.key.clone(),
                        after: manifest.after.clone(),
                        pending_projections: manifest.pending_projections.clone(),
                        superseded: true,
                        conflict_dir: Some(conflict_dir),
                    });
                    continue;
                }
            }
            ChapterCommitPhase::Applied => {}
            ChapterCommitPhase::Finished | ChapterCommitPhase::Superseded => {
                if manifest.phase == ChapterCommitPhase::Finished {
                    let _ = remove_transaction_dir(&tx_dir);
                }
                continue;
            }
        }

        if manifest.pending_projections.is_empty() {
            manifest.phase = ChapterCommitPhase::Finished;
            write_manifest(&tx_dir, &manifest)?;
            let _ = remove_transaction_dir(&tx_dir);
            continue;
        }
        recovered.push(ChapterCommitRecovery {
            transaction_id: manifest.transaction_id.clone(),
            key: manifest.key.clone(),
            after: manifest.after.clone(),
            pending_projections: manifest.pending_projections.clone(),
            superseded: false,
            conflict_dir: None,
        });
    }
    clear_pending_if_idle(project_root)?;
    Ok(recovered)
}
