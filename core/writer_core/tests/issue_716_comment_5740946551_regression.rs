//! Issue #716 评论 5740946551 — `transfer_live_project` publish 前移 LWW 判定回归测试。
//!
//! 覆盖 5 个场景：
//! 1. 不重复 publish：remote Upsert 同时间戳严格赢 → publish_count == 0，返回 NoChanges。
//! 2. candidate 严格赢仍 publish：remote record 时间更小 → candidate 严格赢 → publish_count == 1，CAS Applied，返回 Success。
//! 3. 远端无 record 仍 publish：catalog 无该 target record → publish_count == 1，CAS Applied。
//! 4. CAS 期间 snapshot 变化才下一轮：publish 前判定 candidate 赢，但 CAS 返回 RemoteWinner(Upsert)
//!    → 下一轮重新 merge + 判定，candidate 不赢 → 不再 publish → 返回 NoChanges。publish_count == 1。
//! 5. RemoteWinner(Delete) 语义不变：remote record op == Delete 且严格赢 → 清远端对象，不 publish generation。
//!
//! Issue #716 评论 5741695768 追加 2 个场景：
//! 6. remote Upsert 严格赢 + 本轮 merge 产生 unresolved conflict → publish_count == 0，PartialConflict，冲突列表非空。
//! 7. remote Upsert 严格赢 + merge 返回错误 → publish_count == 0，保持错误/RecoverableError，绝不是 NoChanges。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tempfile::TempDir;
use writer_core::sync::full_sync::{run_transfer, FullSyncPlan, LiveTargetLww, PlannedTarget};
use writer_core::sync::provider::capabilities::SyncCapabilities;
use writer_core::sync::provider::error::ProviderError;
use writer_core::sync::provider::memory::MemoryProvider;
use writer_core::sync::provider::model::{
    DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
};
use writer_core::sync::provider::SyncProvider;
use writer_core::sync::target_lifecycle::{
    load_remote_catalog, upsert_record, write_remote_catalog, TARGET_CATALOG_REMOTE_PATH,
};
use writer_core::sync::types::{
    ManifestFileRecord, PlannedTargetKind, RemoteTargetCatalogSnapshot, SyncManifest, SyncPolicy,
    SyncStatus, SyncTarget, TargetLifecycleCatalog, TargetLifecycleRecord,
};

/// `__generations__` 子目录名 — 与 `full_sync::generation::GENERATION_SUBDIR` 一致。
const GENERATION_SUBDIR: &str = "__generations__";
/// `generation.meta.json` 文件名 — 与 `generation_gc::GENERATION_META_FILENAME` 一致。
const GENERATION_META_FILENAME: &str = "generation.meta.json";

/// 计数对 `__generations__` 路径 write 的 mock provider。
struct CountingProvider {
    inner: MemoryProvider,
    generation_writes: AtomicUsize,
    generation_meta_writes: AtomicUsize,
}

impl CountingProvider {
    fn new(inner: MemoryProvider) -> Self {
        Self {
            inner,
            generation_writes: AtomicUsize::new(0),
            generation_meta_writes: AtomicUsize::new(0),
        }
    }

    fn publish_count(&self) -> usize {
        self.generation_meta_writes.load(Ordering::SeqCst) / 2
    }

    fn generation_writes(&self) -> usize {
        self.generation_writes.load(Ordering::SeqCst)
    }
}

impl SyncProvider for CountingProvider {
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
            self.generation_writes.fetch_add(1, Ordering::SeqCst);
            if path.ends_with(GENERATION_META_FILENAME) {
                self.generation_meta_writes.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.inner.write(path, content, precondition)
    }
    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        self.inner.delete(path, precondition)
    }
}

/// 场景 4 专用：第一次 IfMatch 写 catalog 时注入冲突（写入更新的 record 后返回 PreconditionFailed）。
struct ConflictInjectingProvider {
    inner: MemoryProvider,
    generation_writes: AtomicUsize,
    generation_meta_writes: AtomicUsize,
    conflict_injected: AtomicBool,
    conflict_catalog: Vec<u8>,
}

impl ConflictInjectingProvider {
    fn new(inner: MemoryProvider, conflict_catalog: Vec<u8>) -> Self {
        Self {
            inner,
            generation_writes: AtomicUsize::new(0),
            generation_meta_writes: AtomicUsize::new(0),
            conflict_injected: AtomicBool::new(false),
            conflict_catalog,
        }
    }

    fn publish_count(&self) -> usize {
        self.generation_meta_writes.load(Ordering::SeqCst) / 2
    }
}

impl SyncProvider for ConflictInjectingProvider {
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
        if path == TARGET_CATALOG_REMOTE_PATH
            && !self.conflict_injected.load(Ordering::SeqCst)
            && matches!(precondition, WritePrecondition::IfMatch(_))
        {
            // 注入冲突：先用 Unconditional 写入更新的 catalog，模拟远端被其他设备写入。
            self.inner
                .write(
                    path,
                    &self.conflict_catalog,
                    WritePrecondition::Unconditional,
                )
                .ok();
            self.conflict_injected.store(true, Ordering::SeqCst);
            return Err(ProviderError::PreconditionFailed {
                path: path.to_string(),
                reason: "injected conflict for scenario 4".to_string(),
            });
        }
        if path.contains(GENERATION_SUBDIR) {
            self.generation_writes.fetch_add(1, Ordering::SeqCst);
            if path.ends_with(GENERATION_META_FILENAME) {
                self.generation_meta_writes.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.inner.write(path, content, precondition)
    }
    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        self.inner.delete(path, precondition)
    }
}

