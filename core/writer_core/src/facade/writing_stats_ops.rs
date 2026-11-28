use crate::error::{Error, Result};
use crate::writing_stats::projection::CurrentWritingSpeed;
use crate::writing_stats::{DateRange, EventSource, WritingInputEvent};

use serde_json::Value;

/// 已移除的同步证明意图目录（相对 app_data_root）。
const LEGACY_SYNC_PROOF_DIR: &str = "stats/proof-intents";
/// 一次性迁移后的归档目录。
const LEGACY_SYNC_PROOF_MIGRATED_DIR: &str = "stats/proof-intents-migrated";

/// 旧同步证明意图记录。字段与 Issue #1008 的意图文件一致。
#[derive(serde::Deserialize)]
struct LegacySyncProofRecord {
    #[serde(default)]
    format_version: u32,
    event_id: String,
    project_id: String,
    volume_id: String,
    chapter_id: String,
    #[serde(default)]
    action: String,
    before_text: Option<String>,
    after_text: Option<String>,
    #[serde(default)]
    source_device_id: String,
    #[serde(default)]
    occurred_at_ms: i64,
}

/// 旧意图对应的正文提交状态。只有能确认已提交的记录才补记证明。
enum LegacySyncProofLiveState {
    Committed,
    Uncommitted,
    Unknown,
}

impl super::WriterCore {
    /// 一次性迁移旧 `stats/proof-intents` 记录到现有正文证明链。
    ///
    /// 同步不再维护独立的正文证明意图与恢复状态机。旧目录中未结算的记录在
    /// 启动时按现有正文证明写入入口 [`Self::record_sync_content_change_at`]
    /// 补记；原文件移入 `stats/proof-intents-migrated` 保留。无法确认正文
    /// 是否已提交（或同步后又被编辑）的记录不伪造证明、不删除，只记录日志。
    pub(crate) fn migrate_legacy_sync_proof_records(&self) -> Result<()> {
        let root = self.app_data_root.join(LEGACY_SYNC_PROOF_DIR);
        if !root.is_dir() {
            return Ok(());
        }
        let archive_root = self.app_data_root.join(LEGACY_SYNC_PROOF_MIGRATED_DIR);
        let mut records = Vec::new();
        for entry in std::fs::read_dir(&root)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    log::warn!("[sync-proof-migration] 读取旧证明目录项失败: {error}");
                    continue;
                }
            };
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let parsed = std::fs::read_to_string(&path)
                .map_err(crate::Error::from)
                .and_then(|text| {
                    serde_json::from_str::<LegacySyncProofRecord>(&text)
                        .map_err(crate::Error::from)
                });
            match parsed {
                Ok(record) if record.format_version == 1 && !record.event_id.is_empty() => {
                    records.push((path, record));
                }
                Ok(record) => {
                    log::warn!(
                        "[sync-proof-migration] 旧证明记录格式不受支持，保留原文件 {}: format_version={}",
                        path.display(),
                        record.format_version
                    );
                }
                Err(error) => {
                    log::warn!(
                        "[sync-proof-migration] 旧证明记录无法读取，保留原文件 {}: {error}",
                        path.display()
                    );
                }
            }
        }
        if records.is_empty() {
            return Ok(());
        }
        let event_ids: std::collections::HashSet<String> = records
            .iter()
            .map(|(_, record)| record.event_id.clone())
            .collect();
        let persisted = self
            .get_stats_api()
            .store()
            .find_events_by_ids(&event_ids)?;
        let mut transferred = 0usize;
        let mut archived = 0usize;
        let mut unconfirmed = 0usize;
        let mut retained = 0usize;
        for (path, record) in records {
            if persisted.contains_key(&record.event_id) {
                if move_legacy_sync_proof_record(&path, &archive_root) {
                    archived += 1;
                } else {
                    retained += 1;
                }
                continue;
            }
            match legacy_sync_proof_live_state(self, &record) {
                LegacySyncProofLiveState::Committed => {
                    match self.record_sync_content_change_at(
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
                            transferred += 1;
                            if !move_legacy_sync_proof_record(&path, &archive_root) {
                                retained += 1;
                            }
                        }
                        Err(error) => {
                            log::warn!(
                                "[sync-proof-migration] 旧证明记录转入正文证明失败，保留原文件 {}: {error}",
                                path.display()
                            );
                            retained += 1;
                        }
                    }
                }
                LegacySyncProofLiveState::Uncommitted => {
                    log::warn!(
                        "[sync-proof-migration] 旧证明记录对应正文未提交，不补记证明，归档 {}",
                        path.display()
                    );
                    if move_legacy_sync_proof_record(&path, &archive_root) {
                        unconfirmed += 1;
                    } else {
                        retained += 1;
                    }
                }
                LegacySyncProofLiveState::Unknown => {
                    log::warn!(
                        "[sync-proof-migration] 无法确认旧证明记录对应的正文提交，保留原文件 {}",
                        path.display()
                    );
                    retained += 1;
                }
            }
        }
        let marker = root.join(".pending");
        if marker.is_file() {
            if let Err(error) = std::fs::remove_file(&marker) {
                log::warn!("[sync-proof-migration] 删除旧待恢复标记失败: {error}");
            }
        }
        log::warn!(
            "[sync-proof-migration] 旧同步证明记录迁移完成: 补记={transferred} 已落盘归档={archived} 未提交归档={unconfirmed} 保留={retained}，来源 {}",
            root.display()
        );
        Ok(())
    }
}

