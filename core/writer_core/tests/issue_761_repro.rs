//! Issue #761 回归测试 — GitHub 同步改走 Git tree/commit/ref 原子发布。
//!
//! 覆盖四个核心契约（行为级，不是字符串常量断言）：
//!
//! 1. **批量原子发布**：`capabilities().batch == true` 时，一个 target 的一次
//!    generation 发布 = 恰好一次 `commit_batch`；manifest、内容 mutation 与
//!    `generation.meta.json`（直接 `complete=true`）在同一批提交里，
//!    不再逐文件 `provider.write()`。空远端首次同步（`merge_outcome == None`）同样
//!    走 batch 路径（评论 5828969186 问题 2）。
//! 2. **冲突路径复用远端 blob**：unresolved conflict 的路径必须用
//!    `RemoteVersion`（旧 visible generation 的 blob）生成 `ReuseVersion`，
//!    绝不读本地冲突正文冒充 merged remote record；没有远端 blob 可复用时
//!    整个 target 返回 `PartialConflict`，不发布内容不完整的 generation。
//! 3. **MemoryProvider batch 原子性**：中途失败（如 `ReuseVersion` 引用不存在的版本）
//!    时整个 batch 不生效，不留半事务（评论 5828969186 问题 3）。
//! 4. **ref CAS 冲突重试**：Git branch ref 被别的设备推进（409/422）时重读 catalog
//!    并进入下一轮 CAS 重试，不直接打成 fatal（评论 5828969186 问题 4）。
//!
//! 这些断言驱动真实的 `run_transfer` → `transfer_live_project` →
//! `publish_generation` 路径（`MemoryProvider` 声明 `batch=true`）。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use tempfile::TempDir;
use writer_core::sync::full_sync::{run_transfer, FullSyncPlan, LiveTargetLww, PlannedTarget};
use writer_core::sync::provider::capabilities::SyncCapabilities;
use writer_core::sync::provider::error::ProviderError;
use writer_core::sync::provider::memory::MemoryProvider;
use writer_core::sync::provider::model::{
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
};
use writer_core::sync::provider::SyncProvider;
use writer_core::sync::target_lifecycle::{
    find_record, load_remote_catalog, upsert_record, write_remote_catalog,
};
use writer_core::sync::types::{
    ManifestFileRecord, PlannedTargetKind, RemoteTargetCatalogSnapshot, SyncConflict,
    SyncConflictKind, SyncManifest, SyncPolicy, SyncState, SyncStatus, SyncTarget,
    TargetLifecycleCatalog, TargetLifecycleRecord,
};

/// `__generations__` 子目录名 — 与 `full_sync::generation::GENERATION_SUBDIR` 一致。
const GENERATION_SUBDIR: &str = "__generations__";
/// `generation.meta.json` 文件名。
const GENERATION_META_FILENAME: &str = "generation.meta.json";
/// `manifest.sync.json` 相对路径（`lww::SYNC_MANIFEST_PATH`）。
const SYNC_MANIFEST_PATH: &str = "app-meta/sync/manifest.sync.json";

const PROJECT_PREFIX: &str = "projects/p1";
const GEN_EXISTING: &str = "gen_existing";
const CONFLICT_PATH: &str = "volumes/v1/chapters/c1/chapter.md";
const SAFE_PATH: &str = "volumes/v1/chapters/c2/chapter.md";
const REMOTE_CONFLICT_CONTENT: &[u8] = b"remote conflict content";
const LOCAL_CONFLICT_CONTENT: &[u8] = b"local conflict content";
const SAFE_CONTENT: &[u8] = b"safe new chapter content";
const T: i64 = 10_000;
const DEVICE_REMOTE: &str = "device_remote";
const DEVICE_LOCAL: &str = "device_local";

/// 记录 `commit_batch` 与逐文件 `write()` 的 provider 包装。
///
/// `capabilities()` 透传 `MemoryProvider`（`batch=true`），因此
/// `publish_generation` 必须走批量路径；若代码退回逐文件 `write()`，
/// `generation_file_writes()` 会非零，测试立刻失败。
struct RecordingProvider {
    inner: MemoryProvider,
    batches: Mutex<Vec<RecordedBatch>>,
    generation_file_writes: AtomicUsize,
    /// 第一次 `commit_batch` 时模拟另一台设备推进 branch：
    /// `(target_id, lww_time_ms, device_id)` 是并发设备写入的 upsert record。
    /// 写入后返回 ref CAS 冲突（GitHub `PATCH /git/refs` force=false 的 409/422），
    /// 用于验证 transfer 会重读 catalog 并进入下一轮重试（评论 5828969186 问题 4）。
    concurrent_advance_on_first_batch: Option<(String, i64, String)>,
}

/// 一次 `commit_batch` 的调用记录。
#[derive(Clone)]
struct RecordedBatch {
    message: String,
    mutations: Vec<BatchMutation>,
}

impl RecordingProvider {
    fn new(inner: MemoryProvider) -> Self {
        Self {
            inner,
            batches: Mutex::new(Vec::new()),
            generation_file_writes: AtomicUsize::new(0),
            concurrent_advance_on_first_batch: None,
        }
    }

    /// 构造"第一次 commit_batch 被并发设备推进 branch"的 provider。
    fn with_concurrent_ref_advance(
        inner: MemoryProvider,
        target_id: &str,
        lww_time_ms: i64,
        device_id: &str,
    ) -> Self {
        Self {
            inner,
            batches: Mutex::new(Vec::new()),
            generation_file_writes: AtomicUsize::new(0),
            concurrent_advance_on_first_batch: Some((
                target_id.to_string(),
                lww_time_ms,
                device_id.to_string(),
            )),
        }
    }

