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
// index schema 1 -> 3 / 2 -> 3
// ---------------------------------------------------------------------------

#[test]
fn migrate_index_schema_1_to_3() {
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
    // 旧 index 的内联 meta 只是缓存；meta 文件才是真相，真实升级时文件一定存在。
    for (id, title) in [("sm_a", "A"), ("sm_b", "B"), ("sm_c", "C")] {
        write_json(
            &dir.path().join("starmaps").join(format!("{id}.meta.json")),
            &json!({
                "starmapId": id,
                "title": title,
                "description": "",
                "projectId": "p1",
                "accentColor": "#7B8CDE",
                "createdAt": 100,
                "updatedAt": 200,
            }),
        );
    }

    migrate_index(dir.path()).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    assert_eq!(migrated["schemaVersion"], json!(3));
    assert_eq!(migrated["starmapIds"], json!(["sm_a", "sm_b", "sm_c"]));
    assert_eq!(
        migrated["rootStarmapIds"],
        json!(["sm_a", "sm_b", "sm_c"]),
        "没有 graph 数据时，全部 starmap 都是显式一级身份"
    );
    assert_eq!(migrated["mainStarmapByProject"]["p1"], json!("sm_a"));
    assert_eq!(migrated["mainStarmapByProject"]["p2"], json!("sm_c"));
    // 迁移是真实内容变更，updatedAt 必须推进到现在。
    assert!(migrated["updatedAt"].as_u64().unwrap() > 0);
}

#[test]
fn migrate_index_skips_already_new_schema() {
    let dir = temp_root();
    let index_path = dir.path().join("starmaps").join("index.json");

    let new_index = json!({
        "schemaVersion": 3,
        "starmapIds": ["sm_x"],
        "rootStarmapIds": ["sm_x"],
        "mainStarmapByProject": {},
        "updatedAt": 999,
    });
    write_json(&index_path, &new_index);

    migrate_index(dir.path()).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    // 没有变化。
    assert_eq!(migrated["schemaVersion"], json!(3));
    assert_eq!(migrated["updatedAt"], json!(999));
}

/// schema 2（只有 starmap_ids）第一次升级：用 Embed 关系一次性推导 root 集合，
/// 已嵌入的 child 不进入一级列表，迁移结果持久化。
#[test]
fn migrate_index_schema_2_to_3_derives_roots_from_embeds() {
    let dir = temp_root();
    write_v2_index(dir.path(), &["sm_host", "sm_child"]);

    // 构造 host -> child 的 Embed 关系（meta + graph 对象文件）。
    write_starmap_meta(dir.path(), "sm_host", "Host");
    write_starmap_meta(dir.path(), "sm_child", "Child");
    let mut store = crate::starmap::store::StarMapStore::new(dir.path(), "sm_host");
    store
        .add_embed(crate::starmap::types::StarMapEmbed {
            instance_id: "emb_child".to_string(),
            target_starmap_id: "sm_child".to_string(),
            label: None,
            position: crate::starmap::types::StarMapPoint { x: 0.0, y: 0.0 },
            host_path: crate::starmap::types::reference::StarMapTargetPath {
                starmap_id: "sm_host".to_string(),
                segments: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        })
        .unwrap();
    store.flush().unwrap();

    migrate_index(dir.path()).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index_path_of(dir.path())).unwrap()).unwrap();
    assert_eq!(migrated["schemaVersion"], json!(3));
    assert_eq!(migrated["starmapIds"], json!(["sm_host", "sm_child"]));
    assert_eq!(
        migrated["rootStarmapIds"],
        json!(["sm_host"]),
        "被 Embed 的 child 不能靠扫描关系长期过滤，但迁移当次必须得到正确的 root 集合"
    );

    // 迁移后正常运行：一级列表直接读显式 root，不再需要扫描 graph。
    let roots = crate::starmap::list_root_starmaps(dir.path()).unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].starmap_id, "sm_host");
}

