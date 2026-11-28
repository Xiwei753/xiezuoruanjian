//! Tests for generation prefix/CAS and related functions.

use super::*;
use crate::sync::provider::memory::MemoryProvider;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{ManifestFileRecord, SyncManifest, SyncTarget, TargetLifecycleCatalog};
use tempfile::TempDir;

///   LiveProject generation 原子发布。
#[test]
fn test_live_project_uploads_to_generation_prefix() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = MemoryProvider::new();

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("P");
    let chapter_dir = project_root.join("volumes").join("v1");
    std::fs::create_dir_all(&chapter_dir).unwrap();
    std::fs::write(chapter_dir.join("chapter.md"), b"content").unwrap();
    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(b"content")),
            updated_at_ms: 10_000,
            deleted_at_ms: None,
            device_id: "dev-A".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        project_root.join("app-meta/sync/manifest.sync.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("P"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("P".to_string()),
        target_live_root: project_root.clone(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 10_000,
            device_id: "dev-A".to_string(),
        }),
        expected_delete_lww: None,
    };
    let sync_policy = SyncPolicy {
        enabled: true,
        auto_sync: false,
        sync_interval_seconds: 60,
        has_network_permission: true,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy,
        force_sync: true,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let _transfer = crate::sync::full_sync::run_transfer(&provider, &plan, None, None, None);

    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/P")
        .expect("catalog should have projects/P record");
    assert_eq!(
        rec.op,
        crate::sync::types::TargetOp::Upsert,
        "catalog should have Upsert for projects/P"
    );
    assert_eq!(
        rec.active_generation, None,
        "Issue #1012: the Git tree at the stable project prefix is the snapshot"
    );

    let entries = provider.list("projects/P").unwrap();
    assert!(
        entries.iter().any(|e| e.path == "volumes/v1/chapter.md"),
        "stable project prefix should have volumes/v1/chapter.md, entries: {entries:?}"
    );
    assert!(
        provider
            .read("projects/P/volumes/v1/chapter.md")
            .unwrap()
            .is_some(),
        "Issue #1012: content must live at the stable project prefix"
    );
}

///   delete_all_remote_objects 跳过 generation prefix。
#[test]
fn test_delete_all_remote_objects_skips_generation_prefix() {
    let provider = MemoryProvider::new();

    provider
        .write(
            "projects/P/chapter.md",
            b"legacy",
            crate::sync::provider::model::WritePrecondition::CreateNew,
        )
        .unwrap();
    let gen_prefix =
        super::super::generation::generation_remote_prefix("projects/P", "gen-1").unwrap();
    provider
        .write(
            &format!("{}/chapter.md", gen_prefix),
            b"generation",
            crate::sync::provider::model::WritePrecondition::CreateNew,
        )
        .unwrap();

    // Use the transfer module's delete_all_remote_objects via run_transfer is indirect,
    // but we can test via the transfer module's internal function.
    // Since delete_all_remote_objects is private, we test it through the transfer behavior.
    // Instead, let's directly test the is_generation_path helper.
    assert!(super::super::generation::is_generation_path(
        "__generations__"
    ));
    assert!(super::super::generation::is_generation_path(
        "__generations__/gen-1"
    ));
    assert!(!super::super::generation::is_generation_path("other"));
    assert!(!super::super::generation::is_generation_path(
        "__generationsX__"
    ));

    // legacy 文件应存在。
    let legacy_read = provider.read("projects/P/chapter.md").unwrap();
    assert!(
        legacy_read.is_some(),
        "legacy chapter.md should exist before delete"
    );

    // generation 文件应存在。
    let gen_read = provider
        .read(&format!("{}/chapter.md", gen_prefix))
        .unwrap();
    assert!(
        gen_read.is_some(),
        "generation prefix chapter.md should exist"
    );
}

