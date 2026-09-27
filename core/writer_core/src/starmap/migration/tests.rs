use serde_json::json;

use super::*;

fn temp_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("starmaps")).unwrap();
    dir
}

fn write_json(path: &std::path::Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

// ---------------------------------------------------------------------------
// index schema 1 -> 2
// ---------------------------------------------------------------------------

#[test]
fn migrate_index_schema_1_to_2() {
    let dir = temp_root();
    let index_path = dir.path().join("starmaps").join("index.json");

    // 旧格式 index：schemaVersion 1，starmaps 数组，每个 meta 含 isMainForProject + projectId。
    let old_index = json!({
        "schemaVersion": 1,
        "starmaps": [
            {
                "starmapId": "sm_a",
                "title": "A",
                "projectId": "p1",
                "isMainForProject": true,
                "accentColor": "#7B8CDE",
                "createdAt": 100,
                "updatedAt": 200,
            },
            {
                "starmapId": "sm_b",
                "title": "B",
                "projectId": "p1",
                "isMainForProject": false,
                "accentColor": "#7B8CDE",
                "createdAt": 100,
                "updatedAt": 200,
            },
            {
                "starmapId": "sm_c",
                "title": "C",
                "projectId": "p2",
                "isMainForProject": true,
                "accentColor": "#7B8CDE",
                "createdAt": 100,
                "updatedAt": 200,
            },
        ],
        "updatedAt": 300,
    });
    write_json(&index_path, &old_index);

    migrate_index(dir.path()).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    assert_eq!(migrated["schemaVersion"], json!(2));
    assert_eq!(migrated["starmapIds"], json!(["sm_a", "sm_b", "sm_c"]));
    assert_eq!(migrated["mainStarmapByProject"]["p1"], json!("sm_a"));
    assert_eq!(migrated["mainStarmapByProject"]["p2"], json!("sm_c"));
    assert_eq!(migrated["updatedAt"], json!(300));
}

#[test]
fn migrate_index_skips_already_new_schema() {
    let dir = temp_root();
    let index_path = dir.path().join("starmaps").join("index.json");

    let new_index = json!({
        "schemaVersion": 2,
        "starmapIds": ["sm_x"],
        "mainStarmapByProject": {},
        "updatedAt": 999,
    });
    write_json(&index_path, &new_index);

    migrate_index(dir.path()).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    // 没有变化。
    assert_eq!(migrated["schemaVersion"], json!(2));
    assert_eq!(migrated["updatedAt"], json!(999));
}

#[test]
fn migrate_index_skips_missing_file() {
    let dir = temp_root();
    // index.json 不存在，迁移应成功且不创建文件。
    migrate_index(dir.path()).unwrap();
    assert!(!dir.path().join("starmaps").join("index.json").exists());
}

// ---------------------------------------------------------------------------
// GraphMeta schema "3" -> "4"
// ---------------------------------------------------------------------------

/// 构造一个旧 schema "3" 的星图目录，含 graph.json、node、embed、layout。
fn setup_old_starmap(app_data_root: &std::path::Path, starmap_id: &str) {
    let graph_dir = app_data_root.join("starmaps").join(starmap_id);
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(graph_dir.join("embeds")).unwrap();

    // 旧 graph.json：schemaVersion "3"，含 layoutRevision。
    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": starmap_id,
        "nodeIds": ["n1", "n2"],
        "edgeIds": [],
        "embedInstanceIds": ["emb1"],
        "linkIds": [],
        "hyperlinkIds": [],
        "layoutRevision": 5,
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // 旧 layout：layouts/default/nodes/{bucket}.json，含 node 位置。
    let layout_nodes = json!([
        {"nodeId": "n1", "x": 100.0, "y": 200.0, "width": 80.0, "height": 60.0,
         "radius": 0.0, "collapsed": false, "zIndex": 0, "scale": 1.0},
        {"nodeId": "n2", "x": 300.0, "y": 400.0, "width": 80.0, "height": 60.0,
         "radius": 0.0, "collapsed": false, "zIndex": 0, "scale": 1.0},
    ]);
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    // 简化：把所有 layout nodes 放在一个 shard 里（迁移按 nodeId 匹配，不依赖 bucket）。
    write_json(
        &graph_dir
            .join("layouts")
            .join("default")
            .join("nodes")
            .join(format!("{bucket}.json")),
        &layout_nodes,
    );

    // 旧 node JSON：含 displayPolicy/openBehavior，没有 position。
    let node1 = json!({
        "id": "n1",
        "title": "Node 1",
        "kind": "concept",
        "payload": null,
        "tags": [],
        "content": {"kind": "empty"},
        "anchors": [],
        "portal": {
            "destinationStarmapId": "other_sm",
            "destinationTarget": {"kind": "starmap"},
            "mode": "enterPortal",
            "previewPolicy": "inline",
        },
        "displayPolicy": {"visible": true},
        "openBehavior": {"mode": "navigate"},
        "provenance": {"origin": "user"},
        "createdAt": 0,
        "updatedAt": 0,
    });
    let n1_bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(n1_bucket).join("n1.json"),
        &node1,
    );

    let node2 = json!({
        "id": "n2",
        "title": "Node 2",
        "kind": "concept",
        "payload": null,
        "tags": [],
        "content": {"kind": "empty"},
        "anchors": [],
        "portal": null,
        "displayPolicy": {"visible": true},
        "openBehavior": {"mode": "navigate"},
        "provenance": {"origin": "user"},
        "createdAt": 0,
        "updatedAt": 0,
    });
    let n2_bucket = crate::starmap::package_storage::bucket_for_id("n2");
    write_json(
        &graph_dir.join("nodes").join(n2_bucket).join("n2.json"),
        &node2,
    );

    // 旧 embed JSON：含 placement/targetViewport/displayPolicy/openBehavior，没有 position。
    let embed = json!({
        "instanceId": "emb1",
        "targetStarmapId": "child_sm",
        "label": "子图",
        "displayPolicy": {"visible": true},
        "openBehavior": {"mode": "navigate"},
        "placement": {"x": 500.0, "y": 600.0, "width": 300.0, "height": 200.0,
                       "scale": 1.0, "zIndex": 0, "collapsed": false},
        "targetViewport": {"scale": 1.0, "offsetX": 0.0, "offsetY": 0.0},
        "hostPath": {"starmapId": starmap_id, "segments": [], "target": {"kind": "starmap"}},
        "provenance": {"origin": "user"},
        "createdAt": 0,
        "updatedAt": 0,
    });
    let emb_bucket = crate::starmap::package_storage::bucket_for_id("emb1");
    write_json(
        &graph_dir.join("embeds").join(emb_bucket).join("emb1.json"),
        &embed,
    );

    // 旧 viewport 文件。
    let viewport_path = dir_path_session(app_data_root, starmap_id);
    write_json(
        &viewport_path,
        &json!({"scale": 1.0, "offsetX": 0.0, "offsetY": 0.0}),
    );
}

