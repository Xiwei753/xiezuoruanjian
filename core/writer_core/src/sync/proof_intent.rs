//! 同步正文证明意图日志。
//!
//! 同步提交正文和写入本机证明事件不是同一个原子操作。若正文已经落盘、进程在
//! 证明事件写入前退出，下一次同步会以新正文作为 Prepare 基线，因而不再产生
//! 同一章节的 Apply/Delete；原 `sync_event_id`、before 快照和来源设备就永久
//! 丢失了。
//!
//! 本模块在正文 Commit **之前**把一次同步编辑的稳定事实写到
//! `stats/proof-intents/`（不参与远端作品同步）。正文 `SaveTransaction` 会在
//! manifest 中链接同一批事件 ID；只有事务进入 `FilesCommitted`/`Finished`，
//! 恢复流程才按原事件 ID、原时间戳和来源补记证明。因此“后来正文又被覆盖”
//! 不会再被误判成“这次正文从未提交”。恢复顺序按 `occurred_at_ms`，先发生的
//! 事件先补记，保证证明链不因积压而错位。
//!
//! 损坏的意图文件不会被静默删除或反复按按键解析：它们保留原文件与错误信息，
//! 由启动/同步恢复入口报告，编辑热路径只检查轻量原子状态并做节流重试。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::transaction::{scan_pending_transactions, TransactionPhase};
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
    /// 本次正文提交所在的 target 类型：`app` / `project` / `deleted_project`。
    /// 恢复时据此定位真正的 `SaveTransaction` 目录；旧意图无此字段。
    #[serde(default)]
    pub target_kind: String,
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
}

/// 一个无法解析/未知版本的意图文件。文件本身保留，等待修复或后续版本处理。
#[derive(Debug, Clone)]
pub(crate) struct ProofIntentLoadError {
    pub path: PathBuf,
    pub message: String,
}

/// 一次意图目录扫描结果。
#[derive(Debug, Default)]
pub(crate) struct PendingProofIntents {
    pub intents: Vec<SyncProofIntent>,
    pub errors: Vec<ProofIntentLoadError>,
}

/// Commit 前写入意图日志的入口。
pub(crate) struct ProofIntentSink<'a> {
    root: &'a Path,
    app_data_root: &'a Path,
    projects_root: &'a Path,
    /// `project/volume/chapter` -> 来源设备。
    source_devices: &'a HashMap<String, String>,
}

