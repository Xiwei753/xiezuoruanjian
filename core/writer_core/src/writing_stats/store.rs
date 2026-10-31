//! # 写作统计数据存储模块
//!
//! 本模块只负责事件存储和按日期范围读取事件。所有统计计算（聚合、速度曲线等）
//! 已移至 [`crate::writing_stats::projection`] 模块。
//!
//! ## 存储结构
//!
//! ```text
//! stats/
//!   events/
//!     <origin_device_id>/
//!       2024-01-01.events.jsonl     # 原始输入事件（一天一个文件）
//!       2024-01-01.sessions.jsonl   # 已测量的前台写作计时切片（一天一个文件）
//! ```
//!
//! 两类文件同属该设备的同步分区，但事实含义不同：`.events.jsonl` 只回答
//! 「输入了多少字」，`.sessions.jsonl` 只回答「真实计时了多久」。读取时各自
//! 按 `event_id` 去重，绝不把事件时间差反推成会话时长。
//!
//! - 事件按 UTC 日期分文件存储；业务日历日（每日统计分桶、「今天」查询）走
//!   [`crate::writing_stats::calendar`] 模块，事件中的 `local_date` 保持事件
//!   发生地当时的值。
//! - 每台设备只向自己的 `origin_device_id` 分区追加事件；其他设备同步下载来的
//!   分区只读。读取时扫描全部分区，按 `event_id` 去重后组成同一份聚合输入。
//! - 旧版单分区事件库由 [`crate::writing_stats::migration`] 一次性迁入本结构。

use crate::editor::kernel::ContentDigest;
use crate::editor::{EditorChange, EditorSelection, Utf8ByteOffset};
use crate::error::Result;
use crate::writing_stats::calendar;
use crate::writing_stats::checkpoint::{self, ProofCheckpoint};
use crate::writing_stats::proof::{
    self, apply_changes, chain_content_hash, compute_event_hash, content_hash_of_text,
    text_digest_matches, text_sha256,
};
use crate::writing_stats::{WritingActivitySegment, WritingInputEvent};
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// 统计事件根目录（相对 app_data_root）。
const EVENTS_ROOT: &str = "stats/events";
/// 每 800 个内容事件写一个完整正文 checkpoint；事件仍是长期事实。
const PERIODIC_CHECKPOINT_INTERVAL: u64 = 800;

/// 跨分区重复 `event_id` 的结构化诊断记录。
///
/// 只记录事实用于诊断，不覆盖任何原始行；聚合保留先出现的事件（本机分区优先）。
#[derive(Debug, Serialize)]
struct EventIdConflict {
    event_id: String,
    kept_origin_device_id: String,
    dropped_origin_device_id: String,
    dropped_file: String,
    detected_at_ms: i64,
}

/// 单个来源设备分区里的一个事件文件。
#[derive(Debug, Clone)]
struct EventFile {
    origin: String,
    date: String,
    path: PathBuf,
    /// 已封口的 zstd 归档段（`proof/archive/<date>.events.jsonl.zst`）。
    compressed: bool,
}

/// 写作统计事件存储。
pub struct StatsStore {
    app_data_root: PathBuf,
    /// 本机来源分区标识（懒解析后缓存）。
    origin_device_id: Mutex<Option<String>>,
    /// 按「分区/UTC 日期」缓存已持久化的 event_id，避免每次全量重扫。
    written_ids: Mutex<HashMap<String, HashSet<String>>>,
    /// 证明序号与每章节链头。首次写入时扫描已有事件重建。
    proof_state: Mutex<ProofStoreState>,
    /// 证明事件变化代数；回放缓存据此失效。
    proof_generation: AtomicU64,
}

#[derive(Debug, Clone)]
struct ProofChainHead {
    seq: u64,
    event_hash: String,
    content_hash: String,
    text_sha256: String,
}

#[derive(Debug, Default)]
struct ProofStoreState {
    initialized: bool,
    next_seq: u64,
    chains: HashMap<String, ProofChainHead>,
    /// 每章节链当前正文摘要（首次需要时从检查点/事件重建）。
    digests: HashMap<String, ContentDigest>,
}

