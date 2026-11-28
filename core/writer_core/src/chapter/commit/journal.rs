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

mod manifest;
mod recovery;
pub(crate) use manifest::{
    AdditionalFilePlan, ChapterCommitManifest, ChapterCommitRecovery, ChapterSyncMutationPlan,
};
use recovery::{
    chapter_transaction_manifests, project_manifest_sync_mutations,
    recover_pending_chapter_commits_for_key,
};
pub(crate) use recovery::preserve_superseded_conflict;
pub(crate) use recovery::pending_chapter_proof_transactions_for_key;
pub(crate) use recovery::pending_chapter_proof_payload;
pub use recovery::{
    pending_chapter_draft_acks, pending_chapter_proof_transactions,
    pending_chapter_sync_proof_fact_ids, recover_pending_chapter_commits,
    replay_pending_sync_mutations, PendingChapterProofScan,
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

pub(super) fn sha256_hex(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

pub(super) fn md5_hex(text: &str) -> String {
    format!("{:x}", md5::compute(text.as_bytes()))
}

pub(super) fn journal_root(project_root: &Path) -> PathBuf {
    project_root.join(JOURNAL_DIR)
}

/// 每章事务目录：`chapter-commits/chapters/<volume>/<chapter>/<transaction>/`。
pub(super) fn chapter_transactions_dir(project_root: &Path, key: &ChapterKey) -> PathBuf {
    journal_root(project_root)
        .join(CHAPTERS_DIR)
        .join(&key.volume_id)
        .join(&key.chapter_id)
}

pub(super) fn chapter_transaction_dir(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
) -> PathBuf {
    chapter_transactions_dir(project_root, key).join(transaction_id)
}

/// 冲突回执文件名：只有 manifest 确定的材料全部 durable 后，归档路径才写入。
const CONFLICT_RECEIPT_FILE: &str = "conflict.json";

/// 一笔事务的冲突归档目录（normal 归档与恢复归档共用同一路径）。
pub(super) fn conflict_dir_path(project_root: &Path, transaction_id: &str) -> PathBuf {
    journal_root(project_root).join(CONFLICT_DIR).join(transaction_id)
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
    let next = match counter {
        Some(value) => value.saturating_add(1),
        // 计数器确实缺失/损坏时才扫 journal 重建；正常写入不清点全项目。
        None => max_journal_commit_sequence(project_root)?.saturating_add(1),
    };
    crate::storage::atomic_write_bytes(&path, next.to_string().as_bytes())?;
    Ok(next)
}

fn max_journal_commit_sequence(project_root: &Path) -> Result<u64> {
    let root = journal_root(project_root);
    let mut max_sequence = 0;
    for (_, manifest) in chapter_transaction_manifests(project_root)? {
        max_sequence = max_sequence.max(manifest.commit_sequence);
    }
    if !root.exists() {
        return Ok(max_sequence);
    }
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name().to_string_lossy() == CONFLICT_DIR {
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

pub(super) fn read_current(
    project_root: &Path,
    key: &ChapterKey,
) -> Result<(Chapter, String, ChapterVersion)> {
    let meta_path = key.meta_path(project_root);
    let body_path = key.body_path(project_root);
    let meta_content = match fs::read_to_string(&meta_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::ChapterNotFound)
        }
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter meta {}: {error}",
                meta_path.display()
            )))
        }
    };
    let content = match fs::read_to_string(&body_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::ChapterNotFound)
        }
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter body {}: {error}",
                body_path.display()
            )))
        }
    };
    let meta: Chapter = serde_json::from_str(&meta_content).map_err(|error| {
        Error::Other(format!(
            "invalid chapter meta {}: {error}",
            meta_path.display()
        ))
    })?;
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
    let key = ChapterKey::new(project_id(project_root), volume_id, chapter_id);
    recover_pending_for_read(guard, project_root, &key)?;
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
    _guard: &crate::storage::WorkspaceWriteLock,
    project_root: &Path,
    key: &ChapterKey,
) -> Result<()> {
    // 只读本章定位文件；不依赖全项目 pending 探针，也不枚举其它章。
    recover_pending_chapter_commits_for_key(project_root, key)?;
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

/// Superseded 冲突的 durable 回执（normal 归档与恢复归档共用这一份构造）。
///
/// `before`/`actual` 都取提交决议里的真实版本：`actual` 就是决议时 live 的
/// `before`，恢复入口不再重读 live 生成第二份“猜测”的 actual。`after` 是本次
/// 想发布、但被更新版本放弃的决议。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterConflictReceipt {
    transaction_id: String,
    key: ChapterKey,
    before: ChapterVersion,
    after: ChapterVersion,
    actual: ChapterVersion,
    #[serde(default)]
    expected: Option<ChapterVersion>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    pending_projections: Vec<String>,
}