    fn recorded_batches(&self) -> Vec<RecordedBatch> {
        self.batches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 逐文件 `write()` 命中 `__generations__` 的次数（应为 0）。
    fn generation_file_writes(&self) -> usize {
        self.generation_file_writes.load(Ordering::SeqCst)
    }

    /// 模拟另一台设备在 ref CAS 期间推进 branch：写入更新的远端 catalog record。
    fn simulate_concurrent_ref_advance(&self, target_id: &str, lww_time_ms: i64, device_id: &str) {
        let existing = load_remote_catalog(&self.inner).expect("catalog loadable");
        let mut catalog = existing.catalog.clone();
        upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert(target_id, target_id, lww_time_ms, device_id),
        );
        write_remote_catalog(
            &self.inner,
            &RemoteTargetCatalogSnapshot {
                catalog,
                version: existing.version.clone(),
            },
        )
        .expect("concurrent catalog write");
    }
}

impl SyncProvider for RecordingProvider {
    fn capabilities(&self) -> SyncCapabilities {
        self.inner.capabilities()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        self.inner.list(prefix)
    }

    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        self.inner.read(path)
    }

    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        if path.contains(GENERATION_SUBDIR) {
            self.generation_file_writes.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.write(path, content, precondition)
    }

    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        self.inner.delete(path, precondition)
    }

    fn commit_batch(
        &self,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        let first_attempt = {
            let mut batches = self.batches.lock().unwrap_or_else(|e| e.into_inner());
            let first = batches.is_empty();
            batches.push(RecordedBatch {
                message: message.to_string(),
                mutations: mutations.to_vec(),
            });
            first
        };
        if first_attempt {
            if let Some((target_id, lww_time_ms, device_id)) =
                &self.concurrent_advance_on_first_batch
            {
                self.simulate_concurrent_ref_advance(target_id, *lww_time_ms, device_id);
                // GitHub ref PATCH 用 force=false：别人刚推进 head 时返回 409/422，
                // provider 映射成 PreconditionFailed。
                return Err(ProviderError::PreconditionFailed {
                    path: "refs/heads/main".to_string(),
                    reason: "ref moved by another device".to_string(),
                });
            }
        }
        self.inner.commit_batch(mutations, message)
    }
}

/// 在 staging 写入文件 + manifest（LWW = `(lww_time, device_id)`）。
fn build_staging(
    tmp: &TempDir,
    files: &[(&str, &[u8])],
    lww_time: i64,
    device_id: &str,
) -> PathBuf {
    let staging_root = tmp.path().join("staging-p1");
    let mut records = Vec::new();
    for (rel, content) in files {
        let abs = staging_root.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, content).unwrap();
        records.push(ManifestFileRecord {
            path: (*rel).to_string(),
            content_hash: format!("{:x}", md5::compute(*content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        });
    }
    write_staging_manifest(&staging_root, &SyncManifest { files: records });
    staging_root
}

fn write_staging_manifest(staging_root: &Path, manifest: &SyncManifest) {
    let manifest_dir = staging_root.join("app-meta").join("sync");
    std::fs::create_dir_all(&manifest_dir).unwrap();
    std::fs::write(
        manifest_dir.join("manifest.sync.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
}

/// 预置 staging 的 `sync_state.json`（未解决冲突 / device_id 等）。
fn write_sync_state(staging_root: &Path, state: &SyncState) {
    let state_dir = staging_root.join("app-meta").join("sync");
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::write(
        state_dir.join("sync_state.json"),
        serde_json::to_vec(state).unwrap(),
    )
    .unwrap();
}

/// 在远端 generation 写入文件 + manifest，返回每个相对路径的远端 blob 版本。
///
/// 版本就是 `ReuseVersion` 需要复用的 blob SHA（GitHub 下）／远端版本（Memory 下）。
fn write_remote_generation(
    provider: &dyn SyncProvider,
    gen_prefix: &str,
    files: &[(&str, &[u8])],
    lww_time: i64,
    device_id: &str,
) -> HashMap<String, RemoteVersion> {
    let mut versions = HashMap::new();
    let mut records = Vec::new();
    for (rel, content) in files {
        let remote_path = format!("{gen_prefix}/{rel}");
        let version = provider
            .write(&remote_path, content, WritePrecondition::Unconditional)
            .unwrap();
        versions.insert((*rel).to_string(), version);
        records.push(ManifestFileRecord {
            path: (*rel).to_string(),
            content_hash: format!("{:x}", md5::compute(*content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        });
    }
    let manifest_path = format!("{gen_prefix}/{SYNC_MANIFEST_PATH}");
    provider
        .write(
            &manifest_path,
            &serde_json::to_vec(&SyncManifest { files: records }).unwrap(),
            WritePrecondition::Unconditional,
        )
        .unwrap();
    versions
}

/// 写入远端 catalog（Upsert + active_generation），返回加载后的 snapshot。
fn seed_remote_catalog(
    provider: &dyn SyncProvider,
    lww_time: i64,
    device_id: &str,
    active_generation: &str,
) -> RemoteTargetCatalogSnapshot {
    let record = TargetLifecycleRecord::upsert(PROJECT_PREFIX, PROJECT_PREFIX, lww_time, device_id)
        .with_active_generation(active_generation);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(provider, &snapshot).unwrap();
    load_remote_catalog(provider).unwrap()
}

/// 构造 `FullSyncPlan`（LiveProject target）。
fn build_plan(
    tmp: &TempDir,
    staging_root: PathBuf,
    lww_time: i64,
    device_id: &str,
    remote_catalog_snapshot: RemoteTargetCatalogSnapshot,
) -> FullSyncPlan {
    let local_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&local_root).unwrap();
    let planned = PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: local_root.clone(),
        staging_root: Some(staging_root),
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: local_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: lww_time,
            device_id: device_id.to_string(),
        }),
        expected_delete_lww: None,
    };
    FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot,
    }
}

