//! 同步正文证明意图日志。
//!
//! 同步提交正文和写入本机证明事件不是同一个原子操作。若正文已经落盘、进程在
//! 证明事件写入前退出，下一次同步会以新正文作为 Prepare 基线，因而不再产生
//! 同一章节的 Apply/Delete；原 `sync_event_id`、before 快照和来源设备就永久
//! 丢失了。
//!
//! 本模块在正文 Commit **之前**把一次同步编辑的稳定事实写到
//! `stats/proof-intents/`（不参与远端作品同步）。正文 `SaveTransaction` 会在
//! manifest 中链接同一批事件 ID，并把完整恢复材料写入事务目录；
//! 即使意图文件随后丢失，只要事务进入 `FilesCommitted`/`Finished`，
//! 恢复流程仍能按原事件 ID、原时间戳和来源补记证明。因此“后来正文又被覆盖”
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

use crate::storage::transaction::{
    scan_pending_transactions, BackupEntry, ProofRecoveryRecord, TransactionManifest,
    TransactionPhase, TransactionRecovery, MANIFEST_FILENAME,
};
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

    /// 转换为随正文事务持久化的恢复材料。
    fn recovery_record(&self) -> ProofRecoveryRecord {
        ProofRecoveryRecord {
            event_id: self.event_id.clone(),
            project_id: self.project_id.clone(),
            volume_id: self.volume_id.clone(),
            chapter_id: self.chapter_id.clone(),
            workspace_path: self.workspace_path.clone(),
            before_text: self.before_text.clone(),
            after_text: self.after_text.clone(),
            source_device_id: self.source_device_id.clone(),
            occurred_at_ms: self.occurred_at_ms,
        }
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
    /// `project/volume/chapter` -> 来源设备。
    source_devices: &'a HashMap<String, String>,
}