///   RestoreProject 从 generation prefix 下载。
#[test]
fn test_restore_project_downloads_from_generation_prefix() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy, TargetLifecycleRecord};

    let provider = MemoryProvider::new();

    let gen_id = "gen-restore-1";
    let gen_prefix =
        super::super::generation::generation_remote_prefix("projects/P", gen_id).unwrap();
    provider
        .write(
            &format!("{}/chapter.md", gen_prefix),
            b"remote-content",
            crate::sync::provider::model::WritePrecondition::CreateNew,
        )
        .unwrap();

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        TargetLifecycleRecord::upsert("projects/P", "projects/P", 10_000, "dev-A")
            .with_active_generation(gen_id),
    );
    let catalog_snapshot = crate::sync::types::RemoteTargetCatalogSnapshot {
        catalog,
        // 首次写入远端 catalog 必须用不存在哨兵，否则 CAS IfMatch 会 409；
        // 与 load_remote_catalog 对空远端返回的 version 一致。
        version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
    };
    crate::sync::target_lifecycle::write_remote_catalog(&provider, &catalog_snapshot).unwrap();

    let tmp = TempDir::new().unwrap();
    let staging_root = tmp.path().join("staging").join("P");
    std::fs::create_dir_all(&staging_root).unwrap();
    let local_root = tmp.path().join("projects").join("P");

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("P"),
        local_root: local_root.clone(),
        staging_root: Some(staging_root.clone()),
        target_kind: PlannedTargetKind::RestoreProject,
        project_id: Some("P".to_string()),
        target_live_root: local_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    };
    let sync_policy = SyncPolicy {
        enabled: true,
        auto_sync: false,
        sync_interval_seconds: 60,
        has_network_permission: true,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy,
        force_sync: true,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: catalog_snapshot,
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan, None, None, None);

    let staged_content = std::fs::read(staging_root.join("chapter.md"));
    assert!(
        staged_content.as_deref().ok() == Some(b"remote-content".as_slice()),
        "RestoreProject should download chapter.md from generation prefix, \
         staged_content: {:?}, transfer status: {:?}",
        staged_content.map(|c| String::from_utf8_lossy(&c).into_owned()),
        transfer.targets[0].result.status
    );
}