/// 取出 batch 中命中给定路径后缀的 mutation。
fn mutation_for<'a>(batch: &'a RecordedBatch, path_suffix: &str) -> &'a BatchMutation {
    let matched: Vec<&BatchMutation> = batch
        .mutations
        .iter()
        .filter(|m| m.path().ends_with(path_suffix))
        .collect();
    assert_eq!(
        matched.len(),
        1,
        "batch 中应恰好有 1 个 mutation 命中 {path_suffix}，实际 {:?}",
        batch
            .mutations
            .iter()
            .map(BatchMutation::path)
            .collect::<Vec<_>>()
    );
    matched[0]
}

/// 断言 batch 中 meta mutation 直接写 `complete=true`。
fn assert_meta_complete_true(batch: &RecordedBatch) -> String {
    let meta = mutation_for(batch, GENERATION_META_FILENAME);
    let BatchMutation::Put { path, content } = meta else {
        panic!("generation.meta.json 必须是 Put，实际 {meta:?}");
    };
    let json: serde_json::Value = serde_json::from_slice(content).unwrap();
    assert_eq!(
        json["complete"], true,
        "batch 路径必须直接写 complete=true（不再先 false 再 true），实际 {json}"
    );
    path.trim_end_matches(&format!("/{GENERATION_META_FILENAME}"))
        .to_string()
}

/// 场景 1（Issue #761 Part 3/4）：unresolved conflict → 一次原子 batch 发布。
///
/// 远端 `gen_existing` 有 `c1/chapter.md`（本地也改过 → BothChanged 冲突），
/// 本地 staging 另有 `c2/chapter.md`（本地新增，安全文件）。
/// candidate LWW 严格赢 → 必须发布新 generation：
///
/// - 只调用一次 `commit_batch`，不逐文件 `write()`；
/// - 冲突路径用 `ReuseVersion`（复用旧 visible generation 的远端 blob），
///   不读本地冲突正文；
/// - 安全文件用 `Put` 正常上传；
/// - `manifest.sync.json` 与 `generation.meta.json`（complete=true）同一批提交；
/// - CAS 成功（catalog 指向新 generation），冲突章在新 generation 保持远端可见版本。
#[test]
#[allow(clippy::too_many_lines)]
fn regression_issue_761_conflict_reuses_remote_blob_in_single_atomic_batch() {
    let tmp = TempDir::new().unwrap();
    let inner = MemoryProvider::new();
    let remote_gen_prefix = format!("{PROJECT_PREFIX}/{GENERATION_SUBDIR}/{GEN_EXISTING}");
    let remote_versions = write_remote_generation(
        &inner,
        &remote_gen_prefix,
        &[(CONFLICT_PATH, REMOTE_CONFLICT_CONTENT)],
        T,
        DEVICE_REMOTE,
    );
    // remote record 时间更小 → candidate (T, DEVICE_LOCAL) 严格赢。
    let catalog_snapshot = seed_remote_catalog(&inner, T - 1, DEVICE_REMOTE, GEN_EXISTING);
    let provider = RecordingProvider::new(inner);

    let staging_root = build_staging(
        &tmp,
        &[
            (CONFLICT_PATH, LOCAL_CONFLICT_CONTENT),
            (SAFE_PATH, SAFE_CONTENT),
        ],
        T,
        DEVICE_LOCAL,
    );
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, catalog_snapshot);
    let transfer = run_transfer(&provider, &plan, None, None, None);
    assert_eq!(transfer.targets.len(), 1);

    // ── 一次批量提交，不再逐文件 write() 到 generation prefix ──
    assert_eq!(
        provider.generation_file_writes(),
        0,
        "generation 发布不应再调用逐文件 write()"
    );
    let batches = provider.recorded_batches();
    assert_eq!(
        batches.len(),
        1,
        "一次 generation 发布应恰好一次 commit_batch，实际 {}",
        batches.len()
    );
    let batch = &batches[0];
    assert!(
        batch.message.contains("generation"),
        "commit message 应带 generation 上下文，实际 {:?}",
        batch.message
    );

    // ── 冲突路径：ReuseVersion（远端 blob），不是本地冲突正文 ──
    let conflict_mutation = mutation_for(batch, CONFLICT_PATH);
    match conflict_mutation {
        BatchMutation::ReuseVersion { version, .. } => assert_eq!(
            version, &remote_versions[CONFLICT_PATH],
            "冲突路径必须复用旧 visible generation 的远端 blob"
        ),
        other => panic!("冲突路径必须是 ReuseVersion，实际 {other:?}"),
    }

    // ── 安全文件：Put 本地新内容 ──
    match mutation_for(batch, SAFE_PATH) {
        BatchMutation::Put { content, .. } => {
            assert_eq!(content, SAFE_CONTENT, "安全文件应上传本地内容");
        }
        other => panic!("安全文件必须是 Put，实际 {other:?}"),
    }

    // ── manifest 同一批提交，且是 merged manifest（两个文件都在） ──
    match mutation_for(batch, SYNC_MANIFEST_PATH) {
        BatchMutation::Put { content, .. } => {
            let manifest: SyncManifest = serde_json::from_slice(content).unwrap();
            let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
            assert!(
                paths.contains(&CONFLICT_PATH) && paths.contains(&SAFE_PATH),
                "merged manifest 应同时包含冲突路径与安全路径，实际 {paths:?}"
            );
        }
        other => panic!("manifest 必须是 Put，实际 {other:?}"),
    }

    // ── meta 直接 complete=true，且与内容同一批 ──
    let new_gen_prefix = assert_meta_complete_true(batch);
    assert_ne!(
        new_gen_prefix, remote_gen_prefix,
        "必须发布到新 generation prefix"
    );
    let new_gen_id = new_gen_prefix.rsplit('/').next().expect("generation id");
    assert!(
        !batch.mutations.iter().any(|m| {
            matches!(m, BatchMutation::Put { content, .. }
                if serde_json::from_slice::<serde_json::Value>(content)
                    .map(|v| v["complete"] == false)
                    .unwrap_or(false))
        }),
        "batch 路径不应出现 complete=false 的中间 meta"
    );

    // ── CAS 成功：catalog 指向新 generation，README 说的冲突不阻塞安全文件 ──
    let status = &transfer.targets[0].result.status;
    assert!(
        matches!(status, SyncStatus::PartialConflict),
        "冲突仍保留时应返回 PartialConflict，实际 {status:?}"
    );
    assert!(
        !transfer.targets[0].result.conflicts.is_empty(),
        "冲突列表不应为空"
    );
    let uploaded = &transfer.targets[0].result.uploaded_files;
    assert!(
        uploaded.contains(&SAFE_PATH.to_string()),
        "安全文件应成功上传，实际 uploaded_files={uploaded:?}"
    );

    let catalog_after = load_remote_catalog(&provider).unwrap();
    let record = find_record(&catalog_after.catalog, PROJECT_PREFIX).expect("catalog record");
    assert_eq!(
        record.active_generation.as_deref(),
        Some(new_gen_id),
        "CAS 成功后 catalog 应指向新 generation"
    );

    // ── 新 generation 里冲突章是远端可见版本，安全文件是本地新内容 ──
    let published_conflict = provider
        .read(&format!("{new_gen_prefix}/{CONFLICT_PATH}"))
        .unwrap()
        .expect("冲突路径在新 generation 应有对象");
    assert_eq!(
        published_conflict.content, REMOTE_CONFLICT_CONTENT,
        "新 generation 的冲突路径必须是远端可见版本，不能是本地冲突正文"
    );
    let published_safe = provider
        .read(&format!("{new_gen_prefix}/{SAFE_PATH}"))
        .unwrap()
        .expect("安全文件在新 generation 应有对象");
    assert_eq!(published_safe.content, SAFE_CONTENT);

    eprintln!(
        "[BUGFIX_REGRESSION_TRACE] Issue #761 批量原子发布：commit_batch 调用 {} 次、\
         逐文件 generation write {} 次、新 generation {}（冲突路径 ReuseVersion=远端 blob）",
        batches.len(),
        provider.generation_file_writes(),
        new_gen_id
    );
}

