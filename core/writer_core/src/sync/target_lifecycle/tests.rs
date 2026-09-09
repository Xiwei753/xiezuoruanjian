use super::*;
use crate::sync::provider::memory::MemoryProvider;

#[test]
fn load_missing_returns_empty_snapshot() {
    let p = MemoryProvider::new();
    let snapshot = load_remote_catalog(&p).unwrap();
    assert!(snapshot.catalog.records.is_empty());
    assert_eq!(snapshot.version.as_str(), "__nonexistent__");
}

#[test]
fn write_load_roundtrip_with_version() {
    let p = MemoryProvider::new();
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
    );
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::delete("projects/p2", "projects/p2", 2000, "dev-2"),
    );
    // 首次写入使用 CreateNew。
    let snapshot = RemoteTargetCatalogSnapshot {
        catalog: catalog.clone(),
        version: RemoteVersion::new("__nonexistent__"),
    };
    write_remote_catalog(&p, &snapshot).unwrap();

    let loaded = load_remote_catalog(&p).unwrap();
    assert_eq!(loaded.catalog, catalog);
    // 版本号不再是 sentinel。
    assert_ne!(loaded.version.as_str(), "__nonexistent__");
}

#[test]
fn cas_write_uses_ifmatch_after_initial() {
    let p = MemoryProvider::new();
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
    );

    // 第一次写入（CreateNew）。
    let snap1 = RemoteTargetCatalogSnapshot {
        catalog: catalog.clone(),
        version: RemoteVersion::new("__nonexistent__"),
    };
    write_remote_catalog(&p, &snap1).unwrap();

    // 读取最新版本。
    let snap2 = load_remote_catalog(&p).unwrap();
    assert_ne!(snap2.version.as_str(), "__nonexistent__");

    // 用正确版本 CAS 写入应成功（CAS retry 会重读最新版本）。
    write_remote_catalog(&p, &snap2).unwrap();

    // 用错误版本 CAS 写入：CAS retry 会重读最新版本并重写，
    // 在 MemoryProvider 单线程环境下总会成功（无并发覆盖）。
    let snap_bad = RemoteTargetCatalogSnapshot {
        catalog: catalog.clone(),
        version: RemoteVersion::new("stale-version"),
    };
    let result = write_remote_catalog(&p, &snap_bad);
    // CAS retry 重读后写入成功（MemoryProvider 无并发冲突）。
    assert!(result.is_ok());
}

#[test]
fn upsert_replaces_same_target_id() {
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
    );
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::delete("projects/p1", "projects/p1", 2000, "dev-2"),
    );
    assert_eq!(catalog.records.len(), 1);
    assert_eq!(catalog.records[0].op, TargetOp::Delete);
}

#[test]
fn merge_picks_later_time() {
    let mut a = TargetLifecycleCatalog::default();
    upsert_record(
        &mut a,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-1"),
    );
    let mut b = TargetLifecycleCatalog::default();
    upsert_record(
        &mut b,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 2000, "dev-2"),
    );
    let merged = merge_catalogs(&[a, b]);
    assert_eq!(merged.records.len(), 1);
    assert_eq!(merged.records[0].updated_at_ms, 2000);
}

#[test]
fn merge_tie_break_by_device_id() {
    let mut a = TargetLifecycleCatalog::default();
    upsert_record(
        &mut a,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-a"),
    );
    let mut b = TargetLifecycleCatalog::default();
    upsert_record(
        &mut b,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1000, "dev-b"),
    );
    let merged = merge_catalogs(&[a, b]);
    assert_eq!(merged.records[0].device_id, "dev-b");
}

#[test]
fn merge_delete_uses_deleted_at_ms() {
    let mut a = TargetLifecycleCatalog::default();
    upsert_record(
        &mut a,
        TargetLifecycleRecord::upsert("projects/p1", "projects/p1", 1500, "dev-1"),
    );
    let mut b = TargetLifecycleCatalog::default();
    upsert_record(
        &mut b,
        TargetLifecycleRecord::delete("projects/p1", "projects/p1", 2000, "dev-2"),
    );
    let merged = merge_catalogs(&[a, b]);
    assert_eq!(merged.records[0].op, TargetOp::Delete);
}

#[test]
fn catalog_has_upsert_works() {
    let mut catalog = TargetLifecycleCatalog::default();
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::delete("projects/p1", "projects/p1", 1000, "dev-1"),
    );
    upsert_record(
        &mut catalog,
        TargetLifecycleRecord::upsert("projects/p2", "projects/p2", 1000, "dev-1"),
    );
    assert!(!catalog_has_upsert(&catalog, "projects/p1"));
    assert!(catalog_has_upsert(&catalog, "projects/p2"));
    assert!(!catalog_has_upsert(&catalog, "projects/p3"));
}

#[test]
fn load_parse_failure_returns_err() {
    let p = MemoryProvider::with_entries([(
        TARGET_CATALOG_REMOTE_PATH.to_string(),
        b"not json".to_vec(),
    )]);
    let result = load_remote_catalog(&p);
    assert!(result.is_err());
}
