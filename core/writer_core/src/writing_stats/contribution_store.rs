//! 对象贡献事件的按设备分区存储。
//!
//! 文件布局与章节输入事件一致：`stats/events/<device_id>/YYYY-MM-DD.objects.jsonl`。
//! 每台设备只追加自己的分区；读取时扫描全部已到达分区并按 `event_id` 去重。
//! 同步只搬运文件，不重写 `actor`/`entrypoint`。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::error::{Error, Result};
use crate::storage::atomic_write_string;
use crate::writing_stats::contribution::{
    ContributionEvent, ContributionObjectType, ContributionOperation, MutationActor,
};
use crate::writing_stats::starmap_checkpoint::{
    append_starmap_checkpoint, has_starmap_checkpoints, StarMapProofCheckpoint,
    CHECKPOINT_REASON_BASELINE_INCOMPLETE, CHECKPOINT_REASON_CREATED,
};
use crate::writing_stats::starmap_proof::{
    compute_replay_event_hash, proof_object_sha256, ReplayEventIdentity,
};

const EVENTS_ROOT: &str = "stats/events";
const PENDING_ROOT: &str = "stats/pending/contributions";
const OBJECTS_SUFFIX: &str = ".objects.jsonl";

pub struct ContributionStore {
    app_data_root: PathBuf,
    origin_device_id: Mutex<Option<String>>,
    event_id_cache: Mutex<HashMap<String, OriginEventIdCache>>,
}

struct OriginEventIdCache {
    fingerprint: u64,
    ids: HashSet<String>,
}

/// 跨 `ContributionStore` 实例共享的写锁：同一个 app data 根下只能有一个
/// 写入者完成 recover→去重→分配回放元数据→追加→索引更新。
static CONTRIBUTION_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn contribution_write_lock() -> &'static Mutex<()> {
    CONTRIBUTION_WRITE_LOCK.get_or_init(|| Mutex::new(()))
}

struct ReplayMetadata {
    sequences: HashMap<String, u64>,
    chains: HashMap<(String, String), String>,
    checkpoint_targets: Vec<(String, String, u64)>,
}

