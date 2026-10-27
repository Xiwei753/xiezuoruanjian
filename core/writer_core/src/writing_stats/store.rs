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

use crate::error::Result;
use crate::writing_stats::checkpoint::{self, ProofCheckpoint};use crate::writing_stats::proof::{
    self, chain_content_hash, compute_event_hash, content_hash_of_text, empty_content_hash,
};
use crate::writing_stats::calendar;
use crate::writing_stats::{WritingActivitySegment, WritingInputEvent};
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
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
}

#[derive(Debug, Clone)]
struct ProofChainHead {
    seq: u64,
    event_hash: String,
    content_hash: String,
}

#[derive(Debug, Default)]
struct ProofStoreState {
    initialized: bool,
    next_seq: u64,
    chains: HashMap<String, ProofChainHead>,
}

impl StatsStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            origin_device_id: Mutex::new(None),
            written_ids: Mutex::new(HashMap::new()),
            proof_state: Mutex::new(ProofStoreState::default()),
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
            let ids = self.scan_ids(&self.event_file_path(origin, date))?;
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
            return Err(crate::Error::Other(
                "proof 事件缺少来源设备 ID".to_string(),
            ));
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
        let seq = state.next_seq;
        let starts_new_chain = head.is_none();
        let baseline_checkpoint = if starts_new_chain {
            baseline_text.map(|text| {
                ProofCheckpoint::new(
                    &event.chapter_id,
                    &origin,
                    0,
                    "",
                    event.timestamp_ms,
                    text,
                    &content_hash_of_text(text),
                    "",
                    "baseline",
                )
            })
        } else {
            None
        };

        if let Some(checkpoint) = baseline_checkpoint.as_ref() {
            checkpoint::append_checkpoint(&self.app_data_root, checkpoint)?;
        }

        let before_content_hash = head.as_ref().map_or_else(
            || {
                baseline_text.map_or_else(empty_content_hash, content_hash_of_text)
            },
            |head| head.content_hash.clone(),
        );
        let previous_event_hash = head.map_or_else(String::new, |head| head.event_hash);
        let baseline_id = if starts_new_chain {
            format!("{origin}:{}:0", event.chapter_id)
        } else {
            String::new()
        };
        let changes_for_chain = {
            let draft = event
                .proof
                .as_mut()
                .ok_or_else(|| crate::Error::Other("proof 事件缺少 proof 细节".to_string()))?;
            draft.format_version = proof::PROOF_FORMAT_VERSION;
            draft.seq = seq;
            draft.starts_new_chain = starts_new_chain;
            if draft.baseline_id.is_empty() {
                draft.baseline_id = baseline_id;
            }
            draft.before_content_hash = before_content_hash.clone();
            draft.prev_event_hash = previous_event_hash;
            draft.after_content_hash = chain_content_hash(&before_content_hash, &draft.changes);
            draft.changes.clone()
        };
        let after_content_hash = chain_content_hash(&before_content_hash, &changes_for_chain);
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
            chain_key,
            ProofChainHead {
                seq,
                event_hash: event_hash.clone(),
                content_hash: chain_content_hash(&before_content_hash, &changes_for_chain),
            },
        );
        drop(state);

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
        let all_events = self.load_all_events()?;
        let mut last: Option<&WritingInputEvent> = None;
        for event in &all_events {
            if event.device_id != origin
                || event.project_id != project_id
                || event.volume_id != volume_id
                || event.chapter_id != chapter_id
            {
                continue;
            }
            if let Some(proof) = event.proof.as_ref() {
                if last
                    .and_then(|event| event.proof.as_ref())
                    .is_none_or(|previous| proof.seq > previous.seq)
                {
                    last = Some(event);
                }
            }
        }
        let now = chrono::Utc::now().timestamp_millis();
        let (seq, timestamp_ms, content_hash, event_hash, reason) = match last {
            Some(event) => {
                let proof = event.proof.as_ref().ok_or_else(|| {
                    crate::Error::Other("proof 事件缺少 proof 细节".to_string())
                })?;
                (
                    proof.seq,
                    event.timestamp_ms,
                    proof.after_content_hash.clone(),
                    proof.event_hash.clone(),
                    "save",
                )
            }
            None => (0, now, content_hash_of_text(text), String::new(), "baseline"),
        };
        let checkpoint = ProofCheckpoint::new(
            chapter_id,
            &origin,
            seq,
            session_id,
            timestamp_ms,
            text,
            &content_hash,
            &event_hash,
            reason,
        );
        checkpoint::append_checkpoint(&self.app_data_root, &checkpoint)
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
                    },
                );
            }
        }
        state.initialized = true;
        Ok(())
    }

    fn lock_proof_state(&self) -> std::sync::MutexGuard<'_, ProofStoreState> {
        self.proof_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn event_is_persisted(&self, origin: &str, date: &str, event_id: &str) -> Result<bool> {
        if event_id.is_empty() {
            return Ok(false);
        }
        let mut cache = self.lock_written_ids();
        let cache_key = format!("{origin}/{date}");
        if !cache.contains_key(&cache_key) {
            let ids = self.scan_ids(&self.event_file_path(origin, date))?;
            cache.insert(cache_key.clone(), ids);
        }
        Ok(cache
            .get(&cache_key)
            .is_some_and(|ids| ids.contains(event_id)))
    }

    /// 追加一批已测量的前台写作计时切片，只写入本机来源分区。
    ///
    /// 切片先按写作设备本地日期切开，再按切片起点的 UTC 日期落到
    /// `.sessions.jsonl`。派生 ID 由切片原始 `event_id` 与本地日期确定，
    /// 同一批数据重发时不会重复累计时长。
    pub fn record_activity_segments(
        &self,
        segments: Vec<WritingActivitySegment>,
    ) -> Result<usize> {
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
            let segment: WritingActivitySegment =
                serde_json::from_str(trimmed).map_err(|e| {
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