/// 构造本地 staging：manifest LWW = (lww_time, device_id)，有一个 chapter 文件。
fn build_staging(tmp: &TempDir, lww_time: i64, device_id: &str) -> std::path::PathBuf {
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("volumes").join("v1")).unwrap();
    let chapter_content = b"chapter content";
    std::fs::write(
        staging_root.join("volumes").join("v1").join("chapter.md"),
        chapter_content,
    )
    .unwrap();
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    let staging_manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(chapter_content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();
    staging_root
}

/// 构造 PlannedTarget (LiveProject) 和 FullSyncPlan。
fn build_plan(
    tmp: &TempDir,
    staging_root: std::path::PathBuf,
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

/// 场景 1：不重复 publish — remote Upsert 同时间戳严格赢 → publish_count == 0，NoChanges。
///
/// 远端 generation 有内容，但 remote 严格赢，merge 不会产生任何变化（本地 staging 为空）。
#[test]
fn regression_no_redundant_publish_remote_upsert_strictly_wins() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();
    // 写入远端 generation 内容
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    // 本地 staging 为空（没有本地文件），remote 严格赢，merge 不会产生任何变化
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    // 写入空 manifest
    let staging_manifest = SyncManifest { files: vec![] };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Upsert 同时间戳严格赢时不应 publish"
    );
    assert_eq!(provider.generation_writes(), 0);
    // merge 下载了远端内容，应返回 LatestWinsApplied（不是 NoChanges）
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::LatestWinsApplied
        ),
        "merge 下载了远端正文应返回 LatestWinsApplied，实际 {:?}",
        transfer.targets[0].result.status
    );
    assert!(
        !transfer.targets[0].result.downloaded_files.is_empty(),
        "downloaded_files 不应为空"
    );
}

/// 场景 2：candidate 严格赢仍 publish — remote record 时间更小 → publish_count == 1，Success。
#[test]
fn regression_candidate_strictly_wins_publishes_once() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";

    let provider = CountingProvider::new(MemoryProvider::new());
    // remote record 时间更小（T-1 < T）→ candidate 严格赢
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T - 1, DEVICE_REMOTE)
            .with_active_generation("gen_existing");
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider).unwrap();

    let tmp = TempDir::new().unwrap();
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        1,
        "candidate 严格赢时应 publish 恰好 1 次"
    );
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::Success | SyncStatus::LatestWinsApplied
        ),
        "CAS Applied 应返回 Success 或 LatestWinsApplied，实际 {:?}",
        transfer.targets[0].result.status
    );
}

/// 场景 3：远端无 record 仍 publish — catalog 无该 target record → publish_count == 1，Success。
#[test]
fn regression_no_remote_record_publishes_once() {
    const T: i64 = 10_000;
    const DEVICE_LOCAL: &str = "device_local";

    let provider = CountingProvider::new(MemoryProvider::new());
    // 远端 catalog 为空（无该 target record）
    let remote_catalog_snapshot = load_remote_catalog(&provider).unwrap();

    let tmp = TempDir::new().unwrap();
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        1,
        "远端无 record 时应 publish 恰好 1 次"
    );
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::Success | SyncStatus::LatestWinsApplied
        ),
        "CAS Applied 应返回 Success 或 LatestWinsApplied，实际 {:?}",
        transfer.targets[0].result.status
    );
}

/// 场景 4：CAS 期间 snapshot 变化才下一轮 — publish 前判定 candidate 赢，CAS 返回 RemoteWinner(Upsert)
/// → 下一轮重新 merge + 判定，candidate 不赢 → 不再 publish → LatestWinsApplied。publish_count == 1。
#[test]
fn regression_cas_conflict_retries_only_when_snapshot_changed() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();
    // 初始 remote catalog：Upsert(T-1, device_remote) → candidate (T, device_local) 严格赢
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T - 1, DEVICE_REMOTE)
            .with_active_generation("gen_existing");
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    // 注入冲突时写入的 catalog：Upsert(T+1, device_remote) → 严格赢 candidate (T, device_local)
    let conflict_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T + 1, DEVICE_REMOTE)
            .with_active_generation("gen_after_conflict");
    let mut conflict_catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut conflict_catalog, conflict_record);
    let conflict_catalog_bytes = serde_json::to_vec(&conflict_catalog).unwrap();

    // 在 gen_after_conflict 下写入内容，这样第二轮 merge 会下载新内容
    let conflict_gen_prefix = "projects/p1/__generations__/gen_after_conflict";
    write_remote_generation(
        &provider_inner,
        conflict_gen_prefix,
        b"conflict chapter content",
        T + 1,
        DEVICE_REMOTE,
    );

    let provider = ConflictInjectingProvider::new(provider_inner, conflict_catalog_bytes);

    let tmp = TempDir::new().unwrap();
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        1,
        "第一轮 candidate 赢 publish 1 次，第二轮 candidate 不赢不 publish，总计 1 次"
    );
    // 第二轮 candidate 不赢 → RemoteWins(Upsert)，merge 下载了远端新内容 → LatestWinsApplied
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::LatestWinsApplied
        ),
        "第二轮 merge 下载了远端新内容应返回 LatestWinsApplied，实际 {:?}",
        transfer.targets[0].result.status
    );
}