impl<'a> ProofIntentSink<'a> {
    pub(crate) fn new(
        root: &'a Path,
        app_data_root: &'a Path,
        projects_root: &'a Path,
        source_devices: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            root,
            app_data_root,
            projects_root,
            source_devices,
        }
    }

    /// 在正文事务提交前写入所有章节正文动作的意图。
    ///
    /// `before` 与 `actions` 一一对应，来自事务提交前的真实快照。只有非章节
    /// 路径和实际无内容变化的动作可以跳过；章节动作的 before/after 读取、
    /// UTF-8 解码或路径映射失败都必须中止本次 prepare，绝不能让正文在没有
    /// 对应意图的情况下提交。
    pub(crate) fn prepare(
        &self,
        run_id: &str,
        target_kind: &str,
        project_id: Option<&str>,
        actions: &[CommitAction],
        before: &[CommittedBefore],
    ) -> crate::Result<Vec<String>> {
        let mut created_ids = Vec::new();
        let result = (|| -> crate::Result<Vec<String>> {
            let mut prepared_ids = Vec::new();
            for (index, action) in actions.iter().enumerate() {
                let rel_path = action_rel_path(action);
                // 非章节文件不参与证明，也不需要读取/解码正文。
                if !is_chapter_content_path(rel_path) {
                    continue;
                }
                let workspace_path = to_workspace_rel_path(target_kind, project_id, rel_path);
                let Some((project_id, volume_id, chapter_id)) = chapter_path_parts(&workspace_path)
                else {
                    return Err(crate::Error::Io(std::io::Error::other(format!(
                        "同步章节动作路径无法映射到作品/卷/章: {}",
                        workspace_path.display()
                    ))));
                };
                let before_snapshot = before.get(index).ok_or_else(|| {
                    crate::Error::Io(std::io::Error::other(format!(
                        "同步章节 {} 缺少 commit 前置快照位置 {}",
                        workspace_path.display(),
                        index
                    )))
                })?;
                let before_text = match before_snapshot {
                    CommittedBefore::Missing => None,
                    CommittedBefore::Text(bytes) => Some(
                        std::str::from_utf8(bytes)
                            .map_err(|error| {
                                crate::Error::Io(std::io::Error::other(format!(
                                    "同步章节 {} 的 before 正文不是 UTF-8: {}",
                                    workspace_path.display(),
                                    error
                                )))
                            })?
                            .to_string(),
                    ),
                    CommittedBefore::NotTracked => {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "同步章节 {} 缺少 commit 前置快照",
                            workspace_path.display()
                        ))));
                    }
                    CommittedBefore::Error(message) => {
                        return Err(crate::Error::Io(std::io::Error::other(format!(
                            "同步章节 {} 的 before 读取失败: {}",
                            workspace_path.display(),
                            message
                        ))));
                    }
                };
                let (proof_action, after_text) = match action {
                    CommitAction::Apply { content, .. } => {
                        let text = std::str::from_utf8(content).map_err(|error| {
                            crate::Error::Io(std::io::Error::other(format!(
                                "同步章节 {} 的 after 正文不是 UTF-8: {}",
                                workspace_path.display(),
                                error
                            )))
                        })?;
                        (SyncProofAction::Apply, Some(text.to_string()))
                    }
                    CommitAction::Delete { .. } => (SyncProofAction::Delete, None),
                };
                if before_text.as_deref().unwrap_or("") == after_text.as_deref().unwrap_or("") {
                    continue;
                }
                let source_device_id = self
                    .source_devices
                    .get(&format!("{project_id}/{volume_id}/{chapter_id}"))
                    .cloned()
                    .unwrap_or_default();
                let event_id =
                    format!("sync:{run_id}:{}:{index}", workspace_path.to_string_lossy());
                // 同一意图重跑时保留第一次的时间，避免事件身份漂移。旧意图损坏时
                // 不能覆盖它：否则原始时间/来源会被新时间取代，幂等性被破坏。
                let existing = self.load(&event_id)?;
                let created = existing.is_none();
                let occurred_at_ms = existing
                    .map(|intent| intent.occurred_at_ms)
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
                let before_content_digest = before_text
                    .as_deref()
                    .map(crate::writing_stats::proof::content_digest_of_text);
                let after_content_digest = after_text
                    .as_deref()
                    .map(crate::writing_stats::proof::content_digest_of_text);
                let intent = SyncProofIntent {
                    format_version: INTENT_FORMAT_VERSION,
                    event_id: event_id.clone(),
                    run_id: run_id.to_string(),
                    action_index: index,
                    project_id,
                    volume_id,
                    chapter_id,
                    workspace_path: workspace_path.to_string_lossy().into_owned(),
                    target_kind: target_kind.to_string(),
                    action: proof_action,
                    before_text,
                    after_text,
                    before_content_digest,
                    after_content_digest,
                    source_device_id,
                    occurred_at_ms,
                };
                let serialized = serde_json::to_string(&intent)?;
                mark_pending(self.root)?;
                crate::storage::atomic_write_string(
                    &intent_path(self.root, &intent.event_id),
                    &serialized,
                )?;
                prepared_ids.push(intent.event_id);
                if created {
                    created_ids.push(event_id);
                }
            }
            Ok(prepared_ids)
        })();
        if result.is_err() {
            // prepare 失败时事务尚未创建，本次新写入的意图没有事务链接，
            // 必须立即清理；否则下一次恢复会把它们当成待提交事实。
            for event_id in created_ids {
                if let Err(error) = remove_intent(self.root, &event_id) {
                    log::warn!(
                        "[proof_intent] 清理 prepare 失败后的意图 {} 失败: {error}",
                        event_id
                    );
                }
            }
            // 清理后重新校准标记；若目录里还有更早的待恢复意图，标记会保持。
            if let Err(error) = refresh_pending(self.root) {
                log::warn!("[proof_intent] prepare 失败后刷新待恢复标记失败: {error}");
            }
        }
        result
    }

    /// 读取本次事件对应的意图，用于 Commit 后复用原始时间和来源设备。
    pub(crate) fn load(&self, event_id: &str) -> crate::Result<Option<SyncProofIntent>> {
        load_intent(self.root, event_id)
    }

    /// 删除已写入证明事件的意图；文件不存在视为已完成。
    pub(crate) fn remove(&self, event_id: &str) -> crate::Result<()> {
        remove_intent(self.root, event_id)
    }

    /// 当前目标项目是否仍有未恢复意图（含无法解析的意图）。
    ///
    /// 同步 Commit 在恢复后调用；只要有积压，就不允许继续修改同一项目正文，
    /// 否则新事件会越过旧事件，证明链无法按发生顺序接续。
    pub(crate) fn has_unresolved_for_project(
        &self,
        project_id: Option<&str>,
    ) -> crate::Result<bool> {
        let pending = load_pending(self.root)?;
        if !pending.errors.is_empty() {
            return Ok(true);
        }
        if pending
            .intents
            .iter()
            .any(|intent| project_id.is_none_or(|pid| intent.project_id == pid))
        {
            return Ok(true);
        }
        // 事务目录仍保留，说明还有证明未完成。App target 的事务在 app_data_root，
        // Project target 的事务在 projects_root/<pid>，两者都要检查。
        let mut transaction_roots = vec![self.app_data_root.to_path_buf()];
        if let Some(pid) = project_id {
            transaction_roots.push(self.projects_root.join(pid));
        }
        for transaction_root in transaction_roots {
            let scan = scan_pending_transactions(&transaction_root);
            if !scan.errors.is_empty() {
                return Ok(true);
            }
            for transaction in &scan.transactions {
                for event_id in &transaction.proof_intent_ids {
                    match load_intent(self.root, event_id) {
                        Ok(Some(intent))
                            if project_id.is_none_or(|pid| intent.project_id == pid) =>
                        {
                            return Ok(true);
                        }
                        Ok(Some(_)) => {}
                        Ok(None) => {
                            log::warn!(
                                "[proof_intent] 事务 {} 缺少意图文件 {}，保留待人工恢复",
                                transaction.transaction_id,
                                event_id
                            );
                            return Ok(true);
                        }
                        Err(error) => {
                            log::warn!(
                                "[proof_intent] 事务 {} 的意图 {} 无法读取: {error}",
                                transaction.transaction_id,
                                event_id
                            );
                            return Ok(true);
                        }
                    }
                }
            }
        }
        Ok(false)
    }

    /// 清理已经完成证明、但仍因链接意图而保留的正文事务目录。
    ///
    /// 返回仍被保留事务引用的意图 ID；调用方在删除意图文件前必须排除这些 ID，
    /// 否则事务目录还在、意图文件却已消失，下一次恢复无法判定正文是否提交。
    pub(crate) fn cleanup_completed_transactions(
        &self,
        transaction_root: &Path,
        completed_event_ids: &HashSet<String>,
    ) -> HashSet<String> {
        cleanup_completed_transactions(transaction_root, completed_event_ids)
    }
}

