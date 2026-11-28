// =============================================================================
// chapter_save_queue/worker.rs — 唯一的章节保存 worker
// =============================================================================
//
// 只执行成功从 store.mailbox 原子认领（Pending → Started）的请求；已被同会话
// 更高 revision 接替（Superseded）的旧快照在这里拿不到请求，直接丢弃。
// 同一 worker 还负责编辑事实、草稿正文与项目草稿恢复（draft_jobs）。

use super::draft_jobs;
use super::*;

pub(super) struct ChapterSaveChain {
    /// 本链第一条请求的 base 版本；队列里多份从同一版本入队的快照
    /// 都以它为锚点，不能因为前一请求推进了 expected 就把后续请求判成冲突。
    anchor: Option<ChapterVersionDto>,
    /// 最近一次请求实际使用的 base 版本（成功/失败都记录）。
    expected: Option<ChapterVersionDto>,
    /// 最近一次成功写入后的版本，用来推进后续请求的 base。
    receipt: Option<ChapterVersionDto>,
    /// 与 receipt 对应的正文，作为冲突材料的 base。
    text: Arc<str>,
    /// 已排队/完成请求中最大的编辑器快照 revision。旧快照不得借用新版本。
    snapshot_revision: u64,
    /// 链上出现失败/外部更新后失效，后续请求只能重新对齐磁盘版本。
    invalid: bool,
    last_used: Instant,
}

pub(crate) struct ChapterSaveWorkerState {
    apis: HashMap<String, WriterCoreApi>,
    chains: HashMap<String, ChapterSaveChain>,
    /// 测试注入的 Core API（生产路径按 context 懒建）。
    #[cfg(test)]
    test_api: Option<WriterCoreApi>,
}

impl ChapterSaveWorkerState {
    fn new() -> Self {
        Self {
            apis: HashMap::new(),
            chains: HashMap::new(),
            #[cfg(test)]
            test_api: None,
        }
    }

    /// 测试用：固定返回注入的 Core API，避免依赖真实布局上下文。
    #[cfg(test)]
    pub(super) fn for_tests(api: WriterCoreApi) -> Self {
        let mut state = Self::new();
        state.test_api = Some(api);
        state
    }

    pub(super) fn api_for(&mut self, context: &ChapterSaveContext) -> &WriterCoreApi {
        #[cfg(test)]
        if let Some(api) = self.test_api.as_mut() {
            return api;
        }
        let key = context.key();
        self.apis.entry(key).or_insert_with(|| {
            crate::backend::app_backend::with_layout_core_api(
                &context.data_root,
                &context.projects_root,
                &context.layout(),
            )
        })
    }

    fn prune_chains(&mut self, keep_key: &str) {
        if self.chains.len() <= MAX_CHAPTER_CHAINS {
            return;
        }
        let mut candidates: Vec<(Instant, String)> = self
            .chains
            .iter()
            .filter(|(key, _)| key.as_str() != keep_key)
            .map(|(key, chain)| (chain.last_used, key.clone()))
            .collect();
        candidates.sort_by_key(|(last_used, _)| *last_used);
        for (_, key) in candidates {
            if self.chains.len() <= MAX_CHAPTER_CHAINS {
                break;
            }
            self.chains.remove(&key);
        }
    }
}

fn chapter_chain_key(request: &ChapterSaveRequest) -> String {
    // 卷 ID 不进入链身份：Issue #1051 允许当前章节跨卷移动，而 object_generation
    // / revision 随章节留在原地；用项目 + 章节才能在移动后继续用前一回执推进 base。
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        request.context.key(),
        request.identity.project_id,
        request.identity.chapter_id,
        request.identity.session_id
    )
}