/// 场景 2（Issue #761 Part 4）：unresolved conflict 且远端无 blob 可复用 →
/// `PartialConflict`，不发布内容不完整的 generation。
///
/// staging 预置一个未解决冲突路径 `c9/chapter.md`（只有本地记录，远端 generation
/// 没有对应 blob）。publish 前无法用远端 blob 生成 `ReuseVersion` 时，
/// 必须整个 target 返回 `PartialConflict`：既不 `commit_batch`，
/// 也不逐文件上传本地冲突正文。
#[test]
#[allow(clippy::too_many_lines)]
fn regression_issue_761_unresolved_conflict_without_remote_blob_returns_partial_conflict() {
    let tmp = TempDir::new().unwrap();
    let ghost_path = "volumes/v1/chapters/c9/chapter.md";
    let inner = MemoryProvider::new();
    let remote_gen_prefix = format!("{PROJECT_PREFIX}/{GENERATION_SUBDIR}/{GEN_EXISTING}");
    // 远端 generation 存在但没有 ghost path 的 blob。
    write_remote_generation(&inner, &remote_gen_prefix, &[], T, DEVICE_REMOTE);
    let catalog_snapshot = seed_remote_catalog(&inner, T - 1, DEVICE_REMOTE, GEN_EXISTING);
    let provider = RecordingProvider::new(inner);

    let staging_root = build_staging(
        &tmp,
        &[(ghost_path, LOCAL_CONFLICT_CONTENT)],
        T,
        DEVICE_LOCAL,
    );
    // 预置未解决冲突：BothChanged 且远端没有 snapshot / blob 可复用。
    let conflict = SyncConflict {
        local_path: ghost_path.to_string(),
        remote_path: ghost_path.to_string(),
        local_hash: format!("{:x}", md5::compute(LOCAL_CONFLICT_CONTENT)),
        remote_hash: "missing-remote-hash".to_string(),
        base_hash: "base-hash".to_string(),
        created_at: chrono::Utc::now().timestamp(),
        description: "正文文件双端修改冲突。".to_string(),
        kind: SyncConflictKind::BothChanged,
        remote_snapshot_path: None,
    };
    let mut state = SyncState {
        device_id: DEVICE_LOCAL.to_string(),
        ..Default::default()
    };
    state.conflicted_files.insert(ghost_path.to_string());
    state.conflicts.push(conflict);
    write_sync_state(&staging_root, &state);

    let plan = build_plan(
        &tmp,
        staging_root.clone(),
        T,
        DEVICE_LOCAL,
        catalog_snapshot,
    );
    let transfer = run_transfer(&provider, &plan, None, None, None);
    assert_eq!(transfer.targets.len(), 1);

    let status = &transfer.targets[0].result.status;
    assert!(
        matches!(status, SyncStatus::PartialConflict),
        "无远端 blob 可复用的 unresolved conflict 应返回 PartialConflict，实际 {status:?}"
    );
    let error = transfer.targets[0].result.error.clone().unwrap_or_default();
    assert!(
        error.contains("no remote blob to reuse"),
        "错误信息应说明缺远端 blob 可复用，实际 {error:?}"
    );

    // ── 不发布任何东西：没有 commit_batch、没有逐文件 generation write ──
    assert!(
        provider.recorded_batches().is_empty(),
        "无远端 blob 可复用时不应 commit_batch，实际 {} 次",
        provider.recorded_batches().len()
    );
    assert_eq!(
        provider.generation_file_writes(),
        0,
        "无远端 blob 可复用时不应逐文件上传本地冲突正文"
    );

    // ── CAS 未执行：catalog 仍指向旧 visible generation ──
    let catalog_after = load_remote_catalog(&provider).unwrap();
    let record = find_record(&catalog_after.catalog, PROJECT_PREFIX).expect("catalog record");
    assert_eq!(
        record.active_generation.as_deref(),
        Some(GEN_EXISTING),
        "未发布新 generation 时 catalog 不应改变"
    );

    // ── 冲突保留在 staging，等用户选择（#757 界面）后再发布 ──
    let state_after = writer_core::sync::SyncService::load_sync_state(&staging_root).unwrap();
    assert!(
        state_after.conflicted_files.contains(ghost_path),
        "未解决的冲突必须保留，实际 {:?}",
        state_after.conflicted_files
    );

    eprintln!(
        "[BUGFIX_REGRESSION_TRACE] Issue #761 无远端 blob 可复用：PartialConflict、\
         commit_batch 0 次、catalog 仍指向 {GEN_EXISTING}"
    );
}