/// 场景 5：RemoteWinner(Delete) 语义不变 — remote record op == Delete 且严格赢
/// → 清远端对象，不 publish generation。
#[test]
fn regression_remote_winner_delete_semantics_unchanged() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider = CountingProvider::new(MemoryProvider::new());
    // remote record: Delete(T, device_remote)，device_remote > device_local → 严格赢
    let remote_record =
        TargetLifecycleRecord::delete("projects/p1", "projects/p1", T, DEVICE_REMOTE);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider).unwrap();

    let tmp = TempDir::new().unwrap();
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Delete 严格赢时不应 publish generation"
    );
    // RemoteWins(Delete) → 清远端对象 + LocalLifecycleCommitAction::DeleteProject
    // 返回状态应为 NoChanges（cleanup 成功，无 retained_conflict）
    assert!(
        matches!(transfer.targets[0].result.status, SyncStatus::NoChanges),
        "Delete cleanup 成功应返回 NoChanges，实际 {:?}",
        transfer.targets[0].result.status
    );
    // 验证 LocalLifecycleCommitAction 是 DeleteProject
    let action = &transfer.targets[0].local_lifecycle_action;
    assert!(
        matches!(
            action,
            writer_core::sync::types::LocalLifecycleCommitAction::DeleteProject { .. }
        ),
        "应返回 DeleteProject action，实际 {:?}",
        action
    );
}

/// 场景 7 专用：在读取特定路径时返回错误的 provider。
///
/// 包装 MemoryProvider，当 `read(path)` 匹配 `fail_read_path` 时返回 `ProviderError::Network`。
/// 用于测试 merge 读远端 manifest 失败时，merge_outcome 归一化是否立即返回错误，
/// 而不是被 RemoteWins(Upsert)/AlreadyCurrent 吞成 NoChanges。
struct FailingReadProvider {
    inner: MemoryProvider,
    generation_writes: AtomicUsize,
    generation_meta_writes: AtomicUsize,
    fail_read_path: String,
}

impl FailingReadProvider {
    fn new(inner: MemoryProvider, fail_read_path: String) -> Self {
        Self {
            inner,
            generation_writes: AtomicUsize::new(0),
            generation_meta_writes: AtomicUsize::new(0),
            fail_read_path,
        }
    }

    fn publish_count(&self) -> usize {
        self.generation_meta_writes.load(Ordering::SeqCst) / 2
    }
}

impl SyncProvider for FailingReadProvider {
    fn capabilities(&self) -> SyncCapabilities {
        self.inner.capabilities()
    }
    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        self.inner.list(prefix)
    }
    fn read(&self, path: &str) -> Result<Option<RemoteObject>, ProviderError> {
        if path == self.fail_read_path {
            return Err(ProviderError::Network {
                reason: "injected read failure for scenario 7".to_string(),
            });
        }
        self.inner.read(path)
    }
    fn write(
        &self,
        path: &str,
        content: &[u8],
        precondition: WritePrecondition,
    ) -> Result<RemoteVersion, ProviderError> {
        if path.contains(GENERATION_SUBDIR) {
            self.generation_writes.fetch_add(1, Ordering::SeqCst);
            if path.ends_with(GENERATION_META_FILENAME) {
                self.generation_meta_writes.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.inner.write(path, content, precondition)
    }
    fn delete(&self, path: &str, precondition: DeletePrecondition) -> Result<(), ProviderError> {
        self.inner.delete(path, precondition)
    }
}

/// 构造本地 staging（正文冲突版）：manifest LWW = (lww_time, device_id)，
/// 有一个 `volumes/v1/chapters/chapter.md` 文件（UserTextDocument 路径）。
///
/// 用 `volumes/v1/chapters/chapter.md` 而非 `volumes/v1/chapter.md`，
/// 因为 `classify_content_path` 对含 `/chapters/` 的 .md 路径返回 `UserTextDocument`，
/// 走三路比较，双方内容不同时产生 `BothChanged` 冲突。
fn build_staging_doc_conflict(
    tmp: &TempDir,
    lww_time: i64,
    device_id: &str,
    chapter_content: &[u8],
) -> std::path::PathBuf {
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("volumes").join("v1").join("chapters")).unwrap();
    std::fs::write(
        staging_root
            .join("volumes")
            .join("v1")
            .join("chapters")
            .join("chapter.md"),
        chapter_content,
    )
    .unwrap();
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    let staging_manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapters/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(chapter_content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();
    staging_root
}

/// 在远端 generation 放一个 visible source（manifest + chapter 文件）。
///
/// `gen_prefix` 形如 `projects/p1/__generations__/gen_existing`。
/// 放完后 merge 能从该 generation 读到远端 manifest 和 chapter。
fn write_remote_generation(
    provider: &dyn SyncProvider,
    gen_prefix: &str,
    chapter_content: &[u8],
    lww_time: i64,
    device_id: &str,
) {
    let chapter_path = format!("{}/volumes/v1/chapters/chapter.md", gen_prefix);
    provider
        .write(
            &chapter_path,
            chapter_content,
            WritePrecondition::Unconditional,
        )
        .unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapters/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(chapter_content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    let manifest_path = format!("{}/app-meta/sync/manifest.sync.json", gen_prefix);
    provider
        .write(
            &manifest_path,
            &serde_json::to_vec(&manifest).unwrap(),
            WritePrecondition::Unconditional,
        )
        .unwrap();
}

/// 场景 6（Issue #716 评论 5741695768）：remote Upsert 严格赢 + 本轮 merge 产生 unresolved conflict
/// → publish_count == 0，PartialConflict，冲突列表非空。
///
/// 修复前：merge_outcome 只在 CandidateWins 分支消费，RemoteWins(Upsert) 直接返回
/// `retained_conflict.unwrap_or(NoChanges)` = NoChanges，冲突被丢失。
/// 修复后：merge_outcome 在 lifecycle winner 比较前归一化，retained_conflict 先被
/// 更新为 PartialConflict，RemoteWins(Upsert) 返回 `retained_conflict.unwrap_or(NoChanges)` = PartialConflict。
#[test]
fn regression_remote_upsert_wins_with_merge_conflict_preserves_partial_conflict() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放一个 visible source（chapter 内容与本地不同 → BothChanged 冲突）
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(T, DEVICE_REMOTE), active_generation=GEN_EXISTING
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    // 本地 staging：chapter 内容与远端不同 → BothChanged 冲突
    let staging_root = build_staging_doc_conflict(&tmp, T, DEVICE_LOCAL, b"local chapter content");
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Upsert 严格赢时不应 publish"
    );
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::PartialConflict
        ),
        "merge 产生冲突 + remote Upsert 严格赢应返回 PartialConflict，实际 {:?}",
        transfer.targets[0].result.status
    );
    assert!(
        !transfer.targets[0].result.conflicts.is_empty(),
        "冲突列表不应为空，实际 {:?}",
        transfer.targets[0].result.conflicts
    );
}