impl ContributionStore {
    pub fn new(app_data_root: &Path) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            origin_device_id: Mutex::new(None),
            event_id_cache: Mutex::new(HashMap::new()),
        }
    }

    fn events_root(&self) -> PathBuf {
        self.app_data_root.join(EVENTS_ROOT)
    }

    pub fn app_data_root(&self) -> &Path {
        &self.app_data_root
    }

    fn event_file_path(&self, origin: &str, date: &str) -> PathBuf {
        self.events_root()
            .join(origin)
            .join(format!("{date}{OBJECTS_SUFFIX}"))
    }

    fn replay_sequence_path(&self, origin: &str) -> PathBuf {
        self.events_root().join(origin).join("starmap.sequence")
    }

    fn replay_chain_path(&self, origin: &str, starmap_id: &str) -> PathBuf {
        self.events_root()
            .join(origin)
            .join("starmap")
            .join(format!("{starmap_id}.chain"))
    }

    fn replay_branch_count_path(&self, origin: &str, starmap_id: &str) -> PathBuf {
        self.events_root()
            .join(origin)
            .join("starmap")
            .join(format!("{starmap_id}.count"))
    }

    fn persist_branch_count(&self, origin: &str, starmap_id: &str, count: u64) {
        let _ = atomic_write_string(
            &self.replay_branch_count_path(origin, starmap_id),
            &count.to_string(),
        );
    }

    fn generation_path(&self) -> PathBuf {
        self.events_root().join(".contribution-generation")
    }

    /// 事件库代数：每次追加事件后更新，供回放缓存判断失效。
    pub fn replay_generation(&self) -> u64 {
        fs::read_to_string(self.generation_path())
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0)
    }

    fn bump_replay_generation(&self) {
        let value = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
        let _ = atomic_write_string(&self.generation_path(), &value.to_string());
    }

    /// 事件库真实存储指纹：同步直接搬入文件时也能让回放缓存失效。
    ///
    /// 只读目录元数据，不逐文件扫描；事件追加会同时更新 generation 文件，
    /// 同步替换分区会改变 origin/starmap 目录 mtime。
    pub fn storage_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut hash_entry = |path: &Path| {
            path.to_string_lossy().hash(&mut hasher);
            if let Ok(meta) = fs::metadata(path) {
                meta.len().hash(&mut hasher);
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0);
                modified.hash(&mut hasher);
            }
        };
        let root = self.events_root();
        hash_entry(&root);
        hash_entry(&self.generation_path());
        if let Ok(origins) = fs::read_dir(&root) {
            for origin in origins.flatten() {
                if !origin.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let origin_dir = origin.path();
                hash_entry(&origin_dir);
                hash_entry(&origin_dir.join("starmap"));
            }
        }
        hasher.finish()
    }

    fn origin_fingerprint(&self, origin: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut hash_entry = |path: &Path| {
            path.to_string_lossy().hash(&mut hasher);
            if let Ok(meta) = fs::metadata(path) {
                meta.len().hash(&mut hasher);
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos())
                    .unwrap_or(0);
                modified.hash(&mut hasher);
            }
        };
        hash_entry(&self.generation_path());
        hash_entry(&self.events_root().join(origin));
        hasher.finish()
    }

    fn read_replay_sequence(&self, origin: &str) -> Result<u64> {
        let path = self.replay_sequence_path(origin);
        if let Ok(value) = fs::read_to_string(&path) {
            if let Ok(sequence) = value.trim().parse::<u64>() {
                return Ok(sequence);
            }
        }
        // 首次升级后没有 sequence 文件：从已落盘事件里恢复最大位置。
        self.scan_replay_sequence(origin)
    }

    fn scan_replay_sequence(&self, origin: &str) -> Result<u64> {
        let dir = self.events_root().join(origin);
        let mut max_position = 0u64;
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.ends_with(OBJECTS_SUFFIX) {
                    continue;
                }
                let file = fs::File::open(&path)?;
                for line in BufReader::new(file).lines() {
                    let Ok(line) = line else { continue };
                    let Ok(event) = serde_json::from_str::<ContributionEvent>(&line) else {
                        continue;
                    };
                    if let Some(position) = event.replay_position {
                        max_position = max_position.max(position.saturating_add(1));
                    }
                }
            }
        }
        Ok(max_position)
    }

    fn read_replay_chain_head(&self, origin: &str, starmap_id: &str) -> String {
        fs::read_to_string(self.replay_chain_path(origin, starmap_id))
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    }

    /// 读取某来源分支「星图 + 设备」已落盘的回放事件数与链头。
    ///
    /// 设备级 `replay_position` 只是跨星图排序序号，不能当回放位置。这里按
    /// 真实对象事件计数，供新事件哈希和检查点使用同一 0..N 坐标系。
    fn read_branch_replay_state(&self, origin: &str, starmap_id: &str) -> Result<(u64, String)> {
        if let Ok(value) = fs::read_to_string(self.replay_branch_count_path(origin, starmap_id)) {
            if let Ok(count) = value.trim().parse::<u64>() {
                let head = self.read_replay_chain_head(origin, starmap_id);
                // 只有计数、没有链头时说明本地索引不完整，必须回扫真实事件。
                if count == 0 || !head.is_empty() {
                    return Ok((count, head));
                }
            }
        }
        self.scan_branch_replay_state(origin, starmap_id)
    }

    fn scan_branch_replay_state(&self, origin: &str, starmap_id: &str) -> Result<(u64, String)> {
        let dir = self.events_root().join(origin);
        let chain_head = self.read_replay_chain_head(origin, starmap_id);
        if !dir.is_dir() {
            return Ok((0, chain_head));
        }
        let mut count = 0u64;
        let mut latest: Option<(u64, String, String)> = None;
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.ends_with(OBJECTS_SUFFIX) {
                continue;
            }
            let file = fs::File::open(&path)?;
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else { continue };
                // 强类型读取 camelCase JSONL；手写字段名会在 eventHash 等处出错。
                let Ok(event) = serde_json::from_str::<ContributionEvent>(&line) else {
                    continue;
                };
                let Some(delta) = event.replay_delta.as_ref() else {
                    continue;
                };
                if delta.starmap_id != starmap_id {
                    continue;
                }
                count = count.saturating_add(1);
                if let Some(hash) = delta.event_hash.as_ref() {
                    let candidate = (
                        event.replay_position.unwrap_or(0),
                        event.event_id.clone(),
                        hash.clone(),
                    );
                    if latest.as_ref().is_none_or(|current| {
                        (candidate.0, &candidate.1) >= (current.0, &current.1)
                    }) {
                        latest = Some(candidate);
                    }
                }
            }
        }
        let head = latest
            .map(|(_, _, hash)| hash)
            .filter(|hash| !hash.is_empty())
            .unwrap_or(chain_head);
        Ok((count, head))
    }

    /// 读取某来源分区已落盘的全部对象事件 ID，用于幂等去重。
    ///
    /// 缓存只在 origin 指纹变化（同步替换目录、generation 更新）时重扫。
    fn load_origin_event_ids(&self, origin: &str) -> Result<HashSet<String>> {
        let fingerprint = self.origin_fingerprint(origin);
        {
            let cache = self
                .event_id_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(entry) = cache.get(origin) {
                if entry.fingerprint == fingerprint {
                    return Ok(entry.ids.clone());
                }
            }
        }
        let dir = self.events_root().join(origin);
        let mut ids = HashSet::new();
        if dir.is_dir() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if !name.ends_with(OBJECTS_SUFFIX) {
                    continue;
                }
                ids.extend(read_ids(&path)?);
            }
        }
        let mut cache = self
            .event_id_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(
            origin.to_string(),
            OriginEventIdCache {
                fingerprint,
                ids: ids.clone(),
            },
        );
        Ok(ids)
    }

    /// 写成功后增量更新事件 ID 缓存，避免下一次记录再次全量扫描。
    fn cache_written_event_ids(&self, origin: &str, event_ids: &[String]) {
        if event_ids.is_empty() {
            return;
        }
        let fingerprint = self.origin_fingerprint(origin);
        let mut cache = self
            .event_id_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = cache.get_mut(origin) {
            entry.ids.extend(event_ids.iter().cloned());
            entry.fingerprint = fingerprint;
        }
    }

    /// 追加时分配来源设备内持久、单调的排序序号，并按星图分支内事件数建立
    /// 哈希链；同时返回需要维护检查点的分支目标。
    fn assign_replay_metadata(&self, events: &mut [ContributionEvent]) -> Result<ReplayMetadata> {
        let mut sequences: HashMap<String, u64> = HashMap::new();
        let mut chains: HashMap<(String, String), String> = HashMap::new();
        let mut branch_counts: HashMap<(String, String), u64> = HashMap::new();
        let mut checkpoint_targets: BTreeMap<(String, String), u64> = BTreeMap::new();
        for event in events {
            let Some(delta) = event.replay_delta.as_ref() else {
                continue;
            };
            if event.origin_device_id.is_empty() {
                continue;
            }
            let origin = event.origin_device_id.clone();
            let starmap_id = delta.starmap_id.clone();
            let chain_key = (origin.clone(), starmap_id.clone());
            if !sequences.contains_key(&origin) {
                sequences.insert(origin.clone(), self.read_replay_sequence(&origin)?);
            }
            let next = sequences.get_mut(&origin).expect("sequence inserted");
            match event.replay_position {
                Some(position) => {
                    *next = (*next).max(position.saturating_add(1));
                }
                None => {
                    let position = *next;
                    *next = next.saturating_add(1);
                    event.replay_position = Some(position);
                }
            }
            if !branch_counts.contains_key(&chain_key) {
                let (count, head) = self.read_branch_replay_state(&origin, &starmap_id)?;
                branch_counts.insert(chain_key.clone(), count);
                chains.entry(chain_key.clone()).or_insert(head);
            }
            let branch_position = *branch_counts
                .get(&chain_key)
                .expect("branch count inserted");
            if event
                .replay_delta
                .as_ref()
                .and_then(|delta| delta.event_hash.as_ref())
                .is_some()
            {
                if let Some(hash) = event
                    .replay_delta
                    .as_ref()
                    .and_then(|delta| delta.event_hash.clone())
                {
                    chains.insert(chain_key.clone(), hash);
                }
                let next = branch_counts
                    .get_mut(&chain_key)
                    .expect("branch count inserted");
                *next = next.saturating_add(1);
                checkpoint_targets.insert(chain_key, *next);
                continue;
            }
            let head = chains.get(&chain_key).cloned().unwrap_or_default();
            let mut delta = event.replay_delta.take().expect("delta exists");
            if delta.before_state_hash.is_none() {
                delta.before_state_hash = Some(proof_object_sha256(delta.before.as_ref()));
            }
            if delta.after_state_hash.is_none() {
                delta.after_state_hash = Some(proof_object_sha256(delta.after.as_ref()));
            }
            delta.prev_event_hash = (!head.is_empty()).then_some(head.clone());
            let identity = ReplayEventIdentity {
                event_id: &event.event_id,
                actor: event.actor,
                entrypoint: event.entrypoint,
                occurred_at_ms: event.occurred_at_ms,
                origin_device_id: &origin,
                project_id: &event.project_id,
                transaction_id: &event.transaction_id,
                replay_order: event.replay_order,
                text_input_kind: event.text_input_kind,
                platform: &event.platform,
                device_class: &event.device_class,
                branch_position,
            };
            let event_hash = compute_replay_event_hash(&head, &identity, &delta);
            delta.event_hash = Some(event_hash.clone());
            event.replay_delta = Some(delta);
            chains.insert(chain_key.clone(), event_hash);
            let next = branch_counts
                .get_mut(&chain_key)
                .expect("branch count inserted");
            *next = next.saturating_add(1);
            checkpoint_targets.insert(chain_key, *next);
        }
        Ok(ReplayMetadata {
            sequences,
            chains,
            checkpoint_targets: checkpoint_targets
                .into_iter()
                .map(|((origin, starmap_id), position)| (origin, starmap_id, position))
                .collect(),
        })
    }

    fn persist_replay_metadata(
        &self,
        sequences: &HashMap<String, u64>,
        chains: &HashMap<(String, String), String>,
    ) -> Result<()> {
        for (origin, sequence) in sequences {
            atomic_write_string(&self.replay_sequence_path(origin), &sequence.to_string())?;
        }
        for ((origin, starmap_id), hash) in chains {
            if hash.is_empty() {
                continue;
            }
            atomic_write_string(&self.replay_chain_path(origin, starmap_id), hash)?;
        }
        Ok(())
    }

    /// 事件落盘后按需维护完整图检查点，避免回放热路径写盘。
    fn maintain_starmap_checkpoints(&self, targets: &[(String, String, u64)]) {
        for (origin, starmap_id, position) in targets {
            let last = crate::writing_stats::starmap_checkpoint::last_checkpoint_position(
                &self.app_data_root,
                origin,
                starmap_id,
            )
            .ok()
            .flatten()
            .unwrap_or(0);
            if position.saturating_sub(last) < 64 {
                continue;
            }
            let request = crate::writing_stats::starmap_replay::StarMapReplayRequest::for_position(
                starmap_id, origin, *position,
            );
            let Ok(mut session) =
                crate::writing_stats::starmap_replay::StarMapReplaySession::open(self, &request)
            else {
                continue;
            };
            let frame = session.frame_at(&request);
            if !frame.valid {
                continue;
            }
            // 检查点必须锚定第 N 个位置自己的事件链头，不能读取仍在增长
            // 的分支尾部；否则 N 帧状态会被 N+1 的链头永久拒绝。
            let chain_head = session.event_chain_head_at(*position);
            let checkpoint = StarMapProofCheckpoint::new(
                starmap_id,
                origin,
                *position,
                frame.state,
                crate::writing_stats::starmap_checkpoint::CHECKPOINT_REASON_PERIODIC,
                frame.history_complete,
            )
            .with_event_chain_head(&chain_head);
            if let Err(error) = append_starmap_checkpoint(&self.app_data_root, &checkpoint) {
                log::warn!("failed to append periodic starmap checkpoint: {error}");
            }
        }
    }

    /// pending 重放后从真实已落盘事件重建本地派生索引。
    ///
    /// 不能把旧批次携带的链头直接写回：它可能早于更新的事件，覆盖后会让
    /// 下一次真实操作引用过期前缀。`.count/.chain/.sequence` 都只是可重建
    /// 索引，必须以磁盘事件为准。
    fn rebuild_replay_indexes(&self, events: &[ContributionEvent]) -> Result<()> {
        let mut origins: HashSet<String> = HashSet::new();
        let mut branches: HashSet<(String, String)> = HashSet::new();
        for event in events {
            let Some(delta) = event.replay_delta.as_ref() else {
                continue;
            };
            origins.insert(event.origin_device_id.clone());
            branches.insert((event.origin_device_id.clone(), delta.starmap_id.clone()));
        }
        for origin in origins {
            let sequence = self.scan_replay_sequence(&origin)?;
            atomic_write_string(&self.replay_sequence_path(&origin), &sequence.to_string())?;
        }
        for (origin, starmap_id) in branches {
            let (count, head) = self.scan_branch_replay_state(&origin, &starmap_id)?;
            if !head.is_empty() {
                atomic_write_string(&self.replay_chain_path(&origin, &starmap_id), &head)?;
            }
            self.persist_branch_count(&origin, &starmap_id, count);
        }
        Ok(())
    }

    pub fn resolve_origin_device_id(&self) -> Result<String> {
        if let Some(id) = self
            .origin_device_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
        {
            return Ok(id);
        }
        let id = resolve_device_partition_id(&self.app_data_root)?;
        if let Ok(mut guard) = self.origin_device_id.lock() {
            *guard = Some(id.clone());
        }
        Ok(id)
    }

    /// 幂等追加一批对象事件；返回真正新写入的事件数。
    ///
    /// 事件先以事务为粒度落盘到 pending journal，再追加到正式分区。追加失败时
    /// journal 保留，下次记录/读取时重放；稳定的 `event_id` 保证重放不会重复。
    pub fn record_events(&self, events: Vec<ContributionEvent>) -> Result<usize> {
        if events.is_empty() {
            return Ok(0);
        }
        let _write_guard = contribution_write_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let origin = self.resolve_origin_device_id()?;
        let device_info =
            crate::settings::load_device_info(&self.app_data_root).unwrap_or_default();
        let mut prepared = Vec::with_capacity(events.len());
        for mut event in events {
            if event.local_date.is_empty() {
                event.local_date =
                    crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                        .or_else(|_| {
                            crate::writing_stats::calendar::utc_date_at(event.occurred_at_ms)
                        })
                        .unwrap_or_default();
            }
            if event.origin_device_id.is_empty() {
                event.origin_device_id = origin.clone();
            }
            // 本机事件必须带上真实平台；同步/导入的远端事件保留源值。
            if event.origin_device_id == origin {
                if event.platform.is_empty() {
                    event.platform = device_info.platform.clone();
                }
                if event.device_class.is_empty() {
                    event.device_class = device_info.device_class.clone();
                }
            }
            prepared.push(event);
        }

        self.recover_pending_events_locked()?;
        // 先按真实落盘事实去重，再给新增事件分配序号/计数/哈希。重发相同
        // event_id 时不能推进任何派生索引，否则会留下不存在的“幽灵链头”。
        let mut existing_by_origin: HashMap<String, HashSet<String>> = HashMap::new();
        for event in &prepared {
            if existing_by_origin.contains_key(&event.origin_device_id) {
                continue;
            }
            existing_by_origin.insert(
                event.origin_device_id.clone(),
                self.load_origin_event_ids(&event.origin_device_id)?,
            );
        }
        let mut seen_in_batch: HashSet<(String, String)> = HashSet::new();
        prepared.retain(|event| {
            if event.event_id.is_empty() {
                return true;
            }
            if existing_by_origin
                .get(&event.origin_device_id)
                .is_some_and(|ids| ids.contains(&event.event_id))
            {
                return false;
            }
            seen_in_batch.insert((event.origin_device_id.clone(), event.event_id.clone()))
        });
        if prepared.is_empty() {
            return Ok(0);
        }
        let metadata = self.assign_replay_metadata(&mut prepared)?;
        let checkpoint_targets = metadata.checkpoint_targets.clone();
        let transaction_id = prepared
            .first()
            .map(|event| event.transaction_id.clone())
            .unwrap_or_default();
        let pending_path = self.pending_file_path(&transaction_id, prepared.len());
        self.write_pending(&pending_path, &prepared)?;
        self.persist_baseline_checkpoints(&prepared);

        let mut by_date: BTreeMap<String, Vec<ContributionEvent>> = BTreeMap::new();
        for event in prepared {
            let date = if event.local_date.is_empty() {
                crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                    .or_else(|_| crate::writing_stats::calendar::utc_date_at(event.occurred_at_ms))
                    .unwrap_or_else(|_| "1970-01-01".to_string())
            } else {
                event.local_date.clone()
            };
            by_date.entry(date).or_default().push(event);
        }

        let partition_dir = self.events_root().join(&origin);
        fs::create_dir_all(&partition_dir)?;
        let mut written = 0usize;
        let mut written_ids: Vec<String> = Vec::new();
        let existing_ids = existing_by_origin.entry(origin.clone()).or_default();
        for (date, events) in by_date {
            let path = self.event_file_path(&origin, &date);
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            for event in events {
                if !event.event_id.is_empty() && existing_ids.contains(&event.event_id) {
                    continue;
                }
                let line = serde_json::to_string(&event)?;
                writeln!(file, "{line}")?;
                if !event.event_id.is_empty() {
                    existing_ids.insert(event.event_id.clone());
                    written_ids.push(event.event_id.clone());
                }
                written += 1;
            }
        }
        self.persist_replay_metadata(&metadata.sequences, &metadata.chains)?;
        for (origin, starmap_id, count) in &checkpoint_targets {
            self.persist_branch_count(origin, starmap_id, *count);
        }
        // 元数据和分支计数落稳后才删 journal；中途崩溃时下次重放会重新
        // 扫描真实事件并修正计数，避免沿用旧缓存跳过位置。
        fs::remove_file(&pending_path)?;
        self.bump_replay_generation();
        self.cache_written_event_ids(&origin, &written_ids);
        // 检查点回放会通过 load_all_events 重入 pending recovery；先释放写锁，
        // 否则同一线程会在不可重入的全局锁上死锁。
        drop(_write_guard);
        self.maintain_starmap_checkpoints(&checkpoint_targets);
        Ok(written)
    }

    /// 重放上次进程在「业务已成功、事件已进入 journal」之后未能追加的事件。
    pub fn recover_pending_events(&self) -> Result<()> {
        let _write_guard = contribution_write_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.recover_pending_events_locked()
    }

    fn recover_pending_events_locked(&self) -> Result<()> {
        let pending_dir = self.app_data_root.join(PENDING_ROOT);
        if !pending_dir.is_dir() {
            return Ok(());
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&pending_dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        paths.sort();
        for path in paths {
            let content = fs::read_to_string(&path)?;
            let events: Vec<ContributionEvent> = serde_json::from_str(&content)?;
            if !events.is_empty() {
                self.append_events_locked(&events)?;
            }
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    fn pending_file_path(&self, transaction_id: &str, count: usize) -> PathBuf {
        let safe: String = transaction_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let safe = if safe.is_empty() {
            "unidentified".to_string()
        } else {
            safe
        };
        self.app_data_root
            .join(PENDING_ROOT)
            .join(format!("{safe}-{count}.json"))
    }

    fn write_pending(&self, path: &Path, events: &[ContributionEvent]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write_string(path, &serde_json::to_string(events)?)
    }

    fn append_events_locked(&self, events: &[ContributionEvent]) -> Result<()> {
        let mut by_origin_date: BTreeMap<(String, String), Vec<&ContributionEvent>> =
            BTreeMap::new();
        for event in events {
            let origin = if event.origin_device_id.is_empty() {
                self.resolve_origin_device_id()?
            } else {
                event.origin_device_id.clone()
            };
            let date = if event.local_date.is_empty() {
                crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                    .or_else(|_| crate::writing_stats::calendar::utc_date_at(event.occurred_at_ms))
                    .unwrap_or_else(|_| "1970-01-01".to_string())
            } else {
                event.local_date.clone()
            };
            by_origin_date
                .entry((origin, date))
                .or_default()
                .push(event);
        }
        let mut existing_by_origin: HashMap<String, HashSet<String>> = HashMap::new();
        for (origin, _) in by_origin_date.keys() {
            if !existing_by_origin.contains_key(origin) {
                existing_by_origin.insert(origin.clone(), self.load_origin_event_ids(origin)?);
            }
        }
        let mut written_by_origin: HashMap<String, Vec<String>> = HashMap::new();
        for ((origin, date), events) in by_origin_date {
            let path = self.event_file_path(&origin, &date);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let existing = existing_by_origin.entry(origin.clone()).or_default();
            let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
            for event in &events {
                if !event.event_id.is_empty() && existing.contains(&event.event_id) {
                    continue;
                }
                writeln!(file, "{}", serde_json::to_string(event)?)?;
                if !event.event_id.is_empty() {
                    existing.insert(event.event_id.clone());
                    written_by_origin
                        .entry(origin.clone())
                        .or_default()
                        .push(event.event_id.clone());
                }
            }
        }
        let all_events: Vec<ContributionEvent> = events.to_vec();
        self.persist_baseline_checkpoints(&all_events);
        self.rebuild_replay_indexes(&all_events)?;
        self.bump_replay_generation();
        for (origin, ids) in written_by_origin {
            self.cache_written_event_ids(&origin, &ids);
        }
        Ok(())
    }

    /// 第一次在旧星图上记录回放事件时落一条真实基线检查点。
    ///
    /// 基线明确标记历史不完整：旧星图不是从空白创建出来的，回放不能伪造
    /// 更早的过程。检查点按位置 0 幂等写入，重复调用不会覆盖。
    fn persist_baseline_checkpoints(&self, events: &[ContributionEvent]) {
        for event in events {
            let Some(delta) = event.replay_delta.as_ref() else {
                continue;
            };
            let Some(baseline) = delta.baseline.as_ref() else {
                continue;
            };
            if delta.starmap_id.is_empty() {
                continue;
            }
            let device_id = if event.origin_device_id.is_empty() {
                match self.resolve_origin_device_id() {
                    Ok(id) => id,
                    Err(_) => continue,
                }
            } else {
                event.origin_device_id.clone()
            };
            if has_starmap_checkpoints(&self.app_data_root, &device_id, &delta.starmap_id) {
                continue;
            }
            let checkpoint = StarMapProofCheckpoint::new(
                &delta.starmap_id,
                &device_id,
                0,
                baseline.state(),
                if baseline.history_complete {
                    CHECKPOINT_REASON_CREATED
                } else {
                    CHECKPOINT_REASON_BASELINE_INCOMPLETE
                },
                baseline.history_complete,
            );
            if let Err(error) = append_starmap_checkpoint(&self.app_data_root, &checkpoint) {
                log::warn!("failed to persist starmap baseline checkpoint: {error}");
            }
        }
    }

    pub fn load_all_events(&self) -> Result<Vec<ContributionEvent>> {
        self.recover_pending_events()?;
        let mut events = Vec::new();
        let mut seen = HashSet::new();
        for origin in self.list_origins()? {
            let dir = self.events_root().join(&origin);
            if !dir.is_dir() {
                continue;
            }
            let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(OBJECTS_SUFFIX))
                })
                .collect();
            paths.sort();
            for path in paths {
                let file = fs::File::open(&path)?;
                for line in BufReader::new(file).lines() {
                    let line = line?;
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let mut event: ContributionEvent = match serde_json::from_str(line) {
                        Ok(event) => event,
                        Err(error) => {
                            eprintln!(
                                "[writing_stats] skip malformed contribution event in {}: {error}",
                                path.display()
                            );
                            continue;
                        }
                    };
                    if event.origin_device_id.is_empty() {
                        event.origin_device_id = origin.clone();
                    }
                    if !event.event_id.is_empty() && !seen.insert(event.event_id.clone()) {
                        continue;
                    }
                    events.push(event);
                }
            }
        }
        events.sort_by(|left, right| {
            left.occurred_at_ms
                .cmp(&right.occurred_at_ms)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    pub fn load_events_for_project(&self, project_id: &str) -> Result<Vec<ContributionEvent>> {
        Ok(self
            .load_all_events()?
            .into_iter()
            .filter(|event| event.project_id == project_id)
            .collect())
    }

    pub fn load_events_range(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<ContributionEvent>> {
        Ok(self
            .load_all_events()?
            .into_iter()
            .filter(|event| {
                let date = if event.local_date.is_empty() {
                    crate::writing_stats::calendar::local_date_at(event.occurred_at_ms)
                        .unwrap_or_default()
                } else {
                    event.local_date.clone()
                };
                !date.is_empty() && date.as_str() >= start_date && date.as_str() <= end_date
            })
            .collect())
    }

    fn list_origins(&self) -> Result<Vec<String>> {
        let root = self.events_root();
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut origins = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if valid_segment(&name) {
                origins.push(name);
            }
        }
        origins.sort();
        Ok(origins)
    }
}

/// 同步直接搬入事件文件后显式递增事件库代数，避免依赖目录 mtime。
pub fn invalidate_stats_event_store(app_data_root: &Path) {
    let path = app_data_root
        .join(EVENTS_ROOT)
        .join(".contribution-generation");
    let value = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let _ = atomic_write_string(&path, &value.to_string());
}

/// 解析本机统计事件分区使用的稳定设备 ID。
///
/// 对象贡献事件与 AI 请求/响应事件共用同一来源分区规则；设备信息缺失时
/// 按既有设置流程生成，已有但非法则明确报错，不写入无法同步的目录名。
pub(crate) fn resolve_device_partition_id(app_data_root: &Path) -> Result<String> {
    let info = match crate::settings::load_device_info(app_data_root) {
        Ok(info) if valid_segment(&info.device_id) => info,
        _ => crate::settings::ensure_device_info(app_data_root, "", "", None)?,
    };
    if !valid_segment(&info.device_id) {
        return Err(Error::Other(format!(
            "invalid device id for stats event partition: {:?}",
            info.device_id
        )));
    }
    Ok(info.device_id)
}

fn read_ids(path: &Path) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(ids),
        Err(error) => return Err(error.into()),
    };
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(id) = value.get("event_id").and_then(|id| id.as_str()) {
                ids.insert(id.to_string());
            }
        }
    }
    Ok(ids)
}

pub(crate) fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// 供投影层使用的稳定字符串键。
pub fn actor_key(actor: MutationActor) -> &'static str {
    actor.as_str()
}

pub fn object_type_key(object_type: ContributionObjectType) -> &'static str {
    object_type.as_str()
}

pub fn operation_key(operation: ContributionOperation) -> &'static str {
    operation.as_str()
}