/// 场景 3：`SyncProvider::commit_batch` 在 MemoryProvider 上是单锁内事务。
///
/// Put / ReuseVersion / Delete 三种 mutation 一次提交全部生效，
/// `touched_paths` 覆盖所有 mutation。
#[test]
#[allow(clippy::too_many_lines)]
fn regression_issue_761_memory_commit_batch_is_single_transaction() {
    let provider = MemoryProvider::with_entries(vec![
        ("source/a.txt".to_string(), b"hello".to_vec()),
        ("half/old-b.txt".to_string(), b"old-b".to_vec()),
    ]);
    assert!(provider.capabilities().batch);
    assert!(provider.capabilities().atomic_write);
    let source_version = provider.read("source/a.txt").unwrap().unwrap().version;

    let mutations = vec![
        BatchMutation::Put {
            path: "target/b.txt".to_string(),
            content: b"world".to_vec(),
        },
        BatchMutation::ReuseVersion {
            path: "target/a.txt".to_string(),
            version: source_version.clone(),
        },
        BatchMutation::Delete {
            path: "source/a.txt".to_string(),
        },
    ];

    let result = provider
        .commit_batch(&mutations, "single transaction")
        .unwrap();
    assert_eq!(result.touched_paths.len(), 3);
    assert_eq!(
        provider.read("target/b.txt").unwrap().unwrap().content,
        b"world"
    );
    assert_eq!(
        provider.read("target/a.txt").unwrap().unwrap().content,
        b"hello",
        "ReuseVersion 应复用已有远端版本内容"
    );
    assert!(provider.read("source/a.txt").unwrap().is_none());

    // 半事务回归（评论 5828969186 问题 3）：batch 中途失败时不得留下任何已应用 mutation。
    // 序列 Put(new-a) -> Delete(old-b) -> ReuseVersion(不存在的版本)：
    // 第三步失败返回 Err 后，前两步必须一起回滚。
    let err = provider
        .commit_batch(
            &[
                BatchMutation::Put {
                    path: "half/new-a.txt".to_string(),
                    content: b"new-a".to_vec(),
                },
                BatchMutation::Delete {
                    path: "half/old-b.txt".to_string(),
                },
                BatchMutation::ReuseVersion {
                    path: "half/c.txt".to_string(),
                    version: RemoteVersion::new("missing-version"),
                },
            ],
            "half transaction",
        )
        .unwrap_err();
    assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
    assert!(
        provider.read("half/new-a.txt").unwrap().is_none(),
        "失败 batch 里已应用的 Put 不得留在 store（半事务）"
    );
    assert_eq!(
        provider.read("half/old-b.txt").unwrap().unwrap().content,
        b"old-b",
        "失败 batch 里已应用的 Delete 不得生效（半事务）"
    );
    assert!(
        provider.read("half/c.txt").unwrap().is_none(),
        "失败的 ReuseVersion 不应写入半个对象"
    );
    // 失败 batch 不影响此前成功 batch 的结果。
    assert!(provider.read("target/b.txt").unwrap().is_some());
}

/// 场景 4（Issue #761 Part 1）：能力声明。
///
/// GitHub 改走 Git Database API 后必须声明 `batch=true`、`atomic_write=true`，
/// 同时保留 Contents API 的条件写入能力；`MemoryProvider` 也按一次锁内事务声明 batch。
#[test]
fn regression_issue_761_batch_capabilities_declared() {
    let github = SyncCapabilities::github();
    assert!(github.batch, "GitHub 批量原子提交后必须声明 batch=true");
    assert!(
        github.atomic_write,
        "GitHub 批量原子提交后必须声明 atomic_write=true"
    );
    assert!(
        github.conditional_write,
        "Contents API 的 If-Match 条件写入能力保留"
    );

    let memory = MemoryProvider::new().capabilities();
    assert!(memory.batch, "MemoryProvider 应声明 batch=true");
    assert!(
        memory.atomic_write,
        "MemoryProvider 应声明 atomic_write=true"
    );
}

/// 空远端（无 catalog / 无 visible generation）的首次同步场景。
fn empty_remote_catalog_snapshot() -> RemoteTargetCatalogSnapshot {
    RemoteTargetCatalogSnapshot {
        catalog: TargetLifecycleCatalog::default(),
        version: RemoteVersion::new("__nonexistent__"),
    }
}