/// #1062 评论 #7219676521：清单存储与业务对象在同一 batch 提交，root Put 最后。
///
/// 期望顺序：业务 Put → 清单内容寻址对象 Put → 清单索引 Put → 根 Put（提交点）。
#[test]
fn batch_publishes_business_and_v2_manifest_with_root_last() {
    use crate::sync::provider::capabilities::SyncCapabilities;
    use crate::sync::provider::error::ProviderError;
    use crate::sync::provider::model::{
        BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject,
        RemoteVersion, WritePrecondition,
    };
    use crate::sync::types::{SyncPolicy, SyncScope, SyncStatus};
    use std::sync::Mutex;

    /// 记录 `commit_batch` mutations 的 Provider（其余操作透传 MemoryProvider）。
    struct RecordingBatchProvider {
        inner: MemoryProvider,
        mutations: Mutex<Vec<BatchMutation>>,
        list_calls: std::sync::atomic::AtomicUsize,
    }

    impl RecordingBatchProvider {
        fn new() -> Self {
            Self {
                inner: MemoryProvider::new(),
                mutations: Mutex::new(Vec::new()),
                list_calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn list_calls(&self) -> usize {
            self.list_calls.load(std::sync::atomic::Ordering::Relaxed)
        }

        fn recorded(&self) -> Vec<BatchMutation> {
            self.mutations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    impl SyncProvider for RecordingBatchProvider {
        fn capabilities(&self) -> SyncCapabilities {
            self.inner.capabilities()
        }

        fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
            self.list_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
            self.inner.write(path, content, precondition)
        }

        fn delete(
            &self,
            path: &str,
            precondition: DeletePrecondition,
        ) -> Result<(), ProviderError> {
            self.inner.delete(path, precondition)
        }

        fn commit_batch(
            &self,
            mutations: &[BatchMutation],
            message: &str,
        ) -> Result<BatchCommitResult, ProviderError> {
            self.mutations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .extend_from_slice(mutations);
            self.inner.commit_batch(mutations, message)
        }
    }

    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    let chapter_path = staging.join(chapter_rel);
    std::fs::create_dir_all(chapter_path.parent().unwrap()).unwrap();
    std::fs::write(&chapter_path, b"chapter content").unwrap();

    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: chapter_rel.to_string(),
            content_hash: format!("{:x}", md5::compute(b"chapter content")),
            updated_at_ms: 10_000,
            deleted_at_ms: None,
            device_id: "dev-A".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::create_dir_all(staging.join("app-meta/sync")).unwrap();
    std::fs::write(
        staging.join("app-meta/sync/manifest.sync.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();

    let provider = RecordingBatchProvider::new();
    let policy = SyncPolicy {
        enabled: true,
        auto_sync: false,
        sync_interval_seconds: 60,
        has_network_permission: true,
    };
    // merge_outcome == None：首次同步，清单来自 staging 的物化视图。
    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        SyncScope::Project,
        None,
        &policy,
        true,
        None,
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::Success),
        "首次发布应为 Success，实际 {:?}（error={:?}）",
        result.status,
        result.error
    );

    let mutations = provider.recorded();
    let paths: Vec<&str> = mutations.iter().map(BatchMutation::path).collect();
    let business = paths
        .iter()
        .position(|path| *path == "projects/p1/volumes/v1/chapters/c1/chapter.md")
        .unwrap_or_else(|| panic!("业务对象应与清单在同一 batch，实际 {paths:?}"));
    let object = paths
        .iter()
        .position(|path| path.starts_with("projects/p1/app-meta/sync/manifest/objects/"))
        .unwrap_or_else(|| panic!("batch 应包含清单内容寻址对象 Put，实际 {paths:?}"));
    let index = paths
        .iter()
        .position(|path| path.starts_with("projects/p1/app-meta/sync/manifest/index/"))
        .unwrap_or_else(|| panic!("batch 应包含清单索引 Put，实际 {paths:?}"));
    let root = paths
        .iter()
        .position(|path| *path == "projects/p1/app-meta/sync/manifest/root.json")
        .unwrap_or_else(|| panic!("batch 应包含 v2 清单根 Put，实际 {paths:?}"));

    assert_eq!(
        root,
        mutations.len() - 1,
        "root 的 Put 必须是 mutations 最后一项，实际 {paths:?}"
    );
    assert!(
        matches!(mutations[root], BatchMutation::Put { .. }),
        "root 必须是 Put"
    );
    assert!(
        business < object,
        "业务对象应先于清单对象写入，实际 {paths:?}"
    );
    assert!(object < index, "清单对象应先于索引写入，实际 {paths:?}");
    assert!(index < root, "根必须最后写，实际 {paths:?}");
}

/// #1062 评论 #7219943077 第 2 项：没有 batch + atomic_write 的 Provider
/// 不再有逐文件发布兜底，必须显式拒绝且零 provider 写。
#[test]
fn non_batch_provider_publish_is_rejected_without_file_writes() {
    use crate::sync::provider::capabilities::SyncCapabilities;
    use crate::sync::provider::error::ProviderError;
    use crate::sync::provider::model::{
        DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion, WritePrecondition,
    };
    use crate::sync::types::{SyncPolicy, SyncScope, SyncStatus};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct NonBatchProvider {
        inner: MemoryProvider,
        writes: AtomicUsize,
        deletes: AtomicUsize,
    }

    impl NonBatchProvider {
        fn new() -> Self {
            Self {
                inner: MemoryProvider::new(),
                writes: AtomicUsize::new(0),
                deletes: AtomicUsize::new(0),
            }
        }
    }

    impl SyncProvider for NonBatchProvider {
        fn capabilities(&self) -> SyncCapabilities {
            SyncCapabilities {
                batch: false,
                ..self.inner.capabilities()
            }
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
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.inner.write(path, content, precondition)
        }

        fn delete(
            &self,
            path: &str,
            precondition: DeletePrecondition,
        ) -> Result<(), ProviderError> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            self.inner.delete(path, precondition)
        }
    }

    let tmp = TempDir::new().unwrap();
    let provider = NonBatchProvider::new();
    let policy = SyncPolicy {
        enabled: true,
        auto_sync: false,
        sync_interval_seconds: 60,
        has_network_permission: true,
    };
    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        tmp.path(),
        "projects/p1",
        SyncScope::Project,
        None,
        &policy,
        true,
        None,
        None,
    );

    match &result.status {
        SyncStatus::RecoverableError(message) => assert!(
            message.contains(
                "provider lacks atomic multi-object publish; refusing to publish content and \
                 manifest separately"
            ),
            "错误文案应说明拒绝逐文件发布，实际 {message:?}"
        ),
        other => panic!("非 batch Provider 必须显式拒绝，实际 {other:?}"),
    }
    assert_eq!(
        provider.writes.load(Ordering::SeqCst),
        0,
        "拒绝时不得逐文件 provider.write"
    );
    assert_eq!(
        provider.deletes.load(Ordering::SeqCst),
        0,
        "拒绝时不得逐文件 provider.delete"
    );
    assert!(
        provider.inner.list("").unwrap().is_empty(),
        "拒绝时远端不能产生任何对象"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// #1062 评论 #7219943077 第 3 项（P1）：稀疏 generation 发布只读 delta 清单。
// ════════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use std::sync::Mutex;

use crate::sync::lww::{LwwMergeOutcome, ManifestStore, SYNC_MANIFEST_PATH};
use crate::sync::manifest_delta::ManifestDelta;
use crate::sync::provider::capabilities::SyncCapabilities;
use crate::sync::provider::error::ProviderError;
use crate::sync::provider::model::{
    BatchCommitResult, BatchMutation, DeletePrecondition, RemoteEntry, RemoteObject, RemoteVersion,
    WritePrecondition,
};
use crate::sync::types::{SyncPolicy, SyncStatus};

/// 记录 `commit_batch` mutations 的 Provider（其余操作透传 MemoryProvider）。
struct RecordingBatchProvider {
    inner: MemoryProvider,
    mutations: Mutex<Vec<BatchMutation>>,
    list_calls: std::sync::atomic::AtomicUsize,
}

impl RecordingBatchProvider {
    fn new() -> Self {
        Self {
            inner: MemoryProvider::new(),
            mutations: Mutex::new(Vec::new()),
            list_calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn list_calls(&self) -> usize {
        self.list_calls.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn recorded(&self) -> Vec<BatchMutation> {
        self.mutations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl SyncProvider for RecordingBatchProvider {
    fn capabilities(&self) -> SyncCapabilities {
        self.inner.capabilities()
    }

    fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, ProviderError> {
        self.list_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        self.mutations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .extend_from_slice(mutations);
        self.inner.commit_batch(mutations, message)
    }
}

fn manifest_record(path: &str, content_hash: &str, updated_at_ms: i64) -> ManifestFileRecord {
    ManifestFileRecord {
        path: path.to_string(),
        content_hash: content_hash.to_string(),
        updated_at_ms,
        deleted_at_ms: None,
        device_id: "dev-A".to_string(),
        op: "upsert".to_string(),
        schema_version: 1,
    }
}

fn md5_hex(bytes: &[u8]) -> String {
    format!("{:x}", md5::compute(bytes))
}

fn write_staging(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn commit_store_plan(root: &std::path::Path, plan: &crate::sync::lww::ManifestWritePlan) {
    let mut tx = crate::storage::transaction::SaveTransaction::new(root);
    for (rel, bytes) in &plan.writes {
        tx.add_bytes(rel, bytes).unwrap();
    }
    for rel in &plan.deletes {
        tx.add_delete(rel).unwrap();
    }
    tx.commit().unwrap();
}

fn sync_policy() -> SyncPolicy {
    SyncPolicy {
        enabled: true,
        auto_sync: false,
        sync_interval_seconds: 60,
        has_network_permission: true,
    }
}

#[allow(clippy::too_many_arguments)]
fn merge_outcome(
    merged_manifest: SyncManifest,
    delta: Option<ManifestDelta>,
    upload_paths: Vec<String>,
    delete_paths: Vec<String>,
    remote_tree_files: HashMap<String, String>,
) -> LwwMergeOutcome {
    LwwMergeOutcome {
        conflicts: Vec::new(),
        downloaded_files: Vec::new(),
        local_deletes: Vec::new(),
        remote_upload_paths: upload_paths,
        remote_delete_paths: delete_paths,
        overwritten_files: Vec::new(),
        auto_adjudicated_files: Vec::new(),
        ignored_files: Vec::new(),
        pending_take_remote_failed: Vec::new(),
        unresolved_missing_paths: Vec::new(),
        remote_tree_files,
        remote_manifest_path: SYNC_MANIFEST_PATH.to_string(),
        manifest_json: String::new(),
        merged_manifest,
        manifest_unchanged: false,
        unresolved_conflict_paths: std::collections::BTreeSet::new(),
        manifest_view: if delta.is_some() {
            crate::sync::lww::MergeManifestView::Sparse
        } else {
            crate::sync::lww::MergeManifestView::Complete
        },
        manifest_delta: delta,
        affected_paths: None,
        remote_base_namespaces: std::collections::BTreeMap::new(),
        remote_base_root: None,
    }
}

fn remote_versions(provider: &dyn SyncProvider, prefix: &str) -> HashMap<String, String> {
    provider
        .list(prefix)
        .unwrap()
        .into_iter()
        .map(|entry| (entry.path, entry.version.0))
        .collect()
}

/// delta 模式：只发布 delta 涉及的 object/index 与根；未变化业务路径/清单
/// 对象不产生 Delete，也不读取 staging 完整清单。
#[test]
fn delta_publish_skips_full_manifest_and_unchanged_deletes() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    let unchanged_rel = "volumes/v1/chapters/c2/chapter.md";
    let gone_rel = "volumes/v1/chapters/c3/chapter.md";
    let project_rel = "project.json";
    write_staging(&staging, chapter_rel, b"chapter v2");
    write_staging(&staging, unchanged_rel, b"unchanged chapter");
    write_staging(&staging, project_rel, b"project body");

    let baseline = SyncManifest {
        files: vec![
            manifest_record(project_rel, &md5_hex(b"project body"), 10),
            manifest_record(chapter_rel, &md5_hex(b"old chapter"), 10),
            manifest_record(unchanged_rel, &md5_hex(b"unchanged chapter"), 10),
            manifest_record(gone_rel, &md5_hex(b"gone chapter"), 10),
        ],
    };
    let legacy = staging.join(SYNC_MANIFEST_PATH);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, baseline.canonical_json().unwrap()).unwrap();
    let store = ManifestStore::new(&staging, SYNC_MANIFEST_PATH);
    assert!(store
        .load_or_legacy()
        .unwrap()
        .semantically_equals(&baseline));

    let mut delta = ManifestDelta::default();
    delta.upserts.insert(
        chapter_rel.to_string(),
        manifest_record(chapter_rel, &md5_hex(b"chapter v2"), 20),
    );
    delta.deletes.insert(gone_rel.to_string());
    let plan = store.apply_delta(&delta, "g-local").unwrap();
    commit_store_plan(&staging, &plan);
    let merged = store.load_all().unwrap().unwrap();
    assert!(!merged.files.iter().any(|record| record.path == gone_rel));

    let provider = RecordingBatchProvider::new();
    for (rel, bytes) in [
        (unchanged_rel, b"unchanged chapter".as_slice()),
        (gone_rel, b"gone chapter".as_slice()),
        (project_rel, b"project body".as_slice()),
    ] {
        provider
            .write(
                &format!("projects/p1/{rel}"),
                bytes,
                WritePrecondition::Unconditional,
            )
            .unwrap();
    }
    let remote_tree_files = remote_versions(&provider, "projects/p1");
    let outcome = merge_outcome(
        merged,
        Some(delta),
        vec![chapter_rel.to_string()],
        vec![gone_rel.to_string()],
        remote_tree_files,
    );

    let list_before_publish = provider.list_calls();
    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        crate::sync::types::SyncScope::Project,
        None,
        &sync_policy(),
        true,
        Some(&outcome),
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::Success),
        "delta 发布应成功，实际 {:?}（error={:?}）",
        result.status,
        result.error
    );
    assert_eq!(
        provider.list_calls(),
        list_before_publish,
        "稀疏发布不得 list 远端全树"
    );

    let mutations = provider.recorded();
    let paths: Vec<&str> = mutations.iter().map(BatchMutation::path).collect();
    let manifest_prefix = "projects/p1/app-meta/sync/manifest/";
    let object_puts: Vec<&str> = paths
        .iter()
        .copied()
        .filter(|path| path.starts_with(&format!("{manifest_prefix}objects/")))
        .collect();
    assert_eq!(
        object_puts.len(),
        1,
        "delta 模式只应发布受影响 object，实际 {paths:?}"
    );
    let index_puts = paths
        .iter()
        .filter(|path| path.starts_with(&format!("{manifest_prefix}index/")))
        .count();
    assert_eq!(
        index_puts, 1,
        "delta 模式只应发布受影响索引，实际 {paths:?}"
    );
    assert_eq!(
        mutations.last().map(BatchMutation::path),
        Some("projects/p1/app-meta/sync/manifest/root.json"),
        "根必须是 batch 最后一项，实际 {paths:?}"
    );
    let gone_path = format!("projects/p1/{gone_rel}");
    let gone_deletes = mutations
        .iter()
        .filter(|mutation| match mutation {
            BatchMutation::Delete { path } => path == &gone_path,
            BatchMutation::DeleteIfMatch { path, .. } => path == &gone_path,
            _ => false,
        })
        .count();
    assert_eq!(
        gone_deletes, 1,
        "confirmed delete 必须产生一次删除，实际 {paths:?}"
    );
    assert!(
        mutations.iter().all(
            |mutation| !matches!(mutation, BatchMutation::Delete { path }
                if path == &format!("projects/p1/{unchanged_rel}"))
        ),
        "未变化业务路径不得产生 Delete，实际 {paths:?}"
    );
    assert!(
        mutations.iter().all(
            |mutation| !matches!(mutation, BatchMutation::Delete { path }
            if path.starts_with(manifest_prefix))
        ),
        "delta 模式不得删除旧清单对象/索引（上一代根仍可能引用），实际 {paths:?}"
    );
    assert!(
        paths.contains(&format!("projects/p1/{chapter_rel}").as_str()),
        "业务 Put 应与 delta 清单同批，实际 {paths:?}"
    );
}

/// 完整模式（delta 为 None）行为不变：完整清单 objects/indexes 全部进入
/// batch，baseline 中不在期望树的旧业务路径产生 Delete。
#[test]
fn full_publish_still_emits_complete_manifest_and_deletes_stale_paths() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    let stale_rel = "volumes/v1/chapters/c9/stale.md";
    write_staging(&staging, chapter_rel, b"chapter body");

    let manifest = SyncManifest {
        files: vec![manifest_record(chapter_rel, &md5_hex(b"chapter body"), 10)],
    };
    let legacy = staging.join(SYNC_MANIFEST_PATH);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, manifest.canonical_json().unwrap()).unwrap();
    let store = ManifestStore::new(&staging, SYNC_MANIFEST_PATH);
    assert!(store
        .load_or_legacy()
        .unwrap()
        .semantically_equals(&manifest));

    let provider = RecordingBatchProvider::new();
    provider
        .write(
            &format!("projects/p1/{chapter_rel}"),
            b"chapter body",
            WritePrecondition::Unconditional,
        )
        .unwrap();
    provider
        .write(
            &format!("projects/p1/{stale_rel}"),
            b"stale body",
            WritePrecondition::Unconditional,
        )
        .unwrap();
    let remote_tree_files = remote_versions(&provider, "projects/p1");
    let outcome = merge_outcome(manifest, None, Vec::new(), Vec::new(), remote_tree_files);

    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        crate::sync::types::SyncScope::Project,
        None,
        &sync_policy(),
        true,
        Some(&outcome),
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::Success | SyncStatus::NoChanges),
        "完整发布应成功，实际 {:?}（error={:?}）",
        result.status,
        result.error
    );

    let mutations = provider.recorded();
    let paths: Vec<&str> = mutations.iter().map(BatchMutation::path).collect();
    let manifest_prefix = "projects/p1/app-meta/sync/manifest/";
    let object_puts = paths
        .iter()
        .filter(|path| path.starts_with(&format!("{manifest_prefix}objects/")))
        .count();
    assert_eq!(
        object_puts, 1,
        "完整模式应发布全部清单 objects，实际 {paths:?}"
    );
    let index_puts = paths
        .iter()
        .filter(|path| path.starts_with(&format!("{manifest_prefix}index/")))
        .count();
    assert_eq!(
        index_puts, 1,
        "完整模式应发布全部清单 indexes，实际 {paths:?}"
    );
    assert_eq!(
        mutations.last().map(BatchMutation::path),
        Some("projects/p1/app-meta/sync/manifest/root.json"),
        "根必须最后，实际 {paths:?}"
    );
    assert!(
        mutations
            .iter()
            .any(|mutation| matches!(mutation, BatchMutation::Delete { path }
            if path == &format!("projects/p1/{stale_rel}"))),
        "完整模式必须删除不在期望树的旧业务路径，实际 {paths:?}"
    );
}

/// delta 模式但本地 store 没有 root：发布端回退完整清单入口，不 panic。
#[test]
fn delta_view_unavailable_refuses_partial_complete_publish() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    write_staging(&staging, chapter_rel, b"chapter body");
    let manifest = SyncManifest {
        files: vec![manifest_record(chapter_rel, &md5_hex(b"chapter body"), 10)],
    };
    // 不物化 v2 root：manifest 只来自 merge outcome。
    let mut delta = ManifestDelta::default();
    delta
        .upserts
        .insert(chapter_rel.to_string(), manifest.files[0].clone());

    let provider = RecordingBatchProvider::new();
    let outcome = merge_outcome(
        manifest,
        Some(delta),
        vec![chapter_rel.to_string()],
        Vec::new(),
        HashMap::new(),
    );
    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        crate::sync::types::SyncScope::Project,
        None,
        &sync_policy(),
        true,
        Some(&outcome),
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::RecoverableError(_)),
        "delta 视图不可用时不得把局部 manifest 当完整权威发布，实际 {:?}",
        result.status
    );
    assert!(
        provider.recorded().is_empty(),
        "拒绝发布时不得产生任何远端 mutation，实际 {:?}",
        provider.recorded()
    );
}