/// schema 2 → 3 迁移同样识别旧版"伪子星图"（Note + portal，destinationTarget 为空）：
/// 该目标不进入一级列表。
#[test]
fn migrate_index_schema_2_to_3_excludes_legacy_portal_children() {
    let dir = temp_root();
    write_v2_index(dir.path(), &["sm_host", "sm_legacy_child"]);
    write_starmap_meta(dir.path(), "sm_host", "Host");
    write_starmap_meta(dir.path(), "sm_legacy_child", "Legacy Child");

    let mut store = crate::starmap::store::StarMapStore::new(dir.path(), "sm_host");
    store.add_node(crate::starmap::types::StarMapNode {
        id: "n_portal".to_string(),
        title: "Legacy Child".to_string(),
        kind: crate::starmap::types::StarMapNodeKind::Note,
        payload: None,
        tags: vec![],
        content: Default::default(),
        anchors: vec![],
        portal: Some(crate::starmap::semantic::StarMapPortal {
            destination_starmap_id: "sm_legacy_child".to_string(),
            destination_target: None,
        }),
        position: crate::starmap::types::StarMapPoint { x: 0.0, y: 0.0 },
        style: Default::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    });
    store.flush().unwrap();

    migrate_index(dir.path()).unwrap();

    let migrated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(index_path_of(dir.path())).unwrap()).unwrap();
    assert_eq!(
        migrated["rootStarmapIds"],
        json!(["sm_host"]),
        "legacy portal child 不进入一级列表"
    );
}

fn index_path_of(app_data_root: &std::path::Path) -> std::path::PathBuf {
    app_data_root.join("starmaps").join("index.json")
}

fn write_v2_index(app_data_root: &std::path::Path, starmap_ids: &[&str]) {
    let value = json!({
        "schemaVersion": 2,
        "starmapIds": starmap_ids,
        "mainStarmapByProject": {},
        "updatedAt": 999,
    });
    write_json(&index_path_of(app_data_root), &value);
}

fn write_starmap_meta(app_data_root: &std::path::Path, starmap_id: &str, title: &str) {
    let value = json!({
        "starmapId": starmap_id,
        "title": title,
        "description": "",
        "projectId": null,
        "accentColor": "#7B8CDE",
        "createdAt": 100,
        "updatedAt": 200,
    });
    write_json(
        &app_data_root
            .join("starmaps")
            .join(format!("{}.meta.json", starmap_id)),
        &value,
    );
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
        "content": {"type": "empty"},
        "anchors": [],
        "portal": {
            "destinationStarmapId": "other_sm",
            "destinationTarget": {"type": "starmap"},
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
        "content": {"type": "empty"},
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
        "hostPath": {"starmapId": starmap_id, "segments": [], "target": {"type": "starmap"}},
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
fn migrate_starmap_graph_node_without_layout_errors() {
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
        "tags": [], "content": {"type": "empty"}, "anchors": [], "portal": null,
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &node1,
    );

    // fail-closed：node 无 position 无 layout → Err，不能猜 (0,0)。
    let result = migrate_one_starmap_graph(dir.path(), "sm_x");
    assert!(
        result.is_err(),
        "node without position and without layout must Err, not guess (0,0)"
    );
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

// ---------------------------------------------------------------------------
// 修复 1/2/3：幂等对象迁移、layout 解析失败 Err、revision 推进、meta 重写
// ---------------------------------------------------------------------------

#[test]
fn migrate_node_with_existing_position_keeps_it() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_pos");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_pos",
        "nodeIds": ["n1"],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // node 已有合法 position (50, 60)。
    let node1 = json!({
        "id": "n1", "title": "N1", "kind": "concept", "payload": null,
        "tags": [], "content": {"type": "empty"}, "anchors": [], "portal": null,
        "position": {"x": 50.0, "y": 60.0},
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &node1,
    );

    // 同时提供 layout（位置不同），迁移后应保留 node 已有 position，不被 layout 覆盖。
    let layout_nodes = json!([
        {"nodeId": "n1", "x": 999.0, "y": 888.0, "width": 80.0, "height": 60.0,
         "radius": 0.0, "collapsed": false, "zIndex": 0, "scale": 1.0},
    ]);
    write_json(
        &graph_dir
            .join("layouts")
            .join("default")
            .join("nodes")
            .join(format!("{bucket}.json")),
        &layout_nodes,
    );

    migrate_one_starmap_graph(dir.path(), "sm_pos").unwrap();

    let migrated: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("nodes").join(bucket).join("n1.json")).unwrap(),
    )
    .unwrap();
    // 保留已有 position，不被 layout 覆盖。
    assert_eq!(migrated["position"], json!({"x": 50.0, "y": 60.0}));
}