/// 场景 7（Issue #716 评论 5741695768）：remote Upsert 严格赢 + merge 返回错误
/// → publish_count == 0，保持错误/RecoverableError，绝不是 NoChanges。
///
/// 修复前：merge_outcome 只在 CandidateWins 分支消费，RemoteWins(Upsert) 直接返回
/// `retained_conflict.unwrap_or(NoChanges)` = NoChanges，merge 错误被吞掉。
/// 修复后：merge_outcome 在 lifecycle winner 比较前归一化，Err(e) 立即返回
/// `sync_result_from_error(e)` → RecoverableError 或 FatalError。
#[test]
fn regression_remote_upsert_wins_with_merge_error_preserves_error() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放一个 visible source（manifest + chapter）
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(T, DEVICE_REMOTE), active_generation=GEN_EXISTING
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    // FailingReadProvider 在读取远端 generation 的 manifest 时返回错误
    let fail_path = format!("{}/app-meta/sync/manifest.sync.json", remote_gen_prefix);
    let provider = FailingReadProvider::new(provider_inner, fail_path);

    let tmp = TempDir::new().unwrap();
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "merge 错误应立即返回，不应 publish"
    );
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::RecoverableError(_) | SyncStatus::FatalError(_)
        ),
        "merge 返回错误应保持错误状态，绝不应是 NoChanges，实际 {:?}",
        transfer.targets[0].result.status
    );
}

/// 场景 8（Issue #716 评论 5742191453）：remote Upsert 严格赢 + merge 下载远端正文
/// → publish_count == 0，LatestWinsApplied，downloaded_files 非空。
///
/// 修复前：RemoteWins(Upsert) 分支返回 `retained_conflict.unwrap_or(NoChanges)` = NoChanges，
/// merge 下载的正文被丢弃，full-sync 统计报 0，搜索索引不重建。
/// 修复后：merge_outcome 归一化时构造 merge_result 携带变化字段，
/// RemoteWins(Upsert) 返回 merge_result → LatestWinsApplied + 非空 downloaded_files。
#[test]
fn remote_upsert_wins_with_downloaded_files_returns_latest_wins_applied() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放一个 visible source
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(T, DEVICE_REMOTE), active_generation=GEN_EXISTING
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    // 本地 staging 为空（没有本地文件），remote 严格赢，merge 会下载远端正文
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    // 写入空 manifest
    let staging_manifest = SyncManifest { files: vec![] };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Upsert 严格赢时不应 publish"
    );
    // 关键断言：结果应为 LatestWinsApplied，不是 NoChanges
    assert!(
        matches!(
            transfer.targets[0].result.status,
            SyncStatus::LatestWinsApplied
        ),
        "merge 下载了远端正文应返回 LatestWinsApplied，实际 {:?}",
        transfer.targets[0].result.status
    );
    // downloaded_files 非空，并包含对应 chapter 路径
    assert!(
        !transfer.targets[0].result.downloaded_files.is_empty(),
        "downloaded_files 不应为空，实际 {:?}",
        transfer.targets[0].result.downloaded_files
    );
    let expected_chapter_path = "volumes/v1/chapters/chapter.md";
    assert!(
        transfer.targets[0]
            .result
            .downloaded_files
            .iter()
            .any(|p| p.contains(expected_chapter_path)),
        "downloaded_files 应包含 {}，实际 {:?}",
        expected_chapter_path,
        transfer.targets[0].result.downloaded_files
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 场景 9/10/11 — Issue #716 评论 5742849844 三个问题复现测试
//
// 这三个测试断言当前 buggy 行为（测试会失败证明 bug 存在），不是验证正确行为。
// 修复后这些测试应改为断言正确行为，或删除并由新的正确行为测试替代。
// ════════════════════════════════════════════════════════════════════════════

/// 在 staging root 下写入 sync_state.json（旧格式，load_sync_state 会自动迁移）。
///
/// 用于测试中预设 `pending_take_remote` / `known_files` / `tombstones` 等 SyncState 字段。
fn write_sync_state(staging_root: &std::path::Path, state: &writer_core::sync::types::SyncState) {
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("sync_state.json"),
        serde_json::to_vec(state).unwrap(),
    )
    .unwrap();
}