fn legacy_sync_proof_live_state(
    core: &super::WriterCore,
    record: &LegacySyncProofRecord,
) -> LegacySyncProofLiveState {
    let live_path = core
        .projects_root()
        .join(&record.project_id)
        .join("volumes")
        .join(&record.volume_id)
        .join("chapters")
        .join(&record.chapter_id)
        .join("chapter.md");
    let live = match std::fs::read_to_string(&live_path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return LegacySyncProofLiveState::Unknown,
    };
    match record.action.as_str() {
        "delete" => match live {
            None => LegacySyncProofLiveState::Committed,
            Some(text) if record.before_text.as_deref() == Some(text.as_str()) => {
                LegacySyncProofLiveState::Uncommitted
            }
            Some(_) => LegacySyncProofLiveState::Unknown,
        },
        "apply" => match live {
            Some(text) => {
                if record.after_text.as_deref() == Some(text.as_str()) {
                    LegacySyncProofLiveState::Committed
                } else if record.before_text.as_deref() == Some(text.as_str()) {
                    LegacySyncProofLiveState::Uncommitted
                } else {
                    LegacySyncProofLiveState::Unknown
                }
            }
            None => LegacySyncProofLiveState::Unknown,
        },
        _ => LegacySyncProofLiveState::Unknown,
    }
}

fn move_legacy_sync_proof_record(
    path: &std::path::Path,
    archive_root: &std::path::Path,
) -> bool {
    let Some(file_name) = path.file_name() else {
        return false;
    };
    if let Err(error) = std::fs::create_dir_all(archive_root) {
        log::warn!(
            "[sync-proof-migration] 创建归档目录失败 {}: {error}",
            archive_root.display()
        );
        return false;
    }
    let target = archive_root.join(file_name);
    match std::fs::rename(path, &target) {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "[sync-proof-migration] 归档旧证明记录失败 {}: {error}",
                path.display()
            );
            false
        }
    }
}

impl super::WriterCore {
    /// 按章节回放证明事件，返回目标时刻的正文/选区与校验结果。
    pub fn replay_chapter_proof(
        &self,
        request: &crate::writing_stats::replay::ProofReplayRequest,
    ) -> Result<serde_json::Value> {
        let frame = self.get_stats_api().replay_chapter_proof(request)?;
        serde_json::to_value(frame).map_err(Into::into)
    }