impl<'a> ProofIntentSink<'a> {
    pub(crate) fn new(
        root: &'a Path,
        source_devices: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            root,
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
    ) -> crate::Result<Vec<ProofRecoveryRecord>> {
        let mut created_ids = Vec::new();
        let result = (|| -> crate::Result<Vec<ProofRecoveryRecord>> {
            let mut prepared_records = Vec::new();
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
                prepared_records.push(intent.recovery_record());
                if created {
                    created_ids.push(event_id);
                }
            }
            Ok(prepared_records)
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

/// 恢复后仍无法自动收敛的证明对象。
///
/// 与旧的布尔判断不同，这里返回具体事务/事件 ID，调用方不能再用一个
/// 笼统的 pending 状态永久阻断项目同步。
#[derive(Debug, Clone)]
pub(crate) struct ProofRecoveryIssue {
    pub project_id: Option<String>,
    pub transaction_id: Option<String>,
    pub event_id: Option<String>,
    pub message: String,
}

impl ProofRecoveryIssue {
    pub(crate) fn description(&self) -> String {
        let mut text = self.message.clone();
        if let Some(transaction_id) = &self.transaction_id {
            text.push_str(&format!("（事务 {transaction_id}）"));
        }
        if let Some(event_id) = &self.event_id {
            text.push_str(&format!("（事件 {event_id}）"));
        }
        text
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

/// 从稳定事件 ID 中解析所属项目。
///
/// 事件 ID 形如 `sync:<run_id>:<workspace path>:<action index>`，其中
/// workspace 路径以 `projects/<pid>/...` 开头。旧事务可能已丢失意图文件，
/// 只能靠事件 ID 做项目级归属判断。
fn event_project_id(event_id: &str) -> Option<String> {
    let rest = event_id.strip_prefix("sync:")?;
    let (_run_id, rest) = rest.split_once(':')?;
    let (workspace_path, _action_index) = rest.rsplit_once(':')?;
    let mut parts = workspace_path.split(|character| character == '/' || character == '\\');
    if parts.next()? != "projects" {
        return None;
    }
    parts.next().map(str::to_string)
}

/// 读取事务目录中随正文提交持久化的证明恢复材料。
fn load_proof_recovery_records(
    transaction: &TransactionRecovery,
) -> crate::Result<Vec<ProofRecoveryRecord>> {
    let Some(manifest) = load_transaction_manifest(transaction)? else {
        return Ok(Vec::new());
    };
    let Some(filename) = manifest.proof_recovery_file else {
        return Ok(Vec::new());
    };
    let path = transaction.tx_dir.join(filename);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.into()),
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
/// 本轮同步内复用的证明恢复结果。
///
/// 恢复先把所有意图与事务 manifest 链接的事件 ID 汇总后批量查询一次；
/// commit 阶段按项目过滤本状态，不能为每个 target 重新扫描、解压历史事件。
/// 新证明写入后调用 [`Self::mark_event_persisted`] 只更新集合，不重扫历史。
#[derive(Debug, Clone, Default)]
pub(crate) struct ProofRecoveryState {
    recovered: usize,
    persisted_event_ids: HashSet<String>,
    issues: Vec<ProofRecoveryIssue>,
    failure: Option<String>,
}

impl ProofRecoveryState {
    /// 本轮恢复无法启动（例如读取事务根失败）时，供每个 target 复用同一错误。
    pub(crate) fn failed(message: String) -> Self {
        Self {
            failure: Some(message),
            ..Self::default()
        }
    }

    pub(crate) fn recovered(&self) -> usize {
        self.recovered
    }

    pub(crate) fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    /// 按项目过滤仍未恢复的证明对象；无项目归属的全局错误对所有项目生效。
    pub(crate) fn unresolved_for_project(
        &self,
        project_id: Option<&str>,
    ) -> Vec<&ProofRecoveryIssue> {
        self.issues
            .iter()
            .filter(|issue| match (project_id, issue.project_id.as_deref()) {
                (None, _) => true,
                (Some(_), None) => true,
                (Some(expected), Some(actual)) => expected == actual,
            })
            .collect()
    }

    /// 新证明已经落盘：记录事件 ID 并移除同事件的历史问题。
    pub(crate) fn mark_event_persisted(&mut self, event_id: &str) {
        self.persisted_event_ids.insert(event_id.to_string());
        self.issues
            .retain(|issue| issue.event_id.as_deref() != Some(event_id));
    }

    /// 记录本轮仍未恢复的对象，供后续 target 直接复用，不重新查库。
    pub(crate) fn record_issue(&mut self, issue: ProofRecoveryIssue) {
        if let Some(event_id) = issue.event_id.as_deref() {
            if self.persisted_event_ids.contains(event_id) {
                return;
            }
        }
        let duplicate = self.issues.iter().any(|existing| {
            existing.project_id == issue.project_id
                && existing.transaction_id == issue.transaction_id
                && existing.event_id == issue.event_id
                && existing.message == issue.message
        });
        if !duplicate {
            self.issues.push(issue);
        }
    }

    fn add_issue(
        &mut self,
        project_id: Option<String>,
        transaction_id: Option<String>,
        event_id: Option<String>,
        message: String,
    ) {
        self.record_issue(ProofRecoveryIssue {
            project_id,
            transaction_id,
            event_id,
            message,
        });
    }
}

/// 读取事务目录中的 manifest；目录已被清理时返回 `None`。
fn load_transaction_manifest(
    transaction: &TransactionRecovery,
) -> crate::Result<Option<TransactionManifest>> {
    let manifest_path = transaction.tx_dir.join(MANIFEST_FILENAME);
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(serde_json::from_str(&text)?))
}

/// 从稳定事件 ID 中取回 workspace 相对路径。
fn event_workspace_path(event_id: &str) -> Option<String> {
    let rest = event_id.strip_prefix("sync:")?;
    let (_run_id, rest) = rest.split_once(':')?;
    let (workspace_path, _action_index) = rest.rsplit_once(':')?;
    if workspace_path.is_empty() {
        None
    } else {
        Some(workspace_path.replace('\\', "/"))
    }
}

/// 从旧版事务目录重建证明恢复材料。
///
/// 只能使用真实持久证据：事务创建时间、事务条目、备份目录中的前置正文、
/// 已提交到 live 的正文文件。缺少任一必要证据时返回具体原因，不能凭空
/// 生成时间、来源设备或 before/after。
fn legacy_proof_recovery_record(
    core: &crate::facade::WriterCore,
    transaction: &TransactionRecovery,
    event_id: &str,
) -> std::result::Result<ProofRecoveryRecord, String> {
    let workspace_path = event_workspace_path(event_id)
        .ok_or_else(|| "事件 ID 中没有可解析的 workspace 路径".to_string())?;
    let (project_id, volume_id, chapter_id) = chapter_path_parts(Path::new(&workspace_path))
        .ok_or_else(|| format!("事件路径不是章节正文: {workspace_path}"))?;
    let manifest = load_transaction_manifest(transaction)
        .map_err(|error| format!("读取事务 manifest 失败: {error}"))?
        .ok_or_else(|| "事务 manifest 已不存在".to_string())?;
    if manifest.created_at_ms <= 0 {
        return Err("事务 manifest 缺少原始发生时间".to_string());
    }
    let normalized_target = workspace_path.trim_start_matches('/').to_string();
    // 项目 target 的事务根是 projects/<pid>，manifest 条目保存项目相对路径
    // （volumes/...），而事件 ID 保存 workspace 相对路径（projects/<pid>/volumes/...）。
    // app target 的事务根是 app_data_root，两者本来就一致；先精确匹配，再按项目前缀匹配。
    let project_prefix = format!("projects/{project_id}/");
    let entry = manifest
        .entries
        .iter()
        .find(|entry| {
            let candidate = entry.target_relative.trim_start_matches('/').replace('\\', "/");
            if candidate == normalized_target {
                return true;
            }
            normalized_target
                .strip_prefix(&project_prefix)
                .is_some_and(|stripped| candidate == stripped)
        })
        .ok_or_else(|| "事务条目中找不到该章节正文".to_string())?;
    let target_relative = entry
        .target_relative
        .trim_start_matches('/')
        .replace('\\', "/");
    let before_text = match manifest
        .backup_entries
        .iter()
        .find(|backup| match backup {
            BackupEntry::RestoreFile {
                target_relative: backup_target,
                ..
            } => {
                backup_target
                    .trim_start_matches('/')
                    .replace('\\', "/")
                    == target_relative
            }
            BackupEntry::RemoveCreated {
                target_relative: backup_target,
            } => {
                backup_target
                    .trim_start_matches('/')
                    .replace('\\', "/")
                    == target_relative
            }
        }) {
        Some(BackupEntry::RestoreFile {
            backup_filename, ..
        }) => {
            let path = transaction.tx_dir.join("backup").join(backup_filename);
            let bytes = fs::read(&path)
                .map_err(|error| format!("读取事务备份 {} 失败: {error}", path.display()))?;
            Some(String::from_utf8(bytes).map_err(|error| {
                format!("事务备份 {} 不是 UTF-8: {error}", path.display())
            })?)
        }
        Some(BackupEntry::RemoveCreated { .. }) => None,
        None => {
            return Err(
                "事务没有记录前置正文快照（缺少 backup_entries 与 proof-recovery.json）"
                    .to_string(),
            )
        }
    };
    let after_text = if entry.is_delete {
        None
    } else {
        let path = transaction.target_root.join(&target_relative);
        let bytes = fs::read(&path)
            .map_err(|error| format!("读取提交后正文 {} 失败: {error}", path.display()))?;
        Some(
            String::from_utf8(bytes)
                .map_err(|error| format!("提交后正文 {} 不是 UTF-8: {error}", path.display()))?,
        )
    };
    let source_device_id = core
        .get_stats_api()
        .store()
        .resolve_origin_device_id()
        .map_err(|error| format!("读取本机来源设备失败: {error}"))?;
    Ok(ProofRecoveryRecord {
        event_id: event_id.to_string(),
        project_id,
        volume_id,
        chapter_id,
        workspace_path: normalized_target,
        before_text,
        after_text,
        source_device_id,
        occurred_at_ms: manifest.created_at_ms,
    })
}

/// 扫描并恢复所有未完成证明，返回可在本轮同步内复用的状态。
///
/// 事务扫描既从意图文件的 `transaction_root_for_intent` 进入，也遍历
/// `app_data_root` 与 `projects_root/<pid>`；这样 manifest 仍链接事件、
/// 但意图文件已丢失的旧事务也能进入恢复循环。
pub(crate) fn recover_pending_state(
    core: &crate::facade::WriterCore,
) -> crate::Result<ProofRecoveryState> {
    let root = core.app_data_root().join(PROOF_INTENTS_DIR);
    let pending = load_pending(&root)?;
    let mut state = ProofRecoveryState::default();
    for error in &pending.errors {
        log::warn!(
            "[proof_intent] 未恢复的意图文件 {}: {}",
            error.path.display(),
            error.message
        );
        state.add_issue(
            None,
            None,
            None,
            format!("意图文件 {} 无法解析: {}", error.path.display(), error.message),
        );
    }

    // 双向扫描：既从意图文件找事务根，也遍历所有可能保存正文事务的根。
    // 只从意图反推时，意图文件丢失的已提交事务永远不会进入恢复循环。
    let intent_event_ids: HashSet<String> = pending
        .intents
        .iter()
        .map(|intent| intent.event_id.clone())
        .collect();
    let mut transaction_roots: HashSet<PathBuf> = HashSet::new();
    transaction_roots.insert(core.app_data_root().to_path_buf());
    match fs::read_dir(core.projects_root()) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    transaction_roots.insert(entry.path());
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    for intent in &pending.intents {
        transaction_roots.insert(transaction_root_for_intent(
            intent,
            core.projects_root(),
            core.app_data_root(),
        ));
    }

    let mut scans = HashMap::new();
    for transaction_root in transaction_roots {
        scans.insert(
            transaction_root.clone(),
            scan_pending_transactions(&transaction_root),
        );
    }

    let mut committed = HashMap::new();
    let mut known_transactions: HashMap<String, (PathBuf, TransactionRecovery)> = HashMap::new();
    let mut blocked_ids = HashSet::new();
    let mut blocked_roots = HashSet::new();
    let mut all_event_ids: HashSet<String> = intent_event_ids.clone();
    for (transaction_root, scan) in &scans {
        if !scan.errors.is_empty() {
            for error in &scan.errors {
                log::warn!(
                    "[proof_intent] 正文事务状态未知（{}）: {error}",
                    transaction_root.display()
                );
                state.add_issue(
                    None,
                    None,
                    None,
                    format!(
                        "正文事务目录 {} 无法读取: {error}",
                        transaction_root.display()
                    ),
                );
            }
            blocked_roots.insert(transaction_root.clone());
        }
        for transaction in &scan.transactions {
            for event_id in &transaction.proof_intent_ids {
                all_event_ids.insert(event_id.clone());
                known_transactions.insert(
                    event_id.clone(),
                    (transaction_root.clone(), transaction.clone()),
                );
            }
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

    // 本轮只查一次本机事件库：所有意图与事务链接的事件 ID 合并后批量查询。
    let persisted_events = core
        .get_stats_api()
        .store()
        .find_events_by_ids(&all_event_ids)?;
    state.persisted_event_ids = persisted_events.keys().cloned().collect();

    let mut recovered = 0usize;
    let mut completed_ids: HashSet<String> = HashSet::new();
    let mut to_remove: HashSet<String> = HashSet::new();
    for intent in pending.intents {
        if state.persisted_event_ids.contains(&intent.event_id) {
            completed_ids.insert(intent.event_id.clone());
            to_remove.insert(intent.event_id.clone());
            continue;
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
                    state.mark_event_persisted(&intent.event_id);
                }
                Err(error) => {
                    log::warn!(
                        "[proof_intent] 恢复同步证明事件失败 {}: {error}",
                        intent.event_id
                    );
                    state.add_issue(
                        Some(intent.project_id.clone()),
                        Some(transaction.transaction_id.clone()),
                        Some(intent.event_id.clone()),
                        format!("补记证明事件失败: {error}"),
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
            state.add_issue(
                Some(intent.project_id.clone()),
                None,
                Some(intent.event_id.clone()),
                "正文事务尚未确认提交，证明事件尚未入库".to_string(),
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
                        completed_ids.insert(intent.event_id.clone());
                        to_remove.insert(intent.event_id.clone());
                        state.mark_event_persisted(&intent.event_id);
                    }
                    Err(error) => {
                        log::warn!(
                            "[proof_intent] 补记旧同步证明事件失败 {}: {error}",
                            intent.event_id
                        );
                        state.add_issue(
                            Some(intent.project_id.clone()),
                            None,
                            Some(intent.event_id.clone()),
                            format!("补记旧同步证明事件失败: {error}"),
                        );
                    }
                }
            }
            LegacyLiveState::Unknown => {
                log::warn!(
                    "[proof_intent] 旧意图缺少事务阶段且正文无法确认，保留意图 {}",
                    intent.event_id
                );
                state.add_issue(
                    Some(intent.project_id.clone()),
                    None,
                    Some(intent.event_id.clone()),
                    "旧意图缺少事务阶段且正文无法确认".to_string(),
                );
            }
        }
    }

    // 事务仍在、但意图文件已丢失的孤儿事件：先确认事件是否已落盘；
    // 未落盘时从事务目录内持久化的恢复材料补记。旧版事务没有恢复材料时，
    // 再从 manifest/备份/live 正文追溯真实证据，绝不凭空生成证明。
    for (event_id, (_, transaction)) in &known_transactions {
        if intent_event_ids.contains(event_id) || completed_ids.contains(event_id) {
            continue;
        }
        let project_id = event_project_id(event_id);
        if state.persisted_event_ids.contains(event_id) {
            completed_ids.insert(event_id.clone());
            recovered = recovered.saturating_add(1);
            continue;
        }
        if !matches!(
            transaction.phase,
            TransactionPhase::FilesCommitted | TransactionPhase::Finished
        ) {
            state.add_issue(
                project_id,
                Some(transaction.transaction_id.clone()),
                Some(event_id.clone()),
                "正文事务尚未确认提交，证明事件尚未入库".to_string(),
            );
            continue;
        }
        let record = match load_proof_recovery_records(transaction) {
            Ok(records) => match records.into_iter().find(|record| record.event_id == *event_id) {
                Some(record) => Some(record),
                None => match legacy_proof_recovery_record(core, transaction, event_id) {
                    Ok(record) => Some(record),
                    Err(reason) => {
                        state.add_issue(
                            project_id.clone(),
                            Some(transaction.transaction_id.clone()),
                            Some(event_id.clone()),
                            format!(
                                "事务 {} 缺少意图文件 {}，且无法重建原始证明: {reason}",
                                transaction.transaction_id, event_id
                            ),
                        );
                        None
                    }
                },
            },
            Err(error) => {
                state.add_issue(
                    project_id.clone(),
                    Some(transaction.transaction_id.clone()),
                    Some(event_id.clone()),
                    format!(
                        "事务 {} 的证明恢复材料无法读取: {error}",
                        transaction.transaction_id
                    ),
                );
                None
            }
        };
        let Some(record) = record else {
            continue;
        };
        match core.record_sync_content_change_at(
            &record.project_id,
            &record.volume_id,
            &record.chapter_id,
            record.before_text.as_deref(),
            record.after_text.as_deref(),
            &record.event_id,
            &record.source_device_id,
            "",
            record.occurred_at_ms,
        ) {
            Ok(_) => {
                completed_ids.insert(event_id.clone());
                state.mark_event_persisted(event_id);
                recovered = recovered.saturating_add(1);
            }
            Err(error) => {
                log::warn!(
                    "[proof_intent] 从事务 {} 恢复同步证明事件 {} 失败: {error}",
                    transaction.transaction_id,
                    event_id
                );
                state.add_issue(
                    project_id,
                    Some(transaction.transaction_id.clone()),
                    Some(event_id.clone()),
                    format!("从事务恢复证明事件失败: {error}"),
                );
            }
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
    state.recovered = recovered;
    Ok(state)
}

/// 兼容旧调用：只返回本轮完成（补记、确认已存在或清理）的事件数量。
pub(crate) fn recover_pending(core: &crate::facade::WriterCore) -> crate::Result<usize> {
    Ok(recover_pending_state(core)?.recovered())
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