/// 在远端 generation 写入一个 project.json 文件（Metadata，走 LWW 决胜）。
///
/// 与 `write_remote_generation`（写 chapter.md）对应，这里写 `project.json`
/// 用于构造 LwwLocalWinsDeleteRecord 场景。
fn write_remote_generation_project_json(
    provider: &dyn SyncProvider,
    gen_prefix: &str,
    project_json_content: &[u8],
    lww_time: i64,
    device_id: &str,
) {
    let project_json_path = format!("{}/project.json", gen_prefix);
    provider
        .write(
            &project_json_path,
            project_json_content,
            WritePrecondition::Unconditional,
        )
        .unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "project.json".to_string(),
            content_hash: format!("{:x}", md5::compute(project_json_content)),
            updated_at_ms: lww_time,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    let manifest_path = format!("{}/app-meta/sync/manifest.sync.json", gen_prefix);
    provider
        .write(
            &manifest_path,
            &serde_json::to_vec(&manifest).unwrap(),
            WritePrecondition::Unconditional,
        )
        .unwrap();
}

/// 场景 9（Issue #716 评论 5742849844 问题 1）：pending_take_remote_failed 被吞成成功。
///
/// 复现：remote Upsert 严格赢，`pending_take_remote` 指向远端不存在的文件，
/// publish_count == 0。`attempt.rs` 对 `pending_take_remote_failed` 非空有明确语义：
/// 返回 `RecoverableError`。但 `transfer_helpers.rs` 的 `merge_result` 完全没看
/// `pending_take_remote_failed`，在 RemoteWins(Upsert) 不 publish 路径里返回
/// `LatestWinsApplied`（因为同时下载了远端 generation 的 chapter.md）。
///
/// 当前 buggy 行为：返回 `LatestWinsApplied`（吞掉 pending_take_remote_failed）。
/// 正确行为：返回 `RecoverableError`。
#[test]
fn regression_scenario_9_pending_take_remote_failed_swallowed_as_success() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放一个 visible source（chapter 文件存在）
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(T, DEVICE_REMOTE), active_generation=GEN_EXISTING
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    // 本地 staging 为空（没有本地文件），remote 严格赢
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    let staging_manifest = SyncManifest { files: vec![] };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();

    // 关键：设置 pending_take_remote 指向远端 generation 中不存在的文件。
    // merge 会尝试下载，发现远端缺失 → pending_take_remote_failed 非空。
    // 但 merge_result 没看 pending_take_remote_failed，会吞成成功。
    let pending_missing_path = "volumes/v1/chapters/missing.md";
    let sync_state = writer_core::sync::types::SyncState {
        device_id: DEVICE_LOCAL.to_string(),
        pending_take_remote: std::collections::HashSet::from([pending_missing_path.to_string()]),
        ..Default::default()
    };
    write_sync_state(&staging_root, &sync_state);

    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Upsert 严格赢时不应 publish"
    );

    let status = &transfer.targets[0].result.status;
    eprintln!(
        "场景 9 实际状态: {:?}, downloaded_files: {:?}",
        status, transfer.targets[0].result.downloaded_files
    );

    // ── 断言正确行为（Issue #716 评论 5742849844 问题 1 已修复）──
    // pending_take_remote_failed 非空时必须返回 RecoverableError，
    // 不能吞成 LatestWinsApplied/NoChanges。
    assert!(
        matches!(status, SyncStatus::RecoverableError(_)),
        "pending_take_remote_failed 非空时应返回 RecoverableError，实际 {:?}",
        status
    );
    assert!(
        !transfer.targets[0].result.downloaded_files.is_empty(),
        "merge 仍应下载远端 generation 的 chapter.md，downloaded_files 不应为空，实际 {:?}",
        transfer.targets[0].result.downloaded_files
    );
}

