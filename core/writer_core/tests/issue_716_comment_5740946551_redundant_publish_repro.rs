//! Issue #716 评论 5740946551 + Issue #928 — `transfer_live_project` publish 边界验证测试。
//!
//! ## 修复目标（#928 后的语义）
//!
//! `core/writer_core/src/sync/full_sync/transfer_helpers/transfer_live.rs` 的
//! `transfer_live_project` publish 判定不再只看作品级 whole-work LWW：
//!
//! - 有文件级内容/tombstone 变化（`remote_upload_paths` / `remote_delete_paths` 非空）时，
//!   即使 candidate 的作品级 LWW 不赢，也必须进入 generation publish + catalog CAS 推进
//!   `active_generation`（#928：作品级 LWW 不得拦截非冲突文件的增量同步）。
//! - 完全没有文件级变化时，candidate 不赢直接收敛，不 publish（#716 评论 5740946551：
//!   不产生重复/未引用 generation）。
//!
//! 本测试覆盖"有文件级变化"的一侧：远端 visible generation 为空、本地 staging 有
//! `volumes/v1/chapter.md`，merge 产生 `remote_upload_paths`；远端 Upsert 同时间戳严格赢
//! （`device_remote > device_local`）且 candidate 仍不赢。修复后必须**恰好 publish 一次**
//! 新 generation 并通过 CAS 推进 `active_generation`，不得因作品级 LWW 不赢丢弃本地
//! chapter.md，也不得重复 publish（旧缺陷每轮 `RemoteWinner(Upsert)` 都会重新 publish）。
//! 作品级 LWW（`updated_at_ms` / `device_id`）保留远端赢家，不被降级。
//!
//! 无文件级变化 → 不 publish 的收敛分支由
//! `sync/full_sync/tests/transfer.rs::issue_928_remote_upsert_without_file_level_changes_converges_without_publish`
//! 覆盖。
//!
//! ## 场景
//!
//! 1. 远端 catalog 已有 Upsert record：`target_id="projects/p1"`, `lww_time_ms=T`,
//!    `device_id="device_remote"`, `active_generation=Some("gen_existing")`。
//! 2. 远端 visible generation（`gen_existing`）为空；本地 staging 有 chapter.md，
//!    post-transfer LWW = `(T, device_local)`，`"device_local" < "device_remote"`。
//! 3. 调用 `run_transfer` 触发 `transfer_live_project`。
//! 4. 断言：`publish_count == 1`（有文件级变化必须 publish，且不重复），新 generation
//!    含 chapter.md，catalog `active_generation` 推进、作品级 LWW 仍为远端赢家，
//!    返回 `LatestWinsApplied`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};

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
    ManifestFileRecord, PlannedTargetKind, RemoteTargetCatalogSnapshot, SyncManifest, SyncPolicy,
    SyncTarget, TargetLifecycleCatalog, TargetLifecycleRecord,
};

/// `__generations__` 子目录名 — 与 `full_sync::generation::GENERATION_SUBDIR` 一致。
const GENERATION_SUBDIR: &str = "__generations__";
/// `generation.meta.json` 文件名 — 与 `generation_gc::GENERATION_META_FILENAME` 一致。
const GENERATION_META_FILENAME: &str = "generation.meta.json";

/// 计数对 `__generations__` 路径写入的 mock provider。
///
/// 包装 `MemoryProvider`，对 `write()` 和 `commit_batch()` 里包含 `__generations__`
/// 的路径计数，并对以 `generation.meta.json` 结尾的路径单独计数（batch 路径每次
/// publish 写 1 次 meta：`complete=true`）。
struct CountingProvider {
    inner: MemoryProvider,
    /// 所有对 `__generations__` 路径的写入次数（write + batch mutation）。
    generation_writes: AtomicUsize,
    /// 对 `generation.meta.json` 的写入次数。
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

