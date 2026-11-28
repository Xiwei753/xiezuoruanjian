//! Issue #1062 评论 #7213634487 第 C 项：`try_load_node` 必须把已经合法但
//! 在 `references.json` 里缺项/过期的实例反向绑定幂等补齐，同时不改写实例
//! 文件；与实例事实冲突的既有绑定保留材料并明确报错。

use tempfile::TempDir;

use super::super::*;
use crate::starmap::migration::nodes::{NodeObjectBindings, ReferenceBindingState};
use crate::starmap::package_storage;
use crate::starmap::types::{
    StarMapNodeInstance, StarMapNodeReference, StarMapPoint, StarMapReferenceIndex,
    CURRENT_LIBRARY_SCHEMA_VERSION,
};

fn instance(
    starmap_id: &str,
    instance_id: &str,
    asset_id: &str,
    project_id: Option<&str>,
) -> StarMapNodeInstance {
    StarMapNodeInstance {
        instance_id: instance_id.to_string(),
        asset_id: asset_id.to_string(),
        position: StarMapPoint::default(),
        starmap_id: starmap_id.to_string(),
        project_id: project_id.map(str::to_string),
        created_at: 0,
        updated_at: 0,
    }
}

/// 写一份合法的规范实例文件，返回磁盘路径。
fn write_canonical_instance(
    root: &std::path::Path,
    starmap_id: &str,
    instance_id: &str,
    asset_id: &str,
    project_id: Option<&str>,
) -> std::path::PathBuf {
    let value = serde_json::json!({
        "instanceId": instance_id,
        "assetId": asset_id,
        "position": {"x": 1.0, "y": 2.0},
        "starmapId": starmap_id,
        "projectId": project_id,
        "createdAt": 5,
        "updatedAt": 6,
    });
    let path = root.join(package_storage::node_rel_path(starmap_id, instance_id));
    std::fs::create_dir_all(path.parent().expect("instance path has a parent")).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    path
}

fn reference_index(root: &std::path::Path) -> StarMapReferenceIndex {
    crate::starmap::library::load_reference_index(root).unwrap()
}

#[test]
fn canonical_instance_missing_reference_is_repaired_without_rewrite() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "sm_ref_repair";
    let instance_id = "n1";
    let node_path = write_canonical_instance(
        dir.path(),
        starmap_id,
        instance_id,
        "asset_shared",
        Some("p1"),
    );
    let original = std::fs::read(&node_path).unwrap();
    // 索引文件存在且可读，但缺这一条：走单条补写，不做整体重建。
    crate::starmap::library::save_reference_index(dir.path(), &StarMapReferenceIndex::default())
        .unwrap();

    let mut store = StarMapStore::new(dir.path(), starmap_id);
    let node = store
        .try_load_node(instance_id)
        .expect("canonical instance must load");
    assert_eq!(node.id, instance_id);
    assert_eq!(node.asset_id, "asset_shared");

    assert_eq!(
        std::fs::read(&node_path).unwrap(),
        original,
        "补引用不能改写已经合法的实例文件"
    );
    let refs = reference_index(dir.path());
    assert_eq!(refs.nodes.len(), 1);
    assert_eq!(refs.nodes[0].asset_id, "asset_shared");
    assert_eq!(refs.nodes[0].project_id.as_deref(), Some("p1"));
}

#[test]
fn stale_reference_project_is_backfilled_from_instance() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "sm_ref_stale_project";
    let instance_id = "n1";
    let node_path = write_canonical_instance(
        dir.path(),
        starmap_id,
        instance_id,
        "asset_shared",
        Some("p1"),
    );
    let original = std::fs::read(&node_path).unwrap();
    let stale = StarMapReferenceIndex {
        schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
        nodes: vec![StarMapNodeReference {
            asset_id: "asset_shared".to_string(),
            starmap_id: starmap_id.to_string(),
            instance_id: instance_id.to_string(),
            project_id: Some("p_old".to_string()),
        }],
        updated_at: 0,
    };
    crate::starmap::library::save_reference_index(dir.path(), &stale).unwrap();

    let mut store = StarMapStore::new(dir.path(), starmap_id);
    store
        .try_load_node(instance_id)
        .expect("stale projectId must be repairable");

    assert_eq!(
        std::fs::read(&node_path).unwrap(),
        original,
        "补派生字段不能改写实例文件"
    );
    let refs = reference_index(dir.path());
    assert_eq!(refs.nodes.len(), 1);
    assert_eq!(refs.nodes[0].project_id.as_deref(), Some("p1"));
}