fn action_rel_path(action: &CommitAction) -> &Path {
    match action {
        CommitAction::Apply { rel_path, .. } => rel_path.as_path(),
        CommitAction::Delete { rel_path } => rel_path.as_path(),
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

fn pending_marker_path(root: &Path) -> PathBuf {
    root.join(".pending")
}

/// 在写入任何意图文件前建立轻量待恢复标记。
///
/// 编辑热路径只 stat 这个标记，不解析目录；标记先于意图落盘，崩溃后即使意图
/// 尚未写完也能被启动/同步恢复入口发现并报告。
fn mark_pending(root: &Path) -> crate::Result<()> {
    fs::create_dir_all(root)?;
    let marker = pending_marker_path(root);
    if !marker.is_file() {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&marker)?;
        crate::storage::sync_parent(&marker)?;
    }
    Ok(())
}

fn clear_pending_marker(root: &Path) -> crate::Result<()> {
    match fs::remove_file(pending_marker_path(root)) {
        Ok(()) => crate::storage::sync_parent(root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn update_pending_marker(root: &Path, pending: bool) -> crate::Result<()> {
    if pending {
        mark_pending(root)
    } else {
        clear_pending_marker(root)
    }
}

fn components(path: &Path) -> Option<Vec<String>> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect()
}

fn transaction_root_for_intent(
    intent: &SyncProofIntent,
    projects_root: &Path,
    app_data_root: &Path,
) -> PathBuf {
    match intent.target_kind.as_str() {
        "project" | "deleted_project" => projects_root.join(&intent.project_id),
        "app" => app_data_root.to_path_buf(),
        _ => {
            // 旧意图没有 target_kind：按 workspace 路径推断 project target。
            let Some(parts) = components(Path::new(&intent.workspace_path)) else {
                return app_data_root.to_path_buf();
            };
            if parts.first().map(String::as_str) == Some("projects") && parts.len() >= 2 {
                projects_root.join(&parts[1])
            } else {
                app_data_root.to_path_buf()
            }
        }
    }
}

/// 列出全部待恢复意图，按事件时间升序（先发生的先补记）。
///
/// 损坏/未知版本的 `.json` 不进入 `intents`，而是以 [`ProofIntentLoadError`]
/// 返回；调用方必须保留原文件并报告，不能静默当作已恢复。
pub(crate) fn load_pending(root: &Path) -> crate::Result<PendingProofIntents> {
    let mut pending = PendingProofIntents::default();
    if !root.is_dir() {
        return Ok(pending);
    }
    for entry in fs::read_dir(root)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                pending.errors.push(ProofIntentLoadError {
                    path: root.to_path_buf(),
                    message: format!("读取意图目录项失败: {error}"),
                });
                continue;
            }
        };
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let parsed = fs::read_to_string(&path)
            .map_err(crate::Error::from)
            .and_then(|text| {
                serde_json::from_str::<SyncProofIntent>(&text).map_err(crate::Error::from)
            });
        match parsed {
            Ok(intent) if intent.format_version == INTENT_FORMAT_VERSION => {
                pending.intents.push(intent);
            }
            Ok(intent) => {
                pending.errors.push(ProofIntentLoadError {
                    path,
                    message: format!("未知的意图格式版本: {}", intent.format_version),
                });
            }
            Err(error) => {
                pending.errors.push(ProofIntentLoadError {
                    path,
                    message: error.to_string(),
                });
            }
        }
    }
    pending.intents.sort_by_key(|intent| intent.occurred_at_ms);
    Ok(pending)
}