/// 场景 10（Issue #716 评论 5742849844 问题 2）：CAS 重试丢掉前一轮本地变化。
///
/// 复现：第一轮 merge 下载远端文件 → publish 1 次 → CAS 注入新的 remote generation → continue。
/// 第二轮 generation 与当前 staging 内容相同，merge NoOp，remote lifecycle 仍严格赢。
/// 最终应 publish_count == 1，状态 LatestWinsApplied，保留第一轮的 downloaded_files。
/// 但当前 buggy 代码只看第二轮 merge_result（NoChanges），第一轮的 downloaded_files 消失。
///
/// 当前 buggy 行为：返回 NoChanges，downloaded_files 为空。
/// 正确行为：返回 LatestWinsApplied，downloaded_files 保留第一轮的下载，publish_count == 1。
#[test]
fn regression_scenario_10_cas_retry_drops_first_round_local_changes() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 初始 remote catalog: Upsert(T-1, DEVICE_REMOTE) → candidate (T, DEVICE_LOCAL) 严格赢
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T - 1, DEVICE_REMOTE)
            .with_active_generation("gen_existing");
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    // 远端 gen_existing 放一个 chapter 文件（内容与本地不同 → 第一轮 merge 下载）
    let remote_gen_prefix = "projects/p1/__generations__/gen_existing";
    write_remote_generation(
        &provider_inner,
        remote_gen_prefix,
        b"remote chapter content",
        T - 2,
        DEVICE_REMOTE,
    );

    // 注入冲突时写入的 catalog: Upsert(T+1, DEVICE_REMOTE) → 严格赢 candidate (T, DEVICE_LOCAL)
    let conflict_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T + 1, DEVICE_REMOTE)
            .with_active_generation("gen_after_conflict");
    let mut conflict_catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut conflict_catalog, conflict_record);
    let conflict_catalog_bytes = serde_json::to_vec(&conflict_catalog).unwrap();

    // gen_after_conflict 放相同内容的 chapter（与第一轮下载后 staging 内容相同 → 第二轮 merge NoOp）
    let conflict_gen_prefix = "projects/p1/__generations__/gen_after_conflict";
    write_remote_generation(
        &provider_inner,
        conflict_gen_prefix,
        b"remote chapter content",
        T - 2,
        DEVICE_REMOTE,
    );

    let provider = ConflictInjectingProvider::new(provider_inner, conflict_catalog_bytes);

    let tmp = TempDir::new().unwrap();
    // 本地 staging 有 chapter.md（内容 "chapter content"，时间戳 T）
    // 远端 gen_existing 有 volumes/v1/chapters/chapter.md（内容 "remote chapter content"，时间戳 T-2）
    // 第一轮 merge 下载远端 chapter → downloaded_files 非空
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        1,
        "第一轮 candidate 赢 publish 1 次，第二轮 candidate 不赢不 publish，总计 1 次"
    );

    let status = &transfer.targets[0].result.status;
    eprintln!(
        "场景 10 实际状态: {:?}, downloaded_files: {:?}",
        status, transfer.targets[0].result.downloaded_files
    );

    // ── 断言正确行为（Issue #716 评论 5742849844 问题 2 已修复）──
    // CAS 重试不应丢失前一轮的本地变化。最终状态应为 LatestWinsApplied，
    // downloaded_files 保留第一轮的下载。
    assert!(
        matches!(status, SyncStatus::LatestWinsApplied),
        "CAS 重试后应返回 LatestWinsApplied（保留第一轮本地变化），实际 {:?}",
        status
    );
    assert!(
        !transfer.targets[0].result.downloaded_files.is_empty(),
        "downloaded_files 应保留第一轮的下载（非空），实际 {:?}",
        transfer.targets[0].result.downloaded_files
    );
}

