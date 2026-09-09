//! Tests for run_transfer_* functions.

use super::*;
use crate::sync::provider::memory::MemoryProvider;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{
    DeletedTargetResolution, ManifestFileRecord, SyncManifest, SyncTarget, TargetLifecycleCatalog,
};
use tempfile::TempDir;

///   run_transfer 在 DeleteRemoteProject 时先写 catalog tombstone 再删远端。
#[test]
fn run_transfer_catalog_tombstone_before_remote_delete() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider =
        MemoryProvider::with_entries([("projects/p1/chapter.md".to_string(), b"hello".to_vec())]);
    let target = SyncTarget::project("p1");
    let lww_val = lww(2000, "dev-1");

    let tmp = TempDir::new().unwrap();
    let planned = crate::sync::full_sync::PlannedTarget {
        target,
        local_root: tmp.path().to_path_buf(),
        staging_root: None,
        target_kind: PlannedTargetKind::DeleteRemoteProject,
        project_id: Some("p1".to_string()),
        target_live_root: tmp.path().to_path_buf(),
        deleted_journal_token: Some("token-1".to_string()),
        deleted_lww: Some(lww_val),
        live_lww: None,
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        transfer.targets[0].deleted_resolution,
        Some(DeletedTargetResolution::LocalDeleteWins)
    );

    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/p1");
    assert!(rec.is_some());
    assert_eq!(rec.unwrap().op, crate::sync::types::TargetOp::Delete);
    assert!(provider.read("projects/p1/chapter.md").unwrap().is_none());
}

///   catalog 写失败时 deleted target 走 Retry，pending 保留。
#[test]
fn run_transfer_catalog_write_failure_returns_retry() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = AlwaysFailCatalogProvider::new();
    let target = SyncTarget::project("p1");
    let lww_val = lww(2000, "dev-1");

    let tmp = TempDir::new().unwrap();
    let planned = crate::sync::full_sync::PlannedTarget {
        target,
        local_root: tmp.path().to_path_buf(),
        staging_root: None,
        target_kind: PlannedTargetKind::DeleteRemoteProject,
        project_id: Some("p1".to_string()),
        target_live_root: tmp.path().to_path_buf(),
        deleted_journal_token: Some("token-1".to_string()),
        deleted_lww: Some(lww_val),
        live_lww: None,
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert_eq!(
        transfer.targets[0].deleted_resolution,
        Some(DeletedTargetResolution::Retry)
    );
    assert!(matches!(
        transfer.targets[0].result.status,
        crate::sync::SyncStatus::RecoverableError(_)
    ));
    assert!(provider.read("projects/p1/chapter.md").unwrap().is_some());
}

///   run_transfer 对 LiveProject 先写 catalog upsert。
#[test]
fn run_transfer_writes_catalog_upsert_for_live_project() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(project_root.join("project.json"), b"project content").unwrap();
    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "project.json".to_string(),
            content_hash: format!("{:x}", md5::compute(b"project content")),
            updated_at_ms: 2000,
            deleted_at_ms: None,
            device_id: "dev-1".to_string(),
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
        target: SyncTarget::project("p1"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 1000,
            device_id: "dev-1".to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);

    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/p1");
    assert!(rec.is_some());
    assert_eq!(rec.unwrap().op, crate::sync::types::TargetOp::Upsert);
}

///   LiveProject lifecycle 写失败 → RecoverableError。
#[test]
fn run_transfer_live_project_lifecycle_failure_returns_error() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = AlwaysFailCatalogProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 1000,
            device_id: "dev-1".to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert!(matches!(
        transfer.targets[0].result.status,
        crate::sync::SyncStatus::RecoverableError(_)
    ));
}