    /// 按原始证明批次从基线正文 replay 出最终正文（Issue #1062 1.1）。
    ///
    /// 平台把未提交草稿保存为原始事实后，恢复时需要先还原“这批事实对应的
    /// 最终正文”：这里逐条应用 changes 并核对摘要链，返回 replay 结果。
    /// 批次与正文不一致时返回 [`crate::Error::ProofBatchMismatch`]。
    pub fn replay_editor_proof_batch(
        &self,
        chapter_id: &str,
        baseline: &str,
        proof_batch_json: &str,
    ) -> Result<String> {
        let events: Vec<crate::chapter::ChapterProofProjection> =
            if proof_batch_json.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(proof_batch_json)?
            };
        if events.is_empty() {
            return Ok(baseline.to_string());
        }
        super::chapter_proof_batch::replay_batch_from_baseline(&events, baseline).map_err(|error| {
            crate::error::Error::ProofBatchMismatch {
                chapter_id: chapter_id.to_string(),
                detail: format!("replay failed: {error}"),
            }
        })
    }

    /// 导出离线可校验的章节证明包（包含历史正文，调用方需提示用户）。
    pub fn export_chapter_proof(
        &self,
        chapter_id: &str,
        device_id: &str,
    ) -> Result<serde_json::Value> {
        let package = self
            .get_stats_api()
            .export_chapter_proof(chapter_id, device_id)?;
        serde_json::to_value(package).map_err(Into::into)
    }

    /// 列出某设备/章节的完整正文检查点。
    pub fn list_proof_checkpoints(
        &self,
        device_id: &str,
        chapter_id: &str,
    ) -> Result<serde_json::Value> {
        let checkpoints = self
            .get_stats_api()
            .load_proof_checkpoints(device_id, chapter_id)?;
        serde_json::to_value(checkpoints).map_err(Into::into)
    }

    /// 上报一次已成功提交的编辑事务，同时写入统计事件与手写证明。
    ///
    /// `changes` 是 Editor V2 内核实际执行的插入/删除列表（含删除原文），
    /// Core 据此精确计算字数、来源和内容链；平台端不再只传两个计数。
    /// 无 `changes` 的旧客户端继续走 [`Self::record_editor_change_stats`]，
    /// 只产生统计事件、不产生可回放证明。
    pub fn record_editor_transaction_stats(
        &self,
        input: &crate::api::EditorChangeStatsInputDto,
    ) -> Result<()> {
        // 编辑热路径只追加统计/证明事件，不扫描工作区目录。

        if input.changes.is_empty() {
            return self.record_editor_change_stats(
                &input.platform,
                &input.project_id,
                &input.volume_id,
                &input.chapter_id,
                input.cause.into(),
                input.inserted_chars,
                input.deleted_chars,
                &input.event_id,
                input.occurred_at_ms,
            );
        }

        let Some(projection) = self.build_editor_proof_projection(input)? else {
            return Ok(());
        };
        // 证明事件先留在 Core，随下一次正文提交写入同一 durable journal。
        // 只有 journal 决议成功后才允许写统计/证明，链头不能超越已提交正文。
        let key = self.pending_proof_key(&input.project_id, &input.volume_id, &input.chapter_id);
        self.enqueue_pending_editor_proof(key, projection)?;
        Ok(())
    }

    /// 构造一次编辑事务的原始证明投影并序列化为 JSON 交给平台持久化
    /// （Issue #1062 1.2）。
    ///
    /// 正式保存路径不再依赖进程内全局 pending map：平台把这份原始事实随
    /// 快照冻结（Qt 的编辑事实/草稿 actor），保存时作为显式批次传回，Core
    /// 逐条 replay 校验后与正文进入同一 durable journal。
    /// 返回空串表示本次事务没有可回放的证明（老客户端只报计数）。
    pub fn record_editor_proof_fact(
        &self,
        input: &crate::api::EditorChangeStatsInputDto,
    ) -> Result<String> {
        if input.changes.is_empty() {
            return Ok(String::new());
        }
        let Some(projection) = self.build_editor_proof_projection(input)? else {
            return Ok(String::new());
        };
        Ok(serde_json::to_string(&projection)?)
    }

    /// 由编辑事实构造证明投影；不写 pending map，供显式批次保存直接使用。
    fn build_editor_proof_projection(
        &self,
        input: &crate::api::EditorChangeStatsInputDto,
    ) -> Result<Option<crate::chapter::ChapterProofProjection>> {
        use crate::editor::transaction::{EditorChange, EditorCursor, EditorSelection};
        use crate::editor::EditorTransactionCause;
        use crate::writing_stats::proof::{ProofEditPayload, PROOF_FORMAT_VERSION};

        // 设备身份由 Core 读/建，不让平台端传；平台初始化注入的稳定 ID 优先。
        let platform = self.effective_platform(&input.platform);
        let preferred_device_id = self.preferred_device_id();
        let device_info = self.ensure_device_info(
            platform.to_str_name(),
            platform.default_device_class(),
            preferred_device_id.as_deref(),
        )?;

        let cause: EditorTransactionCause = input.cause.into();
        let changes: Vec<EditorChange> = input.changes.iter().cloned().map(Into::into).collect();
        let inserted_chars: u32 = changes
            .iter()
            .filter_map(|change| match change {
                EditorChange::Insert { text, .. } => Some(text.chars().count() as u32),
                EditorChange::Delete { .. } => None,
            })
            .sum();
        let deleted_chars: u32 = changes
            .iter()
            .filter_map(|change| match change {
                EditorChange::Delete { text, .. } => Some(text.chars().count() as u32),
                EditorChange::Insert { .. } => None,
            })
            .sum();

        let (source, inserted, deleted, pasted) = match cause {
            EditorTransactionCause::Typing
            | EditorTransactionCause::TypingCommit
            | EditorTransactionCause::ImeComposition => {
                (EventSource::HumanTyped, inserted_chars, deleted_chars, 0)
            }
            EditorTransactionCause::Paste => {
                (EventSource::Pasted, 0, deleted_chars, inserted_chars)
            }
            EditorTransactionCause::Delete => (EventSource::Deleted, 0, deleted_chars, 0),
            EditorTransactionCause::Undo
            | EditorTransactionCause::Redo
            | EditorTransactionCause::Load
            | EditorTransactionCause::Format
            | EditorTransactionCause::Programmatic => {
                (EventSource::Unknown, inserted_chars, deleted_chars, 0)
            }
        };

        if inserted == 0 && deleted == 0 && pasted == 0 {
            return Ok(None);
        }

        let mut event = WritingInputEvent::with_identity(
            input.event_id.clone(),
            input.occurred_at_ms,
            &device_info.device_id,
            platform,
            &device_info.device_class,
            &input.project_id,
            &input.volume_id,
            &input.chapter_id,
            source,
            inserted,
            deleted,
            pasted,
            0,
            0,
            &input.session_id,
        );
        let selection = |anchor: u32, head: u32| EditorSelection {
            anchor: EditorCursor::from_offset(crate::editor::Utf8ByteOffset::unchecked(
                anchor as usize,
            )),
            head: EditorCursor::from_offset(crate::editor::Utf8ByteOffset::unchecked(
                head as usize,
            )),
        };
        event.proof = Some(ProofEditPayload {
            format_version: PROOF_FORMAT_VERSION,
            // 序号、基线、前后 hash 和事件 hash 由 store 在串行写入时补齐。
            seq: 0,
            session_id: input.session_id.clone(),
            baseline_id: String::new(),
            starts_new_chain: false,
            base_revision: input.base_revision,
            new_revision: input.new_revision,
            cause,
            changes,
            old_selection: selection(input.old_selection_anchor, input.old_selection_head),
            new_selection: selection(input.new_selection_anchor, input.new_selection_head),
            before_content_hash: String::new(),
            after_content_hash: String::new(),
            before_text_sha256: input.before_text_sha256.clone(),
            after_text_sha256: input.after_text_sha256.clone(),
            prev_event_hash: String::new(),
            event_hash: String::new(),
            sync_source_device_id: input.sync_source_device_id.clone(),
            parent_event_id: input.parent_event_id.clone(),
        });

        Ok(Some(crate::chapter::ChapterProofProjection {
            event,
            baseline_text: input.baseline_text.clone(),
        }))
    }

    fn enqueue_pending_editor_proof(
        &self,
        key: String,
        projection: crate::chapter::ChapterProofProjection,
    ) -> Result<()> {
        let mut pending = self.pending_editor_proof.lock().map_err(|error| {
            Error::Other(format!(
                "pending editor proof queue lock failed while enqueueing: {error}"
            ))
        })?;
        let events = pending.entry(key).or_default();
        if events
            .iter()
            .any(|existing| existing.event_id() == projection.event_id())
        {
            return Ok(());
        }
        let mut projection = projection;
        // 基线正文只对本章第一条待提交事件保留，避免每条编辑都把整章正文
        // 复制进内存/journal。
        if !events.is_empty() {
            projection.baseline_text = None;
        }
        events.push(projection);
        Ok(())
    }

    fn pending_proof_key(&self, project_id: &str, volume_id: &str, chapter_id: &str) -> String {
        // 工作区根必须进入键：不同工作区可以存在相同 project id。
        format!(
            "{}\u{1f}{project_id}/{volume_id}/{chapter_id}",
            self.app_data_root.display()
        )
    }

    pub(crate) fn pending_editor_proof_for(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<Vec<crate::chapter::ChapterProofProjection>> {
        let key = self.pending_proof_key(project_id, volume_id, chapter_id);
        let pending = self.pending_editor_proof.lock().map_err(|error| {
            Error::Other(format!(
                "pending editor proof queue lock failed while reading: {error}"
            ))
        })?;
        Ok(pending.get(&key).cloned().unwrap_or_default())
    }

    pub(crate) fn clear_pending_editor_proof(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        event_ids: &std::collections::HashSet<String>,
    ) -> Result<()> {
        let key = self.pending_proof_key(project_id, volume_id, chapter_id);
        let mut pending = self.pending_editor_proof.lock().map_err(|error| {
            Error::Other(format!(
                "pending editor proof queue lock failed while clearing: {error}"
            ))
        })?;
        if let Some(events) = pending.get_mut(&key) {
            events.retain(|event| !event_ids.contains(event.event_id()));
            if events.is_empty() {
                pending.remove(&key);
            }
        }
        Ok(())
    }

    /// 启动/工作区打开时补写 journal 中滞留的正文证明投影。
    /// 只扫描确实存在 journal 的作品；普通打开章节不触发。
    pub fn recover_pending_chapter_proof_projections(&self) -> Result<()> {
        let entries = std::fs::read_dir(&self.projects_root).map_err(|error| {
            Error::Other(format!(
                "failed to scan projects root {} for pending chapter proofs: {error}",
                self.projects_root.display()
            ))
        })?;
        let mut failures: Vec<String> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                Error::Other(format!(
                    "failed to read projects root {} entry: {error}",
                    self.projects_root.display()
                ))
            })?;
            let project_root = entry.path();
            if !project_root.is_dir() {
                continue;
            }
            let recovered =
                match crate::chapter::commit::recover_pending_chapter_commits(&project_root) {
                    Ok(recovered) => recovered,
                    Err(error) => {
                        // 真实待恢复事项保留在 journal 中，并把失败带回启动错误，
                        // 不能只记日志后报告恢复成功。
                        failures.push(format!("{}: {error}", project_root.display()));
                        continue;
                    }
                };
            // 同一恢复轮只枚举一次真实未决事务，按章节顺序逐个投影；单个
            // 事务失败保留自己的 journal，不阻断其它章节。即使本次没有恢复
            // 出正文决议，也要把损坏清单的真实失败带回结果。
            let scan =
                match crate::chapter::commit::pending_chapter_proof_transactions(&project_root) {
                    Ok(scan) => scan,
                    Err(error) => {
                        failures.push(format!("{}: {error}", project_root.display()));
                        continue;
                    }
                };
            failures.extend(scan.failures);
            let transactions = scan.transactions;
            if recovered.is_empty() && transactions.is_empty() {
                continue;
            }
            // 本轮已成功确认的事务要从“更早未决”集合里移除，否则同章后续
            // 事务会把它误判成仍未投影而整章阻塞。
            let mut remaining = transactions.clone();
            for item in recovered {
                if item.superseded || item.pending_projections.is_empty() {
                    continue;
                }
                let Some(current) = remaining
                    .iter()
                    .find(|transaction| transaction.transaction_id == item.transaction_id)
                else {
                    continue;
                };
                if let Err(error) = self.apply_pending_chapter_proof_transaction(
                    &project_root,
                    current,
                    &remaining,
                ) {
                    failures.push(format!(
                        "{}: transaction {}: {error}",
                        project_root.display(),
                        item.transaction_id
                    ));
                } else {
                    remaining.retain(|transaction| {
                        transaction.transaction_id != item.transaction_id
                    });
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "chapter proof recovery incomplete: {}",
                failures.join("; ")
            )))
        }
    }

    /// 重试当前项目内所有已发布但未确认的证明投影。
    ///
    /// 保存路径不再调用本函数：正文 durable 之后只投影本次事务，历史投影
    /// 失败不会阻断无关章节的正文保存。
    pub(crate) fn retry_pending_chapter_proof_projections(
        &self,
        project_root: &std::path::Path,
    ) -> Result<()> {
        let scan = crate::chapter::commit::pending_chapter_proof_transactions(project_root)?;
        // 同一章前序 journal 投影失败时，后续 journal 不能越过它；其他章
        // 仍继续结算。进程内失败集只用于本轮，durable journal 仍是事实来源。
        let mut blocked_chapters = std::collections::HashSet::new();
        let mut failures: Vec<String> = scan.failures;
        let transactions = scan.transactions;
        // 成功确认的事务从 remaining 移除，供同章后续事务判断“前序仍未决”。
        let mut remaining = transactions.clone();
        for transaction in &transactions {
            if blocked_chapters.contains(&transaction.key) {
                continue;
            }
            if let Err(error) = self.apply_pending_chapter_proof_transaction(
                project_root,
                transaction,
                &remaining,
            ) {
                blocked_chapters.insert(transaction.key.clone());
                failures.push(format!(
                    "{}: proof projection not durable for transaction {}: {error}",
                    project_root.display(),
                    transaction.transaction_id
                ));
            } else {
                remaining.retain(|item| item.transaction_id != transaction.transaction_id);
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "chapter proof retry incomplete: {}",
                failures.join("; ")
            )))
        }
    }

    /// 收集项目内已随正文 journal durable、但尚未投影的同步事实 ID。
    pub(crate) fn pending_chapter_sync_proof_fact_ids(
        &self,
        project_root: &std::path::Path,
    ) -> Result<std::collections::HashSet<String>> {
        crate::chapter::commit::pending_chapter_sync_proof_fact_ids(project_root)
    }

    /// 把单笔 journal 中 durable 的证明事件批量投影到统计，并批量确认 journal。
    ///
    /// 错误只表示“证明投影待补”，调用方必须区分正文已经 durable 与证明
    /// 尚未落盘：不能因此把正文保存判为失败，也不能把未确认当成功。
    pub(crate) fn apply_pending_chapter_proof(
        &self,
        project_root: &std::path::Path,
        transaction_id: &str,
    ) -> Result<()> {
        let Some(current) =
            crate::chapter::commit::pending_chapter_proof_transaction(project_root, transaction_id)?
        else {
            // 事务已确认/已清理：没有待投影事实。
            return Ok(());
        };
        self.apply_pending_chapter_proof_transaction(project_root, &current, &[current.clone()])
    }

    fn apply_pending_chapter_proof_transaction(
        &self,
        project_root: &std::path::Path,
        current: &crate::chapter::commit::PendingChapterProofTransaction,
        transactions: &[crate::chapter::commit::PendingChapterProofTransaction],
    ) -> Result<()> {
        // 同章仍有更早的未确认 journal 时，当前批次的 proof event 不能先写，
        // 否则证明链会按错误的因果顺序接续。
        if transactions.iter().any(|item| {
            item.key == current.key && item.commit_sequence < current.commit_sequence
        }) {
            return Err(Error::Other(format!(
                "chapter proof transaction {} has an earlier pending transaction for the same chapter",
                current.transaction_id
            )));
        }
        let events = crate::chapter::commit::pending_chapter_proof_events(
            project_root,
            &current.transaction_id,
        )?;
        let facts = crate::chapter::commit::pending_chapter_sync_proof_facts(
            project_root,
            &current.transaction_id,
        )?;
        if events.is_empty() && facts.is_empty() {
            return Ok(());
        }
        let stats = self.get_stats_api();
        let mut projection_ids = Vec::with_capacity(events.len() + facts.len());
        let mut items = Vec::with_capacity(events.len());
        for projection in events {
            projection_ids.push(projection.event_id().to_string());
            items.push((projection.event, projection.baseline_text));
        }
        // 本批证明事件按因果顺序计算链头，一次批量落盘并只 sync 涉及的日分区。
        if !items.is_empty() {
            stats.record_proof_events(items).map_err(|error| {
                Error::Other(format!(
                    "chapter proof events not durable for transaction {}: {error}",
                    current.transaction_id
                ))
            })?;
        }
        for fact in facts {
            let event_id = fact.event_id.clone();
            self.record_sync_content_change_at(
                &fact.key.project_id,
                &fact.key.volume_id,
                &fact.key.chapter_id,
                fact.before_text.as_deref(),
                fact.after_text.as_deref(),
                &event_id,
                &fact.source_device_id,
                "",
                fact.occurred_at_ms,
            )
            .map_err(|error| {
                Error::Other(format!(
                    "chapter sync proof event not durable for transaction {}: {error}",
                    current.transaction_id
                ))
            })?;
            projection_ids.push(event_id);
        }
        // 只有全部事件/事实已确认持久化后才批量回执 journal；中途失败保留
        // 原 journal，下一次重放会按 event_id 去重后重试。
        crate::chapter::commit::mark_chapter_commit_projections(
            project_root,
            &current.transaction_id,
            &projection_ids,
        )?;
        Ok(())
    }

    /// 记录一次“同步把正文改成 after_text”的明确事实。
    ///
    /// 同步层在 commit 成功后调用。事件来源固定为 `SyncRemote`，不计入人工
    /// 输入；before 正文取 Core 当前证明链头，不信任同步层传入的旧磁盘文本。
    /// 若该章节尚无证明链（例如旧章节第一次被同步覆盖），返回 `false`，
    /// 由后续保存/编辑建立基线，不伪造逐字历史。
    #[allow(clippy::too_many_arguments)]
    pub fn record_sync_content_change(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        before_text: Option<&str>,
        after_text: Option<&str>,
        sync_event_id: &str,
        sync_source_device_id: &str,
        parent_event_id: &str,
    ) -> Result<bool> {
        self.record_sync_content_change_at(
            project_id,
            volume_id,
            chapter_id,
            before_text,
            after_text,
            sync_event_id,
            sync_source_device_id,
            parent_event_id,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    /// 带稳定事件时间的同步证明写入。恢复与一次性迁移必须复用事务或旧记录
    /// 里的原始时间，否则跨 UTC 日会落到另一个事件分区，幂等判断也会失效。
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub(crate) fn record_sync_content_change_at(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        before_text: Option<&str>,
        after_text: Option<&str>,
        sync_event_id: &str,
        sync_source_device_id: &str,
        parent_event_id: &str,
        occurred_at_ms: i64,
    ) -> Result<bool> {
        use crate::editor::transaction::{EditorChange, EditorCursor, EditorSelection};
        use crate::editor::EditorTransactionCause;
        use crate::editor::Utf8ByteOffset;
        use crate::writing_stats::proof::{
            content_digest_of_text, ProofEditPayload, PROOF_FORMAT_VERSION,
        };

        if sync_event_id.is_empty() {
            return Err(crate::Error::Other("同步证明事件缺少事务 ID".to_string()));
        }
        let stats = self.get_stats_api();
        if stats
            .store()
            .proof_event_persisted(sync_event_id, occurred_at_ms)?
        {
            return Ok(false);
        }
        let after_text = after_text.unwrap_or("");
        if before_text.unwrap_or("") == after_text {
            return Ok(false);
        }
        let origin = stats.store().resolve_origin_device_id()?;
        // continuity_gap 本身没有编辑事件，但它已经建立了新的正文基线；
        // 必须按“已有链”走 replay，而不是回落到 seq=0 的同步导入基线。
        let continuity_gaps = stats.load_proof_continuity_gaps(&origin, chapter_id)?;
        let has_local_chain =
            stats.has_chapter_proof(&origin, chapter_id)? || !continuity_gaps.is_empty();
        let (before_text, chain_parent_event_id) = if has_local_chain {
            let request = crate::writing_stats::replay::ProofReplayRequest {
                chapter_id: chapter_id.to_string(),
                device_id: origin.clone(),
                target_seq: None,
                target_time_ms: None,
                target_index: None,
                baseline_text: None,
            };
            let frame = stats.replay_chapter_proof(&request)?;
            if !frame.valid {
                // 证明链自身校验失败时不能伪造连续历史；以已提交正文建立
                // 明确 continuity gap，让恢复事务完成而不是永久卡在 mismatch。
                stats.seal_proof_continuity_gap(
                    project_id,
                    volume_id,
                    chapter_id,
                    sync_event_id,
                    sync_event_id,
                    &["proof_chain_invalid".to_string()],
                    "sync_commit",
                    after_text,
                )?;
                return Ok(true);
            }
            match before_text {
                Some(expected) if expected != frame.text => {
                    stats.seal_proof_continuity_gap(
                        project_id,
                        volume_id,
                        chapter_id,
                        sync_event_id,
                        sync_event_id,
                        &[
                            "sync_before_text_mismatch".to_string(),
                            format!(
                                "committed_before_len={} proof_head_len={}",
                                expected.len(),
                                frame.text.len()
                            ),
                        ],
                        "sync_commit",
                        after_text,
                    )?;
                    return Ok(true);
                }
                None if !frame.text.is_empty() => {
                    stats.seal_proof_continuity_gap(
                        project_id,
                        volume_id,
                        chapter_id,
                        sync_event_id,
                        sync_event_id,
                        &["sync_before_snapshot_missing".to_string()],
                        "sync_commit",
                        after_text,
                    )?;
                    return Ok(true);
                }
                None => {}
                Some(_) => {}
            }
            (frame.text, frame.event_id)
        } else {
            let baseline = before_text.unwrap_or("");
            stats.record_sync_import_baseline(
                chapter_id,
                baseline,
                sync_event_id,
                sync_source_device_id,
            )?;
            (baseline.to_string(), String::new())
        };

        if before_text == after_text {
            return Ok(false);
        }
        if !parent_event_id.is_empty()
            && !chain_parent_event_id.is_empty()
            && parent_event_id != chain_parent_event_id
        {
            stats.seal_proof_continuity_gap(
                project_id,
                volume_id,
                chapter_id,
                sync_event_id,
                sync_event_id,
                &["sync_parent_event_mismatch".to_string()],
                "sync_commit",
                after_text,
            )?;
            return Ok(true);
        }
        let parent_event_id = if parent_event_id.is_empty() {
            chain_parent_event_id
        } else {
            parent_event_id.to_string()
        };
        let before_sha = content_digest_of_text(&before_text);
        let after_sha = content_digest_of_text(after_text);
        let mut changes = Vec::new();
        if !before_text.is_empty() {
            changes.push(EditorChange::Delete {
                index: Utf8ByteOffset::unchecked(0),
                text: before_text.clone(),
            });
        }
        if !after_text.is_empty() {
            changes.push(EditorChange::Insert {
                index: Utf8ByteOffset::unchecked(0),
                text: after_text.to_string(),
            });
        }

        // 同步证明事件属于本机产生的 SyncRemote 事实，平台字段必须来自当前
        // 设备，不能因为走同步路径就硬编码成 Desktop。优先使用平台初始化注入
        // 的值；直接构造 Core（如 MCP/测试）时回退持久化设备身份。
        let persisted_info = crate::settings::load_device_info(&self.app_data_root).ok();
        let platform = self
            .preferred_platform()
            .or_else(|| {
                persisted_info.as_ref().and_then(|info| {
                    writer_platform_api::PlatformKind::from_str_name(&info.platform)
                })
            })
            .unwrap_or(writer_platform_api::PlatformKind::Desktop);
        // 只有持久化平台与当前平台一致时，持久化的 device_class 才可信；
        // 旧版本可能已把平台写成 desktop，此时改用当前平台的默认设备类型。
        let device_class = persisted_info
            .as_ref()
            .filter(|info| {
                info.platform.is_empty()
                    || writer_platform_api::PlatformKind::from_str_name(&info.platform)
                        == Some(platform)
            })
            .map(|info| info.device_class.clone())
            .filter(|class| !class.is_empty())
            .unwrap_or_else(|| platform.default_device_class().to_string());
        let preferred_device_id = self.preferred_device_id();
        let device_info = self.ensure_device_info(
            platform.to_str_name(),
            &device_class,
            preferred_device_id.as_deref(),
        )?;
        let selection = EditorSelection {
            anchor: EditorCursor::from_offset(Utf8ByteOffset::unchecked(0)),
            head: EditorCursor::from_offset(Utf8ByteOffset::unchecked(0)),
        };
        let mut event = WritingInputEvent::with_identity(
            sync_event_id.to_string(),
            occurred_at_ms,
            &device_info.device_id,
            platform,
            &device_class,
            project_id,
            volume_id,
            chapter_id,
            EventSource::SyncRemote,
            0,
            0,
            0,
            0,
            0,
            sync_event_id,
        );
        event.proof = Some(ProofEditPayload {
            format_version: PROOF_FORMAT_VERSION,
            seq: 0,
            session_id: sync_event_id.to_string(),
            baseline_id: String::new(),
            starts_new_chain: false,
            base_revision: 0,
            new_revision: 0,
            cause: EditorTransactionCause::Programmatic,
            changes,
            old_selection: selection,
            new_selection: selection,
            before_content_hash: String::new(),
            after_content_hash: String::new(),
            before_text_sha256: before_sha,
            after_text_sha256: after_sha,
            prev_event_hash: String::new(),
            event_hash: String::new(),
            sync_source_device_id: sync_source_device_id.to_string(),
            parent_event_id,
        });
        stats.record_proof_event(event, Some(&before_text))
    }

    /// 正文保存成功后记录一份完整正文检查点，作为长章节回放的加速索引。
    /// checkpoint 只是索引，逐字事实仍是事件流；重复保存同一内容会去重。
    pub fn record_chapter_checkpoint(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        text: &str,
        session_id: &str,
    ) -> Result<()> {
        self.get_stats_api()
            .record_chapter_checkpoint(project_id, volume_id, chapter_id, text, session_id)
    }

    /// 按编辑事务上报写作统计（平台端编辑事务与统计同源的正式入口）。
    ///
    /// 平台端只透传「编辑事实」：编辑 cause + contentDelta 的 inserted/deleted，
    /// 不自己猜 source。`cause → EventSource` 和「各计数字段怎么分配」这套业务规则
    /// 由 Core 单侧决定，避免每个平台各写一份、慢慢分叉。
    ///
    /// ## 映射规则
    ///
    /// - `Typing` / `TypingCommit` / `ImeComposition` → `HumanTyped`，记 `inserted` / `deleted`。
    /// - `Paste` → `Pasted`，字数全部记 `pasted`，`inserted` 为 0。
    /// - `Delete` → `Deleted`，只记 `deleted`。
    /// - `Undo` / `Redo` / `Load` / `Format` / `Programmatic` → `Unknown`，
    ///   不是人工输入；仍按实际 delta 记 `inserted` / `deleted`，只是不计入分类计数器。
    ///
    /// 一次事件只让一个来源的「插入」字段非零，这样
    /// `net_delta_chars = inserted + pasted + ai_inserted - deleted` 才成立。
    ///
    /// 不再接受 `duration_seconds` 和 `session_id` 参数：事件只提供字数事实，
    /// 真实时长由平台端通过 [`Self::record_writing_activity_segment`] 上报。
    /// 设备身份从 Core 的 `current_device.json` 取。
    ///
    /// `event_id` / `occurred_at_ms` 由平台端为编辑事务固定并原样重发：同一
    /// `event_id` 只落一条、只计一次；`occurred_at_ms` 是事件发生时间，
    /// 重发也不会把旧输入改记到今天（空串 / 0 表示老客户端，按 Core 旧行为生成）。
    #[allow(clippy::too_many_arguments)]
    pub fn record_editor_change_stats(
        &self,
        platform_str: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        cause: crate::editor::EditorTransactionCause,
        inserted_chars: u32,
        deleted_chars: u32,
        event_id: &str,
        occurred_at_ms: i64,
    ) -> Result<()> {
        use crate::editor::EditorTransactionCause;

        // 同上：统计事件写入只追加事件，不做全工作区扫描。

        // 设备身份由 Core 读/建，不让平台端传；平台初始化注入的稳定 ID 优先。
        let platform = self.effective_platform(platform_str);
        let preferred_device_id = self.preferred_device_id();
        let device_info = self.ensure_device_info(
            platform.to_str_name(),
            platform.default_device_class(),
            preferred_device_id.as_deref(),
        )?;
        let device_id = device_info.device_id.as_str();
        let device_class = device_info.device_class.as_str();

        let (source, inserted, deleted, pasted) = match cause {
            EditorTransactionCause::Typing
            | EditorTransactionCause::TypingCommit
            | EditorTransactionCause::ImeComposition => {
                (EventSource::HumanTyped, inserted_chars, deleted_chars, 0)
            }
            EditorTransactionCause::Paste => {
                (EventSource::Pasted, 0, deleted_chars, inserted_chars)
            }
            EditorTransactionCause::Delete => (EventSource::Deleted, 0, deleted_chars, 0),
            EditorTransactionCause::Undo
            | EditorTransactionCause::Redo
            | EditorTransactionCause::Load
            | EditorTransactionCause::Format
            | EditorTransactionCause::Programmatic => {
                (EventSource::Unknown, inserted_chars, deleted_chars, 0)
            }
        };

        // 空事件不落盘：纯光标移动 / 无内容变化的事务不该污染会话时长。
        if inserted == 0 && deleted == 0 && pasted == 0 {
            return Ok(());
        }

        let event = WritingInputEvent::with_identity(
            event_id.to_string(),
            occurred_at_ms,
            device_id,
            platform,
            device_class,
            project_id,
            volume_id,
            chapter_id,
            source,
            inserted,
            deleted,
            pasted,
            0,  // ai_inserted_chars
            0,  // duration_seconds — 已弃用；真实时长只来自 writing activity segment
            "", // session_id — 真实计时切片上报，不从事件时间差推算
        );

        self.get_stats_api().record_event(event)
    }

    /// 上报一段平台前台状态测得的写作计时切片。
    ///
    /// 平台端只传本机观察到的真实前台区间；设备身份、设备类别和本地日期
    /// 都由 Core 补齐。Core 按 `event_id` 幂等去重，重发不会重复累计。
    pub fn record_writing_activity_segment(
        &self,
        input: crate::api::WritingActivitySegmentInputDto,
    ) -> Result<()> {
        let platform = self.effective_platform(&input.platform);
        let preferred_device_id = self.preferred_device_id();
        let device_info = self.ensure_device_info(
            platform.to_str_name(),
            platform.default_device_class(),
            preferred_device_id.as_deref(),
        )?;
        let segment = if input.starmap_id.is_empty() {
            crate::writing_stats::WritingActivitySegment::new(
                input.event_id,
                &device_info.device_id,
                platform,
                &device_info.device_class,
                &input.project_id,
                &input.volume_id,
                &input.chapter_id,
                &input.session_id,
                input.start_at_ms,
                input.end_at_ms,
                &input.pause_reason,
            )?
        } else {
            crate::writing_stats::WritingActivitySegment::new_for_starmap(
                input.event_id,
                &device_info.device_id,
                platform,
                &device_info.device_class,
                (!input.project_id.is_empty()).then_some(input.project_id.as_str()),
                &input.starmap_id,
                &input.session_id,
                input.start_at_ms,
                input.end_at_ms,
                &input.pause_reason,
            )?
        };
        self.get_stats_api().record_activity_segment(segment)
    }

    pub fn get_writing_stats_summary(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_summary(&range)
    }

    /// 「今天」的写作汇总。今天是哪一天由 **Core 的本地日历口径**决定。
    pub fn get_today_writing_stats_summary(&self) -> Result<Value> {
        self.get_stats_api().get_today_stats_summary()
    }

    pub fn get_writing_stats_by_project(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_project(&range)
    }

    pub fn get_writing_stats_by_chapter(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_chapter(&range)
    }

    pub fn get_writing_stats_by_device(&self, start_date: &str, end_date: &str) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_device(&range)
    }

    pub fn get_writing_stats_by_device_class(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_stats_by_device_class(&range)
    }

    pub fn get_writing_speed_curve(
        &self,
        start_date: &str,
        end_date: &str,
        bucket_minutes: u32,
    ) -> Result<Value> {
        let range = DateRange {
            start_date: start_date.to_string(),
            end_date: end_date.to_string(),
        };
        self.get_stats_api().get_speed_curve(&range, bucket_minutes)
    }

    /// 「当前写作速度」：以调用时刻为终点的实时纯输入速度。
    ///
    /// 与 [`Self::get_writing_speed_curve`] 分工明确：速度曲线是历史分桶，
    /// 这里是以「现在」为终点重算的窗口速度，停笔超过一个窗口后回落到 0。
    pub fn get_current_writing_speed(&self, window_seconds: u32) -> Result<CurrentWritingSpeed> {
        self.get_stats_api().get_current_speed(window_seconds)
    }

    /// 单作品写作档案（#934）：作品元数据 + 历史累计人工码字 + 当前正文字数。
    ///
    /// 数据源语义分开：`created_at` 来自作品元数据；`current_word_count` 来自
    /// 现有章节存量（`get_project_stats`）；其余字段来自合并后的写作事件历史，
    /// `history_scope` 说明覆盖范围（仅本机 / 已同步设备 / 同步不完整）。
    /// 作品不存在时返回 `ProjectNotFound`，不返回「0 字作品」。
    pub fn get_project_writing_lifetime_stats(
        &self,
        project_id: &str,
    ) -> Result<super::ProjectWritingLifetimeStats> {
        let project = crate::project::get_project(self.projects_root(), project_id)?;
        let stats_api = self.get_stats_api();
        let lifetime = stats_api.get_project_lifetime_stats(project_id)?;
        let stats = self.get_project_stats(project_id)?;

        Ok(super::ProjectWritingLifetimeStats {
            project_id: project.id,
            created_at: project.created_at,
            first_written_at_ms: lifetime.first_written_at_ms,
            current_word_count: u64::from(stats.total_word_count),
            total_human_typed_chars: lifetime.total_human_typed_chars,
            best_writing_day: lifetime.best_writing_day,
            best_writing_day_chars: lifetime.best_writing_day_chars,
            writing_days: lifetime.writing_days,
            history_scope: stats_api.history_scope(),
        })
    }

    /// 统计面板一次性快照（#935）：读一次事件，按 Core 本地日历切分
    /// 今日 / 本周 / 本月，并给出本周范围的作品、章节、设备排行与速度曲线。
    pub fn get_writing_stats_dashboard(&self) -> Result<Value> {
        self.get_stats_api().get_dashboard_snapshot()
    }

    /// 统计页 Explorer 一次性快照（#944）：生涯摘要 + 四张时段卡 + 选中时段详情。
    ///
    /// 一次读盘覆盖全部时段；`preset` 取 `"last7Days"` / `"last30Days"` /
    /// `"last6Months"` / `"lifetime"` / `"custom"`，`custom` 必须带
    /// `custom_start` / `custom_end`（两端包含）。日期口径、纪录与趋势口径
    /// 全在 Core，端侧不传「今天」也不自己统计。
    pub fn get_writing_stats_explorer(
        &self,
        preset: &str,
        custom_start: Option<&str>,
        custom_end: Option<&str>,
    ) -> Result<Value> {
        let snapshot =
            self.get_stats_api()
                .get_explorer_snapshot(preset, custom_start, custom_end)?;
        serde_json::to_value(snapshot).map_err(Into::into)
    }

    /// 每日码字日历（#956）：指定公历年的逐日人工输入真值。
    ///
    /// 与 Explorer 同一份合并去重事件投影，端侧只做位置和色彩渲染；
    /// 非法年份返回错误，不静默补全年零数据。
    pub fn get_writing_stats_heatmap(
        &self,
        year: i32,
    ) -> Result<crate::api::types::WritingStatsHeatmapDto> {
        self.get_stats_api().get_heatmap_snapshot(year)
    }
}