fn resolve_chain_base(
    chain: Option<&ChapterSaveChain>,
    expected: &Option<ChapterVersionDto>,
    base_content: &Option<String>,
    snapshot_revision: u64,
) -> (Option<ChapterVersionDto>, Option<String>, bool) {
    let mut effective = expected.clone();
    let mut effective_base = base_content.clone();
    let mut chained = false;
    if let Some(chain) = chain {
        if chain.invalid {
            return (effective, effective_base, false);
        }
        if let (Some(request_base), Some(receipt)) = (expected.as_ref(), chain.receipt.as_ref()) {
            // 请求 base 要么是链当前 expected（QML 已用上一回执推进），
            // 要么仍是链锚点（多份快照在队列积压时都从 V0 入队）。两种
            // 情况都允许接到直接前序回执；但快照必须严格晚于链上已见快照。
            let matches_expected = chain.expected.as_ref().is_some_and(|chain_base| {
                chain_base.object_generation == request_base.object_generation
                    && chain_base.revision == request_base.revision
                    && chain_base.content_sha256 == request_base.content_sha256
            });
            let matches_anchor = chain.anchor.as_ref().is_some_and(|anchor| {
                anchor.object_generation == request_base.object_generation
                    && anchor.revision == request_base.revision
                    && anchor.content_sha256 == request_base.content_sha256
            });
            if (matches_expected || matches_anchor)
                && snapshot_revision > chain.snapshot_revision
                && request_base.object_generation == receipt.object_generation
                && receipt.revision > request_base.revision
            {
                effective = Some(receipt.clone());
                effective_base = Some(chain.text.to_string());
                chained = true;
            }
        }
    }
    (effective, effective_base, chained)
}