///   DeleteLocalProject 不上传，返回 NoChanges。
#[test]
fn run_transfer_delete_local_project_skips_upload() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::DeleteLocalProject,
        project_id: Some("p1".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    assert_eq!(transfer.targets.len(), 1);
    assert!(matches!(
        transfer.targets[0].result.status,
        crate::sync::SyncStatus::RecoverableError(_)
    ));
    assert!(transfer.targets[0].result.uploaded_files.is_empty());
}

/// 问题 3 复现：apply_lifecycle_record 的 CAS retry 被 write_remote_catalog 内部吃掉。
#[test]
fn repro_issue_645_q3_cas_retry_swallowed_by_write_remote_catalog() {
    use crate::sync::types::{
        RemoteTargetCatalogSnapshot, TargetLifecycleApplyResult, TargetLifecycleRecord,
    };

    let provider = MemoryProvider::new();

    {
        let mut cat = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut cat,
            TargetLifecycleRecord::upsert("projects/P", "projects/P", 12_050, "dev-B"),
        );
        let snap = RemoteTargetCatalogSnapshot {
            catalog: cat,
            version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
        };
        crate::sync::target_lifecycle::write_remote_catalog(&provider, &snap).unwrap();
    }

    let stale_snapshot = RemoteTargetCatalogSnapshot {
        catalog: TargetLifecycleCatalog::default(),
        version: crate::sync::provider::model::RemoteVersion::new("stale-version"),
    };

    let candidate = TargetLifecycleRecord::delete("projects/P", "projects/P", 12_000, "dev-A");

    let result = crate::sync::target_lifecycle::apply_lifecycle_record(
        &provider,
        &stale_snapshot,
        candidate,
    );

    assert!(
        matches!(result, TargetLifecycleApplyResult::RemoteWinner { .. }),
        "问题3: apply_lifecycle_record 应返回 RemoteWinner（远端 12:05 > candidate 12:00），\
         但 write_remote_catalog 内部吃掉 CAS 冲突后返回了 {:?}",
        match result {
            TargetLifecycleApplyResult::Applied(s) => {
                format!("Applied(catalog={:?})", s.catalog.records)
            }
            TargetLifecycleApplyResult::AlreadyCurrent(_) => "AlreadyCurrent".to_string(),
            TargetLifecycleApplyResult::RemoteWinner { record, .. } => {
                format!("RemoteWinner(op={:?})", record.op)
            }
            TargetLifecycleApplyResult::Retry(e) => format!("Retry({e})"),
        }
    );
}