#[test]
fn migrate_embed_with_existing_position_keeps_it() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_emb_pos");
    std::fs::create_dir_all(graph_dir.join("embeds")).unwrap();

    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_emb_pos",
        "nodeIds": [],
        "edgeIds": [],
        "embedInstanceIds": ["emb1"],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // embed 已有合法 position (70, 80)。
    let embed = json!({
        "instanceId": "emb1",
        "targetStarmapId": "child_sm",
        "label": "子图",
        "position": {"x": 70.0, "y": 80.0},
        "hostPath": {"starmapId": "sm_emb_pos", "segments": [], "target": {"type": "starmap"}},
        "provenance": {"origin": "user"},
        "createdAt": 0,
        "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id("emb1");
    write_json(
        &graph_dir.join("embeds").join(bucket).join("emb1.json"),
        &embed,
    );

    migrate_one_starmap_graph(dir.path(), "sm_emb_pos").unwrap();

    let migrated: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("embeds").join(bucket).join("emb1.json")).unwrap(),
    )
    .unwrap();
    // 保留已有 position。
    assert_eq!(migrated["position"], json!({"x": 70.0, "y": 80.0}));
}

#[test]
fn migrate_embed_without_position_or_placement_errors() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_emb_err");
    std::fs::create_dir_all(graph_dir.join("embeds")).unwrap();

    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_emb_err",
        "nodeIds": [],
        "edgeIds": [],
        "embedInstanceIds": ["emb1"],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // embed 既没有 position 也没有 placement。
    let embed = json!({
        "instanceId": "emb1",
        "targetStarmapId": "child_sm",
        "label": "子图",
        "hostPath": {"starmapId": "sm_emb_err", "segments": [], "target": {"type": "starmap"}},
        "provenance": {"origin": "user"},
        "createdAt": 0,
        "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id("emb1");
    write_json(
        &graph_dir.join("embeds").join(bucket).join("emb1.json"),
        &embed,
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_emb_err");
    assert!(
        result.is_err(),
        "embed without position or placement must Err, not guess (0,0)"
    );
}

#[test]
fn migrate_corrupt_layout_errors() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_corrupt_layout");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_corrupt_layout",
        "nodeIds": ["n1"],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // 损坏的 layout shard JSON（不是合法 JSON 数组）。
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    std::fs::create_dir_all(graph_dir.join("layouts").join("default").join("nodes")).unwrap();
    std::fs::write(
        graph_dir
            .join("layouts")
            .join("default")
            .join("nodes")
            .join(format!("{bucket}.json")),
        "this is not valid json {{{",
    )
    .unwrap();

    let result = migrate_one_starmap_graph(dir.path(), "sm_corrupt_layout");
    assert!(
        result.is_err(),
        "corrupt layout shard must Err, not unwrap_or_default"
    );
}

#[test]
fn migrate_advances_package_revision() {
    let dir = temp_root();
    setup_old_starmap(dir.path(), "sm_rev");

    migrate_one_starmap_graph(dir.path(), "sm_rev").unwrap();

    let graph_dir = dir.path().join("starmaps").join("sm_rev");
    let graph_meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(graph_dir.join("graph.json")).unwrap())
            .unwrap();

    // packageRevision = old(1) + 1 = 2。
    assert_eq!(graph_meta["packageRevision"], json!(2));

    // 所有迁过的 node 的 revision = 2。
    let node_revisions = graph_meta["nodeRevisions"].as_object().unwrap();
    assert_eq!(node_revisions["n1"], json!(2));
    assert_eq!(node_revisions["n2"], json!(2));

    // 所有迁过的 embed 的 revision = 2。
    let embed_revisions = graph_meta["embedRevisions"].as_object().unwrap();
    assert_eq!(embed_revisions["emb1"], json!(2));

    // updatedAt 被推进。
    let updated_at = graph_meta["updatedAt"].as_u64().unwrap();
    assert!(updated_at > 0, "updatedAt must be advanced to now");
}

#[test]
fn migrate_index_rewrites_meta_files() {
    let dir = temp_root();
    let index_path = dir.path().join("starmaps").join("index.json");

    // 旧格式 index + 旧 meta 文件含废弃字段。
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
                "nodeCount": 5,
                "edgeCount": 3,
                "linkedChapterCount": 2,
            },
        ],
        "updatedAt": 300,
    });
    write_json(&index_path, &old_index);

    // 写一个含废弃字段的 meta 文件。
    let meta_path = dir.path().join("starmaps").join("sm_a.meta.json");
    write_json(
        &meta_path,
        &json!({
            "starmapId": "sm_a",
            "title": "A",
            "description": "desc",
            "projectId": "p1",
            "accentColor": "#7B8CDE",
            "createdAt": 100,
            "updatedAt": 200,
            "isMainForProject": true,
            "nodeCount": 5,
            "edgeCount": 3,
            "linkedChapterCount": 2,
        }),
    );

    migrate_index(dir.path()).unwrap();

    let migrated_meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&meta_path).unwrap()).unwrap();
    // 只保留 7 个字段。
    let keys: Vec<String> = migrated_meta.as_object().unwrap().keys().cloned().collect();
    for removed in [
        "isMainForProject",
        "nodeCount",
        "edgeCount",
        "linkedChapterCount",
    ] {
        assert!(
            !keys.contains(&removed.to_string()),
            "meta should not have {removed} after migration"
        );
    }
    assert_eq!(migrated_meta["starmapId"], json!("sm_a"));
    assert_eq!(migrated_meta["title"], json!("A"));
    assert_eq!(migrated_meta["description"], json!("desc"));
    assert_eq!(migrated_meta["projectId"], json!("p1"));
    assert_eq!(migrated_meta["accentColor"], json!("#7B8CDE"));
}

