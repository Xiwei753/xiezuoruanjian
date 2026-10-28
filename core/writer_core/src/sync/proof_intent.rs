//! 同步正文证明意图日志。
//!
//! 同步提交正文和写入本机证明事件不是同一个原子操作。若正文已经落盘、进程在
//! 证明事件写入前退出，下一次同步会以新正文作为 Prepare 基线，因而不再产生
//! 同一章节的 Apply/Delete；原 `sync_event_id`、before 快照和来源设备就永久
//! 丢失了。
//!
//! 本模块在正文 Commit **之前**把一次同步编辑的稳定事实写到
//! `stats/proof-intents/`（不参与远端作品同步）。Commit 成功后按同一事件 ID
//! 幂等写入 `stats/events`，完成才删除意图。启动、编辑和下次同步前都可以用
//! 意图对照 live 正文补记，崩溃发生在任何一步都能收敛到
//! 「真实正文版本 ↔ 来源清楚的编辑历史」。

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::sync::commit_helpers::CommittedBefore;
use crate::sync::staging::CommitAction;

/// 意图日志目录（相对 app_data_root）。
pub(crate) const PROOF_INTENTS_DIR: &str = "stats/proof-intents";

const INTENT_FORMAT_VERSION: u32 = 1;

/// 本次同步动作类型。`Delete` 的 after 正文为空且 live 文件应不存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SyncProofAction {
    Apply,
    Delete,
}

/// 一次尚未确认写入证明事件的本机同步编辑事实。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SyncProofIntent {
    pub format_version: u32,
    /// 稳定事件 ID：`sync:<run_id>:<workspace path>:<action index>`。
    pub event_id: String,
    pub run_id: String,
    /// 章节动作在本次 Commit 动作列表中的序号（与事件 ID 尾部一致）。
    pub action_index: usize,
    pub project_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    /// workspace-relative 路径，例如 `projects/<pid>/volumes/<vid>/chapters/<cid>/chapter.md`。
    pub workspace_path: String,
    pub action: SyncProofAction,
    pub before_text: Option<String>,
    pub after_text: Option<String>,
    /// 正文内容根摘要，恢复时可对照 live 正文确认提交版本。
    pub before_content_digest: Option<String>,
    pub after_content_digest: Option<String>,
    pub source_device_id: String,
    /// 事件发生时间。恢复时必须复用，保证日期分区和事件身份稳定。
    pub occurred_at_ms: i64,
}

impl SyncProofIntent {
    pub(crate) fn is_delete(&self) -> bool {
        self.action == SyncProofAction::Delete
    }

    /// 把 workspace 路径映射回 live 正文文件；只接受 `projects/<pid>/...`。
    pub(crate) fn live_path(&self, projects_root: &Path) -> Option<PathBuf> {
        let components = Path::new(&self.workspace_path)
            .components()
            .map(|component| match component {
                Component::Normal(value) => value.to_str().map(str::to_string),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        if components.len() < 3 || components[0] != "projects" {
            return None;
        }
        Some(
            components[1..]
                .iter()
                .fold(projects_root.to_path_buf(), |path, component| {
                    path.join(component)
                }),
        )
    }
}

/// Commit 前写入意图日志的入口。
pub(crate) struct ProofIntentSink<'a> {
    root: &'a Path,
    /// `project/volume/chapter` -> 来源设备。
    source_devices: &'a HashMap<String, String>,
}

impl<'a> ProofIntentSink<'a> {
    pub(crate) fn new(root: &'a Path, source_devices: &'a HashMap<String, String>) -> Self {
        Self {
            root,
            source_devices,
        }
    }