/// 问题 4 复现：DeleteLocalProject 只 NoChanges，返回 DeleteProject action。
#[test]
fn repro_issue_645_q4_delete_local_project_no_actual_deletion() {
    use crate::sync::types::{
        LocalLifecycleCommitAction, PlannedTargetKind, RemoteTargetCatalogSnapshot,
        TargetLifecycleCatalog, TargetLifecycleRecord,
    };

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(project_root.join("chapter.md"), b"content").unwrap();

    let mut catalog = TargetLifecycleCatalog::default();
    catalog.records.push(TargetLifecycleRecord::delete(
        "projects/P",
        "projects/P",
        2000,
        "dev-remote",
    ));
    let remote_catalog_snapshot = RemoteTargetCatalogSnapshot {
        catalog: catalog.clone(),
        version: crate::sync::provider::model::RemoteVersion::new("v1"),
    };
    let catalog_json = serde_json::to_vec(&catalog).unwrap();
    provider
        .write(
            crate::sync::target_lifecycle::TARGET_CATALOG_REMOTE_PATH,
            &catalog_json,
            crate::sync::provider::model::WritePrecondition::CreateNew,
        )
        .unwrap();

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("P"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::DeleteLocalProject,
        project_id: Some("P".to_string()),
        target_live_root: project_root.clone(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 1000,
            device_id: "dev-local".to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: crate::sync::types::SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot,
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);

    assert!(
        matches!(
            transfer.targets[0].result.status,
            crate::sync::SyncStatus::NoChanges
        ),
        "DeleteLocalProject 返回 NoChanges（删除延迟到 commit 阶段）"
    );

    assert!(
        matches!(
            &transfer.targets[0].local_lifecycle_action,
            LocalLifecycleCommitAction::DeleteProject { project_id, .. } if project_id == "P"
        ),
        "DeleteLocalProject 应返回 DeleteProject action，\
         实际: {:?}",
        transfer.targets[0].local_lifecycle_action
    );

    assert!(
        project_root.exists(),
        "run_transfer 不应删除本地目录（删除延迟到 commit 阶段）"
    );
}

/// 问题 5 复现：LiveProject 先发布 lifecycle upsert 再同步正文。
#[test]
fn repro_issue_645_q5_live_project_lifecycle_before_content_transfer() {
    use crate::sync::types::PlannedTargetKind;

    struct CatalogOkContentFailProvider {
        inner: MemoryProvider,
    }
    impl SyncProvider for CatalogOkContentFailProvider {
        fn capabilities(&self) -> crate::sync::provider::capabilities::SyncCapabilities {
            self.inner.capabilities()
        }
        fn list(
            &self,
            prefix: &str,
        ) -> Result<
            Vec<crate::sync::provider::model::RemoteEntry>,
            crate::sync::provider::error::ProviderError,
        > {
            self.inner.list(prefix)
        }
        fn read(
            &self,
            path: &str,
        ) -> Result<
            Option<crate::sync::provider::model::RemoteObject>,
            crate::sync::provider::error::ProviderError,
        > {
            self.inner.read(path)
        }
        fn write(
            &self,
            path: &str,
            content: &[u8],
            precondition: crate::sync::provider::model::WritePrecondition,
        ) -> Result<
            crate::sync::provider::model::RemoteVersion,
            crate::sync::provider::error::ProviderError,
        > {
            if path == crate::sync::target_lifecycle::TARGET_CATALOG_REMOTE_PATH {
                return self.inner.write(path, content, precondition);
            }
            Err(crate::sync::provider::error::ProviderError::Other {
                reason: "content transfer failed".to_string(),
            })
        }
        fn delete(
            &self,
            path: &str,
            precondition: crate::sync::provider::model::DeletePrecondition,
        ) -> Result<(), crate::sync::provider::error::ProviderError> {
            self.inner.delete(path, precondition)
        }
    }

    let provider = CatalogOkContentFailProvider {
        inner: MemoryProvider::new(),
    };

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("P");
    std::fs::create_dir_all(&project_root).unwrap();
    std::fs::write(project_root.join("chapter.md"), b"content").unwrap();
    std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
    let manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "chapter.md".to_string(),
            content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
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
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 10_000,
            device_id: "dev-A".to_string(),
        }),
        expected_delete_lww: None,
    };
    let sync_policy = crate::sync::types::SyncPolicy {
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

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);

    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/P");
    let catalog_has_upsert = rec
        .map(|r| r.op == crate::sync::types::TargetOp::Upsert)
        .unwrap_or(false);

    let remote_has_content =
        crate::sync::provider::SyncProvider::read(&provider, "projects/P/chapter.md")
            .unwrap()
            .is_some();

    assert!(
        !(catalog_has_upsert && !remote_has_content),
        "问题5: LiveProject 先发布 lifecycle upsert 再传正文，正文失败时留下假的已发布 target。\
         catalog_has_upsert={}, remote_has_content={}, status={:?}。",
        catalog_has_upsert,
        remote_has_content,
        transfer.targets[0].result.status
    );
}