#[test]
fn migrate_is_safe_to_rerun_after_partial_failure() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_rerun");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_rerun",
        "nodeIds": ["n1"],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // node 没有 position，有 layout → 第一次迁移会从 layout 提取 position。
    let layout_nodes = json!([
        {"nodeId": "n1", "x": 100.0, "y": 200.0, "width": 80.0, "height": 60.0,
         "radius": 0.0, "collapsed": false, "zIndex": 0, "scale": 1.0},
    ]);
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir
            .join("layouts")
            .join("default")
            .join("nodes")
            .join(format!("{bucket}.json")),
        &layout_nodes,
    );

    let node1 = json!({
        "id": "n1", "title": "N1", "kind": "concept", "payload": null,
        "tags": [], "content": {"type": "empty"}, "anchors": [], "portal": null,
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &node1,
    );

    // 第一次迁移：从 layout 提取 position (100, 200)。
    // 但模拟"第 5 步写 graph.json schema 4 失败"——我们手动只迁 node，不迁 graph.json。
    // 实际上我们直接调 migrate_node_json 模拟部分迁移。
    {
        let layout_positions = super::read_layout_positions(&graph_dir).unwrap();
        super::migrate_node_json(&graph_dir, "n1", &layout_positions).unwrap();
        // 故意不写 graph.json schema 4，模拟中途失败。schema 仍是 "3"。
    }

    // 此时 node 已有 position (100, 200)。
    let after_first: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("nodes").join(bucket).join("n1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(after_first["position"], json!({"x": 100.0, "y": 200.0}));

    // 手动把 position 改成非零值 (500, 600)，模拟用户后续移动了节点。
    let mut modified = after_first.clone();
    modified["position"] = json!({"x": 500.0, "y": 600.0});
    std::fs::write(
        graph_dir.join("nodes").join(bucket).join("n1.json"),
        serde_json::to_string_pretty(&modified).unwrap(),
    )
    .unwrap();

    // 重跑迁移（schema 仍是 "3"）。已有合法 position (500, 600) 应保留，不被覆盖。
    migrate_one_starmap_graph(dir.path(), "sm_rerun").unwrap();

    let after_rerun: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(graph_dir.join("nodes").join(bucket).join("n1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        after_rerun["position"],
        json!({"x": 500.0, "y": 600.0}),
        "rerun must keep existing valid position, not overwrite with layout"
    );
}

// ---------------------------------------------------------------------------
// 修复 4：add_starmap_node 的 finite 校验
// ---------------------------------------------------------------------------

#[test]
fn add_starmap_node_with_nan_x_rejected() {
    use crate::facade::WriterCore;
    use crate::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
    use crate::starmap::types::*;

    let dir = temp_root();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let core = WriterCore::new(dir.path(), dir.path().join("projects"));
    let meta = core.create_starmap("S", "", None).unwrap();

    let node = StarMapNode {
        id: "n1".to_string(),
        title: "N1".to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        tags: vec![],
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: StarMapPoint::default(),
        style: StarMapNodeStyle::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    };

    // default_x = NaN 现在会被合进 node.position 再校验，应被拒绝。
    let result = core.add_starmap_node(&meta.starmap_id, node, f32::NAN, 0.0);
    assert!(
        result.is_err(),
        "add_starmap_node with NaN default_x must be rejected by validate_graph"
    );
}

// ---------------------------------------------------------------------------
// 修复 5：bind/unbind 顺序和 change set
// ---------------------------------------------------------------------------

#[test]
fn bind_clears_old_main_before_writing_meta() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    crate::starmap::set_main_starmap_for_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    // 把星图从 p1 迁到 p2，应先清 p1 的 main 映射再写 meta。
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p2").unwrap();

    // 最终不变量：p1 没有 main，meta.project_id == p2。
    let main_p1 = crate::starmap::get_main_starmap_for_project(dir.path(), "p1").unwrap();
    assert!(main_p1.is_none(), "p1 main should be cleared");
    let meta_after = crate::starmap::get_starmap(dir.path(), &meta.starmap_id).unwrap();
    assert_eq!(meta_after.project_id.as_deref(), Some("p2"));
}

#[test]
fn bind_with_changes_no_index_change_when_same_project() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    // 再次 bind 到相同 project，change set 不应含 index。
    let change_set =
        crate::starmap::bind_starmap_to_project_with_changes(dir.path(), &meta.starmap_id, "p1")
            .unwrap();
    let paths = change_set.to_flat_paths();
    let has_index = paths
        .iter()
        .any(|p| p == &std::path::PathBuf::from("starmaps/index.json"));
    assert!(
        !has_index,
        "change set must not contain index when binding to same project"
    );
}

