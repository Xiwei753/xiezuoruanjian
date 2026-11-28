//! Issue #1062 评论 #7236446138 第 2 项（P0）：
//! 只移动"本体尚未到达"的星图节点，不得把临时占位本体持久化为真实共享素材。
//!
//! 断言三条边界：
//! 1. 实例存在、共享本体缺失时，读模型显式是
//!    [`NodeDefinitionState::ProvisionalMissingDefinition`]；只改 position 后
//!    `flush_save_queue` 只写画布实例，素材库里不会出现该 `asset_id` 的本体。
//! 2. 显式 `upsert_node`（本地新建的创建路径）仍会建立共享本体。
//! 3. 真正本体到达后，按 `asset_id` 重新解析即变回
//!    [`NodeDefinitionState::Resolved`]，不产生任何上推。

use std::path::{Path, PathBuf};

use serde_json::json;
use tempfile::TempDir;
use writer_core::starmap::create_starmap;
use writer_core::starmap::semantic::{StarMapNodeContent, StarMapProvenance};
use writer_core::starmap::store::meta::GraphMeta;
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::{
    NodeDefinitionState, NodeInstancePatch, StarMapNode, StarMapNodeDefinition, StarMapNodeKind,
    StarMapNodeStyle, StarMapPoint,
};

/// 画布实例引用、但素材库里还没有文件的共享本体 ID。
const PENDING_ASSET_ID: &str = "asset_pending";
const INSTANCE_ID: &str = "n1";

fn write_json(path: &Path, value: &serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

/// 与 `package_storage::bucket_for_id` 相同的分桶规则：首字节高 4 bit。
fn bucket_for(id: &str) -> String {
    match id.as_bytes().first() {
        None => "00".to_string(),
        Some(byte) => format!("{:02x}", (byte >> 4) & 0x0F),
    }
}

fn instance_path(dir: &Path, starmap_id: &str) -> PathBuf {
    dir.join(format!(
        "starmaps/{starmap_id}/nodes/{}/{INSTANCE_ID}.json",
        bucket_for(INSTANCE_ID)
    ))
}

fn definition_path(dir: &Path, asset_id: &str) -> PathBuf {
    dir.join(format!(
        "starmaps/library/nodes/{}/{asset_id}.json",
        bucket_for(asset_id)
    ))
}

/// 建一个空星图包（有效 `graph.json`，没有任何节点成员）。
fn empty_starmap() -> (TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = create_starmap(dir.path(), "占位节点", "", None).unwrap();
    let starmap_id = meta.starmap_id.clone();
    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.clone(),
        ..Default::default()
    };
    write_json(
        &dir.path().join(format!("starmaps/{starmap_id}/graph.json")),
        &serde_json::to_value(&graph).unwrap(),
    );
    (dir, starmap_id)
}

/// 实例先于共享本体存在：`n1` 的实例文件在画布上，素材库里没有
/// `asset_pending` 的本体。
fn provisional_fixture() -> (TempDir, String) {
    let (dir, starmap_id) = empty_starmap();
    let graph = GraphMeta {
        schema_version: "4".to_string(),
        starmap_id: starmap_id.clone(),
        node_ids: vec![INSTANCE_ID.to_string()],
        ..Default::default()
    };
    write_json(
        &dir.path().join(format!("starmaps/{starmap_id}/graph.json")),
        &serde_json::to_value(&graph).unwrap(),
    );
    write_json(
        &instance_path(dir.path(), &starmap_id),
        &json!({
            "instanceId": INSTANCE_ID,
            "assetId": PENDING_ASSET_ID,
            "position": {"x": 12.0, "y": 34.0},
            "starmapId": starmap_id,
            "createdAt": 7,
            "updatedAt": 9
        }),
    );
    (dir, starmap_id)
}

fn explicit_new_node() -> StarMapNode {
    StarMapNode {
        id: "n_new".to_string(),
        asset_id: "asset_new".to_string(),
        title: "显式新建节点".to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        content: StarMapNodeContent::Empty,
        anchors: Vec::new(),
        portal: None,
        position: StarMapPoint { x: 1.0, y: 2.0 },
        style: StarMapNodeStyle::default(),
        provenance: StarMapProvenance::default(),
        created_at: 5,
        updated_at: 6,
        definition_revision: 0,
    }
}