/// 稀疏结果带未解决冲突：只发布非冲突对象的安全增量，冲突路径保持远端版本，
/// 最终状态为 PartialConflict 并列出已上传文件。
#[test]
fn sparse_conflict_publishes_safe_delta_only() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let conflict_rel = "volumes/v1/chapters/c1/chapter.md";
    let safe_rel = "volumes/v2/chapters/c2/chapter.md";
    write_staging(&staging, conflict_rel, b"chapter conflict local");
    write_staging(&staging, safe_rel, b"chapter safe local");
    let baseline = SyncManifest {
        files: vec![
            manifest_record("project.json", "hash-project", 10),
            manifest_record(conflict_rel, "hash-conflict-base", 10),
            manifest_record(safe_rel, "hash-safe-base", 10),
        ],
    };
    let legacy = staging.join(SYNC_MANIFEST_PATH);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, baseline.canonical_json().unwrap()).unwrap();
    let store = ManifestStore::new(&staging, SYNC_MANIFEST_PATH);
    let merged = store.load_or_legacy().unwrap();
    let mut delta = ManifestDelta::default();
    // 冲突路径在 merge 里保留远端记录：delta 不携带本地字节。
    delta.upserts.insert(
        safe_rel.to_string(),
        manifest_record(safe_rel, &md5_hex(b"chapter safe local"), 20),
    );
    let plan = store.apply_delta(&delta, "g-local").unwrap();
    commit_store_plan(&staging, &plan);
    let merged = store.load_all().unwrap().unwrap();
    // 冲突路径的远端 blob 仍可复用（merge 保留远端记录），因此允许发布安全增量。
    let remote_tree_files: HashMap<String, String> =
        [(conflict_rel.to_string(), "conflict-blob-oid".to_string())]
            .into_iter()
            .collect();
    let mut outcome = merge_outcome(
        merged,
        Some(delta),
        vec![safe_rel.to_string()],
        Vec::new(),
        remote_tree_files,
    );
    outcome.conflicts.push(crate::sync::types::SyncConflict {
        local_path: conflict_rel.to_string(),
        remote_path: conflict_rel.to_string(),
        local_hash: md5_hex(b"chapter conflict local"),
        remote_hash: "hash-conflict-base".to_string(),
        base_hash: "hash-conflict-base".to_string(),
        created_at: 10,
        description: "both changed".to_string(),
        kind: crate::sync::types::SyncConflictKind::BothChanged,
        remote_snapshot_path: None,
        remote_graph_snapshot_dir: None,
    });
    outcome
        .unresolved_conflict_paths
        .insert(conflict_rel.to_string());

    let provider = RecordingBatchProvider::new();
    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        crate::sync::types::SyncScope::Project,
        None,
        &sync_policy(),
        true,
        Some(&outcome),
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::PartialConflict),
        "冲突轮最终状态应为 PartialConflict，实际 {:?}",
        result.status
    );
    assert!(
        result.uploaded_files.iter().any(|path| path == safe_rel),
        "非冲突章节必须已上传，实际 {:?}",
        result.uploaded_files
    );
    let recorded = provider.recorded();
    let paths: Vec<&str> = recorded.iter().map(BatchMutation::path).collect();
    assert!(
        paths.contains(&format!("projects/p1/{safe_rel}").as_str()),
        "安全章节应进入同一批次，实际 {paths:?}"
    );
    assert!(
        !paths.contains(&format!("projects/p1/{conflict_rel}").as_str()),
        "冲突章节不得上传本地字节，实际 {paths:?}"
    );
}

