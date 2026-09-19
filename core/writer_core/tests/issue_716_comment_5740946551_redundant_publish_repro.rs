//! Issue #716 评论 5740946551 — `transfer_live_project` 不再重复 publish generation 验证测试。
//!
//! ## 修复目标
//!
//! `core/writer_core/src/sync/full_sync/transfer_helpers.rs` 的 `transfer_live_project`
//! 循环逻辑：修复前每轮先 `publish_generation`（上传一整份新 generation 到远端），再 CAS
//! `apply_lifecycle_record`。当远端 Upsert 同时间戳严格赢（`device_remote > device_local`）
//! 且 merge 后 candidate 仍不赢时，CAS 返回 `RemoteWinner(Upsert)` 触发 `continue`，
//! 下一轮又 publish 一个新的 generation。明知 winner 不会改变仍重复 publish，每轮产生
//! 一个未引用 generation。
//!
//! 修复后 publish 前先用 `compare_lifecycle_candidate` 判定：candidate 不赢直接收敛不 publish。
//! 本测试用计数 `__generations__` 路径 write 的 mock provider 验证修复后 `publish_count == 0`。
//!
//! ## 场景
//!
//! 1. 远端 catalog 已有 Upsert record：`target_id="projects/p1"`, `lww_time_ms=T`,
//!    `device_id="device_remote"`, `active_generation=Some("gen_existing")`。
//! 2. 本地 staging post-transfer LWW：`lww_time_ms=T`（同时间戳）,
//!    `device_id="device_local"`，其中 `"device_local" < "device_remote"`（字典序，
//!    remote 严格赢）。
//! 3. 调用 `run_transfer` 触发 `transfer_live_project`。
//! 4. 断言：`publish_count == 0`（candidate 不赢直接收敛，不 publish），
//!    `generation_writes == 0`，返回 `NoChanges`。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::{AtomicUsize, Ordering};

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
    load_remote_catalog, upsert_record, write_remote_catalog,
};
use writer_core::sync::types::{
    ManifestFileRecord, PlannedTargetKind, RemoteTargetCatalogSnapshot, SyncManifest, SyncPolicy,
    SyncTarget, TargetLifecycleCatalog, TargetLifecycleRecord,
};

/// `__generations__` 子目录名 — 与 `full_sync::generation::GENERATION_SUBDIR` 一致。
const GENERATION_SUBDIR: &str = "__generations__";
/// `generation.meta.json` 文件名 — 与 `generation_gc::GENERATION_META_FILENAME` 一致。
const GENERATION_META_FILENAME: &str = "generation.meta.json";

/// 计数对 `__generations__` 路径 write 的 mock provider。
///
/// 包装 `MemoryProvider`，在 `write()` 里对包含 `__generations__` 的路径计数，
/// 并对以 `generation.meta.json` 结尾的路径单独计数（每次 publish 写 2 次 meta：
/// `complete=false` + `complete=true`，故 `publish_count = meta_writes / 2`）。
struct CountingProvider {
    inner: MemoryProvider,
    /// 所有对 `__generations__` 路径的 write 次数。
    generation_writes: AtomicUsize,
    /// 对 `generation.meta.json` 的 write 次数（每次 publish 2 次）。
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
}

/// Issue #716 评论 5740946551 验证：修复后 `transfer_live_project` 在 remote Upsert 同时间戳
/// 严格赢且 merge 后 candidate 仍不赢时，不再调用 `publish_generation`，直接收敛返回 NoChanges。
///
/// 场景：
/// - 远端 catalog：Upsert(`projects/p1`, T, `device_remote`, `active_generation=gen_existing`)
/// - 本地 staging post-transfer LWW：(T, `device_local`)，`device_local < device_remote`
/// - candidate = Upsert(`projects/p1`, T, `device_local`, `active_generation=new_gen`)
/// - `compare_lifecycle_candidate(candidate, remote)`：
///   - `lww_record_wins(candidate, existing)`：时间相等，`device_local < device_remote` → false
///   - `records_equal`：`active_generation` 不同 → false
///   - → `RemoteWins(existing(Upsert))` → 不 publish，直接收敛返回 NoChanges
///
/// 修复后 publish 前先判定 candidate 不赢，故 `publish_count == 0`，不产生未引用 generation。
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
    // staging manifest 不变，post-transfer LWW 仍是 (T, device_local)。
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
    let transfer = run_transfer(&provider, &plan);
    assert_eq!(
        transfer.targets.len(),
        1,
        "应只有一个 target（projects/p1）"
    );

    // ── 5. 断言：publish 次数 == 0（修复后 candidate 不赢直接收敛，不 publish）──
    let meta_writes = provider.generation_meta_writes.load(Ordering::SeqCst);
    let generation_writes = provider.generation_writes.load(Ordering::SeqCst);
    // 每次 publish 写 2 次 generation.meta.json (complete=false + complete=true)
    let publish_count = meta_writes / 2;

    assert_eq!(
        publish_count, 0,
        "Issue #716 评论 5740946551 修复失败：transfer_live_project 在 remote Upsert 同时间戳 \
         严格赢（device_remote={:?} > device_local={:?}, T={}）且 merge 后 candidate 仍不赢时， \
         应直接收敛不 publish。实际 publish_count={}（meta_writes={}, generation_writes={}）， \
         期望 0。修复后 publish 前先用 compare_lifecycle_candidate 判定，candidate 不赢走 RemoteWins(Upsert) 分支直接返回。",
        DEVICE_REMOTE,
        DEVICE_LOCAL,
        T,
        publish_count,
        meta_writes,
        generation_writes,
    );

    // 附加证据：generation_writes == 0（没有任何对 __generations__ 路径的 write）。
    assert_eq!(
        generation_writes, 0,
        "Issue #716 评论 5740946551 修复失败：不应有对 __generations__ 路径的 write，\
         实际 generation_writes={}",
        generation_writes
    );

    // 返回的 SyncResult 应该是 NoChanges（本测试无冲突）。
    assert!(
        matches!(
            transfer.targets[0].result.status,
            writer_core::sync::SyncStatus::NoChanges
        ),
        "Issue #716 评论 5740946551 修复失败：返回状态应为 NoChanges，实际 {:?}",
        transfer.targets[0].result.status
    );

    eprintln!(
        "[issue_716_repro] publish_count={}, meta_writes={}, generation_writes={}, status={:?}",
        publish_count, meta_writes, generation_writes, transfer.targets[0].result.status
    );
}