#[test]
fn moving_provisional_node_never_creates_shared_definition() {
    let (dir, starmap_id) = provisional_fixture();
    let mut store = StarMapStore::new(dir.path(), &starmap_id);
    store.load_full().expect("provisional instance must load");

    // 1. 读模型显式标记"本体尚未到达"，保留真实 asset_id。
    assert_eq!(
        store.node_definition_state(INSTANCE_ID),
        NodeDefinitionState::ProvisionalMissingDefinition
    );
    let read = store.current_node_read(INSTANCE_ID).unwrap();
    assert!(read.is_provisional());
    assert_eq!(
        read.definition_state,
        NodeDefinitionState::ProvisionalMissingDefinition
    );
    assert_eq!(read.node.asset_id, PENDING_ASSET_ID);

    // 占位视图不伪造共享内容：标题退回稳定 asset_id，内容为空。
    let placeholder = store.get_node(INSTANCE_ID).unwrap();
    assert_eq!(placeholder.title, PENDING_ASSET_ID);
    assert_eq!(placeholder.kind, StarMapNodeKind::Note);
    assert!(matches!(placeholder.content, StarMapNodeContent::Empty));

    // 2. 只移动实例。
    store
        .update_node_instance(
            INSTANCE_ID,
            &NodeInstancePatch {
                position: Some(StarMapPoint { x: 55.0, y: 66.0 }),
            },
            None,
        )
        .expect("moving a provisional instance must succeed");
    assert!(store.is_dirty());
    store
        .flush_save_queue()
        .expect("flushing an instance-only change must succeed");

    // 3. flush 没有从占位读模型补写共享本体。
    assert!(
        !definition_path(dir.path(), PENDING_ASSET_ID).exists(),
        "flush must not create a shared definition from the provisional view"
    );
    assert!(
        writer_core::starmap::library::load_node_definition(dir.path(), PENDING_ASSET_ID)
            .unwrap()
            .is_none()
    );

    // 4. 实例文件只保存实例事实：位置更新了，共享内容与占位状态都没进磁盘。
    let instance: serde_json::Value =
        serde_json::from_slice(&std::fs::read(instance_path(dir.path(), &starmap_id)).unwrap())
            .unwrap();
    assert_eq!(instance["assetId"], json!(PENDING_ASSET_ID));
    assert_eq!(instance["position"]["x"], json!(55.0));
    assert_eq!(instance["position"]["y"], json!(66.0));
    assert!(instance.get("title").is_none());
    assert!(instance.get("kind").is_none());
    assert!(instance.get("definitionState").is_none());
    assert!(instance.get("definition_state").is_none());
    assert!(instance.get("provisional").is_none());

    let graph_json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join(format!("starmaps/{starmap_id}/graph.json"))).unwrap(),
    )
    .unwrap();
    assert!(!graph_json.to_string().contains("definitionState"));
    assert!(!graph_json.to_string().contains("provisional"));

    // 5. 本体仍未到达，状态保持占位。
    assert_eq!(
        store.node_definition_state(INSTANCE_ID),
        NodeDefinitionState::ProvisionalMissingDefinition
    );
    assert!(store.node_definition_state(INSTANCE_ID).is_provisional());
}

#[test]
fn explicit_upsert_still_creates_shared_definition() {
    let (dir, starmap_id) = empty_starmap();
    let mut store = StarMapStore::new(dir.path(), &starmap_id);

    store.upsert_node(explicit_new_node());

    // 显式新建入口允许从调用方提供的完整节点建立共享本体。
    let definition = writer_core::starmap::library::load_node_definition(dir.path(), "asset_new")
        .unwrap()
        .expect("explicit upsert must create the shared definition");
    assert_eq!(definition.asset_id, "asset_new");
    assert_eq!(definition.title, "显式新建节点");
    assert_eq!(definition.kind, StarMapNodeKind::Concept);
    assert_eq!(
        store.node_definition_state("n_new"),
        NodeDefinitionState::Resolved
    );
    assert!(!store.current_node_read("n_new").unwrap().is_provisional());

    // flush 仍然只写实例，不覆盖已建立的共享本体。
    store.flush_save_queue().unwrap();
    let instance: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join(format!(
            "starmaps/{starmap_id}/nodes/{}/n_new.json",
            bucket_for("n_new")
        )))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(instance["assetId"], json!("asset_new"));
    assert!(instance.get("title").is_none());
    let definition_after =
        writer_core::starmap::library::load_node_definition(dir.path(), "asset_new")
            .unwrap()
            .expect("definition must survive instance flush");
    assert_eq!(definition_after.title, "显式新建节点");
}

#[test]
fn arriving_definition_replaces_provisional_view_without_up_push() {
    let (dir, starmap_id) = provisional_fixture();
    let mut store = StarMapStore::new(dir.path(), &starmap_id);
    store.load_full().unwrap();
    assert!(store.node_definition_state(INSTANCE_ID).is_provisional());

    // 真正本体到达素材库（同步/另一画布写入的真实内容，不是占位）。
    let real_definition = StarMapNodeDefinition {
        asset_id: PENDING_ASSET_ID.to_string(),
        title: "真实共享本体".to_string(),
        kind: StarMapNodeKind::Note,
        payload: None,
        content: StarMapNodeContent::Empty,
        anchors: Vec::new(),
        portal: None,
        style: StarMapNodeStyle::default(),
        provenance: StarMapProvenance::default(),
        created_at: 1,
        updated_at: 42,
    };
    writer_core::starmap::library::save_node_definition(dir.path(), &real_definition).unwrap();
    let written_path = definition_path(dir.path(), PENDING_ASSET_ID);
    let written_bytes = std::fs::read(&written_path).unwrap();

    // 同一 store 按 asset_id 重新解析：占位视图被真实本体替换。
    let refreshed = store.refresh_node_definition(INSTANCE_ID).unwrap();
    assert_eq!(refreshed.title, "真实共享本体");
    assert_eq!(refreshed.definition_revision, 42);
    assert_eq!(
        refreshed.position,
        StarMapPoint { x: 12.0, y: 34.0 },
        "实例位置必须保留"
    );
    assert_eq!(
        store.node_definition_state(INSTANCE_ID),
        NodeDefinitionState::Resolved
    );

    // 不产生任何上推：flush 不改写已到达的真实本体。
    store.flush_save_queue().unwrap();
    assert_eq!(
        std::fs::read(&written_path).unwrap(),
        written_bytes,
        "a real arriving definition must never be rewritten by instance flush"
    );

    // 重新加载同样得到 Resolved。
    let mut reloaded = StarMapStore::new(dir.path(), &starmap_id);
    reloaded.load_full().unwrap();
    assert_eq!(
        reloaded.node_definition_state(INSTANCE_ID),
        NodeDefinitionState::Resolved
    );
    assert_eq!(
        reloaded.get_node(INSTANCE_ID).unwrap().title,
        "真实共享本体"
    );
}