/// 场景 11（Issue #716 评论 5742849844 问题 3）：no-publish 路径 local_deletes 报告未执行的远端删除。
///
/// 复现：本地有 project.json 的 delete tombstone（LWW 本地赢），远端 generation 有 project.json
/// 的 upsert 记录。merge 产生 LwwLocalWinsDeleteRecord → remote_delete_paths 非空。
/// 但整体 lifecycle remote 严格赢 → RemoteWins(Upsert) → 不 publish。
/// `transfer_helpers.rs` 在 no-publish 路径写了 `r.local_deletes = outcome.remote_delete_paths.clone()`，
/// 把没执行的远端删除报成 local_deletes。
///
/// 当前 buggy 行为：local_deletes 非空（报告了未执行的远端删除）。
/// 正确行为：local_deletes 应为空（remote_delete_paths 是远端操作，merge 没执行本地删除）。
///
/// 时间戳设计（避免 30 天 delete 墓碑清理）：
/// - `now_ms`：当前毫秒时间戳
/// - `T_SMALL = now_ms - 200000`：远端 generation project.json upsert 时间戳
/// - `deleted_at = (now_ms - 100000) / 1000`（秒）→ `deleted_at_ms ≈ now_ms - 100000`
///   - `deleted_at_ms > T_SMALL` → LWW 本地赢 → LwwLocalWinsDeleteRecord
///   - `deleted_at_ms > purge_time`（30 天前）→ delete 记录不被清理
/// - `T_BIG = now_ms + 1000000`：remote catalog lww_time → remote 严格赢 candidate
#[test]
fn regression_scenario_11_no_publish_local_deletes_reports_unexecuted_remote_deletes() {
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let now_ms = chrono::Utc::now().timestamp_millis();
    let t_small = now_ms - 200_000; // 远端 upsert 时间戳
    let deleted_at_secs = (now_ms - 100_000) / 1000; // 秒 → deleted_at_ms ≈ now_ms - 100000
    let t_big = now_ms + 1_000_000; // remote catalog lww_time

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放 project.json（Metadata，走 LWW 决胜）
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    let project_json_content = br#"{"name":"test"}"#;
    write_remote_generation_project_json(
        &provider_inner,
        &remote_gen_prefix,
        project_json_content,
        t_small,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(t_big, DEVICE_REMOTE), active_generation=GEN_EXISTING
    // t_big > deleted_at_ms > t_small → remote lifecycle 严格赢 candidate
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", t_big, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("app-meta").join("sync")).unwrap();
    // 本地 staging manifest 为空（project.json 不在磁盘上）
    let staging_manifest = SyncManifest { files: vec![] };
    std::fs::write(
        staging_root
            .join("app-meta")
            .join("sync")
            .join("manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();

    // 关键：设置 known_files + tombstone，让 snapshot_local_records_read_only 生成 delete 记录。
    // deleted_at_ms = deleted_at_secs * 1000 ≈ now_ms - 100000 > t_small → LWW 本地赢
    // → LwwLocalWinsDeleteRecord → remote_delete_paths = ["project.json"]
    let original_hash = "some_hash_value";
    let sync_state = writer_core::sync::types::SyncState {
        device_id: DEVICE_LOCAL.to_string(),
        known_files: std::collections::HashMap::from([(
            "project.json".to_string(),
            original_hash.to_string(),
        )]),
        tombstones: vec![writer_core::sync::types::Tombstone {
            original_path: "project.json".to_string(),
            trash_path: "app-meta/sync/trash/deleted_project.json".to_string(),
            deleted_at: deleted_at_secs,
            purge_after: i64::MAX,
            deleted_by: DEVICE_LOCAL.to_string(),
            original_hash: original_hash.to_string(),
            kind: "local_delete".to_string(),
        }],
        ..Default::default()
    };
    write_sync_state(&staging_root, &sync_state);

    // candidate lww_time ≈ now_ms - 100000（delete 记录），remote record lww_time = t_big
    // candidate < remote → remote 严格赢 → RemoteWins(Upsert) → 不 publish
    let candidate_lww_time = deleted_at_secs * 1000;
    let plan = build_plan(
        &tmp,
        staging_root,
        candidate_lww_time,
        DEVICE_LOCAL,
        remote_catalog_snapshot,
    );

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Upsert 严格赢时不应 publish"
    );

    let status = &transfer.targets[0].result.status;
    eprintln!(
        "场景 11 实际状态: {:?}, local_deletes: {:?}, remote_deletes: {:?}",
        status, transfer.targets[0].result.local_deletes, transfer.targets[0].result.remote_deletes
    );

    // ── 断言正确行为（Issue #716 评论 5742849844 问题 3 已修复）──
    // no-publish 路径没有执行远端删除（既没 delete_remote_files，也没发布新 generation），
    // local_deletes 应为空。remote_delete_paths 是"调用方应从远端删除的路径"，
    // merge 本身没执行，不应报告为 local_deletes。
    assert!(
        transfer.targets[0].result.local_deletes.is_empty(),
        "no-publish 路径 local_deletes 应为空（未执行远端删除），实际 {:?}",
        transfer.targets[0].result.local_deletes
    );
}

// ════════════════════════════════════════════════════════════════════════════
// 场景 12/13 — Issue #716 评论 5743264448 两个漏口复现测试
// ════════════════════════════════════════════════════════════════════════════

/// 场景 12（Issue #716 评论 5743264448 问题 1）：CandidateWins + pending_take_remote_failed
/// → publish_count == 0，RecoverableError。
///
/// 复现：remote lifecycle 比 candidate 旧（CandidateWins 成立），同时 pending_take_remote
/// 指向远端不存在的文件。CandidateWins 分支不看 merge_result，直接 publish_generation，
/// 即使 pending_take_remote_failed 非空也吞掉继续 publish。
///
/// 正确行为：没有 unresolved conflict 且 pending_take_remote_failed 非空 → 直接返回
/// RecoverableError，不进入 CandidateWins publish。
#[test]
fn regression_scenario_12_candidate_wins_pending_take_remote_failed_no_publish() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放一个 visible source（chapter 文件存在）
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T - 2,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(T-1, DEVICE_REMOTE), active_generation=GEN_EXISTING
    // candidate (T, DEVICE_LOCAL) 严格赢 remote (T-1, DEVICE_REMOTE) → CandidateWins
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T - 1, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    // 本地 staging 有 chapter.md（LWW 时间 T > 远端 T-2 → 本地赢，无冲突）
    let staging_root = build_staging(&tmp, T, DEVICE_LOCAL);

    // 关键：设置 pending_take_remote 指向远端 generation 中不存在的文件。
    // merge 会尝试下载，发现远端缺失 → pending_take_remote_failed 非空。
    let pending_missing_path = "volumes/v1/chapters/missing.md";
    let sync_state = writer_core::sync::types::SyncState {
        device_id: DEVICE_LOCAL.to_string(),
        pending_take_remote: std::collections::HashSet::from([pending_missing_path.to_string()]),
        ..Default::default()
    };
    write_sync_state(&staging_root, &sync_state);

    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "CandidateWins + pending_take_remote_failed 不应 publish"
    );

    let status = &transfer.targets[0].result.status;
    eprintln!(
        "场景 12 实际状态: {:?}, downloaded_files: {:?}",
        status, transfer.targets[0].result.downloaded_files
    );

    // ── 断言正确行为（Issue #716 评论 5743264448 问题 1）──
    // 没有 unresolved conflict 且 pending_take_remote_failed 非空 → RecoverableError
    assert!(
        matches!(status, SyncStatus::RecoverableError(_)),
        "CandidateWins + pending_take_remote_failed 应返回 RecoverableError，实际 {:?}",
        status
    );
}