/// 场景 5（Issue #761 评论 5828969186 问题 2）：空远端首次同步必须走 batch 路径。
///
/// 首次同步没有 merge_outcome（远端没有 visible generation 可 merge），
/// 但 generation 发布仍必须是一次 `commit_batch`：
/// - `caps.batch == true` 时不得退回 `run_single_target()` 的逐文件 Contents API；
/// - staging 的本地 manifest 里所有 upsert 都是 Put（含 UTF-8 正文原字节）；
/// - manifest + `generation.meta.json`（complete=true）在同一批提交；
/// - CAS 成功，catalog 指向这次 batch 发布的 generation，正文按原字节可读回。
#[test]
#[allow(clippy::too_many_lines)]
fn regression_issue_761_first_sync_empty_remote_uses_batch_without_file_writes() {
    let tmp = TempDir::new().unwrap();
    let provider = RecordingProvider::new(MemoryProvider::new());

    let utf8_path = "volumes/v1/chapters/c1/chapter.md";
    let utf8_content = "你好，世界".as_bytes();
    let staging_root = build_staging(
        &tmp,
        &[(utf8_path, utf8_content), (SAFE_PATH, SAFE_CONTENT)],
        T,
        DEVICE_LOCAL,
    );
    let plan = build_plan(
        &tmp,
        staging_root,
        T,
        DEVICE_LOCAL,
        empty_remote_catalog_snapshot(),
    );
    let transfer = run_transfer(&provider, &plan, None, None, None);
    assert_eq!(transfer.targets.len(), 1);

    // ── 首次同步也必须是一次批量提交，不得逐文件 write() 到 generation prefix ──
    assert_eq!(
        provider.generation_file_writes(),
        0,
        "首次同步不得退回逐文件 Contents API 路径"
    );
    let batches = provider.recorded_batches();
    assert_eq!(
        batches.len(),
        1,
        "空远端首次同步应恰好一次 commit_batch，实际 {}",
        batches.len()
    );
    let batch = &batches[0];

    // ── 没有旧 generation 的 blob 可复用：所有 upsert 都是 Put，内容是 staging 原字节 ──
    match mutation_for(batch, utf8_path) {
        BatchMutation::Put { content, .. } => assert_eq!(
            content.as_slice(),
            utf8_content,
            "首次同步的 upsert 必须以原正文字节 Put"
        ),
        other => panic!("首次同步所有 upsert 都应是 Put，实际 {other:?}"),
    }
    match mutation_for(batch, SAFE_PATH) {
        BatchMutation::Put { content, .. } => assert_eq!(content, SAFE_CONTENT),
        other => panic!("首次同步所有 upsert 都应是 Put，实际 {other:?}"),
    }
    assert!(
        batch
            .mutations
            .iter()
            .all(|m| !matches!(m, BatchMutation::ReuseVersion { .. })),
        "首次同步没有旧 blob 可复用，不应出现 ReuseVersion"
    );

    // ── manifest 与 meta(complete=true) 同一批 ──
    match mutation_for(batch, SYNC_MANIFEST_PATH) {
        BatchMutation::Put { content, .. } => {
            let manifest: SyncManifest = serde_json::from_slice(content).unwrap();
            let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
            assert!(
                paths.contains(&utf8_path) && paths.contains(&SAFE_PATH),
                "首次同步 manifest 应包含全部 upsert 路径，实际 {paths:?}"
            );
        }
        other => panic!("manifest 必须是 Put，实际 {other:?}"),
    }
    let new_gen_prefix = assert_meta_complete_true(batch);
    assert!(
        new_gen_prefix.contains(GENERATION_SUBDIR),
        "meta 必须落在 generation prefix，实际 {new_gen_prefix}"
    );

    // ── 发布成功：catalog 指向这次 batch 的 generation，正文按原字节存在 ──
    let status = &transfer.targets[0].result.status;
    assert!(
        matches!(status, SyncStatus::LatestWinsApplied),
        "首次同步发布后应是 LatestWinsApplied，实际 {status:?}"
    );
    let catalog_after = load_remote_catalog(&provider).unwrap();
    let record = find_record(&catalog_after.catalog, PROJECT_PREFIX).expect("catalog record");
    let gen_id = record
        .active_generation
        .as_deref()
        .expect("首次同步应写入 active_generation");
    assert_eq!(
        format!("{PROJECT_PREFIX}/{GENERATION_SUBDIR}/{gen_id}"),
        new_gen_prefix,
        "catalog 应指向同一次 batch 发布的 generation"
    );

    let published = provider
        .read(&format!("{new_gen_prefix}/{utf8_path}"))
        .unwrap()
        .expect("generation 内应有正文对象");
    assert_eq!(
        published.content, utf8_content,
        "generation 内正文必须是原始 UTF-8 字节"
    );

    eprintln!(
        "[BUGFIX_REGRESSION_TRACE] Issue #761 空远端首次同步：commit_batch 1 次、\
         逐文件 generation write {} 次、generation {}（全部 upsert 走 Put）",
        provider.generation_file_writes(),
        gen_id
    );
}