impl StatsStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            origin_device_id: Mutex::new(None),
            written_ids: Mutex::new(HashMap::new()),
            proof_state: Mutex::new(ProofStoreState::default()),
            proof_generation: AtomicU64::new(0),
        }
    }

    fn events_root(&self) -> PathBuf {
        self.app_data_root.join(EVENTS_ROOT)
    }

    /// 本机数据根路径（供同 crate 的统计覆盖范围查询读取同步状态）。
    pub(crate) fn app_data_root(&self) -> &Path {
        &self.app_data_root
    }

    fn event_file_path(&self, origin: &str, date: &str) -> PathBuf {
        self.events_root()
            .join(origin)
            .join(format!("{date}.events.jsonl"))
    }

    fn session_file_path(&self, origin: &str, date: &str) -> PathBuf {
        self.events_root()
            .join(origin)
            .join(format!("{date}.sessions.jsonl"))
    }

    /// 仅读取本机设备标识，不创建任何设备信息。
    pub fn load_origin_device_id(&self) -> Option<String> {
        match crate::settings::load_device_info(&self.app_data_root) {
            Ok(info) if validate_device_segment(&info.device_id) => Some(info.device_id),
            Ok(_) => None,
            Err(e) => {
                eprintln!("[writing_stats] load device info failed: {e}");
                None
            }
        }
    }

    /// 解析本机来源分区标识；设备信息缺失时按需生成本机设备身份。
    pub fn resolve_origin_device_id(&self) -> Result<String> {
        if let Some(id) = self.cached_origin_device_id() {
            return Ok(id);
        }
        if let Some(id) = self.load_origin_device_id() {
            self.cache_origin_device_id(&id);
            return Ok(id);
        }
        let info = crate::settings::ensure_device_info(&self.app_data_root, "", "", None)?;
        if !validate_device_segment(&info.device_id) {
            return Err(crate::Error::Other(format!(
                "invalid device id for stats event partition: {:?}",
                info.device_id
            )));
        }
        self.cache_origin_device_id(&info.device_id);
        Ok(info.device_id)
    }

    fn cached_origin_device_id(&self) -> Option<String> {
        self.origin_device_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    fn cache_origin_device_id(&self, id: &str) {
        if let Ok(mut guard) = self.origin_device_id.lock() {
            *guard = Some(id.to_string());
        }
    }

    /// 已存在统计文件（输入事件或计时切片）的来源设备分区，按名称排序。
    pub fn list_event_origins(&self) -> Result<Vec<String>> {
        let mut origins: Vec<String> = self
            .list_event_files()?
            .into_iter()
            .map(|file| file.origin)
            .chain(
                self.list_session_files()?
                    .into_iter()
                    .map(|file| file.origin),
            )
            .collect();
        origins.sort();
        origins.dedup();
        Ok(origins)
    }

    /// 追加一批事件，只写入本机来源分区。按 `event_id` 幂等：已存在的事件跳过，
    /// 返回真正新写入的事件数。
    pub fn record_events(&self, events: Vec<WritingInputEvent>) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let origin = self.resolve_origin_device_id()?;
        let has_proof = events.iter().any(|event| event.proof.is_some());

        let mut by_date: BTreeMap<String, Vec<WritingInputEvent>> = BTreeMap::new();
        for event in events {
            let date = self.timestamp_to_date(event.timestamp_ms)?;
            by_date.entry(date).or_default().push(event);
        }

        let partition_dir = self.events_root().join(&origin);
        fs::create_dir_all(&partition_dir)?;

        let mut written = 0usize;
        for (date, events) in by_date {
            match self.append_confirmed_events(&origin, &date, events) {
                Ok(count) => written += count,
                Err(error) => {
                    // 写入失败后丢弃该日期的 ID 缓存：重试时以磁盘真值重新判重，
                    // 不能把「调用过写入」当成「持久化完成」而漏字。
                    self.invalidate_written_ids(&format!("{origin}/{date}"));
                    return Err(error);
                }
            }
        }
        // 同步导入的 proof 事件同样会让回放索引过期；按写入批次递增版本，
        // 避免 StatsApi 继续命中旧的内存回放会话。
        if written > 0 && has_proof {
            self.proof_generation.fetch_add(1, Ordering::SeqCst);
        }
        // UTC 日封口：切日后的第一次写入顺手把已关闭日期封成不可变 zstd 段。
        // 归档失败不否定已经落盘的统计事件，下次写入继续尝试。
        if let Err(error) = self.archive_closed_days(&origin) {
            log::warn!("proof archive maintenance failed: {error}");
        }
        Ok(written)
    }

    /// 追加一批新事件，并在**写入确认后**才登记 `event_id`。
    ///
    /// 筛选只读、不改缓存；整批序列化后一次写盘，成功后统一登记 ID。
    /// 失败时调用方丢弃该日期缓存，让重试从磁盘真值重新判重，
    /// `event_id` 不变且重试不会重复计数。
    fn append_confirmed_events(
        &self,
        origin: &str,
        date: &str,
        events: Vec<WritingInputEvent>,
    ) -> Result<usize> {
        let fresh = self.select_fresh_events(origin, date, events)?;
        if fresh.is_empty() {
            return Ok(0);
        }
        let mut buffer = String::new();
        for event in &fresh {
            buffer.push_str(&serde_json::to_string(event)?);
            buffer.push('\n');
        }
        {
            let file_path = self.event_file_path(origin, date);
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;
            file.write_all(buffer.as_bytes())?;
        }

        let mut cache = self.lock_written_ids();
        let ids = cache.entry(format!("{origin}/{date}")).or_default();
        for event in &fresh {
            if !event.event_id.is_empty() {
                ids.insert(event.event_id.clone());
            }
        }
        Ok(fresh.len())
    }

    /// 只读筛选本机分区里尚未写入过的 `event_id`（含本批次内部重复）。
    fn select_fresh_events(
        &self,
        origin: &str,
        date: &str,
        events: Vec<WritingInputEvent>,
    ) -> Result<Vec<WritingInputEvent>> {
        let mut cache = self.lock_written_ids();
        let cache_key = format!("{origin}/{date}");
        if !cache.contains_key(&cache_key) {
            let ids = self.scan_persisted_ids(origin, date)?;
            cache.insert(cache_key.clone(), ids);
        }
        let Some(ids) = cache.get(&cache_key) else {
            return Ok(Vec::new());
        };

        let mut batch_ids: HashSet<String> = HashSet::new();
        let mut fresh = Vec::new();
        for event in events {
            if event.event_id.is_empty() {
                fresh.push(event);
                continue;
            }
            if ids.contains(&event.event_id) || batch_ids.contains(&event.event_id) {
                continue;
            }
            batch_ids.insert(event.event_id.clone());
            fresh.push(event);
        }
        Ok(fresh)
    }

    /// 丢弃某个缓存键的 ID 缓存，让下一次写入以磁盘真值重建。
    fn invalidate_written_ids(&self, cache_key: &str) {
        let mut cache = self.lock_written_ids();
        cache.remove(cache_key);
    }

    /// 事件 ID 缓存只是幂等去重的加速器，磁盘始终是事实源：
    /// 锁中毒时接管内部数据，不让缓存状态阻塞写入。
    fn lock_written_ids(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashSet<String>>> {
        self.written_ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 追加单个事件。
    pub fn record_event(&self, event: WritingInputEvent) -> Result<()> {
        self.record_events(vec![event]).map(|_| ())
    }

    /// 写入一条带证明的编辑事务。序号、前后链摘要和事件哈希由 Core 在这里补齐。
    ///
    /// - 同一 `event_id` 重发只落一条，返回 `false`；
    /// - 没有实际正文变更、没有 proof 细节的事件不写证明；
    /// - 写盘失败时不推进内存链状态，重试不会造成序号/哈希跳跃；
    /// - 首次记录时若提供 `baseline_text`，先写 seq=0 的基线 checkpoint。
    pub(crate) fn record_proof_event(
        &self,
        mut event: WritingInputEvent,
        baseline_text: Option<&str>,
    ) -> Result<bool> {
        if event
            .proof
            .as_ref()
            .is_none_or(|proof| !proof.has_content_change())
        {
            return Ok(false);
        }
        let origin = event.device_id.clone();
        if origin.is_empty() {
            return Err(crate::Error::Other("proof 事件缺少来源设备 ID".to_string()));
        }
        let date = self.timestamp_to_date(event.timestamp_ms)?;
        if self.event_is_persisted(&origin, &date, &event.event_id)? {
            return Ok(false);
        }

        let mut state = self.lock_proof_state();
        if !state.initialized {
            self.initialize_proof_state(&mut state)?;
        }

        let chain_key = format!("{origin}/{}", event.chapter_id);
        let head = state.chains.get(&chain_key).cloned();
        // 链头缓存存在时，逐字事件只消费缓存的增量摘要；绝不为了确认基线
        // 去扫描整个检查点历史。只有首次建立链（head 为空）才需要读最后
        // 一个基线检查点。
        let latest_checkpoint = if head.is_none() {
            checkpoint::load_last_checkpoint(&self.app_data_root, &origin, &event.chapter_id)?
        } else {
            None
        };

        let (seq, before_digest, mut before_text, starts_new_chain, baseline_checkpoint) =
            if let Some(head) = head.as_ref() {
                let seq = state.next_seq.max(head.seq.saturating_add(1));
                let legacy_head = !head.text_sha256.is_empty()
                    && !head.text_sha256.starts_with(proof::CONTENT_DIGEST_VERSION);
                let cached = if legacy_head {
                    None
                } else {
                    state.digests.get(&chain_key).cloned()
                };
                let (before_digest, frame_text) = if let Some(digest) = cached {
                    (digest, None)
                } else {
                    // 缓存缺失（或链头仍是旧完整 SHA）时，从已验证检查点 + 事件重建一次。
                    let frame = crate::writing_stats::replay::replay_chapter(
                        self,
                        &crate::writing_stats::replay::ProofReplayRequest {
                            chapter_id: event.chapter_id.clone(),
                            device_id: origin.clone(),
                            target_seq: Some(head.seq),
                            target_time_ms: None,
                            target_index: None,
                            baseline_text: baseline_text.map(ToString::to_string),
                        },
                    )?;
                    if !frame.valid {
                        return Err(crate::Error::Other(format!(
                            "无法重建章节 {} 的证明链正文: {}",
                            event.chapter_id,
                            frame.errors.join("; ")
                        )));
                    }
                    let digest = ContentDigest::from_text(&frame.text);
                    if !head.text_sha256.is_empty()
                        && !text_digest_matches(&head.text_sha256, &frame.text, &digest)
                    {
                        return Err(crate::Error::Other(format!(
                            "章节 {} 的证明链正文摘要与链头不一致",
                            event.chapter_id
                        )));
                    }
                    state.digests.insert(chain_key.clone(), digest.clone());
                    (digest, Some(frame.text))
                };
                (seq, before_digest, frame_text, false, None)
            } else {
                match latest_checkpoint {
                    Some(checkpoint) => {
                        if checkpoint.seq != 0 {
                            return Err(crate::Error::Other(format!(
                                "章节 {} 有检查点但没有事件链，拒绝从 seq {} 继续",
                                event.chapter_id, checkpoint.seq
                            )));
                        }
                        if checkpoint.text_sha256 != text_sha256(&checkpoint.text)
                            || checkpoint.content_hash != content_hash_of_text(&checkpoint.text)
                        {
                            return Err(crate::Error::Other(format!(
                                "章节 {} 的基线检查点校验失败",
                                event.chapter_id
                            )));
                        }
                        if let Some(given) = baseline_text {
                            if given != checkpoint.text {
                                return Err(crate::Error::Other(format!(
                                    "章节 {} 的正文基线已变化，不能沿用旧检查点；请记录外部变更事件",
                                    event.chapter_id
                                )));
                            }
                        }
                        let digest = ContentDigest::from_text(&checkpoint.text);
                        (
                            state.next_seq.max(checkpoint.seq.saturating_add(1)),
                            digest,
                            Some(checkpoint.text),
                            true,
                            None,
                        )
                    }
                    None => {
                        let text = baseline_text.unwrap_or_default().to_string();
                        let checkpoint = ProofCheckpoint::new(
                            &event.chapter_id,
                            &origin,
                            0,
                            "",
                            event.timestamp_ms,
                            &text,
                            &content_hash_of_text(&text),
                            "",
                            "baseline",
                        );
                        (
                            state.next_seq.max(1),
                            ContentDigest::from_text(&text),
                            Some(text),
                            true,
                            Some(checkpoint),
                        )
                    }
                }
            };

        if let Some(checkpoint) = baseline_checkpoint.as_ref() {
            // 先落基线再写事件：事件写失败时基线仍可复用，不会被伪造。
            checkpoint::append_checkpoint(&self.app_data_root, checkpoint)?;
        }

        let previous_event_hash = head
            .as_ref()
            .map_or_else(String::new, |head| head.event_hash.clone());
        let baseline_id = if starts_new_chain {
            format!("{origin}:{}:0", event.chapter_id)
        } else {
            String::new()
        };
        let expected_before_content_hash = head.as_ref().map_or_else(
            || content_hash_of_text(before_text.as_deref().unwrap_or_default()),
            |head| head.content_hash.clone(),
        );
        let before_digest_value = before_digest.digest();

        let (changes_for_chain, after_digest) = {
            let draft = event
                .proof
                .as_mut()
                .ok_or_else(|| crate::Error::Other("proof 事件缺少 proof 细节".to_string()))?;
            if !draft.before_content_hash.is_empty()
                && draft.before_content_hash != expected_before_content_hash
            {
                return Err(crate::Error::Other(format!(
                    "章节 {} 的 before_content_hash 与链头不一致",
                    event.chapter_id
                )));
            }
            if draft.before_text_sha256.is_empty() != draft.after_text_sha256.is_empty() {
                return Err(crate::Error::Other(format!(
                    "章节 {} 的编辑前后正文摘要必须同时提供或同时省略",
                    event.chapter_id
                )));
            }

            let legacy_before = !draft.before_text_sha256.is_empty()
                && !draft
                    .before_text_sha256
                    .starts_with(proof::CONTENT_DIGEST_VERSION);
            let legacy_after = !draft.after_text_sha256.is_empty()
                && !draft
                    .after_text_sha256
                    .starts_with(proof::CONTENT_DIGEST_VERSION);
            if (legacy_before || legacy_after) && before_text.is_none() {
                let frame = crate::writing_stats::replay::replay_chapter(
                    self,
                    &crate::writing_stats::replay::ProofReplayRequest {
                        chapter_id: event.chapter_id.clone(),
                        device_id: origin.clone(),
                        target_seq: Some(seq.saturating_sub(1)),
                        target_time_ms: None,
                        target_index: None,
                        baseline_text: baseline_text.map(ToString::to_string),
                    },
                )?;
                if !frame.valid {
                    return Err(crate::Error::Other(format!(
                        "无法校验章节 {} 的旧正文摘要: {}",
                        event.chapter_id,
                        frame.errors.join("; ")
                    )));
                }
                before_text = Some(frame.text);
            }

            if !draft.before_text_sha256.is_empty()
                && !text_digest_matches(
                    &draft.before_text_sha256,
                    before_text.as_deref().unwrap_or_default(),
                    &before_digest,
                )
            {
                return Err(crate::Error::Other(format!(
                    "章节 {} 的内核 before_text_sha256 与证明链当前正文不一致（可能有漏记事件）",
                    event.chapter_id
                )));
            }
            let mut after_digest = before_digest.clone();
            after_digest.apply_changes(&draft.changes)?;
            if !draft.after_text_sha256.is_empty() {
                let matches = if draft
                    .after_text_sha256
                    .starts_with(proof::CONTENT_DIGEST_VERSION)
                {
                    draft.after_text_sha256 == after_digest.digest()
                } else {
                    let mut legacy_after_text = before_text.clone().unwrap_or_default();
                    apply_changes(&mut legacy_after_text, &draft.changes)?;
                    draft.after_text_sha256 == text_sha256(&legacy_after_text)
                };
                if !matches {
                    return Err(crate::Error::Other(format!(
                        "章节 {} 的内核 after_text_sha256 与变更应用结果不一致",
                        event.chapter_id
                    )));
                }
            }

            draft.format_version = proof::PROOF_FORMAT_VERSION;
            draft.seq = seq;
            draft.starts_new_chain = starts_new_chain;
            if draft.baseline_id.is_empty() {
                draft.baseline_id = baseline_id;
            }
            draft.before_content_hash = expected_before_content_hash.clone();
            draft.prev_event_hash = previous_event_hash;
            draft.before_text_sha256 = before_digest_value;
            draft.after_text_sha256 = after_digest.digest();
            draft.after_content_hash =
                chain_content_hash(&draft.before_content_hash, &draft.changes);
            (draft.changes.clone(), after_digest)
        };
        let after_content_hash =
            chain_content_hash(&expected_before_content_hash, &changes_for_chain);
        let after_text_sha256 = after_digest.digest();
        let event_hash = compute_event_hash(&event);
        if let Some(draft) = event.proof.as_mut() {
            draft.event_hash = event_hash.clone();
        }
        let chapter_for_checkpoint = event.chapter_id.clone();
        let timestamp_for_checkpoint = event.timestamp_ms;

        let written = self.record_events(vec![event])?;
        if written == 0 {
            return Ok(false);
        }
        state.next_seq = seq.saturating_add(1);
        state.chains.insert(
            chain_key.clone(),
            ProofChainHead {
                seq,
                event_hash: event_hash.clone(),
                content_hash: after_content_hash.clone(),
                text_sha256: after_text_sha256,
            },
        );
        state.digests.insert(chain_key, after_digest);
        drop(state);
        self.proof_generation.fetch_add(1, Ordering::SeqCst);

        // Issue #1008：事件是长期事实，checkpoint 只是加速索引。长会话按固定事件
        // 间隔写一次完整正文，回放不必从会话起点重放；写失败不能反过来否定已落盘的
        // 编辑事件，因此只记录错误并继续。
        if seq > 0 && seq % PERIODIC_CHECKPOINT_INTERVAL == 0 {
            if let Err(error) = self.write_periodic_checkpoint(
                &chapter_for_checkpoint,
                &origin,
                seq,
                timestamp_for_checkpoint,
                &after_content_hash,
                &event_hash,
            ) {
                log::warn!("proof periodic checkpoint failed: {error}");
            }
        }
        Ok(true)
    }

    /// 从最近 checkpoint 回放到当前序号，写一个周期正文检查点。
    fn write_periodic_checkpoint(
        &self,
        chapter_id: &str,
        device_id: &str,
        seq: u64,
        timestamp_ms: i64,
        content_hash: &str,
        event_hash: &str,
    ) -> Result<()> {
        let request = crate::writing_stats::replay::ProofReplayRequest {
            chapter_id: chapter_id.to_string(),
            device_id: device_id.to_string(),
            target_seq: Some(seq),
            target_time_ms: None,
            target_index: None,
            baseline_text: None,
        };
        let frame = crate::writing_stats::replay::replay_chapter(self, &request)?;
        if !frame.valid {
            log::warn!(
                "proof periodic checkpoint skipped for {chapter_id}: {}",
                frame.errors.join("; ")
            );
            return Ok(());
        }
        let checkpoint = ProofCheckpoint::new(
            chapter_id,
            device_id,
            seq,
            "",
            timestamp_ms,
            &frame.text,
            content_hash,
            event_hash,
            "periodic",
        );
        checkpoint::append_checkpoint(&self.app_data_root, &checkpoint)
    }

    /// 保存正文后写一个完整正文 checkpoint，作为后续回放加速索引。
    ///
    /// `chapters` 由保存事务提供完整正文，因此检查点不需要重新回放事件；
    /// 无证明事件时写 seq=0 基线，之后第一条证明事件会复用它。
    pub(crate) fn record_chapter_checkpoints(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        text: &str,
        session_id: &str,
    ) -> Result<()> {
        let origin = self.resolve_origin_device_id()?;
        let saved_text_sha256 = text_sha256(text);

        let find_last = |events: &[WritingInputEvent]| -> Option<WritingInputEvent> {
            events
                .iter()
                .filter(|event| {
                    event.device_id == origin
                        && event.project_id == project_id
                        && event.volume_id == volume_id
                        && event.chapter_id == chapter_id
                        && event.proof.is_some()
                })
                .max_by_key(|event| event.proof.as_ref().map_or(0, |proof| proof.seq))
                .cloned()
        };

        let all_events = self.load_all_events()?;
        let mut last = find_last(&all_events);
        let mut repaired_external = false;
        if let Some(event) = last.as_ref() {
            let proof = event
                .proof
                .as_ref()
                .ok_or_else(|| crate::Error::Other("proof 事件缺少 proof 细节".to_string()))?;
            let frame = crate::writing_stats::replay::replay_chapter(
                self,
                &crate::writing_stats::replay::ProofReplayRequest {
                    chapter_id: chapter_id.to_string(),
                    device_id: origin.clone(),
                    target_seq: Some(proof.seq),
                    target_time_ms: None,
                    target_index: None,
                    baseline_text: None,
                },
            )?;
            if !frame.valid {
                return Err(crate::Error::Other(format!(
                    "保存前无法验证章节 {chapter_id} 的证明链: {}",
                    frame.errors.join("; ")
                )));
            }
            if text_sha256(&frame.text) != saved_text_sha256 {
                // 磁盘正文与事件重建结果不一致（同步、外部编辑、队列滞后）。
                // 不能把保存正文硬盖到旧 seq 上；记录一条明确的程序性变更事件，
                // 由 Core 从旧链正文应用变更并生成新的可验证链头。
                let external = self.build_external_change_event(
                    &origin,
                    project_id,
                    volume_id,
                    chapter_id,
                    &frame.text,
                    text,
                    session_id,
                    Some(event),
                );
                if !self.record_proof_event(external, Some(&frame.text))? {
                    return Err(crate::Error::Other(format!(
                        "章节 {chapter_id} 的外部变更事件未写入"
                    )));
                }
                let refreshed = self.load_all_events()?;
                last = find_last(&refreshed);
                repaired_external = true;
            }
        }

        let now = chrono::Utc::now().timestamp_millis();
        if let Some(event) = last.as_ref() {
            let proof = event
                .proof
                .as_ref()
                .ok_or_else(|| crate::Error::Other("proof 事件缺少 proof 细节".to_string()))?;
            let frame = crate::writing_stats::replay::replay_chapter(
                self,
                &crate::writing_stats::replay::ProofReplayRequest {
                    chapter_id: chapter_id.to_string(),
                    device_id: origin.clone(),
                    target_seq: Some(proof.seq),
                    target_time_ms: None,
                    target_index: None,
                    baseline_text: None,
                },
            )?;
            if !frame.valid || text_sha256(&frame.text) != saved_text_sha256 {
                return Err(crate::Error::Other(format!(
                    "章节 {chapter_id} 保存后的证明链仍无法对应正文"
                )));
            }
            let checkpoint = ProofCheckpoint::new(
                chapter_id,
                &origin,
                proof.seq,
                session_id,
                event.timestamp_ms,
                text,
                &proof.after_content_hash,
                &proof.event_hash,
                if repaired_external {
                    "external"
                } else {
                    "save"
                },
            );
            return checkpoint::append_checkpoint(&self.app_data_root, &checkpoint);
        }

        match checkpoint::load_last_checkpoint(&self.app_data_root, &origin, chapter_id)? {
            Some(checkpoint) => {
                if checkpoint.seq != 0 {
                    return Err(crate::Error::Other(format!(
                        "章节 {chapter_id} 有检查点 seq {} 却没有事件链",
                        checkpoint.seq
                    )));
                }
                if checkpoint.text_sha256 == saved_text_sha256 {
                    return Ok(());
                }
                let external = self.build_external_change_event(
                    &origin,
                    project_id,
                    volume_id,
                    chapter_id,
                    &checkpoint.text,
                    text,
                    session_id,
                    None,
                );
                if !self.record_proof_event(external, Some(&checkpoint.text))? {
                    return Err(crate::Error::Other(format!(
                        "章节 {chapter_id} 的基线外部变更事件未写入"
                    )));
                }
                let refreshed = self.load_all_events()?;
                let last = find_last(&refreshed).ok_or_else(|| {
                    crate::Error::Other(format!("章节 {chapter_id} 的外部变更事件写入后缺少证明链"))
                })?;
                let proof = last
                    .proof
                    .as_ref()
                    .ok_or_else(|| crate::Error::Other("proof 事件缺少 proof 细节".to_string()))?;
                let frame = crate::writing_stats::replay::replay_chapter(
                    self,
                    &crate::writing_stats::replay::ProofReplayRequest {
                        chapter_id: chapter_id.to_string(),
                        device_id: origin.clone(),
                        target_seq: Some(proof.seq),
                        target_time_ms: None,
                        target_index: None,
                        baseline_text: None,
                    },
                )?;
                if !frame.valid || text_sha256(&frame.text) != saved_text_sha256 {
                    return Err(crate::Error::Other(format!(
                        "章节 {chapter_id} 的外部变更无法通过回放校验"
                    )));
                }
                let checkpoint = ProofCheckpoint::new(
                    chapter_id,
                    &origin,
                    proof.seq,
                    session_id,
                    last.timestamp_ms,
                    text,
                    &proof.after_content_hash,
                    &proof.event_hash,
                    "external",
                );
                checkpoint::append_checkpoint(&self.app_data_root, &checkpoint)
            }
            None => {
                let checkpoint = ProofCheckpoint::new(
                    chapter_id,
                    &origin,
                    0,
                    session_id,
                    now,
                    text,
                    &content_hash_of_text(text),
                    "",
                    "baseline",
                );
                checkpoint::append_checkpoint(&self.app_data_root, &checkpoint)
            }
        }
    }

    fn build_external_change_event(
        &self,
        origin: &str,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
        old_text: &str,
        new_text: &str,
        session_id: &str,
        template: Option<&WritingInputEvent>,
    ) -> WritingInputEvent {
        let now = chrono::Utc::now().timestamp_millis();
        let platform = template
            .map(|event| event.platform.clone())
            .unwrap_or(writer_platform_api::PlatformKind::Desktop);
        let device_class = template.map_or("desktop", |event| event.device_class.as_str());
        let mut event = WritingInputEvent::with_identity(
            uuid::Uuid::new_v4().to_string(),
            now,
            origin,
            platform,
            device_class,
            project_id,
            volume_id,
            chapter_id,
            crate::writing_stats::EventSource::Unknown,
            0,
            0,
            0,
            0,
            0,
            session_id,
        );
        let mut changes = Vec::new();
        if !old_text.is_empty() {
            changes.push(EditorChange::Delete {
                index: Utf8ByteOffset::unchecked(0),
                text: old_text.to_string(),
            });
        }
        if !new_text.is_empty() {
            changes.push(EditorChange::Insert {
                index: Utf8ByteOffset::unchecked(0),
                text: new_text.to_string(),
            });
        }
        event.proof = Some(proof::ProofEditPayload {
            format_version: 0,
            seq: 0,
            session_id: session_id.to_string(),
            baseline_id: String::new(),
            starts_new_chain: false,
            base_revision: template.map_or(0, |event| {
                event.proof.as_ref().map_or(0, |proof| proof.new_revision)
            }),
            new_revision: template.map_or(0, |event| {
                event.proof.as_ref().map_or(0, |proof| proof.new_revision)
            }) + 1,
            cause: crate::editor::EditorTransactionCause::Programmatic,
            changes,
            old_selection: EditorSelection::collapsed(old_text, old_text.len()),
            new_selection: EditorSelection::collapsed(new_text, new_text.len()),
            before_content_hash: String::new(),
            after_content_hash: String::new(),
            before_text_sha256: proof::content_digest_of_text(old_text),
            after_text_sha256: proof::content_digest_of_text(new_text),
            prev_event_hash: String::new(),
            event_hash: String::new(),
            sync_source_device_id: String::new(),
            parent_event_id: template.map_or_else(String::new, |event| event.event_id.clone()),
        });
        event
    }

    /// 本机某章节是否已有逐字证明事件。
    pub(crate) fn has_chapter_proof(
        &self,
        origin_device_id: &str,
        chapter_id: &str,
    ) -> Result<bool> {
        Ok(self.load_all_events()?.iter().any(|event| {
            event.device_id == origin_device_id
                && event.chapter_id == chapter_id
                && event.proof.is_some()
        }))
    }

    /// 首次从 Git/同步导入章节、且本机没有逐字历史时，写一个有来源标记的
    /// `sync_import` 基线。基线声明「此前没有本机逐字记录」，不是人工输入事件。
    pub(crate) fn record_sync_import_baseline(
        &self,
        chapter_id: &str,
        text: &str,
        sync_event_id: &str,
        source_device_id: &str,
    ) -> Result<()> {
        if chapter_id.is_empty() {
            return Err(crate::Error::Other("同步证明基线缺少章节 ID".to_string()));
        }
        let origin = self.resolve_origin_device_id()?;
        if self.has_chapter_proof(&origin, chapter_id)? {
            return Err(crate::Error::Other(format!(
                "章节 {chapter_id} 已有本机证明事件，不能再写同步导入基线"
            )));
        }
        if let Some(last) =
            checkpoint::load_last_checkpoint(&self.app_data_root, &origin, chapter_id)?
        {
            if last.seq != 0 {
                return Err(crate::Error::Other(format!(
                    "章节 {chapter_id} 的证明链已从 seq {} 开始，不能写同步导入基线",
                    last.seq
                )));
            }
            if last.text != text {
                return Err(crate::Error::Other(format!(
                    "章节 {chapter_id} 的同步前置正文与已有基线不一致"
                )));
            }
            return Ok(());
        }
        let mut checkpoint = ProofCheckpoint::new(
            chapter_id,
            &origin,
            0,
            sync_event_id,
            chrono::Utc::now().timestamp_millis(),
            text,
            &content_hash_of_text(text),
            "",
            "sync_import",
        );
        checkpoint.source_device_id = source_device_id.to_string();
        checkpoint::append_checkpoint(&self.app_data_root, &checkpoint)?;
        self.proof_generation.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    /// 本机当前证明链的最近序号（没有事件时为 0）。
    pub(crate) fn latest_proof_seq(&self) -> Result<u64> {
        let mut state = self.lock_proof_state();
        if !state.initialized {
            self.initialize_proof_state(&mut state)?;
        }
        Ok(state.next_seq.saturating_sub(1))
    }

    fn initialize_proof_state(&self, state: &mut ProofStoreState) -> Result<()> {
        for event in self.load_all_events()? {
            let Some(proof) = event.proof.as_ref() else {
                continue;
            };
            state.next_seq = state.next_seq.max(proof.seq.saturating_add(1));
            let key = format!("{}/{}", event.device_id, event.chapter_id);
            let replace = state
                .chains
                .get(&key)
                .is_none_or(|head| proof.seq > head.seq);
            if replace {
                state.chains.insert(
                    key,
                    ProofChainHead {
                        seq: proof.seq,
                        event_hash: proof.event_hash.clone(),
                        content_hash: proof.after_content_hash.clone(),
                        text_sha256: proof.after_text_sha256.clone(),
                    },
                );
            }
        }
        state.initialized = true;
        Ok(())
    }

    /// 证明事件变化代数，供回放缓存失效。
    pub(crate) fn proof_generation(&self) -> u64 {
        self.proof_generation.load(Ordering::SeqCst)
    }

    fn lock_proof_state(&self) -> std::sync::MutexGuard<'_, ProofStoreState> {
        self.proof_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 扫描某设备某个 UTC 日期的原始 JSONL 与已封口归档，按 event_id 去重。
    fn scan_persisted_ids(&self, origin: &str, date: &str) -> Result<HashSet<String>> {
        let mut ids = self.scan_ids(&self.event_file_path(origin, date))?;
        let archive_path = self
            .app_data_root
            .join(checkpoint::archive_relative_path(origin, date));
        if archive_path.is_file() {
            let file = EventFile {
                origin: origin.to_string(),
                date: date.to_string(),
                path: archive_path,
                compressed: true,
            };
            for event in self.parse_events_file(&file)? {
                if !event.event_id.is_empty() {
                    ids.insert(event.event_id);
                }
            }
        }
        Ok(ids)
    }

    /// 把本机分区中早于今天（UTC）且尚未归档的事件日封口。
    fn archive_closed_days(&self, origin: &str) -> Result<()> {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let files = list_origin_files(&self.events_root(), origin, ".events.jsonl")?;
        for file in files {
            if file.date.as_str() >= today.as_str() {
                continue;
            }
            let archive = self
                .app_data_root
                .join(checkpoint::archive_relative_path(origin, &file.date));
            if archive.is_file() {
                // 归档不可改写；如果封口后又有迟到事件形成新的原始段，保留两者，
                // 读取与去重仍以 event_id 为准。
                continue;
            }
            checkpoint::archive_event_day(&self.app_data_root, origin, &file.date)?;
        }
        Ok(())
    }

    fn event_is_persisted(&self, origin: &str, date: &str, event_id: &str) -> Result<bool> {
        if event_id.is_empty() {
            return Ok(false);
        }
        let mut cache = self.lock_written_ids();
        let cache_key = format!("{origin}/{date}");
        if !cache.contains_key(&cache_key) {
            let ids = self.scan_persisted_ids(origin, date)?;
            cache.insert(cache_key.clone(), ids);
        }
        Ok(cache
            .get(&cache_key)
            .is_some_and(|ids| ids.contains(event_id)))
    }

    /// 判断一条同步证明事件是否已经按原始事件时间落盘。
    ///
    /// 崩溃恢复必须使用意图日志里固定的事件时间来判断日期分区，不能拿
    /// “现在”重新计算，否则跨 UTC 日恢复会把同一事件写到另一个日期。
    pub(crate) fn proof_event_persisted(
        &self,
        event_id: &str,
        occurred_at_ms: i64,
    ) -> Result<bool> {
        let origin = self.resolve_origin_device_id()?;
        let date = self.timestamp_to_date(occurred_at_ms)?;
        self.event_is_persisted(&origin, &date, event_id)
    }

    /// 按稳定事件 ID 在本机事件分区中查找已落盘事件。
    ///
    /// 事务恢复只持有 `event_id`，可能已经丢失原意图文件（及其时间）。
    /// 该查询遍历本机原始段与归档段，返回真实事件及其原始 `timestamp_ms`，
    /// 不按“当前时间”重建事件。
    pub(crate) fn find_event_by_id(&self, event_id: &str) -> Result<Option<WritingInputEvent>> {
        if event_id.is_empty() {
            return Ok(None);
        }
        let origin = self.resolve_origin_device_id()?;
        let root = self.events_root();
        let mut files = Vec::new();
        if root.join(&origin).is_dir() {
            files.extend(list_origin_files(&root, &origin, ".events.jsonl")?);
        }
        files.extend(list_archive_files(&root, &origin)?);
        for file in files {
            for event in self.parse_events_file(&file)? {
                if event.event_id == event_id {
                    return Ok(Some(event));
                }
            }
        }
        Ok(None)
    }

    /// 追加一批已测量的前台写作计时切片，只写入本机来源分区。
    ///
    /// 切片先按写作设备本地日期切开，再按切片起点的 UTC 日期落到
    /// `.sessions.jsonl`。派生 ID 由切片原始 `event_id` 与本地日期确定，
    /// 同一批数据重发时不会重复累计时长。
    pub fn record_activity_segments(&self, segments: Vec<WritingActivitySegment>) -> Result<usize> {
        if segments.is_empty() {
            return Ok(0);
        }
        let origin = self.resolve_origin_device_id()?;
        let mut by_date: BTreeMap<String, Vec<WritingActivitySegment>> = BTreeMap::new();
        for segment in segments {
            for slice in segment.split_by_local_date()? {
                let date = self.timestamp_to_date(slice.start_at_ms)?;
                by_date.entry(date).or_default().push(slice);
            }
        }
        if by_date.is_empty() {
            return Ok(0);
        }

        let partition_dir = self.events_root().join(&origin);
        fs::create_dir_all(&partition_dir)?;

        let mut written = 0usize;
        for (date, segments) in by_date {
            match self.append_confirmed_segments(&origin, &date, segments) {
                Ok(count) => written += count,
                Err(error) => {
                    self.invalidate_written_ids(&format!("sessions:{origin}/{date}"));
                    return Err(error);
                }
            }
        }
        Ok(written)
    }

    /// 追加一批新计时切片，写入确认后才登记 `event_id`。
    fn append_confirmed_segments(
        &self,
        origin: &str,
        date: &str,
        segments: Vec<WritingActivitySegment>,
    ) -> Result<usize> {
        let fresh = self.select_fresh_segments(origin, date, segments)?;
        if fresh.is_empty() {
            return Ok(0);
        }
        let mut buffer = String::new();
        for segment in &fresh {
            buffer.push_str(&serde_json::to_string(segment)?);
            buffer.push('\n');
        }
        {
            let file_path = self.session_file_path(origin, date);
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;
            file.write_all(buffer.as_bytes())?;
        }

        let mut cache = self.lock_written_ids();
        let ids = cache
            .entry(format!("sessions:{origin}/{date}"))
            .or_default();
        for segment in &fresh {
            if !segment.event_id.is_empty() {
                ids.insert(segment.event_id.clone());
            }
        }
        Ok(fresh.len())
    }

    fn select_fresh_segments(
        &self,
        origin: &str,
        date: &str,
        segments: Vec<WritingActivitySegment>,
    ) -> Result<Vec<WritingActivitySegment>> {
        let mut cache = self.lock_written_ids();
        let cache_key = format!("sessions:{origin}/{date}");
        if !cache.contains_key(&cache_key) {
            let ids = self.scan_ids(&self.session_file_path(origin, date))?;
            cache.insert(cache_key.clone(), ids);
        }
        let Some(ids) = cache.get(&cache_key) else {
            return Ok(Vec::new());
        };

        let mut batch_ids: HashSet<String> = HashSet::new();
        let mut fresh = Vec::new();
        for segment in segments {
            if segment.event_id.is_empty() {
                fresh.push(segment);
                continue;
            }
            if ids.contains(&segment.event_id) || batch_ids.contains(&segment.event_id) {
                continue;
            }
            batch_ids.insert(segment.event_id.clone());
            fresh.push(segment);
        }
        Ok(fresh)
    }

    /// 读取某个 UTC 日期在全部来源分区中的事件（按 `event_id` 去重）。
    pub fn load_events_for_date(&self, date: &str) -> Result<Vec<WritingInputEvent>> {
        let files: Vec<EventFile> = self
            .list_event_files()?
            .into_iter()
            .filter(|file| file.date == date)
            .collect();
        self.load_merged_files(files)
    }

    /// 读取全部来源分区的事件（按 `event_id` 去重）。
    pub fn load_all_events(&self) -> Result<Vec<WritingInputEvent>> {
        let files = self.list_event_files()?;
        self.load_merged_files(files)
    }

    /// 读取全部来源分区的已测量写作计时切片（按 `event_id` 去重）。
    pub fn load_all_segments(&self) -> Result<Vec<WritingActivitySegment>> {
        let files = self.list_session_files()?;
        self.load_merged_segments(files)
    }

    /// 读取业务日期 `[start_date, end_date]`（含两端）内的计时切片。
    ///
    /// 切片在写入时已按本地午夜切开，所以按切片的 `local_date` 过滤即可；
    /// 仍按 UTC 文件日期前后各扩一天，覆盖时区差异。
    pub fn load_segments_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<WritingActivitySegment>> {
        let start = calendar::parse_date(start_date)
            .map_err(|e| crate::Error::Other(format!("Invalid start date: {e}")))?;
        let end = calendar::parse_date(end_date)
            .map_err(|e| crate::Error::Other(format!("Invalid end date: {e}")))?;
        if start > end {
            return Ok(Vec::new());
        }
        let utc_start = start.pred_opt().unwrap_or(start);
        let utc_end = end.succ_opt().unwrap_or(end);

        let files: Vec<EventFile> = self
            .list_session_files()?
            .into_iter()
            .filter(|file| {
                NaiveDate::parse_from_str(&file.date, "%Y-%m-%d")
                    .map(|date| date >= utc_start && date <= utc_end)
                    .unwrap_or(false)
            })
            .collect();
        let mut segments = self.load_merged_segments(files)?;
        segments.retain(|segment| {
            let date = segment.business_date();
            !date.is_empty() && date.as_str() >= start_date && date.as_str() <= end_date
        });
        Ok(segments)
    }

    /// 读取指定作品的全部事件。
    pub fn load_events_for_project(&self, project_id: &str) -> Result<Vec<WritingInputEvent>> {
        let events = self.load_all_events()?;
        Ok(events
            .into_iter()
            .filter(|event| event.project_id == project_id)
            .collect())
    }

    /// 读取业务日期 `[start_date, end_date]`（含两端）的事件。
    pub fn load_events_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<WritingInputEvent>> {
        let start = calendar::parse_date(start_date)
            .map_err(|e| crate::Error::Other(format!("Invalid start date: {e}")))?;
        let end = calendar::parse_date(end_date)
            .map_err(|e| crate::Error::Other(format!("Invalid end date: {e}")))?;
        if start > end {
            return Ok(Vec::new());
        }
        // 事件的存储分区按 UTC 日期：本地日期会在 UTC 日期前后一天内浮动。
        let utc_start = start.pred_opt().unwrap_or(start);
        let utc_end = end.succ_opt().unwrap_or(end);

        let files: Vec<EventFile> = self
            .list_event_files()?
            .into_iter()
            .filter(|file| {
                NaiveDate::parse_from_str(&file.date, "%Y-%m-%d")
                    .map(|date| date >= utc_start && date <= utc_end)
                    .unwrap_or(false)
            })
            .collect();
        let mut events = self.load_merged_files(files)?;
        events.retain(|event| {
            let date = event.business_date();
            !date.is_empty() && date.as_str() >= start_date && date.as_str() <= end_date
        });
        Ok(events)
    }

    /// 读取发生时间在 `[start_ms, end_ms]`（含两端）的事件，按时间升序。
    pub fn load_events_in_window(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> Result<Vec<WritingInputEvent>> {
        let start_date = calendar::local_date_at(start_ms)?;
        let end_date = calendar::local_date_at(end_ms)?;
        let mut events = self.load_events_range(&start_date, &end_date)?;
        events.retain(|event| event.timestamp_ms >= start_ms && event.timestamp_ms <= end_ms);
        events.sort_by_key(|event| event.timestamp_ms);
        Ok(events)
    }

    /// 把时间戳转换为存储分区使用的 UTC 日期。
    pub fn timestamp_to_date(&self, timestamp_ms: i64) -> Result<String> {
        calendar::utc_date_at(timestamp_ms)
    }

    /// 事件文件集合的轻量指纹（路径、大小、修改时间），用于识别同步导入、
    /// 归档切换等不经过 `record_events` 的外部变化，避免回放会话无限期陈旧。
    pub(crate) fn storage_fingerprint(&self) -> Result<u64> {
        let mut hash = 0xcbf29ce484222325u64;
        let mut files = self.list_event_files()?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        for file in files {
            let Ok(metadata) = fs::metadata(&file.path) else {
                // 归档 rename/删除与查询并发时跳过瞬时缺失文件；下一次查询会
                // 得到新的稳定指纹。
                continue;
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos() as u64);
            for byte in file.path.to_string_lossy().as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
            for value in [metadata.len(), modified] {
                for byte in value.to_le_bytes() {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x100000001b3);
                }
            }
        }
        Ok(hash)
    }

    /// 扫描事件库里的全部 `<origin>/<date>.events.jsonl` 文件。
    fn list_event_files(&self) -> Result<Vec<EventFile>> {
        let root = self.events_root();
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for origin in list_origin_dirs(&root)? {
            files.extend(list_origin_files(&root, &origin, ".events.jsonl")?);
            files.extend(list_archive_files(&root, &origin)?);
        }
        Ok(files)
    }

    /// 扫描事件库里的全部 `<origin>/<date>.sessions.jsonl` 文件。
    fn list_session_files(&self) -> Result<Vec<EventFile>> {
        let root = self.events_root();
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut files = Vec::new();
        for origin in list_origin_dirs(&root)? {
            files.extend(list_origin_files(&root, &origin, ".sessions.jsonl")?);
        }
        Ok(files)
    }

    /// 严格读取单个事件文件（任何一行解析失败都报错）。
    fn parse_events_file(&self, file: &EventFile) -> Result<Vec<WritingInputEvent>> {
        let handle = File::open(&file.path)?;
        let reader: Box<dyn BufRead> = if file.compressed {
            Box::new(BufReader::new(
                zstd::stream::read::Decoder::new(handle).map_err(|error| {
                    crate::Error::Other(format!(
                        "Open proof archive stats/events/{}/proof/archive/{}.events.jsonl.zst: {}",
                        file.origin, file.date, error
                    ))
                })?,
            ))
        } else {
            Box::new(BufReader::new(handle))
        };
        let mut events = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event: WritingInputEvent = serde_json::from_str(trimmed).map_err(|e| {
                crate::Error::Other(format!(
                    "Corrupt event at stats/events/{}/{}.events.jsonl:{}: {}",
                    file.origin,
                    file.date,
                    idx + 1,
                    e
                ))
            })?;
            events.push(event);
        }
        Ok(events)
    }

    /// 合并多个分区的事件并按 `event_id` 去重。
    ///
    /// 选择顺序与本机身份无关（来源设备 ID、UTC 日期），两台设备拿到相同
    /// 文件集合时对同 ID 冲突会保留同一条事实，聚合结果一致。跨分区重复
    /// 写一次性冲突诊断，不在每次读取时重复追加。
    fn load_merged_files(&self, mut files: Vec<EventFile>) -> Result<Vec<WritingInputEvent>> {
        files.sort_by(|a, b| a.origin.cmp(&b.origin).then_with(|| a.date.cmp(&b.date)));

        let mut merged = MergeAccumulator::default();
        for file in &files {
            for event in self.parse_events_file(file)? {
                merged.ingest(file, event);
            }
        }

        if let Err(e) = self.append_conflicts(&merged.conflicts) {
            log::warn!("[writing_stats] failed to append event id conflicts: {e}");
        }
        Ok(merged.events)
    }

    /// 把跨分区重复 `event_id` 的结构化事实追加到本机诊断日志（best-effort）。
    ///
    /// 先读出日志里已记录过的 `event_id`，同一次冲突只写一次，避免统计查询
    /// 越频繁日志越大。
    fn append_conflicts(&self, conflicts: &[EventIdConflict]) -> Result<()> {
        if conflicts.is_empty() {
            return Ok(());
        }
        let dir = self.app_data_root.join("app-meta/stats");
        fs::create_dir_all(&dir)?;
        let path = dir.join("event_conflicts.local.jsonl");
        let logged = load_logged_conflict_ids(&path)?;

        let mut batch_seen: HashSet<&str> = HashSet::new();
        let mut pending = String::new();
        for conflict in conflicts {
            let event_id = conflict.event_id.as_str();
            if event_id.is_empty() || logged.contains(event_id) || !batch_seen.insert(event_id) {
                continue;
            }
            pending.push_str(&serde_json::to_string(conflict)?);
            pending.push('\n');
        }
        if pending.is_empty() {
            return Ok(());
        }

        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(pending.as_bytes())?;
        Ok(())
    }

    /// 严格读取单个计时切片文件（任何一行解析失败都报错）。
    fn parse_session_file(&self, file: &EventFile) -> Result<Vec<WritingActivitySegment>> {
        let handle = File::open(&file.path)?;
        let reader = BufReader::new(handle);
        let mut segments = Vec::new();
        for (idx, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let segment: WritingActivitySegment = serde_json::from_str(trimmed).map_err(|e| {
                crate::Error::Other(format!(
                    "Corrupt writing activity segment at stats/events/{}/{}.sessions.jsonl:{}: {}",
                    file.origin,
                    file.date,
                    idx + 1,
                    e
                ))
            })?;
            segments.push(segment);
        }
        Ok(segments)
    }

    /// 合并多个分区的计时切片并按 `event_id` 去重。
    fn load_merged_segments(
        &self,
        mut files: Vec<EventFile>,
    ) -> Result<Vec<WritingActivitySegment>> {
        files.sort_by(|a, b| a.origin.cmp(&b.origin).then_with(|| a.date.cmp(&b.date)));
        let mut seen: HashSet<String> = HashSet::new();
        let mut merged = Vec::new();
        for file in &files {
            for segment in self.parse_session_file(file)? {
                if segment.event_id.is_empty() || seen.insert(segment.event_id.clone()) {
                    merged.push(segment);
                }
            }
        }
        Ok(merged)
    }

    /// best-effort 扫描已写入的 event_id，损坏行跳过。
    fn scan_ids(&self, path: &Path) -> Result<HashSet<String>> {
        let mut ids = HashSet::new();
        if !path.is_file() {
            return Ok(ids);
        }
        let handle = File::open(&path)?;
        for line in BufReader::new(handle).lines() {
            let Ok(line) = line else {
                continue;
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                continue;
            };
            let Some(id) = value.get("event_id").and_then(|value| value.as_str()) else {
                continue;
            };
            if !id.is_empty() {
                ids.insert(id.to_string());
            }
        }
        Ok(ids)
    }
}

/// 去重合并的中间状态：保留每个 `event_id` 的首次出现，记录跨分区重复。
#[derive(Default)]
struct MergeAccumulator {
    seen: HashMap<String, String>,
    events: Vec<WritingInputEvent>,
    conflicts: Vec<EventIdConflict>,
}

impl MergeAccumulator {
    fn ingest(&mut self, file: &EventFile, event: WritingInputEvent) {
        if event.event_id.is_empty() {
            self.events.push(event);
            return;
        }
        match self.seen.get(&event.event_id) {
            Some(kept_origin) => {
                if kept_origin != &file.origin {
                    self.conflicts.push(EventIdConflict {
                        event_id: event.event_id.clone(),
                        kept_origin_device_id: kept_origin.clone(),
                        dropped_origin_device_id: file.origin.clone(),
                        dropped_file: format!(
                            "stats/events/{}/{}.events.jsonl",
                            file.origin, file.date
                        ),
                        detected_at_ms: chrono::Utc::now().timestamp_millis(),
                    });
                }
            }
            None => {
                self.seen
                    .insert(event.event_id.clone(), file.origin.clone());
                self.events.push(event);
            }
        }
    }
}

/// 事件根目录下合法的来源设备分区名（按名称排序）。
fn list_origin_dirs(root: &Path) -> Result<Vec<String>> {
    let mut origins: Vec<String> = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if validate_device_segment(&name) {
            origins.push(name);
        }
    }
    origins.sort();
    Ok(origins)
}