    /// 在正文事务提交前写入所有有效的章节正文动作。
    ///
    /// `before` 与 `actions` 一一对应，来自事务提交前的真实快照。非章节路径、
    /// 读取失败、非 UTF-8 或无内容变化的动作不生成意图（正常证明路径也会跳过）。
    pub(crate) fn prepare(
        &self,
        run_id: &str,
        target_kind: &str,
        project_id: Option<&str>,
        actions: &[CommitAction],
        before: &[CommittedBefore],
    ) -> crate::Result<()> {
        for (index, action) in actions.iter().enumerate() {
            let Some((rel_path, action, after_text)) = action_edit(action) else {
                continue;
            };
            if !is_chapter_content_path(rel_path) {
                continue;
            }
            let before_text = match before.get(index) {
                Some(CommittedBefore::Missing) => None,
                Some(CommittedBefore::Text(bytes)) => match std::str::from_utf8(bytes) {
                    Ok(text) => Some(text.to_string()),
                    Err(_) => continue,
                },
                Some(CommittedBefore::NotTracked | CommittedBefore::Error(_)) | None => continue,
            };
            if before_text.as_deref().unwrap_or("") == after_text.as_deref().unwrap_or("") {
                continue;
            }
            let workspace_path = to_workspace_rel_path(target_kind, project_id, rel_path);
            let Some((project_id, volume_id, chapter_id)) = chapter_path_parts(&workspace_path)
            else {
                continue;
            };
            let source_device_id = self
                .source_devices
                .get(&format!("{project_id}/{volume_id}/{chapter_id}"))
                .cloned()
                .unwrap_or_default();
            let event_id = format!("sync:{run_id}:{}:{index}", workspace_path.to_string_lossy());
            // 同一意图重跑时保留第一次的时间，避免事件身份漂移。
            // 旧意图损坏时不能让一次正文同步永久失败：按新意图重写即可。
            let occurred_at_ms = match self.load(&event_id) {
                Ok(Some(intent)) => intent.occurred_at_ms,
                Ok(None) => chrono::Utc::now().timestamp_millis(),
                Err(error) => {
                    log::warn!(
                        "[proof_intent] 读取旧意图失败，将重写 {}: {error}",
                        event_id
                    );
                    chrono::Utc::now().timestamp_millis()
                }
            };
            let before_content_digest = before_text
                .as_deref()
                .map(crate::writing_stats::proof::content_digest_of_text);
            let after_content_digest = after_text
                .as_deref()
                .map(crate::writing_stats::proof::content_digest_of_text);
            let intent = SyncProofIntent {
                format_version: INTENT_FORMAT_VERSION,
                event_id,
                run_id: run_id.to_string(),
                action_index: index,
                project_id,
                volume_id,
                chapter_id,
                workspace_path: workspace_path.to_string_lossy().into_owned(),
                action,
                before_text,
                after_text,
                before_content_digest,
                after_content_digest,
                source_device_id,
                occurred_at_ms,
            };
            let serialized = serde_json::to_string(&intent)?;
            crate::storage::atomic_write_string(
                &intent_path(self.root, &intent.event_id),
                &serialized,
            )?;
        }
        Ok(())
    }

    /// 读取本次事件对应的意图，用于 Commit 后复用原始时间和来源设备。
    pub(crate) fn load(&self, event_id: &str) -> crate::Result<Option<SyncProofIntent>> {
        load_intent(self.root, event_id)
    }

    /// 删除已写入证明事件的意图；文件不存在视为已完成。
    pub(crate) fn remove(&self, event_id: &str) -> crate::Result<()> {
        remove_intent(self.root, event_id)
    }
}

fn action_edit(action: &CommitAction) -> Option<(&Path, SyncProofAction, Option<String>)> {
    match action {
        CommitAction::Apply { rel_path, content } => {
            std::str::from_utf8(content).ok().map(|text| {
                (
                    rel_path.as_path(),
                    SyncProofAction::Apply,
                    Some(text.to_string()),
                )
            })
        }
        CommitAction::Delete { rel_path } => {
            Some((rel_path.as_path(), SyncProofAction::Delete, None))
        }
    }
}

fn is_chapter_content_path(path: &Path) -> bool {
    let mut has_chapters = false;
    let mut last = None;
    for component in path.components() {
        let part = component.as_os_str();
        if part == "chapters" {
            has_chapters = true;
        }
        last = Some(part);
    }
    has_chapters && last.is_some_and(|name| name == "chapter.md")
}

fn to_workspace_rel_path(target_kind: &str, project_id: Option<&str>, rel_path: &Path) -> PathBuf {
    if target_kind == "project" || target_kind == "deleted_project" {
        if let Some(pid) = project_id {
            return PathBuf::from("projects").join(pid).join(rel_path);
        }
    }
    PathBuf::from(rel_path)
}

fn chapter_path_parts(path: &Path) -> Option<(String, String, String)> {
    let parts = path
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()?;
    let project = parts.iter().position(|part| part == "projects")?;
    let volume = parts.iter().position(|part| part == "volumes")?;
    let chapter = parts.iter().position(|part| part == "chapters")?;
    if parts.get(chapter + 2).map(String::as_str) != Some("chapter.md") {
        return None;
    }
    Some((
        parts.get(project + 1)?.clone(),
        parts.get(volume + 1)?.clone(),
        parts.get(chapter + 1)?.clone(),
    ))
}

fn intent_filename(event_id: &str) -> String {
    let digest = Sha256::digest(event_id.as_bytes());
    let mut name = String::with_capacity(digest.len() * 2 + 5);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(name, "{byte:02x}");
    }
    name.push_str(".json");
    name
}

fn intent_path(root: &Path, event_id: &str) -> PathBuf {
    root.join(intent_filename(event_id))
}

