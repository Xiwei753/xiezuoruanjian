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

use super::publish::{publish_decided, publish_deleted};

mod recovery;
pub use recovery::{
    pending_chapter_proof_events, pending_chapter_proof_transactions,
    pending_chapter_sync_proof_fact_ids, pending_chapter_sync_proof_facts,
    recover_pending_chapter_commits,
};

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
    /// 项目内单调提交序号，用于证明投影按因果顺序重放。
    #[serde(default)]
    pub(super) commit_sequence: u64,
    pub(super) event_ids: Vec<String>,
    #[serde(default)]
    pub(super) proof_events: Vec<super::ChapterProofProjection>,
    #[serde(default)]
    pub(super) sync_proof_facts: Vec<super::ChapterSyncProofFact>,
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
    /// Create 提交的初始 meta；发布时正文/meta 都不存在才允许写入。
    #[serde(default)]
    pub(super) created_meta: Option<Chapter>,
    /// 与正文同一决议的标题/顺序；None 表示不修改。
    #[serde(default)]
    pub(super) meta_title: Option<String>,
    #[serde(default)]
    pub(super) meta_order: Option<i32>,
    /// 恢复时判断是否为创建提交；旧 journal 无此字段默认 false。
    #[serde(default)]
    pub(super) created: bool,
    /// 删除提交：after 是 tombstone 版本，live 章节目录应移动到 trash。
    #[serde(default)]
    pub(super) deleted: bool,
    /// 删除提交预先确定的 trash 绝对路径；恢复时按它幂等补全移动。
    #[serde(default)]
    pub(super) trash_path: Option<String>,
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