/// 问题 6 复现：静态代码路径证据。
#[test]
fn repro_issue_645_q6_network_io_inside_core_write_lock_static_evidence() {
    let source = include_str!("../../../api/sync_api.rs");
    let perform_full_sync_start = source.find("pub fn perform_full_sync(");
    assert!(
        perform_full_sync_start.is_some(),
        "问题6: perform_full_sync 函数应存在于 sync_api.rs"
    );
    let start = perform_full_sync_start.unwrap();
    let body = &source[start..];

    let has_core_write = body.contains("let core = self.core_write();");
    let has_discover_catalog = body.contains("discover_legacy_remote_catalog(provider.as_ref())");
    let has_build_plan_unlocked = body.contains("build_full_sync_plan_unlocked(");
    let has_lock_released = body.contains("写锁已释放");

    assert!(has_core_write, "问题6: core_write() 调用应存在");
    assert!(
        has_discover_catalog,
        "问题6: discover_legacy_remote_catalog 调用应存在"
    );
    assert!(
        has_build_plan_unlocked,
        "问题6: build_full_sync_plan_unlocked 调用应存在（锁外扫描）"
    );
    assert!(has_lock_released, "问题6: '写锁已释放' 注释应存在");

    let core_write_pos = body.find("let core = self.core_write();").unwrap();
    let discover_catalog_pos = body
        .find("discover_legacy_remote_catalog(provider.as_ref())")
        .unwrap();
    let lock_released_pos = body.find("写锁已释放").unwrap();

    assert!(
        discover_catalog_pos > lock_released_pos,
        "问题6: discover_legacy_remote_catalog (pos={}) 应在 '写锁已释放' (pos={}) 之后\
         （即网络 IO 不在写锁内）。当前 discover_legacy_remote_catalog 在 core_write (pos={}) 之后、\
         写锁释放 (pos={}) 之前，网络 IO 期间持 core 写锁，阻塞正文/作品读取，\
         与 #644 拆锁路线冲突。",
        discover_catalog_pos,
        lock_released_pos,
        core_write_pos,
        lock_released_pos
    );
}

///   run_transfer 用 plan 携带的 snapshot 作为起点。
#[test]
fn q4_run_transfer_uses_plan_catalog_snapshot() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy, TargetLifecycleRecord};

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();

    let mut plan_catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut plan_catalog,
        TargetLifecycleRecord::delete("projects/p1", "projects/p1", 12000, "dev-old"),
    );
    let plan_snapshot_with_delete = crate::sync::types::RemoteTargetCatalogSnapshot {
        catalog: plan_catalog,
        version: crate::sync::provider::model::RemoteVersion::new("v-plan"),
    };

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 10_000,
            device_id: "dev-1".to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: plan_snapshot_with_delete,
    };

    let _transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    let remote_catalog_after =
        crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let remote_has_record = !remote_catalog_after.catalog.records.is_empty();
    assert!(
        !remote_has_record,
        "问题4: run_transfer 应用 plan 携带的 snapshot（含 delete tombstone）\
         走 LostToRemote，不应写远端 catalog。但远端出现了记录"
    );
}

///   lifecycle publish 用 post-transfer staging manifest LWW。
#[test]
fn q3_publish_uses_post_transfer_staging_lww() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(staging_root.join("volumes").join("v1")).unwrap();
    std::fs::write(
        staging_root.join("volumes").join("v1").join("chapter.md"),
        b"chapter content",
    )
    .unwrap();
    std::fs::create_dir_all(staging_root.join("app-meta/sync")).unwrap();
    let staging_manifest = SyncManifest {
        files: vec![ManifestFileRecord {
            path: "volumes/v1/chapter.md".to_string(),
            content_hash: format!("{:x}", md5::compute(b"chapter content")),
            updated_at_ms: 5000,
            deleted_at_ms: None,
            device_id: "dev-1".to_string(),
            op: "upsert".to_string(),
            schema_version: 1,
        }],
    };
    std::fs::write(
        staging_root.join("app-meta/sync/manifest.sync.json"),
        serde_json::to_vec(&staging_manifest).unwrap(),
    )
    .unwrap();

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: project_root.clone(),
        staging_root: Some(staging_root.clone()),
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 1000,
            device_id: "dev-1".to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let _transfer = crate::sync::full_sync::run_transfer(&provider, &plan);

    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/p1")
        .expect("catalog should have upsert record");
    assert_eq!(rec.op, crate::sync::types::TargetOp::Upsert);
    assert_eq!(
        rec.updated_at_ms, 5000,
        "问题3: lifecycle publish 应使用 post-transfer staging LWW (5000)，\
         不是旧 live_lww (1000)"
    );
}

