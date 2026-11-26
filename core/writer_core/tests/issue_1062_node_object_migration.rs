//! Issue #1062 二.1：旧格式画布节点在加载时经统一迁移入口拆成
//! 共享本体（`starmaps/library/nodes/**`）+ 画布实例。

use serde_json::json;
use writer_core::starmap::create_starmap;
use writer_core::starmap::store::meta::GraphMeta;
use writer_core::starmap::store::StarMapStore;

fn write_json(path: &std::path::Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

/// 旧格式完整节点：没有 `assetId`，有标题/正文/位置等共享内容。
fn legacy_node_json() -> serde_json::Value {
    json!({
        "id": "n1",
        "title": "旧节点",
        "kind": "Note",
        "content": {"type": "empty"},
        "anchors": [],
        "portal": null,
        "position": {"x": 7.0, "y": 9.0},
        "style": {},
        "provenance": {},
        "createdAt": 11,
        "updatedAt": 22
    })
}

#[test]
fn legacy_canvas_node_load_migrates_to_definition_and_instance() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
    let starmap_id = meta.starmap_id.clone();

    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.clone(),
        node_ids: vec!["n1".to_string()],
        ..Default::default()
    };
    write_json(
        &dir.path().join(format!("starmaps/{starmap_id}/graph.json")),
        &serde_json::to_value(&graph).unwrap(),
    );
    // bucket = id 首字节高 4 bit：'n' = 0x6e → "06"。
    let node_rel = format!("starmaps/{starmap_id}/nodes/06/n1.json");
    let node_path = dir.path().join(&node_rel);
    write_json(&node_path, &legacy_node_json());

    let mut store = StarMapStore::new(dir.path(), &starmap_id);
    store
        .load_full()
        .expect("legacy canvas node must load through the migration entry");
    let node = store.get_node("n1").expect("node must be loaded");
    assert_eq!(node.title, "旧节点");
    assert_eq!(node.asset_id, "n1");
    assert_eq!(node.position.x, 7.0);
    assert_eq!(node.position.y, 9.0);

    // 共享本体写进了素材库。
    let definition_path = dir.path().join("starmaps/library/nodes/06/n1.json");
    assert!(
        definition_path.is_file(),
        "shared definition must be written"
    );
    let definition: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&definition_path).unwrap()).unwrap();
    assert_eq!(definition["assetId"], json!("n1"));
    assert_eq!(definition["title"], json!("旧节点"));

    // 画布节点文件重写成了实例，不再保存共享内容。
    let instance: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&node_path).unwrap()).unwrap();
    assert_eq!(instance["instanceId"], json!("n1"));
    assert_eq!(instance["assetId"], json!("n1"));
    assert_eq!(instance["position"]["x"], json!(7.0));
    assert!(
        instance.get("title").is_none(),
        "instance file must not keep shared content: {instance}"
    );

    // 反向引用（关系记录）已登记。
    let references = dir.path().join("starmaps/library/references.json");
    assert!(references.is_file(), "instance reference must be recorded");
    let references: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&references).unwrap()).unwrap();
    assert!(references["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["assetId"] == json!("n1") && record["instanceId"] == json!("n1")));

    // 幂等：再次加载不改写已经迁移好的实例文件。
    let rewritten_bytes = std::fs::read(&node_path).unwrap();
    let mut store2 = StarMapStore::new(dir.path(), &starmap_id);
    store2.load_full().expect("second load must succeed");
    assert_eq!(
        std::fs::read(&node_path).unwrap(),
        rewritten_bytes,
        "new-format instance must not be rewritten"
    );
}