/// 分配项目内单调提交序号。调用方必须持有章节写锁；序号只用于排序，
/// 不参与正文版本比较，因此即使回滚留下空洞也不影响正确性。
///
/// 计数器损坏或丢失时不能 `unwrap_or(0)` 重新从 1 开始，否则新 journal 会
/// 排到旧 journal 前面，证明投影顺序倒退。这里扫描已有 journal 的持久
/// 序号取最大值修复，再原子写回；扫描也只读取 manifest，不触碰正文。
fn next_commit_sequence(project_root: &Path) -> Result<u64> {
    let path = journal_root(project_root).join("sequence");
    let counter = match fs::read_to_string(&path) {
        Ok(raw) => match raw.trim().parse::<u64>() {
            Ok(value) => Some(value),
            Err(error) => {
                log::warn!(
                    "chapter commit sequence counter corrupt, rebuilding from journal: {}: {error}",
                    path.display()
                );
                None
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let max_persisted = max_journal_commit_sequence(project_root)?;
    let current = counter.map_or(max_persisted, |value| value.max(max_persisted));
    let next = current.saturating_add(1);
    crate::storage::atomic_write_bytes(&path, next.to_string().as_bytes())?;
    Ok(next)
}

fn max_journal_commit_sequence(project_root: &Path) -> Result<u64> {
    let root = journal_root(project_root);
    if !root.exists() {
        return Ok(0);
    }
    let mut max_sequence = 0;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let Ok(manifest) =
            serde_json::from_str::<ChapterCommitManifest>(&fs::read_to_string(&manifest_path)?)
        else {
            continue;
        };
        max_sequence = max_sequence.max(manifest.commit_sequence);
    }
    Ok(max_sequence)
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
    let key = ChapterKey::new(project_id(project_root), volume_id, chapter_id);
    read_current_migrated(project_root, &key)
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

/// 旧章节的稳定 generation：由 project/volume/chapter 身份派生。
fn legacy_generation(key: &ChapterKey) -> String {
    let digest = Sha256::digest(
        format!(
            "sujian-chapter:{}/{}/{}",
            key.project_id, key.volume_id, key.chapter_id
        )
        .as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    // 标记为 RFC 4122 version 5 / variant 10xx，便于诊断时识别派生身份。
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

/// 在锁内一次性补齐旧 meta 的稳定 generation/revision/digest。
fn read_current_migrated(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<(Chapter, String, ChapterVersion)> {
    let (mut meta, content, mut version) = read_current(project_root, key)?;
    let mut changed = false;
    if meta.object_generation.is_empty() {
        // 旧对象没有 generation。用稳定对象身份派生（而非随机 UUID），
        // 保证同一旧章节在不同设备迁移后得到同一个 generation。
        meta.object_generation = legacy_generation(key);
        changed = true;
    }
    if meta.content_sha256.is_empty() {
        meta.content_sha256 = sha256_hex(&content);
        changed = true;
    }
    // 迁移只补齐 generation/digest，不推进 revision 或 updated_at。
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
    let (mut meta, current, before) = match read_current_migrated(project_root, &key) {
        Ok(state) => state,
        Err(Error::ChapterNotFound) if request.mutation == ChapterMutationKind::Create => {
            let mut meta = request.meta_template.clone().ok_or_else(|| {
                Error::Other(format!(
                    "chapter create requires meta template: {}/{}",
                    key.volume_id, key.chapter_id
                ))
            })?;
            meta.id = key.chapter_id.clone();
            let now = Utc::now().to_rfc3339();
            if meta.created_at.is_empty() {
                meta.created_at = now.clone();
            }
            meta.updated_at = now;
            if meta.object_generation.is_empty() {
                meta.object_generation = Uuid::new_v4().to_string();
            }
            meta.revision = 0;
            meta.word_count = 0;
            meta.hash = md5_hex("");
            meta.content_sha256 = sha256_hex("");
            let before = ChapterVersion {
                object_generation: meta.object_generation.clone(),
                revision: 0,
                content_sha256: sha256_hex(""),
            };
            (meta, String::new(), before)
        }
        Err(error) => return Err(error),
    };
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
    let deleting = request.mutation == ChapterMutationKind::Delete;
    if deleting && request.expected.is_none() {
        return Err(Error::Other(format!(
            "chapter delete requires expected version: {}/{}",
            key.volume_id, key.chapter_id
        )));
    }
    let content_changed = deleting || current != request.content;
    let meta_changed = request
        .meta_title
        .as_ref()
        .is_some_and(|title| title != &meta.title)
        || request.meta_order.is_some_and(|order| order != meta.order);

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

    if request.mutation != ChapterMutationKind::Create
        && !deleting
        && !content_changed
        && !meta_changed
        && changed_additional.is_empty()
        && request.proof_events.is_empty()
        && request.sync_proof_facts.is_empty()
    {
        // 纯 no-op：不创建 journal、不推进 revision/updated_at，返回磁盘真实现状。
        // 有证明事件时仍要建仅事件 journal，否则输入后还原的编辑会丢失。
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
        revision: if content_changed || meta_changed || deleting {
            before.revision.saturating_add(1)
        } else {
            before.revision
        },
        content_sha256: sha256_hex(&request.content),
    };
    let word_count = calculate_word_count(&request.content);
    let updated_at = if content_changed || meta_changed {
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
    if content_changed || meta_changed {
        meta.updated_at = updated_at.clone();
    }
    if let Some(title) = request.meta_title.as_ref() {
        meta.title = title.clone();
    }
    if let Some(order) = request.meta_order {
        meta.order = order;
    }

    ensure_journal_dir(project_root)?;
    mark_pending(project_root)?;
    let commit_sequence = next_commit_sequence(project_root)?;
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

    let trash_path = if deleting {
        let app_data_root = project_root.ancestors().nth(2).ok_or_else(|| {
            Error::Other("chapter delete cannot derive app data root".to_string())
        })?;
        Some(
            app_data_root
                .join("sync/trash")
                .join(format!(
                    "{}_{}_{}",
                    Utc::now().timestamp_millis(),
                    Uuid::new_v4(),
                    key.chapter_id
                ))
                .to_string_lossy()
                .to_string(),
        )
    } else {
        None
    };
    let mut pending_projections = request.projection_ids.clone();
    for fact in &request.sync_proof_facts {
        if !pending_projections.contains(&fact.event_id) {
            pending_projections.push(fact.event_id.clone());
        }
    }
    let mut manifest = ChapterCommitManifest {
        schema_version: 1,
        transaction_id: transaction_id.clone(),
        project_root: project_root.to_string_lossy().to_string(),
        key: key.clone(),
        mutation: request.mutation,
        source: request.source,
        created_at_ms: Utc::now().timestamp_millis(),
        occurred_at_ms: Utc::now().timestamp_millis(),
        commit_sequence,
        event_ids: request.event_ids.clone(),
        proof_events: request.proof_events.clone(),
        sync_proof_facts: request.sync_proof_facts.clone(),
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
        pending_projections,
        created_meta: (request.mutation == ChapterMutationKind::Create).then(|| meta.clone()),
        meta_title: request.meta_title.clone(),
        meta_order: request.meta_order,
        created: request.mutation == ChapterMutationKind::Create,
        deleted: deleting,
        trash_path,
        superseded_reason: None,
    };
    write_manifest(&tx_dir, &manifest)?;

    // 2) 决议 durable。只有这一步成功后才允许触碰 live。
    manifest.phase = ChapterCommitPhase::Decided;
    write_manifest(&tx_dir, &manifest)?;

    // 3) 发布正文/元数据/附加文件投影；删除提交单独发布 trash+tombstone。
    let published = if deleting {
        publish_deleted(project_root, &tx_dir, &mut manifest)?
    } else {
        publish_decided(project_root, &tx_dir, &mut manifest)?
    };
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
        if content_changed || request.mutation == ChapterMutationKind::Create {
            files.push(chapter_relative_path(&key));
        }
        if content_changed || meta_changed || request.mutation == ChapterMutationKind::Create {
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
    let meta_hash = if deleting {
        String::new()
    } else {
        md5_hex(&fs::read_to_string(key.meta_path(project_root))?)
    };

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