/// 编辑热路径的轻量检查：只看维护好的标记文件，不解析意图目录。
///
/// 标记由写意图、启动/同步恢复入口维护；即使损坏文件令标记保持为真，编辑路径
/// 也只会在节流窗口内扫描，不会每键遍历整个目录。
pub(crate) fn has_pending(root: &Path) -> bool {
    pending_marker_path(root).is_file()
}

/// 重新扫描意图目录，报告无法解析的文件，并同步待恢复标记。
///
/// 该入口只用于启动/同步等低频路径；编辑热路径不得调用。
pub(crate) fn refresh_pending(root: &Path) -> crate::Result<()> {
    let pending = load_pending(root)?;
    update_pending_marker(
        root,
        !pending.intents.is_empty() || !pending.errors.is_empty(),
    )?;
    for error in pending.errors {
        log::warn!(
            "[proof_intent] 未恢复的意图文件 {}: {}",
            error.path.display(),
            error.message
        );
    }
    Ok(())
}

fn load_intent(root: &Path, event_id: &str) -> crate::Result<Option<SyncProofIntent>> {
    let path = intent_path(root, event_id);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let intent: SyncProofIntent = serde_json::from_str(&text)?;
            if intent.format_version != INTENT_FORMAT_VERSION {
                return Err(crate::Error::Io(std::io::Error::other(format!(
                    "证明意图 {} 格式版本不受支持: {}",
                    path.display(),
                    intent.format_version
                ))));
            }
            Ok(Some(intent))
        }
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

fn remove_completed_intent(root: &Path, event_id: &str) -> bool {
    match remove_intent(root, event_id) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("[proof_intent] 清理意图 {event_id} 失败: {error}");
            false
        }
    }
}