///   post-transfer staging manifest 读取失败 → RecoverableError。
#[test]
fn q3_post_transfer_manifest_unreadable_returns_error() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy};

    let provider = MemoryProvider::new();
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("p1");
    std::fs::create_dir_all(&project_root).unwrap();
    let staging_root = tmp.path().join("staging-p1");
    std::fs::create_dir_all(&staging_root).unwrap();

    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("p1"),
        local_root: project_root.clone(),
        staging_root: Some(staging_root),
        target_kind: PlannedTargetKind::LiveProject,
        project_id: Some("p1".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: Some(LiveTargetLww {
            lww_time_ms: 1000,
            device_id: "dev-1".to_string(),
        }),
        expected_delete_lww: None,
    };
    let plan = crate::sync::full_sync::FullSyncPlan {
        sync_policy: SyncPolicy {
            enabled: true,
            ..Default::default()
        },
        force_sync: false,
        targets: vec![planned],
        app_data_root: tmp.path().to_path_buf(),
        remote_catalog_snapshot: test_empty_catalog_snapshot(),
    };

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);
    assert!(
        matches!(
            transfer.targets[0].result.status,
            crate::sync::SyncStatus::RecoverableError(_)
        ),
        "问题3: post-transfer manifest 不可读应返回 RecoverableError，\
         不伪造旧 live_lww。status={:?}",
        transfer.targets[0].result.status
    );
    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
    assert!(
        snapshot.catalog.records.is_empty(),
        "问题3: manifest 不可读时不应写 catalog"
    );
}

///   remote-only Delete 的 RemoteCleanupProject 实际执行 cleanup。
#[test]
fn test_remote_only_delete_cleanup_executes() {
    use crate::sync::types::{PlannedTargetKind, SyncPolicy, TargetLifecycleCatalog};

    let provider = MemoryProvider::new();

    provider
        .write(
            "projects/P/project.json",
            b"residue",
            crate::sync::provider::model::WritePrecondition::CreateNew,
        )
        .unwrap();

    let mut catalog = TargetLifecycleCatalog::default();
    crate::sync::target_lifecycle::upsert_record(
        &mut catalog,
        crate::sync::types::TargetLifecycleRecord::delete(
            "projects/P",
            "projects/P",
            20_000,
            "dev-A",
        ),
    );
    let catalog_snapshot = crate::sync::types::RemoteTargetCatalogSnapshot {
        catalog: catalog.clone(),
        version: crate::sync::provider::model::RemoteVersion::new("v1"),
    };
    crate::sync::target_lifecycle::write_remote_catalog(&provider, &catalog_snapshot).unwrap();

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("projects").join("P");
    let planned = crate::sync::full_sync::PlannedTarget {
        target: SyncTarget::project("P"),
        local_root: project_root.clone(),
        staging_root: None,
        target_kind: PlannedTargetKind::RemoteCleanupProject,
        project_id: Some("P".to_string()),
        target_live_root: project_root,
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: Some(DeletedTargetLww {
            deleted_at_ms: 20_000,
            device_id: "dev-A".to_string(),
        }),
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

    let transfer = crate::sync::full_sync::run_transfer(&provider, &plan);

    assert!(
        matches!(
            transfer.targets[0].result.status,
            crate::sync::SyncStatus::Success | crate::sync::SyncStatus::NoChanges
        ),
        "RemoteCleanupProject should succeed, status: {:?}",
        transfer.targets[0].result.status
    );
    let residue = provider.read("projects/P/project.json").unwrap();
    assert!(
        residue.is_none(),
        "legacy residue projects/P/project.json should be deleted"
    );
}