fn dir_path_session(app_data_root: &std::path::Path, starmap_id: &str) -> std::path::PathBuf {
    app_data_root
        .join("session")
        .join("starmaps")
        .join(starmap_id)
        .join("viewport.json")
}

#[test]
fn migrate_starmap_graph_schema_3_to_4() {
    let dir = temp_root();
    setup_old_starmap(dir.path(), "sm_test");

    migrate_one_starmap_graph(dir.path(), "sm_test").unwrap();

    let graph_dir = dir.path().join("starmaps").join("sm_test");

    // graph.json schema 升到 "4"，layoutRevision 被删除。
    let graph_meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(graph_dir.join("graph.json")).unwrap())
            .unwrap();
    assert_eq!(graph_meta["schemaVersion"], json!("4"));
    assert!(graph_meta.get("layoutRevision").is_none());

    // node JSON：position 从 layout 来，displayPolicy/openBehavior 被删除。
    let n1_bucket = crate::starmap::package_storage::bucket_for_id("n1");
    let node1: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("nodes").join(n1_bucket).join("n1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(node1["position"], json!({"x": 100.0, "y": 200.0}));
    assert!(node1.get("displayPolicy").is_none());
    assert!(node1.get("openBehavior").is_none());
    // portal 删除 mode/previewPolicy。
    let portal_keys: Vec<String> = node1["portal"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert!(!portal_keys.contains(&"mode".to_string()));
    assert!(!portal_keys.contains(&"previewPolicy".to_string()));

    let n2_bucket = crate::starmap::package_storage::bucket_for_id("n2");
    let node2: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("nodes").join(n2_bucket).join("n2.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(node2["position"], json!({"x": 300.0, "y": 400.0}));
    assert!(node2.get("displayPolicy").is_none());

    // embed JSON：position 从 placement 来，旧字段被删除。
    let emb_bucket = crate::starmap::package_storage::bucket_for_id("emb1");
    let embed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("embeds").join(emb_bucket).join("emb1.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(embed["position"], json!({"x": 500.0, "y": 600.0}));
    for removed in [
        "placement",
        "targetViewport",
        "displayPolicy",
        "openBehavior",
    ] {
        assert!(
            embed.get(removed).is_none(),
            "embed should not have {removed}"
        );
    }

    // 旧 layouts 目录被删除。
    assert!(!graph_dir.join("layouts").exists());
    // 旧 viewport 文件被删除。
    assert!(!dir_path_session(dir.path(), "sm_test").exists());
}

#[test]
fn migrate_starmap_graph_skips_already_new_schema() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_new");
    std::fs::create_dir_all(&graph_dir).unwrap();

    let graph_meta = json!({
        "schemaVersion": "4",
        "starmapId": "sm_new",
        "nodeIds": [],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    migrate_one_starmap_graph(dir.path(), "sm_new").unwrap();

    // 没有变化。
    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(graph_dir.join("graph.json")).unwrap())
            .unwrap();
    assert_eq!(migrated["schemaVersion"], json!("4"));
}

#[test]
fn migrate_starmap_graph_skips_missing_graph_json() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_empty");
    std::fs::create_dir_all(&graph_dir).unwrap();
    // graph.json 不存在，迁移应成功。
    migrate_one_starmap_graph(dir.path(), "sm_empty").unwrap();
}

#[test]
fn migrate_starmap_graph_node_without_layout_gets_default_position() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_x");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    // graph.json schema "3"，但没有 layouts 目录。
    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_x",
        "nodeIds": ["n1"],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // node 没有 position，也没有 layout。
    let node1 = json!({
        "id": "n1", "title": "N1", "kind": "concept", "payload": null,
        "tags": [], "content": {"kind": "empty"}, "anchors": [], "portal": null,
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &node1,
    );

    migrate_one_starmap_graph(dir.path(), "sm_x").unwrap();

    let migrated: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("nodes").join(bucket).join("n1.json")).unwrap(),
    )
    .unwrap();
    // 没有 layout 时 position 默认 (0, 0)。
    assert_eq!(migrated["position"], json!({"x": 0.0, "y": 0.0}));
}