/// 恢复所有未完成意图。
///
/// 判定正文是否提交只依据正文 `SaveTransaction` 的持久 phase，不再依据
/// “当前 live 正文是否等于 after”：
/// - `FilesCommitted`/`Finished`：正文已提交，即使后来被覆盖也按原事件补记。
/// - 事务目录不存在，且扫描没有错误：正文从未进入提交，删除无效意图。
/// - `Prepared` 且重放材料不完整，或事务目录无法读取：状态未知，保留意图。
///
/// 事件已落盘的意图直接清理。补记失败保留意图，下次启动/同步继续重试。
pub(crate) fn recover_pending(core: &crate::facade::WriterCore) -> crate::Result<usize> {
    let root = core.app_data_root().join(PROOF_INTENTS_DIR);
    let pending = load_pending(&root)?;
    for error in &pending.errors {
        log::warn!(
            "[proof_intent] 未恢复的意图文件 {}: {}",
            error.path.display(),
            error.message
        );
    }

    let mut scans = HashMap::new();
    for intent in &pending.intents {
        let transaction_root =
            transaction_root_for_intent(intent, core.projects_root(), core.app_data_root());
        scans
            .entry(transaction_root.clone())
            .or_insert_with(|| scan_pending_transactions(&transaction_root));
    }

    let mut committed = HashMap::new();
    let mut blocked_ids = HashSet::new();
    let mut blocked_roots = HashSet::new();
    for (transaction_root, scan) in &scans {
        if !scan.errors.is_empty() {
            for error in &scan.errors {
                log::warn!(
                    "[proof_intent] 正文事务状态未知（{}）: {error}",
                    transaction_root.display()
                );
            }
            blocked_roots.insert(transaction_root.clone());
        }
        for transaction in &scan.transactions {
            match transaction.phase {
                TransactionPhase::FilesCommitted | TransactionPhase::Finished => {
                    for event_id in &transaction.proof_intent_ids {
                        committed.insert(
                            event_id.clone(),
                            (transaction_root.clone(), transaction.clone()),
                        );
                    }
                }
                TransactionPhase::Prepared => {
                    blocked_ids.extend(transaction.proof_intent_ids.iter().cloned());
                }
                TransactionPhase::RolledBack => {}
            }
        }
    }

    let mut recovered = 0usize;
    let mut completed_ids: HashSet<String> = HashSet::new();
    let mut to_remove: HashSet<String> = HashSet::new();
    for intent in pending.intents {
        let stats = core.get_stats_api();
        match stats
            .store()
            .proof_event_persisted(&intent.event_id, intent.occurred_at_ms)
        {
            Ok(true) => {
                completed_ids.insert(intent.event_id.clone());
                to_remove.insert(intent.event_id.clone());
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

        let transaction_root =
            transaction_root_for_intent(&intent, core.projects_root(), core.app_data_root());
        if let Some((_, transaction)) = committed.get(&intent.event_id) {
            log::debug!(
                "[proof_intent] 按已提交事务 {} 补记事件 {}",
                transaction.transaction_id,
                intent.event_id
            );
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
                    completed_ids.insert(intent.event_id.clone());
                    to_remove.insert(intent.event_id.clone());
                }
                Err(error) => {
                    log::warn!(
                        "[proof_intent] 恢复同步证明事件失败 {}: {error}",
                        intent.event_id
                    );
                }
            }
            continue;
        }

        if blocked_ids.contains(&intent.event_id) || blocked_roots.contains(&transaction_root) {
            log::warn!(
                "[proof_intent] 正文事务状态未确认，保留意图 {}（{}）",
                intent.event_id,
                intent.workspace_path
            );
            continue;
        }

        // 新格式意图在 prepare 时已写入 target_kind，正文提交必须经过同一
        // SaveTransaction 链接。如果事务根里既没有该 ID 的已提交记录，也没有
        // Prepared/读取错误，说明事务从未创建或明确回滚：按未提交清理，不能
        // 再用 live 正文猜测，否则未提交的新章节会永久卡住同步，或把别的操作
        // 恰好写出的 after 误记成本次同步。
        if !intent.target_kind.is_empty() {
            log::warn!(
                "[proof_intent] 新格式意图没有对应提交事务，按未提交清理 {}（{}）",
                intent.event_id,
                intent.workspace_path
            );
            to_remove.insert(intent.event_id.clone());
            continue;
        }

        // 兼容没有事务链接的旧意图：只有能证明正文仍停在 before 时才清理。
        // 若正文已经是 after，说明旧版本已经提交但证明漏写，必须补记；
        // 其他文本视为状态未知，保留意图等待人工/后续恢复。
        match legacy_live_state(&intent, core.projects_root()) {
            LegacyLiveState::Uncommitted => {
                to_remove.insert(intent.event_id.clone());
            }
            LegacyLiveState::Committed => {
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
                        to_remove.insert(intent.event_id.clone());
                    }
                    Err(error) => log::warn!(
                        "[proof_intent] 补记旧同步证明事件失败 {}: {error}",
                        intent.event_id
                    ),
                }
            }
            LegacyLiveState::Unknown => log::warn!(
                "[proof_intent] 旧意图缺少事务阶段且正文无法确认，保留意图 {}",
                intent.event_id
            ),
        }
    }

    let mut retained_ids: HashSet<String> = HashSet::new();
    for transaction_root in scans.keys() {
        retained_ids.extend(cleanup_completed_transactions(
            transaction_root,
            &completed_ids,
        ));
    }
    // 事务目录仍在时不能删除它引用的意图文件，否则下次恢复会看到
    // “已提交事务 + 缺失意图”的不可判定状态。
    to_remove.retain(|event_id| !retained_ids.contains(event_id));
    for event_id in &to_remove {
        if remove_completed_intent(&root, event_id) {
            recovered = recovered.saturating_add(1);
        }
    }
    Ok(recovered)
}