pub(super) fn chapter_save_worker_loop(receiver: mpsc::Receiver<ChapterSaveJob>) {
    let mut state = ChapterSaveWorkerState::new();
    loop {
        // Issue #1062 三.4：草稿正文不再随命令进 channel，而是留在内存
        // registry 里按 2s 节流落盘。worker 在等保存命令时最多睡到最近一次
        // persist 截止；到点先 flush_due，再继续接收。
        match crate::backend::chapter_draft::next_persist_deadline() {
            Some(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                match receiver.recv_timeout(wait) {
                    Ok(job) => handle_job(&mut state, job),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        crate::backend::chapter_draft::flush_due();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match receiver.recv() {
                Ok(job) => handle_job(&mut state, job),
                Err(_) => break,
            },
        }
    }
    // worker 退出后不会再消费 DraftText 命令：丢弃 mailbox 里的正文快照，
    // 不能把整章正文留在进程内（发送失败路径也会自行清理）。
    super::clear_draft_text_mailbox();
}

fn handle_job(state: &mut ChapterSaveWorkerState, job: ChapterSaveJob) {
    match job {
        ChapterSaveJob::Save(request_id) => handle_chapter_save(state, request_id),
        ChapterSaveJob::RecordFact(command) => {
            draft_jobs::handle_record_fact(state, command);
        }
        ChapterSaveJob::DraftText(command) => draft_jobs::handle_draft_text(command),
        ChapterSaveJob::RestoreProject(command) => {
            super::restore_draft::handle_restore_project(state, command);
        }
        ChapterSaveJob::Drain(ack) => {
            // 退出边界：先把已到期的 dirty 草稿落盘，再确认此前保存全部完成；
            // flush_due 只记录持久化错误，worker 不因草稿写盘失败退出。
            crate::backend::chapter_draft::flush_due();
            let _ = ack.send(());
        }
    }
}

fn handle_chapter_save(state: &mut ChapterSaveWorkerState, request_id: u32) {
    // 只有成功把 Pending 原子转成 Started 的请求才允许执行；已被更高
    // revision 接替（Superseded）的旧快照在这里直接丢弃，绝不能覆盖终态。
    let Some(request) = claim_save(request_id) else {
        return;
    };
    // Issue #1062 评论 7191076411 第 1 项（平台侧）：Core durable journal 是
    // 草稿 ack 的唯一事实源。保存（freeze）前先把 Core 侧本批未确认的草稿
    // 提交对账到本地：写成本地 ack、重放 mark_committed 并回写 Core，避免
    // 旧事件以旧 base 再次进入证明批次。
    //
    // 评论 #7192111899 第 2 项：对账失败时不能继续 freeze。Core journal 里
    // 尚未复制到本地的草稿 ack 对 freeze 不可见，放行会让旧事实以旧 base
    // 进入保存链。这里以终态错误完成请求，让 QML continuation 拿到明确失败。
    {
        let api = state.api_for(&request.context);
        if let Err(error) = super::core_ack::reconcile_core_acks(
            api,
            request.context.projects_root(),
            &request.identity.project_id,
            Some(request.identity.chapter_id.as_str()),
        ) {
            let message = error.to_string();
            log::warn!(
                "reconcile core draft acks before save failed: project={} chapter={}: {message}",
                request.identity.project_id,
                request.identity.chapter_id
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code: "DRAFT_ACK_RECONCILE_FAILED".to_string(),
                    message_key: "error.draft_ack_reconcile_failed".to_string(),
                    message_args: serde_json::json!({
                        "projectId": request.identity.project_id,
                        "chapterId": request.identity.chapter_id,
                        "detail": message.clone(),
                    }),
                    raw_error: message,
                    conflict: false,
                },
                false,
                String::new(),
            );
            return;
        }
    }
    let chain_key = chapter_chain_key(&request);
    let (effective_expected, effective_base, chained) = {
        let chain = state.chains.get(&chain_key);
        resolve_chain_base(
            chain,
            &request.expected,
            &request.base_content,
            request.identity.snapshot_revision,
        )
    };
    if let Some(chain) = state.chains.get_mut(&chain_key) {
        chain.expected = effective_expected.clone();
        chain.last_used = Instant::now();
    } else {
        state.chains.insert(
            chain_key.clone(),
            ChapterSaveChain {
                anchor: effective_expected.clone(),
                expected: effective_expected.clone(),
                receipt: None,
                text: Arc::from(""),
                snapshot_revision: request.identity.snapshot_revision,
                invalid: false,
                last_used: Instant::now(),
            },
        );
    }
    state.prune_chains(&chain_key);

    let api = state.api_for(&request.context);
    let projects_root = PathBuf::from(&request.context.projects_root);
    // Issue #1062 1.1：证明批次由 actor 从自己的编辑事实草稿里冻结，不再用
    // QML 传来的 event ID 去 Core 全局 pending map 猜批次。快照 revision 之前
    // 的连续事实前缀随正文一起提交；之后的事实留给下一次保存。
    let base_generation = request
        .expected
        .as_ref()
        .map_or("", |version| version.object_generation.as_str());
    // Issue #1062 复审第 3 项：Core 投影构造失败的原始命令必须先重试成功，
    // 否则这次保存会越过真实编辑事件的 revision，把证明链断开。
    if let Err(error) = draft_jobs::retry_pending_inputs(
        api,
        &projects_root,
        &request.identity.project_id,
        &request.identity.chapter_id,
        &request.identity.volume_id,
        request.identity.session_id,
        base_generation,
    ) {
        complete_error(
            request_id,
            ChapterSaveError {
                code: "PROOF_FACT_PENDING_RETRY_FAILED".to_string(),
                message_key: "error.proof_fact_pending_retry_failed".to_string(),
                message_args: serde_json::json!({
                    "projectId": request.identity.project_id,
                    "chapterId": request.identity.chapter_id,
                }),
                raw_error: error,
                conflict: false,
            },
            chained,
            String::new(),
        );
        return;
    }
    let frozen_facts = match crate::backend::chapter_draft::freeze_batch_for(
        &projects_root,
        &request.identity.project_id,
        &request.identity.chapter_id,
        request.identity.session_id,
        base_generation,
        request.identity.snapshot_revision,
    ) {
        Ok(facts) => facts,
        Err(error) => {
            // 冻结失败不能退化成“无证明保存”：否则这批事实的 before 链会
            // 与之后提交的正文基线脱节，后续保存永久 PROOF_BATCH_MISMATCH。
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "freeze_proof_batch_failed",
                &format!(
                    "request_id={} project={} chapter={}: {error}",
                    request_id, request.identity.project_id, request.identity.chapter_id
                ),
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code: "PROOF_BATCH_FREEZE_FAILED".to_string(),
                    message_key: "error.proof_batch_freeze_failed".to_string(),
                    message_args: serde_json::json!({ "detail": error }),
                    raw_error: error,
                    conflict: false,
                },
                chained,
                String::new(),
            );
            return;
        }
    };
    let frozen_event_ids: Vec<String> = frozen_facts
        .iter()
        .map(|fact| fact.event_id.clone())
        .collect();
    let proof_batch_json = match frozen_facts
        .iter()
        .map(|fact| {
            serde_json::from_str::<serde_json::Value>(&fact.projection_json)
                .map_err(|error| format!("event={} projection 无法解析: {error}", fact.event_id))
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|values| {
            serde_json::to_string(&values).map_err(|error| format!("批次序列化失败: {error}"))
        }) {
        Ok(json) => json,
        Err(error) => {
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "freeze_proof_batch_failed",
                &format!(
                    "request_id={} project={} chapter={}: {error}",
                    request_id, request.identity.project_id, request.identity.chapter_id
                ),
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code: "PROOF_BATCH_FREEZE_FAILED".to_string(),
                    message_key: "error.proof_batch_freeze_failed".to_string(),
                    message_args: serde_json::json!({ "detail": error }),
                    raw_error: error,
                    conflict: false,
                },
                chained,
                String::new(),
            );
            return;
        }
    };
    let save_result = api.save_chapter_content_versioned_with_proof_batch(
        &request.identity.project_id,
        &request.identity.volume_id,
        &request.identity.chapter_id,
        &request.content,
        effective_expected,
        effective_base,
        request.identity.snapshot_revision,
        &proof_batch_json,
        request.allow_empty_overwrite,
    );

    match save_result {
        Ok(receipt) => {
            // journal 已确认：只 ack 本批包含的事件；快照之后的编辑事实留在
            // 草稿里等下一次保存。
            //
            // Issue #1062 评论 7191076411 第 3 项：不能用“冻结批次为空”推断
            // 整章草稿为空并删除目录——GUI 可能已经带来更新的正文（或在
            // 处理中的较新快照）。统一交给 mark_committed 在锁内按 Core
            // durable 回执结算：只有它确认没有剩余的 facts/正文时才返回
            // Ok(true) 表示目录已删。
            // 以 journal 回执确认的 event IDs 为准（Core 只回本批进入
            // durable journal 的投影）；回执缺失时才退回冻结批次。
            let acknowledged = if receipt.pending_projections.is_empty() {
                frozen_event_ids.clone()
            } else {
                receipt.pending_projections.clone()
            };
            let mut pending_ack_recorded = false;
            if !acknowledged.is_empty() {
                // Issue #1062 复审 P0：Core journal 已确认这一批，但本地 ack
                // 可能失败。先持久化 pending-ack（transaction + 已确认
                // event IDs + 新版本），下次 freeze/恢复前幂等补做，避免旧
                // 事件以旧 base 再次进入批次而永久 PROOF_BATCH_MISMATCH。
                let pending_ack = crate::backend::chapter_draft::PendingAck {
                    transaction_id: receipt.transaction_id.clone(),
                    event_ids: acknowledged.clone(),
                    base_generation: receipt.version.object_generation.clone(),
                    base_revision: receipt.version.revision,
                    base_sha256: receipt.version.content_sha256.clone(),
                    editor_session_id: request.identity.session_id,
                    snapshot_revision: request.identity.snapshot_revision,
                    snapshot_digest: request.identity.snapshot_sha256.clone(),
                    commit_sequence: receipt.commit_sequence,
                    reconciled: false,
                };
                match crate::backend::chapter_draft::record_pending_ack(
                    &projects_root,
                    &request.identity.project_id,
                    &request.identity.chapter_id,
                    &pending_ack,
                ) {
                    Ok(()) => pending_ack_recorded = true,
                    Err(error) => crate::backend::app_backend::debug_error_static(
                        "chapter_draft",
                        "record_pending_ack_failed",
                        &error,
                    ),
                }
            }
            let draft_removed = match crate::backend::chapter_draft::mark_committed_with_settlement(
                &projects_root,
                &request.identity.project_id,
                &request.identity.chapter_id,
                &receipt.transaction_id,
                &acknowledged,
                &receipt.version.object_generation,
                receipt.version.revision,
                &receipt.version.content_sha256,
                Some(&crate::backend::chapter_draft::TextSettlement {
                    session_id: request.identity.session_id,
                    snapshot_revision: request.identity.snapshot_revision,
                    snapshot_digest: request.identity.snapshot_sha256.clone(),
                }),
                receipt.commit_sequence,
            ) {
                Ok(decision) => {
                    // Issue #1062 评论 7193847871 第 2 项：正文收口已并入
                    // mark_committed 的同一 meta 决议；只有它返回 Ok（meta 已
                    // durable）才会回写 Core journal 的 draft ack。
                    //
                    // Issue #1062 评论 #7209683560 第 1 项：决议是强类型；
                    // NoDraft 表示本地没有可应用的草稿，不能在这里把 Core
                    // ack 标成完成——留给下一次对账按归属重新判定。
                    let removed = decision.removed();
                    if pending_ack_recorded && decision.settled() {
                        super::core_ack::acknowledge_core_transaction(
                            api,
                            request.context.projects_root(),
                            &request.identity.project_id,
                            &request.identity.volume_id,
                            &request.identity.chapter_id,
                            &receipt.transaction_id,
                            &acknowledged,
                        );
                    }
                    removed
                }
                Err(error) => {
                    // ack 决议没有落盘：pending-ack 文件保留，下一次
                    // freeze/恢复按 Core journal 的确认事件补做。
                    crate::backend::app_backend::debug_error_static(
                        "chapter_draft",
                        "mark_committed_deferred",
                        &error,
                    );
                    false
                }
            };
            // Issue #1062 复审第 2 项：保存边界立即强制 flush 草稿正文，
            // 不能把回执之后的更新只留在内存里等 2s 节流。`persist_current`
            // 自己按 `text_is_verified` 判断正文是否已被证明覆盖；没有可发布
            // 正文（含草稿本就不存在）时不必空转。
            if !draft_removed
                && crate::backend::chapter_draft::text_ready_for_commit(
                    &projects_root,
                    &request.identity.project_id,
                    &request.identity.chapter_id,
                )
            {
                if let Err(error) = crate::backend::chapter_draft::persist_current(
                    &projects_root,
                    &request.identity.project_id,
                    &request.identity.chapter_id,
                    true,
                ) {
                    log::warn!(
                        "force persist chapter draft after save failed: project={} chapter={}: {error}",
                        request.identity.project_id,
                        request.identity.chapter_id
                    );
                }
            }
            if let Some(chain) = state.chains.get_mut(&chain_key) {
                chain.receipt = Some(receipt.version.clone());
                chain.text = Arc::clone(&request.content);
                chain.snapshot_revision = chain
                    .snapshot_revision
                    .max(request.identity.snapshot_revision);
                chain.invalid = false;
                chain.last_used = Instant::now();
            }
            complete_ok(request_id, receipt, chained);
        }
        Err(error) => {
            if let Some(chain) = state.chains.get_mut(&chain_key) {
                // 失败后链不能再把后续快照接到旧回执上；下一请求必须
                // 重新读取磁盘版本或显式冲突。
                chain.invalid = true;
                chain.last_used = Instant::now();
            }
            let code = error.code().to_string();
            let conflict = code == "CHAPTER_VERSION_CONFLICT";
            let conflict_draft_path = if conflict {
                // Core 回执直接携带本次冲突的真实事务 ID 与归档目录；
                // 不按目录 mtime/正文相等猜测，避免同章并行冲突或两章
                // 正文相同时指到错误路径。
                let params = error.params();
                let conflict_dir = params.get("conflict_dir").cloned().unwrap_or_default();
                let transaction_id = params.get("transaction_id").cloned().unwrap_or_default();
                crate::backend::app_backend::debug_error_static(
                    "chapter_save_queue",
                    "chapter_conflict_receipt",
                    &format!(
                        "request_id={request_id} transaction_id={transaction_id} conflict_dir={conflict_dir}"
                    ),
                );
                conflict_dir
            } else {
                String::new()
            };
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "chapter_save_failed",
                &format!(
                    "request_id={} code={} chained={} conflict_dir={}",
                    request_id, code, chained, conflict_draft_path
                ),
            );
            complete_error(
                request_id,
                ChapterSaveError {
                    code,
                    message_key: error.message_key().to_string(),
                    message_args: serde_json::to_value(error.params())
                        .unwrap_or_else(|_| serde_json::json!({})),
                    raw_error: error.to_string(),
                    conflict,
                },
                chained,
                conflict_draft_path,
            );
        }
    }
}