// ---------------------------------------------------------------------------
// 迁移幂等性
// ---------------------------------------------------------------------------

#[test]
fn migrate_is_idempotent() {
    let dir = temp_root();
    setup_old_starmap(dir.path(), "sm_idem");

    migrate_one_starmap_graph(dir.path(), "sm_idem").unwrap();
    // 第二次迁移应该跳过（已经是 schema "4"）。
    migrate_one_starmap_graph(dir.path(), "sm_idem").unwrap();

    let graph_dir = dir.path().join("starmaps").join("sm_idem");
    let graph_meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(graph_dir.join("graph.json")).unwrap())
            .unwrap();
    assert_eq!(graph_meta["schemaVersion"], json!("4"));
}

// ---------------------------------------------------------------------------
// set_main_starmap_for_project 校验
// ---------------------------------------------------------------------------

#[test]
fn set_main_rejects_unbound_starmap() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    // 不 bind 就直接 set_main 应该 Err。
    let result = crate::starmap::set_main_starmap_for_project(dir.path(), &meta.starmap_id, "p1");
    assert!(result.is_err(), "set_main on unbound starmap must Err");
}

#[test]
fn set_main_succeeds_when_bound() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    crate::starmap::set_main_starmap_for_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    let main = crate::starmap::get_main_starmap_for_project(dir.path(), "p1").unwrap();
    assert!(main.is_some());
    assert_eq!(main.unwrap().starmap_id, meta.starmap_id);
}

#[test]
fn set_main_does_not_modify_target_meta() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    let meta_path = dir
        .path()
        .join("starmaps")
        .join(format!("{}.meta.json", meta.starmap_id));
    let meta_before = std::fs::read_to_string(&meta_path).unwrap();

    crate::starmap::set_main_starmap_for_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    let meta_after = std::fs::read_to_string(&meta_path).unwrap();
    assert_eq!(
        meta_before, meta_after,
        "set_main must not modify target meta"
    );
}

#[test]
fn bind_clears_old_main_when_moving_to_different_project() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    crate::starmap::set_main_starmap_for_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    // 把星图从 p1 迁到 p2，应该清掉 p1 的 main 映射。
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p2").unwrap();

    let main_p1 = crate::starmap::get_main_starmap_for_project(dir.path(), "p1").unwrap();
    assert!(
        main_p1.is_none(),
        "p1 main should be cleared after bind to p2"
    );
}

// ---------------------------------------------------------------------------
// node position finite 校验
// ---------------------------------------------------------------------------

#[test]
fn validate_nodes_rejects_nan_position() {
    use crate::starmap::graph::resolve::GraphResolverContext;
    use crate::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
    use crate::starmap::types::*;

    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();

    let graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: meta.starmap_id.clone(),
        nodes: vec![StarMapNode {
            id: "n1".to_string(),
            title: "N1".to_string(),
            kind: StarMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: None,
            position: StarMapPoint {
                x: f32::NAN,
                y: 0.0,
            },
            style: StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };

    let context = GraphResolverContext::new_disk_only(dir.path());
    let result = crate::starmap::graph::validation::validate_graph(&context, &graph);
    assert!(
        result.is_err(),
        "NaN position must be rejected by validate_graph"
    );
}

#[test]
fn validate_nodes_rejects_infinity_position() {
    use crate::starmap::graph::resolve::GraphResolverContext;
    use crate::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
    use crate::starmap::types::*;

    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();

    let graph = StarMapGraph {
        schema_version: CURRENT_GRAPH_SCHEMA_VERSION,
        starmap_id: meta.starmap_id.clone(),
        nodes: vec![StarMapNode {
            id: "n1".to_string(),
            title: "N1".to_string(),
            kind: StarMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: None,
            position: StarMapPoint {
                x: 0.0,
                y: f32::INFINITY,
            },
            style: StarMapNodeStyle::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }],
        edges: vec![],
        embeds: vec![],
        links: vec![],
        hyperlinks: vec![],
    };

    let context = GraphResolverContext::new_disk_only(dir.path());
    let result = crate::starmap::graph::validation::validate_graph(&context, &graph);
    assert!(
        result.is_err(),
        "Infinity position must be rejected by validate_graph"
    );
}