#[test]
fn stale_reference_asset_is_rebuilt_from_instance() {
    let dir = TempDir::new().unwrap();
    let starmap_id = "sm_ref_conflict";
    let instance_id = "n1";
    let node_path =
        write_canonical_instance(dir.path(), starmap_id, instance_id, "asset_new", Some("p1"));
    let original = std::fs::read(&node_path).unwrap();
    let stale = StarMapReferenceIndex {
        schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
        nodes: vec![StarMapNodeReference {
            asset_id: "asset_old".to_string(),
            starmap_id: starmap_id.to_string(),
            instance_id: instance_id.to_string(),
            project_id: Some("p1".to_string()),
        }],
        updated_at: 0,
    };
    crate::starmap::library::save_reference_index(dir.path(), &stale).unwrap();

    // 反向索引是派生视图：实例文件是身份权威，索引落后时按实例重建，
    // 不阻断加载。
    let mut store = StarMapStore::new(dir.path(), starmap_id);
    let node = store
        .try_load_node(instance_id)
        .expect("stale reference index must be rebuilt from the instance");
    assert_eq!(node.asset_id, "asset_new");

    assert_eq!(
        std::fs::read(&node_path).unwrap(),
        original,
        "重建派生索引不能改写已经合法的实例文件"
    );
    let refs = reference_index(dir.path());
    assert_eq!(refs.nodes.len(), 1);
    assert_eq!(refs.nodes[0].asset_id, "asset_new");
}

#[test]
fn reference_binding_state_separates_missing_stale_and_consistent() {
    let starmap_id = "sm_state";
    let with_project = instance(starmap_id, "n1", "asset_a", Some("p1"));
    let mut bindings = NodeObjectBindings::new();
    assert_eq!(
        bindings.reference_binding_state(&with_project),
        ReferenceBindingState::Missing
    );

    bindings.remember_reference(StarMapNodeReference {
        asset_id: "asset_a".to_string(),
        starmap_id: starmap_id.to_string(),
        instance_id: "n1".to_string(),
        project_id: Some("p1".to_string()),
    });
    assert_eq!(
        bindings.reference_binding_state(&with_project),
        ReferenceBindingState::Consistent
    );

    bindings.remember_reference(StarMapNodeReference {
        asset_id: "asset_a".to_string(),
        starmap_id: starmap_id.to_string(),
        instance_id: "n1".to_string(),
        project_id: Some("p2".to_string()),
    });
    assert_eq!(
        bindings.reference_binding_state(&with_project),
        ReferenceBindingState::ProjectStale
    );

    // 实例没有项目归属时，索引里的归属是唯一来源，不算待修复。
    let without_project = instance(starmap_id, "n1", "asset_a", None);
    assert_eq!(
        bindings.reference_binding_state(&without_project),
        ReferenceBindingState::Consistent
    );

    bindings.remember_reference(StarMapNodeReference {
        asset_id: "asset_a".to_string(),
        starmap_id: starmap_id.to_string(),
        instance_id: "n1".to_string(),
        project_id: None,
    });
    assert_eq!(
        bindings.reference_binding_state(&with_project),
        ReferenceBindingState::ProjectStale
    );

    bindings.remember_reference(StarMapNodeReference {
        asset_id: "asset_b".to_string(),
        starmap_id: starmap_id.to_string(),
        instance_id: "n1".to_string(),
        project_id: Some("p1".to_string()),
    });
    // 派生索引落后于实例：只是待重建，不是阻断性的身份冲突。
    assert_eq!(
        bindings.reference_binding_state(&with_project),
        ReferenceBindingState::Stale {
            reference_asset_id: "asset_b".to_string(),
        }
    );
}