/// 场景 6（Issue #761 评论 5828969186 问题 4）：branch ref CAS 冲突进入重试，不是 fatal。
///
/// 远端已有 visible generation（因此 generation 发布必然走 batch 路径），
/// 第一次 `commit_batch` 模拟另一台设备在本次提交期间推进 branch（ref PATCH 409/422）：
/// - `PreconditionFailed` → Core `SyncRemoteError(category=precondition_failed)`；
/// - transfer 必须重新 `load_remote_catalog` 后 continue `MAX_CAS_RETRIES` 循环；
/// - 重试轮次看到并发设备更新的 record（LWW 更大）→ 收敛，不再发布第二次；
/// - 最终状态不得是 `FatalError`（修复前这里会直接打成 fatal）。
#[test]
fn regression_issue_761_ref_cas_conflict_retries_instead_of_fatal() {
    let tmp = TempDir::new().unwrap();
    let inner = MemoryProvider::new();
    // 远端已有 visible generation（与场景 1 同一 fixture：CONFLICT_PATH 冲突、
    // SAFE_PATH 为本地新文件 → candidate (T, DEVICE_LOCAL) 严格赢，必须 publish）。
    let remote_gen_prefix = format!("{PROJECT_PREFIX}/{GENERATION_SUBDIR}/{GEN_EXISTING}");
    write_remote_generation(
        &inner,
        &remote_gen_prefix,
        &[(CONFLICT_PATH, REMOTE_CONFLICT_CONTENT)],
        T,
        DEVICE_REMOTE,
    );
    let provider = RecordingProvider::with_concurrent_ref_advance(
        inner,
        PROJECT_PREFIX,
        T + 1000,
        "device_other",
    );
    let catalog_snapshot = seed_remote_catalog(&provider, T - 1, DEVICE_REMOTE, GEN_EXISTING);

    let staging_root = build_staging(
        &tmp,
        &[
            (CONFLICT_PATH, LOCAL_CONFLICT_CONTENT),
            (SAFE_PATH, SAFE_CONTENT),
        ],
        T,
        DEVICE_LOCAL,
    );
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, catalog_snapshot);
    let transfer = run_transfer(&provider, &plan, None, None, None);
    assert_eq!(transfer.targets.len(), 1);

    let status = &transfer.targets[0].result.status;
    assert!(
        !matches!(
            status,
            SyncStatus::FatalError(_) | SyncStatus::RecoverableError(_)
        ),
        "ref CAS 冲突是正常竞争，应重试后收敛，实际 {status:?}（error={:?}）",
        transfer.targets[0].result.error
    );

    // 第一次 commit_batch 撞冲突后重读 catalog，看到并发设备更新的 record
    // （LWW 更大）→ candidate 不赢，收敛且不再 publish。
    assert_eq!(
        provider.recorded_batches().len(),
        1,
        "重试轮次应看到更新的远端 record 并收敛，不再 publish"
    );

    // 并发设备的 record 未被覆盖（force=false 语义：绝不覆盖别人刚提交的 head）。
    let catalog_after = load_remote_catalog(&provider).unwrap();
    let record = find_record(&catalog_after.catalog, PROJECT_PREFIX).expect("catalog record");
    assert_eq!(record.device_id, "device_other");
    assert_eq!(record.updated_at_ms, T + 1000);

    eprintln!(
        "[BUGFIX_REGRESSION_TRACE] Issue #761 ref CAS 冲突：commit_batch 1 次即收敛、\
         状态 {status:?}（不再是 FatalError）"
    );
}