/// 单个来源分区内合法的 `<date><suffix>` 文件（按日期升序）。
fn list_origin_files(root: &Path, origin: &str, suffix: &str) -> Result<Vec<EventFile>> {
    let dir = root.join(origin);
    let mut dates: Vec<(String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(date) = name.strip_suffix(suffix) else {
            continue;
        };
        if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            continue;
        }
        dates.push((date.to_string(), entry.path()));
    }
    dates.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(dates
        .into_iter()
        .map(|(date, path)| EventFile {
            origin: origin.to_string(),
            date,
            path,
            compressed: false,
        })
        .collect())
}

/// 列出 `proof/archive/<date>.events.jsonl.zst` 封口段（按日期升序）。
fn list_archive_files(root: &Path, origin: &str) -> Result<Vec<EventFile>> {
    let dir = root.join(origin).join("proof/archive");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut dates: Vec<(String, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(date) = name.strip_suffix(".events.jsonl.zst") else {
            continue;
        };
        if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            continue;
        }
        dates.push((date.to_string(), entry.path()));
    }
    dates.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(dates
        .into_iter()
        .map(|(date, path)| EventFile {
            origin: origin.to_string(),
            date,
            path,
            compressed: true,
        })
        .collect())
}

/// 设备标识必须是单个安全路径 segment。
fn validate_device_segment(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && !id.contains('/') && !id.contains('\\')
}