impl ChapterConflictReceipt {
    fn from_manifest(manifest: &ChapterCommitManifest) -> Self {
        Self {
            transaction_id: manifest.transaction_id.clone(),
            key: manifest.key.clone(),
            before: manifest.before.clone(),
            after: manifest.after.clone(),
            actual: manifest.before.clone(),
            expected: manifest.expected.clone(),
            reason: manifest.superseded_reason.clone(),
            pending_projections: manifest.pending_projections.clone(),
        }
    }

    /// 回执是否确实是同一笔 durable 决议的产物（旧失败留下的回执不算）。
    fn matches_manifest(&self, manifest: &ChapterCommitManifest) -> bool {
        self.transaction_id == manifest.transaction_id
            && self.key == manifest.key
            && self.before == manifest.before
            && self.after == manifest.after
            && self.actual == manifest.before
            && self.reason == manifest.superseded_reason
    }

    fn write(&self, conflict_dir: &Path) -> Result<()> {
        crate::storage::atomic_write_bytes(
            &conflict_dir.join(CONFLICT_RECEIPT_FILE),
            serde_json::to_string_pretty(self)?.as_bytes(),
        )?;
        Ok(())
    }

    /// 读取归档回执；缺失或损坏都按“尚未完成归档”处理（由归档路径覆盖重建）。
    fn read(conflict_dir: &Path) -> Result<Option<Self>> {
        match fs::read_to_string(conflict_dir.join(CONFLICT_RECEIPT_FILE)) {
            Ok(raw) => Ok(serde_json::from_str(&raw).ok()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

fn write_conflict_material(
    project_root: &Path,
    request: &ChapterCommitRequest,
    current: &str,
    actual: &ChapterVersion,
    transaction_id: &str,
) -> Result<String> {
    let root = conflict_dir_path(project_root, transaction_id);
    write_conflict_material_files(
        &root,
        request.base_content.as_deref(),
        current,
        &request.content,
    )?;
    // 没有 journal 决议的 CAS 冲突没有 manifest，只能在材料目录里留版本快照；
    // Superseded 冲突的 before/actual 只由 ChapterConflictReceipt 生成一份。
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

/// 只写三方正文材料，不碰 CAS/空覆盖/正文投影。
fn write_conflict_material_files(
    root: &Path,
    base: Option<&str>,
    local: &str,
    incoming: &str,
) -> Result<()> {
    fs::create_dir_all(root)?;
    if let Some(base) = base {
        crate::storage::atomic_write_bytes(&root.join("base.md"), base.as_bytes())?;
    }
    crate::storage::atomic_write_bytes(&root.join("local.md"), local.as_bytes())?;
    crate::storage::atomic_write_bytes(&root.join("incoming.md"), incoming.as_bytes())?;
    Ok(())
}

/// 编辑会话冲突材料入口：只保存 Core 当前正文（local.md）、原生正文（incoming.md）、
/// 共同基线（base.md，空串也保留）与会话/版本身份，不发布正文、不做版本 CAS。
pub(crate) fn write_editor_conflict_material(
    project_root: &Path,
    request: &EditorConflictMaterialRequest,
) -> Result<EditorConflictMaterialReceipt> {
    let transaction_id = Uuid::new_v4().to_string();
    let root = journal_root(project_root)
        .join(CONFLICT_DIR)
        .join(&transaction_id);
    write_conflict_material_files(
        &root,
        request.base_content.as_deref(),
        &request.core_content,
        &request.native_content,
    )?;
    let metadata = serde_json::json!({
        "kind": "editor-session",
        "projectId": request.key.project_id,
        "volumeId": request.key.volume_id,
        "chapterId": request.key.chapter_id,
        "coreContentLen": request.core_content.len(),
        "nativeContentLen": request.native_content.len(),
        "baseContentLen": request.base_content.as_ref().map(String::len),
        "coreVersion": request.core_version,
        "localVersion": request.local_version,
        "coreSessionId": request.core_session_id,
        "localSessionId": request.local_session_id,
        "coreRevision": request.core_revision,
        "baseRevision": request.base_revision,
        "changeSequence": request.change_sequence,
        "detail": request.detail,
        "createdAt": Utc::now().to_rfc3339(),
    });
    crate::storage::atomic_write_bytes(
        &root.join("editor-session.json"),
        serde_json::to_string_pretty(&metadata)?.as_bytes(),
    )?;
    crate::storage::sync_dir(&root)?;
    Ok(EditorConflictMaterialReceipt {
        transaction_id,
        conflict_dir: root.to_string_lossy().to_string(),
        core_content_sha256: sha256_hex(&request.core_content),
        native_content_sha256: sha256_hex(&request.native_content),
    })
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

fn remove_transaction_dir(
    project_root: &Path,
    key: &ChapterKey,
    transaction_id: &str,
) -> Result<()> {
    let tx_dir = chapter_transaction_dir(project_root, key, transaction_id);
    match fs::remove_dir_all(&tx_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Other(format!(
            "failed to remove chapter commit transaction {}: {error}",
            tx_dir.display()
        ))),
    }
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
    let key = request.key.clone();
    if let Some(guard) = guard {
        recover_pending_for_read(guard, project_root, &key)?;
    }

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
                transaction_id,
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
            commit_sequence: 0,
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
    let commit_sequence = next_commit_sequence(project_root)?;
    let tx_dir = chapter_transaction_dir(project_root, &key, &transaction_id);
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

    // Issue #1062 1.4：删除决议必须拿到调用方传入的真实 app data root / trash
    // 路径；不再从 `project_root.ancestors()` 推断（独立 projects_root 会推错）。
    let deletion_app_data_root = if deleting {
        Some(request.app_data_root.clone().ok_or_else(|| {
            Error::Other(format!(
                "chapter delete commit has no app_data_root: tx={}, key={}/{}/{}",
                request.transaction_id.as_deref().unwrap_or("<new>"),
                key.project_id,
                key.volume_id,
                key.chapter_id
            ))
        })?)
    } else {
        None
    };
    let trash_path = if let Some(app_data_root) = deletion_app_data_root.as_ref() {
        Some(
            request
                .trash_path
                .clone()
                .unwrap_or_else(|| {
                    app_data_root.join("sync/trash").join(format!(
                        "{}_{}_{}",
                        Utc::now().timestamp_millis(),
                        Uuid::new_v4(),
                        key.chapter_id
                    ))
                })
                .to_string_lossy()
                .to_string(),
        )
    } else {
        None
    };
    // 平台草稿负责确认的原始证明事件：同一 durable 决议保存，不进入
    // pending_projections（那套由 Core 自己写统计）。
    let mut draft_ack_ids: Vec<String> = request
        .proof_events
        .iter()
        .map(|fact| fact.event_id().to_string())
        .filter(|event_id| !event_id.is_empty())
        .collect();
    draft_ack_ids.sort();
    draft_ack_ids.dedup();
    let mut pending_projections = request.projection_ids.clone();
    for fact in &request.sync_proof_facts {
        if !pending_projections.contains(&fact.event_id) {
            pending_projections.push(fact.event_id.clone());
        }
    }
    // Issue #1065 复审 #1：同步投影材料与提交决议同时形成（Decided 之前）。
    // 只记录对象路径/op/业务版本/事务 ID；发布字节的摘要由投影时读取已落盘
    // 的目标文件得到（那正是本次决议发布的内容），不另起第二份 outbox 文件。
    let device_id = crate::sync::SyncService::load_sync_state_read_only(project_root, None)
        .map(|state| state.device_id)
        .unwrap_or_default();
    let occurred_at_ms = Utc::now().timestamp_millis();
    let sync_mutations = build_chapter_sync_mutations(
        &key,
        &request,
        &after,
        &additional_plans,
        deleting,
        content_changed,
        meta_changed,
        occurred_at_ms,
        &device_id,
    );
    let mut manifest = ChapterCommitManifest {
        schema_version: 1,
        transaction_id: transaction_id.clone(),
        project_root: project_root.to_string_lossy().to_string(),
        key: key.clone(),
        mutation: request.mutation,
        source: request.source,
        created_at_ms: occurred_at_ms,
        occurred_at_ms,
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
        draft_ack_projections: draft_ack_ids,
        created_meta: (request.mutation == ChapterMutationKind::Create).then(|| meta.clone()),
        meta_title: request.meta_title.clone(),
        meta_order: request.meta_order,
        created: request.mutation == ChapterMutationKind::Create,
        deleted: deleting,
        app_data_root: deletion_app_data_root
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        trash_path,
        superseded_reason: None,
        sync_mutations,
        sync_projection_done: false,
    };
    write_manifest(&tx_dir, &manifest)?;

    // 2) 决议 durable。只有这一步成功后才允许触碰 live；事务目录本身
    // 就是恢复入口，不再维护单独定位。
    manifest.phase = ChapterCommitPhase::Decided;
    write_manifest(&tx_dir, &manifest)?;

    // 3) 发布正文/元数据/附加文件投影；删除提交单独发布 trash+tombstone。
    let published = if deleting {
        publish_deleted(project_root, &tx_dir, &mut manifest)?
    } else {
        publish_decided(project_root, &tx_dir, &mut manifest)?
    };
    if !published {
        // Superseded 回执（conflict.json）已由 mark_superseded 按同一份 durable
        // manifest 归档，这里只补三方合并材料，不再另写一份 before/actual。
        let conflict_dir = conflict_dir_path(project_root, &transaction_id);
        write_conflict_material_files(
            &conflict_dir,
            request.base_content.as_deref(),
            &current,
            &request.content,
        )?;
        crate::storage::sync_dir(&conflict_dir)?;
        // 回执即将返回调用方；归档目录已 durable，源事务不再保留。
        if let Err(error) = remove_transaction_dir(project_root, &key, &transaction_id) {
            log::warn!(
                "failed to clean delivered superseded chapter commit {}: {error}",
                tx_dir.display()
            );
        }
        return Ok(ChapterCommitOutcome::Conflict(ChapterConflict {
            key,
            expected: request.expected,
            actual: before,
            transaction_id,
            conflict_dir: conflict_dir.to_string_lossy().to_string(),
        }));
    }

    // 正文已发布；仍有证明投影时定位继续指向本事务，供本次/后续该章保存
    // 按最早顺序补投影。没有待投影时才清掉正文定位。

    // 4) 先把同步投影按同一决议落进账本；投影未 durable 时 journal 保持
    //    Applied 且不删除，恢复入口用同一份 manifest.sync_mutations 重放。
    let sync_projected = project_manifest_sync_mutations(project_root, &tx_dir, &mut manifest)?;

    // 5) 无滞后投影（Core 统计投影 + 平台草稿 ack）时立即清理；否则保留
    // journal，由调用方逐条确认后清除。
    let mut pending = manifest.pending_projections.clone();
    if sync_projected && pending.is_empty() && manifest.draft_ack_projections.is_empty() {
        manifest.phase = ChapterCommitPhase::Finished;
        write_manifest(&tx_dir, &manifest)?;
        let _ = remove_transaction_dir(project_root, &key, &transaction_id);
    } else {
        // 去重，事件 ID 重发不会制造第二份 pending。
        pending.sort();
        pending.dedup();
        manifest.pending_projections = pending.clone();
        // journal 本身就是这笔事务的 durable 恢复入口：写失败直接返回，
        // 由调用方保留事务目录。
        if let Err(error) = write_manifest(&tx_dir, &manifest) {
            return Err(error);
        }
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
        commit_sequence: manifest.commit_sequence,
    }))
}

mod ack;
pub use ack::{
    mark_chapter_commit_draft_ack, mark_chapter_commit_projection,
    mark_chapter_commit_projections,
};

/// 把一次提交决议要投影的同步对象整理成 durable 材料（复审 #1）。
///
/// 远程同步提交（`ChapterCommitSource::Sync`）是远端内容在本地落地，不是本地
/// 业务变更，不产生任何待上传事实；纯远端删除同理，不再生成 committed deletes。
fn build_chapter_sync_mutations(
    key: &ChapterKey,
    request: &ChapterCommitRequest,
    after: &ChapterVersion,
    additional: &[AdditionalFilePlan],
    deleting: bool,
    content_changed: bool,
    meta_changed: bool,
    occurred_at_ms: i64,
    device_id: &str,
) -> Vec<ChapterSyncMutationPlan> {
    if request.source == crate::chapter::ChapterCommitSource::Sync {
        return Vec::new();
    }
    let mut plans = Vec::new();
    let body_path = chapter_relative_path(key);
    let meta_path = format!(
        "volumes/{}/chapters/{}/{}",
        key.volume_id, key.chapter_id, META_FILE
    );
    if deleting {
        for path in [body_path, meta_path] {
            plans.push(ChapterSyncMutationPlan {
                path,
                op: "delete".to_string(),
                content_hash: String::new(),
                object_revision: None,
                updated_at_ms: occurred_at_ms,
                deleted_at_ms: Some(occurred_at_ms),
                device_id: device_id.to_string(),
            });
        }
        return plans;
    }
    let created = request.mutation == ChapterMutationKind::Create;
    if content_changed || created {
        plans.push(ChapterSyncMutationPlan {
            path: body_path,
            op: "upsert".to_string(),
            content_hash: crate::sync::hash::content_md5(request.content.as_bytes()),
            object_revision: Some(after.revision),
            updated_at_ms: occurred_at_ms,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
        });
    }
    if content_changed || meta_changed || created {
        plans.push(ChapterSyncMutationPlan {
            path: meta_path,
            op: "upsert".to_string(),
            // meta JSON 由发布路径按 live 当前值构造，摘要由投影时读取已落盘
            // 文件得到；决议里固定的是对象与业务版本。
            content_hash: String::new(),
            object_revision: Some(after.revision),
            updated_at_ms: occurred_at_ms,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
        });
    }
    for plan in additional {
        plans.push(ChapterSyncMutationPlan {
            path: plan.target_relative.clone(),
            op: "upsert".to_string(),
            content_hash: String::new(),
            object_revision: None,
            updated_at_ms: occurred_at_ms,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
        });
    }
    plans
}