/// 场景 7（Issue #761 评论 5829270182）：真实首次同步无 manifest 时走 batch 路径。
///
/// 与场景 5（`regression_issue_761_first_sync_empty_remote_uses_batch_without_file_writes`）
/// 的关键区别：场景 5 用 `build_staging()` helper，该 helper **手工先写了
/// manifest.sync.json**（见 `write_staging_manifest`），这不是真实首次同步的状态。
///
/// 本测试用正式 `StagingRun::create` + `seed_from_live` 从 live seed staging：
/// 1. 建 live project，只写 project.json、卷章正文等正常作品文件；
/// 2. **明确不创建** `app-meta/sync/manifest.sync.json`；
/// 3. `seed_from_live` 只复制 live 已存在文件，不会凭空创建 manifest → staging 里
///    同样没有 manifest.sync.json；
/// 4. 空远端 catalog；
/// 5. 跑 `run_transfer()`；
/// 6. 修复后行为：`transfer_live_project` 中远端无 visible source → 调用
///    `materialize_local_snapshot_for_empty_remote` 把本地完整快照 materialize 成
///    staging 的 manifest + state，返回 `Some(LwwMergeOutcome)`，后续
///    `publish_generation_batch` 走 batch 路径，所有 upsert 都是 Put，
///    manifest + meta(complete=true) 同一批提交，CAS 成功后 catalog 指向新 generation。
#[test]
#[allow(clippy::too_many_lines)]
#[allow(clippy::cognitive_complexity)]
fn regression_issue_761_real_first_sync_no_manifest_device_id_consistent() {
    let tmp = TempDir::new().unwrap();
    let provider = RecordingProvider::new(MemoryProvider::new());

    // 1. 建 live project，只写正常作品文件，明确不创建 manifest.sync.json。
    let live_root = tmp.path().join("projects").join("p1");
    let project_json_path = live_root.join("project.json");
    std::fs::create_dir_all(project_json_path.parent().unwrap()).unwrap();
    std::fs::write(
        &project_json_path,
        br#"{"id":"p1","title":"first-sync-test","volumes":[]}"#,
    )
    .unwrap();

    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    let chapter_path = live_root.join(chapter_rel);
    std::fs::create_dir_all(chapter_path.parent().unwrap()).unwrap();
    let chapter_content = b"first chapter content";
    std::fs::write(&chapter_path, chapter_content).unwrap();

    // 真实首次同步：live project 不应有 manifest.sync.json。
    let live_manifest_path = live_root.join(SYNC_MANIFEST_PATH);
    assert!(
        !live_manifest_path.exists(),
        "真实首次同步：live project 不应有 manifest.sync.json"
    );

    // 2. 用正式 StagingRun::create + seed_from_live 从 live seed staging。
    //    seed_from_live 只复制 live 已存在文件，不会凭空创建 manifest。
    let staging_run = writer_core::sync::staging::StagingRun::create(tmp.path(), live_root.clone())
        .expect("StagingRun::create");
    staging_run
        .seed_from_live(&live_root)
        .expect("seed_from_live");

    // staging 里同样没有 manifest.sync.json（seed_from_live 只复制 live 已存在文件）。
    let staging_manifest_path = staging_run.staging_root().join(SYNC_MANIFEST_PATH);
    assert!(
        !staging_manifest_path.exists(),
        "seed_from_live 不应凭空创建 manifest.sync.json — \
         真实首次同步 staging 里没有 manifest"
    );

    // 3. 构造 FullSyncPlan（LiveProject）。
    let planned = PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: live_root.clone(),
        staging_root: Some(staging_run.staging_root()),
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: live_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 0,
            device_id: DEVICE_LOCAL.to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: empty_remote_catalog_snapshot(),
    };

    // 4. 空远端 catalog，跑 run_transfer。
    let transfer = run_transfer(&provider, &plan, None, None, None);
    assert_eq!(transfer.targets.len(), 1);

    // 5. 修复后行为：target 不应是 RecoverableError，应成功走 batch 路径。
    let status = &transfer.targets[0].result.status;
    assert!(
        !matches!(status, SyncStatus::RecoverableError(_)),
        "真实首次同步无 manifest 时修复后不应返回 RecoverableError，实际: {status:?}\
         （error={:?}）",
        transfer.targets[0].result.error
    );

    // 6. generation provider.write() 次数 = 0（只走 batch，不逐文件 write）。
    assert_eq!(
        provider.generation_file_writes(),
        0,
        "真实首次同步应走 batch 路径，不应逐文件 write() 到 generation prefix"
    );

    // 7. 只走 batch：恰好一次 commit_batch。
    let batches = provider.recorded_batches();
    assert_eq!(
        batches.len(),
        1,
        "真实首次同步应恰好一次 commit_batch，实际 {} 次",
        batches.len()
    );
    let batch = &batches[0];

    // 8. staging 已生成 manifest（materialize_local_snapshot_for_empty_remote 写入）。
    let staging_manifest_after = staging_run.staging_root().join(SYNC_MANIFEST_PATH);
    assert!(
        staging_manifest_after.exists(),
        "修复后 staging 应已生成 manifest.sync.json"
    );

    // 9. staging SyncState 的 known_files 已包含正文/元数据。
    let state_after = writer_core::sync::SyncService::load_sync_state(&staging_run.staging_root())
        .expect("load_sync_state after transfer");
    assert!(
        state_after.known_files.contains_key(chapter_rel),
        "staging SyncState 的 known_files 应包含正文路径 {chapter_rel}，\
         实际 known_files={:?}",
        state_after.known_files.keys().collect::<Vec<_>>()
    );
    // device_id 一致性：staging state 必须使用平台注入的 DEVICE_LOCAL，
    // 不能在真实首次同步（无 state.local.json）时随机生成新 UUID。
    assert_eq!(
        state_after.device_id, DEVICE_LOCAL,
        "首次同步 staging state 的 device_id 必须是平台注入的 DEVICE_LOCAL，实际 {:?}",
        state_after.device_id
    );

    // 10. batch 内 upsert 全部是 Put（首次同步没有旧 blob 可复用）。
    assert!(
        batch
            .mutations
            .iter()
            .all(|m| !matches!(m, BatchMutation::ReuseVersion { .. })),
        "真实首次同步没有旧 blob 可复用，不应出现 ReuseVersion"
    );
    // 正文路径应是 Put 且内容是原字节。
    match mutation_for(batch, chapter_rel) {
        BatchMutation::Put { content, .. } => assert_eq!(
            content.as_slice(),
            chapter_content,
            "真实首次同步的正文 upsert 必须以原字节 Put"
        ),
        other => panic!("真实首次同步正文路径应是 Put，实际 {other:?}"),
    }

    // 11. manifest 同一批提交，且包含正文路径。
    match mutation_for(batch, SYNC_MANIFEST_PATH) {
        BatchMutation::Put { content, .. } => {
            let manifest: SyncManifest = serde_json::from_slice(content).unwrap();
            let paths: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
            assert!(
                paths.contains(&chapter_rel),
                "merged manifest 应包含正文路径，实际 {paths:?}"
            );
            // device_id 一致性：manifest upsert records 与 staging state 必须同源。
            // 首次同步生成的 manifest upsert record 的 device_id 必须等于平台注入的
            // DEVICE_LOCAL（也即 state_after.device_id），不能是随机 UUID。
            for rec in &manifest.files {
                if rec.op == "upsert" {
                    assert_eq!(
                        rec.device_id, state_after.device_id,
                        "首次同步 manifest upsert record 的 device_id 必须与 staging state 一致，\
                         path={} manifest device_id={} state device_id={}",
                        rec.path, rec.device_id, state_after.device_id
                    );
                }
            }
        }
        other => panic!("manifest 必须是 Put，实际 {other:?}"),
    }

    // 12. meta 直接 complete=true。
    let new_gen_prefix = assert_meta_complete_true(batch);
    let new_gen_id = new_gen_prefix.rsplit('/').next().expect("generation id");

    // 13. catalog 最终指向新 generation。
    let catalog_after = load_remote_catalog(&provider).unwrap();
    let record = find_record(&catalog_after.catalog, PROJECT_PREFIX).expect("catalog record");
    assert_eq!(
        record.active_generation.as_deref(),
        Some(new_gen_id),
        "CAS 成功后 catalog 应指向新 generation"
    );

    // 14. 状态应是 LatestWinsApplied（首次同步发布了内容）。
    assert!(
        matches!(status, SyncStatus::LatestWinsApplied),
        "首次同步发布后应是 LatestWinsApplied，实际 {status:?}"
    );

    eprintln!(
        "[BUGFIX_REGRESSION_TRACE] Issue #761 真实首次同步无 manifest 修复后：\
         commit_batch {} 次、逐文件 generation write {} 次、generation {}（全部 upsert 走 Put）、\
         staging 已生成 manifest、known_files 包含正文、catalog 指向新 generation",
        batches.len(),
        provider.generation_file_writes(),
        new_gen_id
    );
}