#[test]
fn unbind_with_changes_no_index_change_when_not_main() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    // 不设为 main。

    // unbind 一个不是 main 的星图，change set 不应含 index。
    let change_set =
        crate::starmap::unbind_starmap_from_project_with_changes(dir.path(), &meta.starmap_id)
            .unwrap();
    let paths = change_set.to_flat_paths();
    let has_index = paths
        .iter()
        .any(|p| p == &std::path::PathBuf::from("starmaps/index.json"));
    assert!(
        !has_index,
        "change set must not contain index when unbinding non-main starmap"
    );
}

#[test]
fn set_main_with_changes_empty_when_already_main() {
    let dir = temp_root();
    let meta = crate::starmap::create_starmap(dir.path(), "S", "", None).unwrap();
    crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "p1").unwrap();
    crate::starmap::set_main_starmap_for_project(dir.path(), &meta.starmap_id, "p1").unwrap();

    // 再次 set_main 相同星图，change set 应为空。
    let change_set = crate::starmap::set_main_starmap_for_project_with_changes(
        dir.path(),
        &meta.starmap_id,
        "p1",
    )
    .unwrap();
    assert!(
        change_set.is_empty(),
        "change set must be empty when starmap is already main"
    );
}

// ---------------------------------------------------------------------------
// 修复：migrate_index fail-closed 版本策略
// ---------------------------------------------------------------------------

#[test]
fn migrate_index_rejects_unknown_schema_version() {
    let dir = temp_root();
    let index_path = dir.path().join("starmaps").join("index.json");

    // schemaVersion=99 是未知未来版本，fail-closed 应返回 Err(UnsupportedVersion)。
    let unknown_index = json!({
        "schemaVersion": 99,
        "starmapIds": ["sm_x"],
        "mainStarmapByProject": {},
        "updatedAt": 999,
    });
    write_json(&index_path, &unknown_index);

    let result = migrate_index(dir.path());
    assert!(
        matches!(result, Err(crate::error::Error::UnsupportedVersion { .. })),
        "unknown schemaVersion must be rejected with UnsupportedVersion, got: {result:?}"
    );

    // 文件应保持原样，没有被静默改写。
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    assert_eq!(after["schemaVersion"], json!(99));
}