/// 场景 13（Issue #716 评论 5743264448 问题 2）：unresolved conflict + remote_delete_paths
/// + RemoteWins → PartialConflict，local_deletes 为空。
///
/// 复现：同一轮同时产生 unresolved conflict（chapter.md BothChanged）和 remote_delete_paths
/// （project.json 本地删除墓碑赢远端 upsert）。remote lifecycle 严格赢 → RemoteWins(Upsert)
/// → 不 publish。retained_conflict 被构造时写了 `r.local_deletes = outcome.remote_delete_paths.clone()`，
/// 把没执行的远端删除报成 local_deletes。
///
/// 正确行为：retained_conflict 的 local_deletes 应为空（merge 没执行远端删除）。
/// 只有 CandidateWins publish 且 CAS 成功后才合并 content_result.local_deletes。
#[test]
fn regression_scenario_13_conflict_plus_remote_delete_paths_no_publish_local_deletes_empty() {
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local";
    const GEN_EXISTING: &str = "gen_existing";
    assert!(DEVICE_LOCAL < DEVICE_REMOTE);

    let provider_inner = MemoryProvider::new();

    // 远端 generation 放 chapter.md（与本地内容不同 → BothChanged 冲突）
    let remote_gen_prefix = format!("projects/p1/__generations__/{}", GEN_EXISTING);
    write_remote_generation(
        &provider_inner,
        &remote_gen_prefix,
        b"remote chapter content",
        T,
        DEVICE_REMOTE,
    );

    // 远端 generation 还放 project.json（Metadata，走 LWW 决胜）
    let project_json_content = br#"{"name":"test"}"#;
    write_remote_generation_project_json(
        &provider_inner,
        &remote_gen_prefix,
        project_json_content,
        T - 5000,
        DEVICE_REMOTE,
    );

    // remote catalog: Upsert(T, DEVICE_REMOTE), active_generation=GEN_EXISTING
    // 同时间戳 T，DEVICE_REMOTE > DEVICE_LOCAL → remote 严格赢 candidate
    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider_inner, &snapshot).unwrap();
    let remote_catalog_snapshot = load_remote_catalog(&provider_inner).unwrap();

    let provider = CountingProvider::new(provider_inner);

    let tmp = TempDir::new().unwrap();
    // 本地 staging：chapter 内容与远端不同 → BothChanged 冲突
    let staging_root =
        build_staging_doc_conflict(&tmp, T, DEVICE_LOCAL, b"local chapter content");

    // 关键：设置 known_files + tombstone，让 snapshot_local_records_read_only 生成 delete 记录。
    // deleted_at_ms = 8_000 > T-5000 = 5_000 → LWW 本地赢 → LwwLocalWinsDeleteRecord
    // → remote_delete_paths = ["project.json"]
    let original_hash = "some_hash_value";
    let sync_state = writer_core::sync::types::SyncState {
        device_id: DEVICE_LOCAL.to_string(),
        known_files: std::collections::HashMap::from([(
            "project.json".to_string(),
            original_hash.to_string(),
        )]),
        tombstones: vec![writer_core::sync::types::Tombstone {
            original_path: "project.json".to_string(),
            trash_path: "app-meta/sync/trash/deleted_project.json".to_string(),
            deleted_at: 8, // 秒 → deleted_at_ms = 8_000 > T-5000 = 5_000
            purge_after: i64::MAX,
            deleted_by: DEVICE_LOCAL.to_string(),
            original_hash: original_hash.to_string(),
            kind: "local_delete".to_string(),
        }],
        ..Default::default()
    };
    write_sync_state(&staging_root, &sync_state);

    // candidate lww_time = T（chapter.md 的 LWW 时间），remote record lww_time = T
    // 同时间戳，DEVICE_REMOTE > DEVICE_LOCAL → remote 严格赢 → RemoteWins(Upsert) → 不 publish
    let plan = build_plan(&tmp, staging_root, T, DEVICE_LOCAL, remote_catalog_snapshot);

    let transfer = run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        provider.publish_count(),
        0,
        "remote Upsert 严格赢时不应 publish"
    );

    let status = &transfer.targets[0].result.status;
    eprintln!(
        "场景 13 实际状态: {:?}, local_deletes: {:?}, remote_deletes: {:?}, conflicts: {}",
        status,
        transfer.targets[0].result.local_deletes,
        transfer.targets[0].result.remote_deletes,
        transfer.targets[0].result.conflicts.len()
    );

    // ── 断言正确行为（Issue #716 评论 5743264448 问题 2）──
    // 有 unresolved conflict + remote_delete_paths + RemoteWins no-publish →
    // PartialConflict（冲突状态保留），但 local_deletes 必须为空（远端删除未执行）。
    assert!(
        matches!(status, SyncStatus::PartialConflict),
        "有 unresolved conflict + RemoteWins 应返回 PartialConflict，实际 {:?}",
        status
    );
    assert!(
        !transfer.targets[0].result.conflicts.is_empty(),
        "冲突列表不应为空，实际 {:?}",
        transfer.targets[0].result.conflicts
    );
    assert!(
        transfer.targets[0].result.local_deletes.is_empty(),
        "no-publish 路径 local_deletes 应为空（远端删除未执行），实际 {:?}",
        transfer.targets[0].result.local_deletes
    );
}