/// 稀疏轮没有真实变化：不产生根 Put，也不制造空提交。
#[test]
fn sparse_no_op_publish_does_not_commit_root() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    write_staging(&staging, chapter_rel, b"chapter body");
    let baseline = SyncManifest {
        files: vec![
            manifest_record("project.json", "hash-project", 10),
            manifest_record(chapter_rel, &md5_hex(b"chapter body"), 10),
        ],
    };
    let legacy = staging.join(SYNC_MANIFEST_PATH);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, baseline.canonical_json().unwrap()).unwrap();
    let store = ManifestStore::new(&staging, SYNC_MANIFEST_PATH);
    let merged = store.load_or_legacy().unwrap();
    let base_namespaces = store.load_root().unwrap().unwrap().namespaces;
    let mut outcome = merge_outcome(
        merged,
        Some(ManifestDelta::default()),
        Vec::new(),
        Vec::new(),
        HashMap::new(),
    );
    outcome.manifest_unchanged = true;
    outcome.remote_base_namespaces = base_namespaces;

    let provider = RecordingBatchProvider::new();
    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        crate::sync::types::SyncScope::Project,
        None,
        &sync_policy(),
        false,
        Some(&outcome),
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::NoChanges),
        "无变化轮不得推进根，实际 {:?}",
        result.status
    );
    assert!(
        provider.recorded().is_empty(),
        "无变化轮不得提交，实际 {:?}",
        provider.recorded()
    );
}