#[test]
fn migrate_index_rejects_missing_schema_version_field() {
    let dir = temp_root();
    let index_path = dir.path().join("starmaps").join("index.json");

    // 完全没有 schemaVersion 字段，fail-closed 应返回 Err(UnsupportedVersion)。
    let missing_version_index = json!({
        "starmapIds": ["sm_x"],
        "mainStarmapByProject": {},
        "updatedAt": 999,
    });
    write_json(&index_path, &missing_version_index);

    let result = migrate_index(dir.path());
    assert!(
        matches!(result, Err(crate::error::Error::UnsupportedVersion { .. })),
        "missing schemaVersion field must be rejected with UnsupportedVersion, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Issue #781 评论 5863487463 复现测试
// ---------------------------------------------------------------------------
//
// 这组测试断言 issue 要求的 fail-closed / 数据完整性行为。
// 当前代码违反这些要求（静默猜 (0,0)、if let 跳过坏 x/y），
// 因此这些测试在未修复的代码上会失败，从而证明 bug 存在。
// 修复后这些测试应当通过。

/// 复现问题 1：旧 Node 迁移在缺失 layout 时静默猜成 (0,0)。
///
/// 场景：schema "3" graph.json 声明 node n1，n1 无 position 且无 layout。
/// 期望：migrate_one_starmap_graph 返回 Err（不能把"迁移源数据缺失"伪装成合法 authored position）。
/// 当前错误行为：返回 Ok 且 position 被写成 (0,0)。
#[test]
fn repro_781_migration_missing_layout_must_error() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_repro1");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    // schema "3" graph.json 声明 node n1，但没有 layouts 目录。
    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_repro1",
        "nodeIds": ["n1"],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // node n1 没有 position，也没有 layout 提供位置。
    let node1 = json!({
        "id": "n1", "title": "N1", "kind": "concept", "payload": null,
        "tags": [], "content": {"type": "empty"}, "anchors": [], "portal": null,
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &node1,
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_repro1");

    // 期望：fail-closed 返回 Err。
    // 当前 bug 行为：返回 Ok，且 position 被静默写成 (0,0)。
    assert!(
        result.is_err(),
        "repro_781: migrate_one_starmap_graph must Err when node has no position and no layout, \
         but got Ok (current bug: silently guesses (0,0)). result={result:?}"
    );
}

/// 复现问题 1（补充）：read_layout_positions 对 shard 内声明的坏 x/y 必须 fail-closed。
///
/// 场景：layout shard 声明 nodeId="n1" 但 x 字段为 null（坏 x/y）。
/// 期望：migrate_one_starmap_graph 返回 Err（不能 if let 跳过坏 x/y 然后猜 (0,0)）。
/// 当前错误行为：read_layout_positions 用 if let 跳过坏 x/y，
///   layout_positions 为空，migrate_node_json 用 unwrap_or((0,0)) → Ok 且 position=(0,0)。
#[test]
fn repro_781_read_layout_positions_bad_xy_must_error() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_repro3");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    let graph_meta = json!({
        "schemaVersion": "3",
        "starmapId": "sm_repro3",
        "nodeIds": ["n1"],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    write_json(&graph_dir.join("graph.json"), &graph_meta);

    // layout shard 声明 n1，但 x 是 null（坏 x/y）。
    let layout_nodes = json!([
        {"nodeId": "n1", "x": null, "y": 200.0, "width": 80.0, "height": 60.0,
         "radius": 0.0, "collapsed": false, "zIndex": 0, "scale": 1.0},
    ]);
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir
            .join("layouts")
            .join("default")
            .join("nodes")
            .join(format!("{bucket}.json")),
        &layout_nodes,
    );

    // node n1 没有 position，需要从 layout 提取，但 layout 的 x 是 null。
    let node1 = json!({
        "id": "n1", "title": "N1", "kind": "concept", "payload": null,
        "tags": [], "content": {"type": "empty"}, "anchors": [], "portal": null,
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    let n1_bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(n1_bucket).join("n1.json"),
        &node1,
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_repro3");

    // 期望：fail-closed 返回 Err（layout 声明了 n1 但 x/y 坏）。
    // 当前 bug 行为：read_layout_positions 用 if let 跳过坏 x/y，
    //   migrate_node_json 用 unwrap_or((0,0)) → Ok 且 position=(0,0)。
    assert!(
        result.is_err(),
        "repro_781: migrate_one_starmap_graph must Err when layout declares nodeId but has bad x/y, \
         but got Ok (current bug: if let skips bad x/y then guesses (0,0)). result={result:?}"
    );
}

// ---------------------------------------------------------------------------
// 评论 5863582949：旧 layouts/default/** 只能在所有 Node 和 Embed 迁移成功后删除
// ---------------------------------------------------------------------------

/// 写 schema "3" 的 graph.json。`members = None` 表示完全省略成员列表字段。
fn write_schema3_graph_json(
    graph_dir: &std::path::Path,
    starmap_id: &str,
    members: Option<(serde_json::Value, serde_json::Value)>,
) {
    let mut meta = json!({
        "schemaVersion": "3",
        "starmapId": starmap_id,
        "edgeIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    if let Some((node_ids, embed_ids)) = members {
        meta["nodeIds"] = node_ids;
        meta["embedInstanceIds"] = embed_ids;
    }
    write_json(&graph_dir.join("graph.json"), &meta);
}

/// 写一份旧 layout shard，声明每个 node 的 x/y。
fn write_layout_shard(graph_dir: &std::path::Path, entries: &[(&str, f64, f64)]) {
    let nodes: Vec<serde_json::Value> = entries
        .iter()
        .map(|(node_id, x, y)| {
            json!({
                "nodeId": node_id, "x": x, "y": y, "width": 80.0, "height": 60.0,
                "radius": 0.0, "collapsed": false, "zIndex": 0, "scale": 1.0,
            })
        })
        .collect();
    let Some((first_id, _, _)) = entries.first() else {
        return;
    };
    let bucket = crate::starmap::package_storage::bucket_for_id(first_id);
    write_json(
        &graph_dir
            .join("layouts")
            .join("default")
            .join("nodes")
            .join(format!("{bucket}.json")),
        &serde_json::Value::Array(nodes),
    );
}

/// 写一个没有 position 的旧 node JSON 文件（旧数据需要从 layout 迁位置）。
fn write_legacy_node_file(graph_dir: &std::path::Path, node_id: &str) {
    let node = json!({
        "id": node_id, "title": node_id, "kind": "concept", "payload": null,
        "tags": [], "content": {"type": "empty"}, "anchors": [], "portal": null,
        "provenance": {"origin": "user"}, "createdAt": 0, "updatedAt": 0,
    });
    let bucket = crate::starmap::package_storage::bucket_for_id(node_id);
    write_json(
        &graph_dir
            .join("nodes")
            .join(bucket)
            .join(format!("{node_id}.json")),
        &node,
    );
}

/// 读回 graph.json 的 schemaVersion。
fn read_graph_schema_version(graph_dir: &std::path::Path) -> serde_json::Value {
    let content = std::fs::read_to_string(graph_dir.join("graph.json")).unwrap();
    let meta: serde_json::Value = serde_json::from_str(&content).unwrap();
    meta["schemaVersion"].clone()
}

#[test]
fn migrate_declared_node_without_file_errors_and_keeps_legacy_layout() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_missing_node");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    // GraphMeta 声明 n1、n2，但只写了 n1 的对象文件。
    write_schema3_graph_json(
        &graph_dir,
        "sm_missing_node",
        Some((json!(["n1", "n2"]), json!([]))),
    );
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0), ("n2", 300.0, 400.0)]);
    write_legacy_node_file(&graph_dir, "n1");

    let result = migrate_one_starmap_graph(dir.path(), "sm_missing_node");
    assert!(
        result.is_err(),
        "declared node without object file must Err, got: {result:?}"
    );

    // 旧 layout 是 n2 position 的唯一来源，缺文件时必须保留。
    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when a declared node was not migrated"
    );
    assert_eq!(
        read_graph_schema_version(&graph_dir),
        json!("3"),
        "schema must not advance when migration did not finish"
    );
}