/// 列出全部待恢复意图，按事件时间升序（先发生的先补记）。
pub(crate) fn load_pending(root: &Path) -> crate::Result<Vec<SyncProofIntent>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut intents = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log::warn!("[proof_intent] 读取意图目录项失败: {error}");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        match fs::read_to_string(&path)
            .map_err(crate::Error::from)
            .and_then(|text| serde_json::from_str::<SyncProofIntent>(&text).map_err(Into::into))
        {
            Ok(intent) if intent.format_version == INTENT_FORMAT_VERSION => intents.push(intent),
            Ok(intent) => {
                log::warn!(
                    "[proof_intent] 忽略未知格式版本的意图 {}: {}",
                    path.display(),
                    intent.format_version
                );
            }
            Err(error) => {
                log::warn!("[proof_intent] 跳过损坏的意图 {}: {error}", path.display());
            }
        }
    }
    intents.sort_by_key(|intent| intent.occurred_at_ms);
    Ok(intents)
}

pub(crate) fn has_pending(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .path()
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| extension == "json")
    })
}

fn load_intent(root: &Path, event_id: &str) -> crate::Result<Option<SyncProofIntent>> {
    let path = intent_path(root, event_id);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn remove_intent(root: &Path, event_id: &str) -> crate::Result<()> {
    let path = intent_path(root, event_id);
    match fs::remove_file(&path) {
        Ok(()) => {
            crate::storage::sync_parent(&path)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

enum LiveCommitState {
    Committed,
    NotCommitted,
    Unknown,
}

fn remove_completed_intent(root: &Path, event_id: &str) -> bool {
    match remove_intent(root, event_id) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("[proof_intent] 清理意图 {event_id} 失败: {error}");
            false
        }
    }
}

fn live_commit_state(intent: &SyncProofIntent, projects_root: &Path) -> LiveCommitState {
    let Some(live_path) = intent.live_path(projects_root) else {
        log::warn!(
            "[proof_intent] 无法映射意图的 live 路径: {}",
            intent.workspace_path
        );
        return LiveCommitState::Unknown;
    };
    if intent.is_delete() {
        return match fs::metadata(&live_path) {
            Ok(_) => LiveCommitState::NotCommitted,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                LiveCommitState::Committed
            }
            Err(error) => {
                log::warn!(
                    "[proof_intent] 检查删除结果失败 {}: {error}",
                    live_path.display()
                );
                LiveCommitState::Unknown
            }
        };
    }
    match fs::read_to_string(&live_path) {
        Ok(text)
            if intent.after_text.as_deref() == Some(text.as_str())
                && intent.after_content_digest.as_deref().is_none_or(|digest| {
                    digest == crate::writing_stats::proof::content_digest_of_text(&text)
                }) =>
        {
            LiveCommitState::Committed
        }
        Ok(_) => LiveCommitState::NotCommitted,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LiveCommitState::NotCommitted,
        Err(error) => {
            log::warn!(
                "[proof_intent] 读取恢复正文失败 {}: {error}",
                live_path.display()
            );
            LiveCommitState::Unknown
        }
    }
}

/// 恢复所有未完成意图。
///
/// - 事件已持久化：删除意图。
/// - live 正文已经等于意图 after（删除动作则文件已不存在）：按原事件 ID、
///   原时间戳、原来源设备补记，成功后删除意图。
/// - live 正文不是意图 after：正文 Commit 没有发生（或被后续版本覆盖），
///   删除意图，不伪造编辑事实。
///
/// 补记失败保留意图，下次启动/编辑/同步继续重试。
pub(crate) fn recover_pending(core: &crate::facade::WriterCore) -> crate::Result<usize> {
    let root = core.app_data_root().join(PROOF_INTENTS_DIR);
    let pending = load_pending(&root)?;
    let mut recovered = 0usize;
    for intent in pending {
        let stats = core.get_stats_api();
        match stats
            .store()
            .proof_event_persisted(&intent.event_id, intent.occurred_at_ms)
        {
            Ok(true) => {
                if remove_completed_intent(&root, &intent.event_id) {
                    recovered = recovered.saturating_add(1);
                }
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                log::warn!(
                    "[proof_intent] 检查事件是否已落盘失败 {}: {error}",
                    intent.event_id
                );
                continue;
            }
        }

        match live_commit_state(&intent, core.projects_root()) {
            LiveCommitState::Committed => {}
            LiveCommitState::NotCommitted => {
                // 正文没有落在意图版本上；这个意图不对应任何真实提交，不能补写。
                remove_completed_intent(&root, &intent.event_id);
                continue;
            }
            LiveCommitState::Unknown => continue,
        }

        match core.record_sync_content_change_at(
            &intent.project_id,
            &intent.volume_id,
            &intent.chapter_id,
            intent.before_text.as_deref(),
            intent.after_text.as_deref(),
            &intent.event_id,
            &intent.source_device_id,
            "",
            intent.occurred_at_ms,
        ) {
            Ok(_) => {
                if remove_completed_intent(&root, &intent.event_id) {
                    recovered = recovered.saturating_add(1);
                }
            }
            Err(error) => {
                log::warn!(
                    "[proof_intent] 恢复同步证明事件失败 {}: {error}",
                    intent.event_id
                );
            }
        }
    }
    Ok(recovered)
}