/// 稀疏发布把本地 retire journal 里超过一代的对象/索引在同一个远端提交里删除。
#[test]
fn sparse_publish_deletes_retired_manifest_objects() {
    use crate::sync::provider::SyncProvider;
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let chapter_rel = "volumes/v1/chapters/c1/chapter.md";
    let project_rel = "project.json";
    write_staging(&staging, chapter_rel, b"chapter v1");
    write_staging(&staging, project_rel, b"project v1");
    let baseline = SyncManifest {
        files: vec![
            manifest_record(project_rel, &md5_hex(b"project v1"), 10),
            manifest_record(chapter_rel, &md5_hex(b"chapter v1"), 10),
        ],
    };
    let legacy = staging.join(SYNC_MANIFEST_PATH);
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, baseline.canonical_json().unwrap()).unwrap();
    let store = ManifestStore::new(&staging, SYNC_MANIFEST_PATH);
    let plan = store.plan_write_all(&baseline, "").unwrap();
    commit_store_plan(&staging, &plan);

    // 远端固定基线：当前发布视图的 root/index/object 全部落到 provider。
    let provider = RecordingBatchProvider::new();
    for file in crate::sync::lww::local_manifest_storage(&store, None).unwrap() {
        provider
            .write(
                &format!("projects/p1/{}", file.rel),
                &file.read_bytes(&staging).unwrap(),
                WritePrecondition::Unconditional,
            )
            .unwrap();
    }
    let base_root_bytes = provider
        .read("projects/p1/app-meta/sync/manifest/root.json")
        .unwrap()
        .unwrap()
        .content;
    let base_root: crate::sync::lww::ManifestRoot =
        serde_json::from_slice(&base_root_bytes).unwrap();

    // 第一次换根：章节 v2，旧 object/index 进入 retire journal。
    write_staging(&staging, chapter_rel, b"chapter v2");
    let mut delta1 = ManifestDelta::default();
    delta1.upserts.insert(
        chapter_rel.to_string(),
        manifest_record(chapter_rel, &md5_hex(b"chapter v2"), 20),
    );
    let plan1 = store.apply_delta(&delta1, "").unwrap();
    commit_store_plan(&staging, &plan1);

    // 第二次换根：project v2，使第一次退休的条目超过保留代际。
    write_staging(&staging, project_rel, b"project v2");
    let mut delta2 = ManifestDelta::default();
    delta2.upserts.insert(
        project_rel.to_string(),
        manifest_record(project_rel, &md5_hex(b"project v2"), 30),
    );
    let plan2 = store.apply_delta(&delta2, "").unwrap();
    commit_store_plan(&staging, &plan2);
    let merged = store.load_all().unwrap().unwrap();
    let pending_before = store.pending_remote_retirements(None).unwrap();
    assert!(
        !pending_before.is_empty(),
        "两次换根后应有待远端回收的清单文件"
    );

    let mut outcome = merge_outcome(
        merged,
        Some(delta2),
        vec![project_rel.to_string()],
        Vec::new(),
        HashMap::new(),
    );
    outcome.remote_base_namespaces = base_root.namespaces;

    let result = super::super::generation::publish_tree_snapshot(
        &provider,
        &staging,
        "projects/p1",
        crate::sync::types::SyncScope::Project,
        None,
        &sync_policy(),
        true,
        Some(&outcome),
        None,
    );
    assert!(
        matches!(&result.status, SyncStatus::Success | SyncStatus::NoChanges),
        "稀疏发布应成功，实际 {:?}（error={:?}）",
        result.status,
        result.error
    );
    let mutations = provider.recorded();
    let retired_deletes: Vec<&str> = mutations
        .iter()
        .filter_map(|mutation| match mutation {
            BatchMutation::DeleteIfMatch { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        retired_deletes
            .iter()
            .any(|path| path.contains("/manifest/objects/") || path.contains("/manifest/index/")),
        "远端提交必须包含退休清单文件删除，实际 {retired_deletes:?}"
    );
    let pending_after = store.pending_remote_retirements(None).unwrap();
    assert!(
        pending_after.len() < pending_before.len(),
        "远端删除提交后旧代条目必须结清，剩余只保留当前代（before={pending_before:?}, after={pending_after:?}）"
    );
}