#[test]
fn migrate_declared_embed_without_file_errors_and_keeps_legacy_layout() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_missing_embed");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(graph_dir.join("embeds")).unwrap();

    write_schema3_graph_json(
        &graph_dir,
        "sm_missing_embed",
        Some((json!(["n1"]), json!(["emb1"]))),
    );
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0)]);
    write_legacy_node_file(&graph_dir, "n1");
    // 故意不写 embeds/emb1.json。
    write_json(
        &dir_path_session(dir.path(), "sm_missing_embed"),
        &json!({"scale": 1.0, "offsetX": 0.0, "offsetY": 0.0}),
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_missing_embed");
    assert!(
        result.is_err(),
        "declared embed without object file must Err, got: {result:?}"
    );

    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when a declared embed was not migrated"
    );
    assert!(
        dir_path_session(dir.path(), "sm_missing_embed").exists(),
        "legacy viewport must be kept when migration did not finish"
    );
    assert_eq!(
        read_graph_schema_version(&graph_dir),
        json!("3"),
        "schema must not advance when migration did not finish"
    );
}

#[test]
fn migrate_missing_member_list_errors_and_keeps_legacy_layout() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_no_members");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    // 成员列表缺失 → 无法知道要迁哪些 node，必须 Err，
    // 不能当成空集合然后在什么都没迁的情况下删掉旧 layout。
    write_schema3_graph_json(&graph_dir, "sm_no_members", None);
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0)]);
    write_legacy_node_file(&graph_dir, "n1");

    let result = migrate_one_starmap_graph(dir.path(), "sm_no_members");
    assert!(
        result.is_err(),
        "missing nodeIds member list must Err, got: {result:?}"
    );
    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when the member list cannot be parsed"
    );
    assert_eq!(read_graph_schema_version(&graph_dir), json!("3"));
}