/// 删除所有“事务已提交、且全部链接事件证明已写入”的事务目录。
///
/// 只依据调用方本次实际确认完成的事件集合，不依据“意图文件不存在”：
/// 意图文件可能因外部删除、权限或上次清理而缺失，但缺失不能证明事件已经写入。
/// 因而这里宁可保留事务目录，也不能把唯一的“正文已提交”证据清掉。
pub(crate) fn cleanup_completed_transactions(
    transaction_root: &Path,
    completed_event_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut retained = HashSet::new();
    let scan = scan_pending_transactions(transaction_root);
    for error in &scan.errors {
        log::warn!(
            "[proof_intent] 清理事务时无法读取 {}: {error}",
            transaction_root.display()
        );
    }
    for transaction in scan.transactions {
        if transaction.proof_intent_ids.is_empty() {
            continue;
        }
        if !matches!(
            transaction.phase,
            TransactionPhase::FilesCommitted | TransactionPhase::Finished
        ) {
            retained.extend(transaction.proof_intent_ids);
            continue;
        }
        let all_proofs_done = transaction
            .proof_intent_ids
            .iter()
            .all(|event_id| completed_event_ids.contains(event_id));
        if !all_proofs_done {
            retained.extend(transaction.proof_intent_ids);
            continue;
        }
        match fs::remove_dir_all(&transaction.tx_dir) {
            Ok(()) => {
                if let Some(parent) = transaction.tx_dir.parent() {
                    if let Err(error) = crate::storage::sync_dir(parent) {
                        log::warn!(
                            "[proof_intent] fsync 事务目录父目录失败 {}: {error}",
                            parent.display()
                        );
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                log::warn!(
                    "[proof_intent] 清理已完成事务目录 {} 失败: {error}",
                    transaction.tx_dir.display()
                );
                retained.extend(transaction.proof_intent_ids);
            }
        }
    }
    retained
}
enum LegacyLiveState {
    Committed,
    Uncommitted,
    Unknown,
}

/// 没有事务链接的旧意图：只在正文可明确判定时处理。
fn legacy_live_state(intent: &SyncProofIntent, projects_root: &Path) -> LegacyLiveState {
    let Some(relative) = Path::new(&intent.workspace_path)
        .strip_prefix("projects")
        .ok()
    else {
        return LegacyLiveState::Unknown;
    };
    let path = projects_root.join(relative);
    let live = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return LegacyLiveState::Unknown,
    };
    match intent.action {
        SyncProofAction::Delete => match live {
            None => LegacyLiveState::Committed,
            Some(text) if intent.before_text.as_deref() == Some(text.as_str()) => {
                LegacyLiveState::Uncommitted
            }
            Some(_) => LegacyLiveState::Unknown,
        },
        SyncProofAction::Apply => match live {
            Some(text) => {
                if intent.after_text.as_deref() == Some(text.as_str()) {
                    LegacyLiveState::Committed
                } else if intent.before_text.as_deref() == Some(text.as_str()) {
                    LegacyLiveState::Uncommitted
                } else {
                    LegacyLiveState::Unknown
                }
            }
            None => LegacyLiveState::Unknown,
        },
    }
}