/// 读出冲突诊断日志里已经记录过的 `event_id`（best-effort，坏行跳过）。
fn load_logged_conflict_ids(path: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    if !path.is_file() {
        return Ok(ids);
    }
    let handle = File::open(path)?;
    for line in BufReader::new(handle).lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let Some(id) = value.get("event_id").and_then(|value| value.as_str()) else {
            continue;
        };
        if !id.is_empty() {
            ids.insert(id.to_string());
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing_stats::{EventSource, Platform};
    use tempfile::TempDir;

    fn create_mock_store() -> (TempDir, StatsStore) {
        let tmp = tempfile::tempdir().unwrap();
        crate::settings::ensure_device_info(tmp.path(), "", "", Some("device_1")).unwrap();
        let store = StatsStore::new(tmp.path());
        (tmp, store)
    }

    fn create_mock_event() -> WritingInputEvent {
        WritingInputEvent::new(
            "device_1",
            Platform::Desktop,
            "desktop",
            "proj_1",
            "vol_1",
            "ch_1",
            EventSource::HumanTyped,
            10,
            0,
            0,
            0,
            0,
            "session_1",
        )
    }

    #[test]
    fn test_record_event_direct_append() {
        let (tmp, store) = create_mock_store();
        let event = create_mock_event();
        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        store.record_event(event).unwrap();

        let path = tmp
            .path()
            .join("stats/events/device_1")
            .join(format!("{date}.events.jsonl"));
        assert!(path.exists());
        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_record_event_multiple() {
        let (_tmp, store) = create_mock_store();
        store.record_event(create_mock_event()).unwrap();
        let other = {
            let mut event = create_mock_event();
            event.event_id = "event-2".to_string();
            event
        };
        store.record_event(other).unwrap();
        assert_eq!(store.load_all_events().unwrap().len(), 2);
    }

    #[test]
    fn test_load_events_for_date_corrupt_line() {
        let (tmp, store) = create_mock_store();
        let event = create_mock_event();
        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        store.record_event(event).unwrap();

        let path = tmp
            .path()
            .join("stats/events/device_1")
            .join(format!("{date}.events.jsonl"));
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "{{\"bad\":}}").unwrap();

        let err = store.load_events_for_date(&date).unwrap_err();
        assert!(err.to_string().contains("Corrupt event at stats/events/"));
    }

    #[test]
    fn test_list_event_origins() {
        let (_tmp, store) = create_mock_store();
        store.record_event(create_mock_event()).unwrap();
        assert_eq!(store.list_event_origins().unwrap(), vec!["device_1"]);
    }

    #[test]
    fn test_record_events_dedup_by_event_id() {
        let (_tmp, store) = create_mock_store();
        let mut event = create_mock_event();
        event.event_id = "evt-1".to_string();

        store.record_event(event.clone()).unwrap();
        store.record_event(event.clone()).unwrap();
        store.record_event(event.clone()).unwrap();
        assert_eq!(store.load_all_events().unwrap().len(), 1);

        // 冷缓存（模拟重启）后再次写入仍不会重复。
        let cold = StatsStore::new(&store.app_data_root);
        cold.record_event(event).unwrap();
        assert_eq!(cold.load_all_events().unwrap().len(), 1);
    }

    #[test]
    fn test_record_events_resend_keeps_original_storage_date() {
        let (tmp, store) = create_mock_store();
        let yesterday_ms = chrono::Utc::now().timestamp_millis() - 86_400_000;
        let mut event = create_mock_event();
        event.event_id = "old-event-id".to_string();
        event.timestamp_ms = yesterday_ms;
        event.local_date = String::new();

        let yesterday = store.timestamp_to_date(yesterday_ms).unwrap();
        let today = store
            .timestamp_to_date(chrono::Utc::now().timestamp_millis())
            .unwrap();

        store.record_event(event.clone()).unwrap();
        store.record_event(event).unwrap();

        let yesterday_path = tmp
            .path()
            .join("stats/events/device_1")
            .join(format!("{yesterday}.events.jsonl"));
        assert!(yesterday_path.exists());
        assert!(!tmp
            .path()
            .join("stats/events/device_1")
            .join(format!("{today}.events.jsonl"))
            .exists());
        assert_eq!(store.load_events_for_date(&yesterday).unwrap().len(), 1);
    }

    #[test]
    fn test_record_events_counts_only_new_writes() {
        let (_tmp, store) = create_mock_store();
        let first = create_mock_event();
        assert_eq!(store.record_events(vec![first.clone()]).unwrap(), 1);

        let mut fresh = create_mock_event();
        fresh.event_id = "event-new".to_string();
        assert_eq!(store.record_events(vec![first, fresh]).unwrap(), 1);
    }

    #[test]
    fn test_record_events_tolerates_corrupt_history_lines() {
        let (tmp, store) = create_mock_store();
        let event = create_mock_event();
        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        let dir = tmp.path().join("stats/events/device_1");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{date}.events.jsonl"));
        fs::write(&path, "{\"bad\":}\n").unwrap();

        let mut fresh = create_mock_event();
        fresh.event_id = "event-after-corrupt".to_string();
        assert_eq!(store.record_events(vec![fresh]).unwrap(), 1);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("event-after-corrupt"));
        assert!(content.contains("{\"bad\":}"));
    }

    #[test]
    fn test_cross_partition_dedup_uses_deterministic_order_and_logs_conflict_once() {
        let (tmp, store) = create_mock_store();
        let dir = tmp.path().join("stats/events");
        let date = "2024-05-01";

        let mut remote_event = create_mock_event();
        remote_event.event_id = "shared-event".to_string();
        remote_event.timestamp_ms = 1_714_550_400_000;
        remote_event.local_date = String::new();
        remote_event.device_id = "device-remote".to_string();
        remote_event.inserted_chars = 30;

        let mut local_event = remote_event.clone();
        local_event.device_id = "device_1".to_string();
        local_event.inserted_chars = 20;

        let remote_dir = dir.join("device-remote");
        fs::create_dir_all(&remote_dir).unwrap();
        fs::write(
            remote_dir.join(format!("{date}.events.jsonl")),
            format!("{}\n", serde_json::to_string(&remote_event).unwrap()),
        )
        .unwrap();
        let local_dir = dir.join("device_1");
        fs::create_dir_all(&local_dir).unwrap();
        fs::write(
            local_dir.join(format!("{date}.events.jsonl")),
            format!("{}\n", serde_json::to_string(&local_event).unwrap()),
        )
        .unwrap();

        let events = store.load_events_for_date(date).unwrap();
        assert_eq!(events.len(), 1);
        // 与本机身份无关的确定性顺序：origin 字典序在前的分区胜出。
        // "device-remote" < "device_1"，两台设备读到同一份同步输入必得同一结果。
        assert_eq!(events[0].inserted_chars, 30);
        assert_eq!(events[0].device_id, "device-remote");

        // 同一冲突只在首次发现时写一次诊断日志，重复读取不追加。
        let again = store.load_events_for_date(date).unwrap();
        assert_eq!(again.len(), 1);
        let log = fs::read_to_string(
            tmp.path()
                .join("app-meta/stats/event_conflicts.local.jsonl"),
        )
        .unwrap();
        assert!(log.contains("shared-event"));
        assert!(log.contains("device-remote"));
        assert_eq!(log.lines().count(), 1);
    }

    #[test]
    fn test_write_failure_does_not_mark_event_written_so_retry_persists() {
        let (tmp, store) = create_mock_store();
        let mut event = create_mock_event();
        event.event_id = "retry-event".to_string();
        let date = store.timestamp_to_date(event.timestamp_ms).unwrap();
        let partition = tmp.path().join("stats/events/device_1");
        fs::create_dir_all(&partition).unwrap();
        // 在目标文件路径上放一个目录，强制追加打开失败。
        let blocked_path = partition.join(format!("{date}.events.jsonl"));
        fs::create_dir_all(&blocked_path).unwrap();

        assert!(store.record_event(event.clone()).is_err());

        // 障碍移除后原样重发：必须真正落盘，且不重复计数。
        fs::remove_dir(&blocked_path).unwrap();
        store.record_event(event.clone()).unwrap();
        store.record_event(event).unwrap();
        let events = store.load_events_for_date(&date).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_id, "retry-event");
    }

    #[test]
    fn test_load_origin_device_id_without_device_info_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let store = StatsStore::new(tmp.path());
        assert!(store.load_origin_device_id().is_none());
        assert!(store.list_event_origins().unwrap().is_empty());
        assert!(store.load_all_events().unwrap().is_empty());
        assert!(!tmp.path().join("device").exists());
    }

    #[test]
    fn test_resolve_origin_device_id_creates_device_info() {
        let tmp = tempfile::tempdir().unwrap();
        let store = StatsStore::new(tmp.path());
        let origin = store.resolve_origin_device_id().unwrap();
        assert!(!origin.is_empty());
        assert!(tmp.path().join("device/current_device.json").exists());
    }
}