#[test]
fn migrate_non_string_member_id_errors() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_bad_member");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    // 成员列表里混入非字符串条目 → 成员集合不完整，必须 Err。
    write_schema3_graph_json(
        &graph_dir,
        "sm_bad_member",
        Some((json!(["n1", 42]), json!([]))),
    );
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0)]);
    write_legacy_node_file(&graph_dir, "n1");

    let result = migrate_one_starmap_graph(dir.path(), "sm_bad_member");
    assert!(
        result.is_err(),
        "non-string member id must Err, got: {result:?}"
    );
    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when the member list is corrupt"
    );
}

// ---------------------------------------------------------------------------
// Issue #787 评论 5865110129：声明的 node/embed 文件是合法 JSON 但根节点不是
// object（[] / null）时，migration 必须 Err 并保留 legacy layout
// ---------------------------------------------------------------------------

/// GraphMeta 声明 n1，但 n1.json 内容是 `[]`（合法 JSON 但不是 object）。
/// 期望：migrate_one_starmap_graph 返回 Err，旧 layout 保留，schema 仍是 "3"。
#[test]
fn migrate_declared_node_with_non_object_json_errors_and_keeps_legacy_layout() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_node_non_object");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    write_schema3_graph_json(
        &graph_dir,
        "sm_node_non_object",
        Some((json!(["n1"]), json!([]))),
    );
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0)]);
    // n1.json 是 []（合法 JSON，但根节点不是 object）。
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &json!([]),
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_node_non_object");
    assert!(
        result.is_err(),
        "declared node with non-object JSON root must Err, got: {result:?}"
    );

    // 旧 layout 是 n1 position 的唯一来源，迁移失败时必须保留。
    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when a declared node has non-object JSON"
    );
    assert_eq!(
        read_graph_schema_version(&graph_dir),
        json!("3"),
        "schema must not advance when migration did not finish"
    );
}

/// GraphMeta 声明 n1，但 n1.json 内容是 `null`（合法 JSON 但不是 object）。
/// 期望：migrate_one_starmap_graph 返回 Err，旧 layout 保留，schema 仍是 "3"。
#[test]
fn migrate_declared_node_with_null_json_errors() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_node_null");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();

    write_schema3_graph_json(&graph_dir, "sm_node_null", Some((json!(["n1"]), json!([]))));
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0)]);
    // n1.json 是 null（合法 JSON，但根节点不是 object）。
    let bucket = crate::starmap::package_storage::bucket_for_id("n1");
    write_json(
        &graph_dir.join("nodes").join(bucket).join("n1.json"),
        &serde_json::Value::Null,
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_node_null");
    assert!(
        result.is_err(),
        "declared node with null JSON root must Err, got: {result:?}"
    );
    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when a declared node has null JSON"
    );
    assert_eq!(
        read_graph_schema_version(&graph_dir),
        json!("3"),
        "schema must not advance when migration did not finish"
    );
}

/// GraphMeta 声明 n1（合法 node）和 emb1，但 emb1.json 内容是 `null`
/// （合法 JSON 但不是 object）。node n1 能迁移成功，证明 Err 来自 embed。
/// 期望：migrate_one_starmap_graph 返回 Err，旧 layout 保留，schema 仍是 "3"。
#[test]
fn migrate_declared_embed_with_non_object_json_errors_and_keeps_legacy_layout() {
    let dir = temp_root();
    let graph_dir = dir.path().join("starmaps").join("sm_embed_non_object");
    std::fs::create_dir_all(graph_dir.join("nodes")).unwrap();
    std::fs::create_dir_all(graph_dir.join("embeds")).unwrap();

    write_schema3_graph_json(
        &graph_dir,
        "sm_embed_non_object",
        Some((json!(["n1"]), json!(["emb1"]))),
    );
    write_layout_shard(&graph_dir, &[("n1", 100.0, 200.0)]);
    // n1 是合法 node，能迁移成功——这样能证明 Err 来自 embed 的非 object JSON。
    write_legacy_node_file(&graph_dir, "n1");
    // emb1.json 是 null（合法 JSON，但根节点不是 object）。
    let emb_bucket = crate::starmap::package_storage::bucket_for_id("emb1");
    write_json(
        &graph_dir.join("embeds").join(emb_bucket).join("emb1.json"),
        &serde_json::Value::Null,
    );

    let result = migrate_one_starmap_graph(dir.path(), "sm_embed_non_object");
    assert!(
        result.is_err(),
        "declared embed with non-object JSON root must Err, got: {result:?}"
    );

    assert!(
        graph_dir.join("layouts").exists(),
        "legacy layouts must be kept when a declared embed has non-object JSON"
    );
    assert_eq!(
        read_graph_schema_version(&graph_dir),
        json!("3"),
        "schema must not advance when migration did not finish"
    );
}
