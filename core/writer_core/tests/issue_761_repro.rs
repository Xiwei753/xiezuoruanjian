//! Issue #761 回归测试 — GitHub 同步改走 Git tree/commit/ref 原子发布。
//!
//! 覆盖两个核心契约（行为级，不是字符串常量断言）：
//!
//! 1. **批量原子发布**：`capabilities().batch == true` 时，一个 target 的一次
//!    generation 发布 = 恰好一次 `commit_batch`；manifest、内容 mutation 与
//!    `generation.meta.json`（直接 `complete=true`）在同一批提交里，
//!    不再逐文件 `provider.write()`。
//! 2. **冲突路径复用远端 blob**：unresolved conflict 的路径必须用
//!    `RemoteVersion`（旧 visible generation 的 blob）生成 `ReuseVersion`，
//!    绝不读本地冲突正文冒充 merged remote record；没有远端 blob 可复用时
//!    整个 target 返回 `PartialConflict`，不发布内容不完整的 generation。
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
        self.batches
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(RecordedBatch {
                message: message.to_string(),
                mutations: mutations.to_vec(),
            });
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
    let transfer = run_transfer(&provider, &plan, None);
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
    let transfer = run_transfer(&provider, &plan, None);
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
fn regression_issue_761_memory_commit_batch_is_single_transaction() {
    let provider =
        MemoryProvider::with_entries(vec![("source/a.txt".to_string(), b"hello".to_vec())]);
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

    // ReuseVersion 引用不存在的版本 → PreconditionFailed（远端无此 blob）。
    let err = provider
        .commit_batch(
            &[BatchMutation::ReuseVersion {
                path: "target/c.txt".to_string(),
                version: RemoteVersion::new("missing-version"),
            }],
            "missing blob",
        )
        .unwrap_err();
    assert!(matches!(err, ProviderError::PreconditionFailed { .. }));
    assert!(
        provider.read("target/c.txt").unwrap().is_none(),
        "失败的 ReuseVersion 不应写入半个对象"
    );
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