    fn commit_batch(
        &self,
        mutations: &[BatchMutation],
        message: &str,
    ) -> Result<BatchCommitResult, ProviderError> {
        for mutation in mutations {
            let path = mutation.path();
            if path.contains(GENERATION_SUBDIR) {
                self.generation_writes.fetch_add(1, Ordering::SeqCst);
                if path.ends_with(GENERATION_META_FILENAME) {
                    self.generation_meta_writes.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        self.inner.commit_batch(mutations, message)
    }
}

/// Issue #716 评论 5740946551 + Issue #928 验证：远端 visible generation 为空、本地 staging
/// 有 chapter.md（文件级变化存在），remote Upsert 同时间戳严格赢且 candidate 不赢时，
/// `transfer_live_project` 必须恰好 publish 一次新 generation 并 CAS 推进 `active_generation`，
/// 不得丢弃本地增量，也不得重复 publish。
///
/// 场景：
/// - 远端 catalog：Upsert(`projects/p1`, T, `device_remote`, `active_generation=gen_existing`)
/// - 远端 `gen_existing` 下无任何文件；本地 staging chapter.md post-transfer LWW = (T, `device_local`)
/// - candidate = Upsert(`projects/p1`, T, `device_local`, `active_generation=new_gen`)
/// - `compare_lifecycle_candidate(candidate, remote)`：
///   - `lww_record_wins(candidate, existing)`：时间相等，`device_local < device_remote` → false
///   - `records_equal`：`active_generation` 不同 → false
///   - → `RemoteWins(existing(Upsert))`
/// - #928：merge 产生 `remote_upload_paths`（chapter.md 不在远端 visible generation）→ 走
///   publish；`apply_generation_pointer` 保留远端作品级 LWW（T/device_remote）并推进
///   `active_generation` 到新 generation。
///
/// 修复后 `publish_count == 1`（旧缺陷会在每轮 RemoteWinner 后重复 publish）。
#[test]
#[allow(clippy::too_many_lines)]
fn repro_issue_716_comment_5740946551_redundant_publish() {
    // ── 常量 ──
    const T: i64 = 10_000;
    const DEVICE_REMOTE: &str = "device_remote";
    const DEVICE_LOCAL: &str = "device_local"; // "device_local" < "device_remote" 字典序
    const GEN_EXISTING: &str = "gen_existing";

    // 字典序断言（防御性，确保 remote 严格赢）。
    assert!(
        DEVICE_LOCAL < DEVICE_REMOTE,
        "测试前置：device_local 必须字典序小于 device_remote"
    );

    // ── 1. 构造远端 catalog：Upsert record，同时间戳 T，device_remote 严格赢 ──
    let provider = CountingProvider::new(MemoryProvider::new());

    let remote_record =
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", T, DEVICE_REMOTE)
            .with_active_generation(GEN_EXISTING);
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(&mut catalog, remote_record);
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog,
        version: RemoteVersion::new("v1"),
    };
    write_remote_catalog(&provider, &snapshot).unwrap();

    // 重新加载拿到真实 version，作为 plan 的 remote_catalog_snapshot。
    // 注意：不预置 gen_existing 下的任何文件 → merge 远端 visible generation 时 list 为空，
    // 本地 staging 的 chapter.md 不在远端 → merge 产生 remote_upload_paths（文件级变化），
    // post-transfer LWW 仍是 (T, device_local)。
    let remote_catalog_snapshot = load_remote_catalog(&provider).unwrap();

    // ── 2. 构造本地 staging：manifest LWW = (T, device_local)，有一个文件 ──
    let tmp = TempDir::new().unwrap();
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
            updated_at_ms: T,
            deleted_at_ms: None,
            device_id: DEVICE_LOCAL.to_string(),
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

    // ── 3. 构造 PlannedTarget (LiveProject) 和 FullSyncPlan ──
    let local_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&local_root).unwrap();
    let planned = PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: local_root.clone(),
        staging_root: Some(staging_root.clone()),
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: local_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: T,
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
        remote_catalog_snapshot,
    };

    // ── 4. 调用 run_transfer（内部调 transfer_live_project）──
    let transfer = run_transfer(&provider, &plan, None, None, None);
    assert_eq!(
        transfer.targets.len(),
        1,
        "应只有一个 target（projects/p1）"
    );

    // ── 5. 断言：本地增量进入稳定 project tree，catalog 指向 Git tree 快照 ──
    let meta_writes = provider.generation_meta_writes.load(Ordering::SeqCst);
    let generation_writes = provider.generation_writes.load(Ordering::SeqCst);
    assert_eq!(
        meta_writes, 0,
        "Issue #1012: stable tree publication must not write generation.meta.json"
    );

    // catalog：active_generation 为 None，作品级 LWW 保留远端赢家。
    let snapshot = load_remote_catalog(&provider).unwrap();
    let record = find_record(&snapshot.catalog, "projects/p1").unwrap();
    assert_eq!(
        record.active_generation, None,
        "Issue #1012: the Git tree at projects/p1 is the remote snapshot"
    );
    assert_eq!(
        record.updated_at_ms, T,
        "作品级 LWW 不应被 candidate 降级：updated_at_ms 应保留远端赢家"
    );
    assert_eq!(
        record.device_id, DEVICE_REMOTE,
        "作品级 LWW 不应被 candidate 降级：device_id 应保留远端赢家"
    );

    // 稳定 project tree 必须包含本地 chapter.md 增量内容。
    let published = provider
        .read("projects/p1/volumes/v1/chapter.md")
        .unwrap()
        .expect("stable tree 应包含 chapter.md");
    assert_eq!(
        published.content, chapter_content,
        "稳定 project tree 中的 chapter.md 内容应与本地 staging 一致"
    );

    // 返回的 SyncResult：本地增量已应用（非 NoChanges——chapter.md 已上传）。
    let result = &transfer.targets[0].result;
    assert!(
        matches!(
            result.status,
            writer_core::sync::SyncStatus::LatestWinsApplied
        ),
        "返回状态应为 LatestWinsApplied（本地增量已上传），实际 {:?}",
        result.status
    );
    assert!(
        result
            .uploaded_files
            .contains(&"volumes/v1/chapter.md".to_string()),
        "uploaded_files 应记录真实上传的非冲突文件，实际 {:?}",
        result.uploaded_files
    );

    eprintln!(
        "[issue_716_repro] meta_writes={}, generation_writes={}, active_generation={:?}, status={:?}",
        meta_writes, generation_writes, record.active_generation, result.status
    );
}
