//! Issue #781 评论 5854215552 回归测试 — 星图 Core 最终收口。
//!
//! 本评论把星图内核职责收敛为四类：**数据对象、对象位置、对象关系、
//! 持久化/校验/引用解析**。显示/交互/渲染职责（布局算法、视口、运动策略、
//! 显示策略、节点打开行为、命中测试、边渲染几何）全部退出 Core。
//!
//! 覆盖的行为契约：
//! 1. 对象位置跟对象自己走：`StarMapNode.position` / `StarMapEmbed.position`
//!    就是底层数据；移动后经对象文件往返仍是同一份真相，不再另有 layout record。
//! 2. `StarMapGraph` 能完整序列化全部 authored object + position + style，
//!    不依赖 layout/viewport/display policy 才能还原星图本身。
//! 3. Portal 只有 destination 语义、Embed 只有 position、节点没有
//!    displayPolicy/openBehavior——显示状态不再是第二份持久化真相。
//! 4. `starmaps/{id}.meta.json` 是元数据唯一真相，`starmaps/index.json`
//!    只有 schemaVersion/starmapIds/mainStarmapByProject/updatedAt；
//!    统计字段不再持久化，`set_main_starmap_for_project` 不再遍历改写其他 meta。
//! 5. DTO 解码 fail-closed：未知 kind、缺失/空 ID、payload JSON 解析失败都返回 Err。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use tempfile::TempDir;

use writer_core::api::{
    StarMapNodeContentDto, StarMapNodeDto, StarMapNodeKindDto, StarMapNodeStyleDto,
    StarMapPathSegmentDto, StarMapPointDto, StarMapTargetDetailDto,
};
use writer_core::starmap::semantic::{StarMapNodeContent, StarMapPortal, StarMapTargetDetail};
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use writer_core::starmap::types::*;

// ---------------------------------------------------------------------------
// 构造辅助
// ---------------------------------------------------------------------------

fn setup() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let meta = writer_core::starmap::create_starmap(dir.path(), "星图", "", None).unwrap();
    (dir, meta.starmap_id)
}

fn make_node(id: &str, title: &str) -> StarMapNode {
    StarMapNode {
        id: id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        tags: vec![],
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: StarMapPoint::default(),
        style: StarMapNodeStyle::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

/// 把 node 的 position 设成 (x, y) 后返回，用于 add_node 调用。
fn node_at(mut node: StarMapNode, x: f32, y: f32) -> StarMapNode {
    node.position = StarMapPoint { x, y };
    node
}

fn local_path(host: &str, node_id: &str) -> StarMapTargetPath {
    StarMapTargetPath {
        starmap_id: host.to_string(),
        segments: vec![],
        target: StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        },
    }
}

/// 移动 + 换样式的节点补丁（`StarMapNodePatch` 无 `Default`，逐字段显式给出）。
fn move_and_style_patch(x: f32, y: f32, fill: &str) -> StarMapNodePatch {
    StarMapNodePatch {
        title: None,
        kind: None,
        payload: None,
        tags: None,
        content: None,
        anchors: None,
        portal: None,
        position: Some(StarMapPoint { x, y }),
        style: Some(StarMapNodeStyle {
            fill_color: Some(fill.to_string()),
        }),
        provenance: None,
    }
}

fn move_embed_patch(x: f32, y: f32) -> StarMapEmbedPatch {
    StarMapEmbedPatch {
        label: None,
        position: Some(StarMapPoint { x, y }),
        host_path: None,
    }
}

fn make_embed(
    instance_id: &str,
    target_starmap_id: &str,
    host: &str,
    host_node: &str,
) -> StarMapEmbed {
    StarMapEmbed {
        instance_id: instance_id.to_string(),
        target_starmap_id: target_starmap_id.to_string(),
        label: Some("子星图".to_string()),
        position: StarMapPoint::default(),
        host_path: local_path(host, host_node),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

/// 建一张含节点（带 portal、position、style）、嵌入（带 position）、链接、
/// 超链接的星图，flush 后返回 store。
fn seed_full_graph(dir: &TempDir, host_sid: &str, child_sid: &str) -> StarMapStore {
    let mut store = StarMapStore::new(dir.path(), host_sid);
    let mut node = make_node("n1", "甲");
    node.portal = Some(StarMapPortal {
        destination_starmap_id: child_sid.to_string(),
        destination_target: Some(StarMapTargetDetail::Starmap),
    });
    store.add_node(node_at(node, 30.0, 40.0));
    store
        .update_node("n1", &move_and_style_patch(30.0, 40.0, "#aabbcc"))
        .unwrap();
    store
        .add_embed(make_embed("emb1", child_sid, host_sid, "n1"))
        .unwrap();
    store
        .update_embed("emb1", &move_embed_patch(700.0, 90.0))
        .unwrap();
    store
        .add_link(StarMapLink {
            link_id: "lk1".to_string(),
            source: local_path(host_sid, "n1"),
            target: StarMapTargetPath {
                starmap_id: child_sid.to_string(),
                segments: vec![],
                target: StarMapTargetDetail::Starmap,
            },
            label: Some("跳转".to_string()),
            created_at: 0,
            updated_at: 0,
        })
        .unwrap();
    store
        .add_hyperlink(StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: local_path(host_sid, "n1"),
            target_uri: "https://example.com".to_string(),
            label: None,
            created_at: 0,
            updated_at: 0,
        })
        .unwrap();
    store.flush().unwrap();
    store
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn sorted_keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

fn starmap_dir(dir: &TempDir, starmap_id: &str) -> std::path::PathBuf {
    dir.path().join("starmaps").join(starmap_id)
}

// ---------------------------------------------------------------------------
// 1. 对象位置是数据层字段
// ---------------------------------------------------------------------------

/// `add_node` 的 x/y 直接写进 `node.position`；移动就是更新 position，
/// 经对象文件往返后仍是同一份真相（不存在第二套 layout record）。
#[test]
fn node_position_is_authored_data_and_survives_object_file_roundtrip() {
    let (dir, sid) = setup();
    let mut store = StarMapStore::new(dir.path(), &sid);

    let created = store.add_node(node_at(make_node("n1", "甲"), 42.0, 24.0));
    assert_eq!(
        created.position,
        StarMapPoint { x: 42.0, y: 24.0 },
        "add_node 必须把 x/y 写进 node.position"
    );

    store
        .update_node("n1", &move_and_style_patch(120.0, -8.5, "#112233"))
        .unwrap();
    assert_eq!(
        store.get_node("n1").unwrap().position,
        StarMapPoint { x: 120.0, y: -8.5 },
        "移动节点就是更新 node.position"
    );

    store.flush().unwrap();

    let mut reloaded = StarMapStore::new(dir.path(), &sid);
    reloaded.load_full().unwrap();
    let node = reloaded.get_node("n1").expect("节点必须从对象文件恢复");
    assert_eq!(
        node.position,
        StarMapPoint { x: 120.0, y: -8.5 },
        "移动后的 position 必须持久化在节点对象里"
    );
    assert_eq!(
        node.style.fill_color.as_deref(),
        Some("#112233"),
        "节点样式是用户对节点数据的自定义，必须跟节点走"
    );
}

/// Embed 只保存自己在宿主星图中的 position；移动 Embed 就是更新 `embed.position`。
#[test]
fn embed_position_is_authored_data_and_survives_object_file_roundtrip() {
    let (dir, host_sid) = setup();
    let child = writer_core::starmap::create_starmap(dir.path(), "子图", "", None).unwrap();

    let mut store = StarMapStore::new(dir.path(), &host_sid);
    store.add_node(node_at(make_node("n1", "宿主节点"), 10.0, 10.0));
    store
        .add_embed(make_embed("emb1", &child.starmap_id, &host_sid, "n1"))
        .unwrap();
    store
        .update_embed("emb1", &move_embed_patch(500.0, 25.0))
        .unwrap();
    store.flush().unwrap();

    let mut reloaded = StarMapStore::new(dir.path(), &host_sid);
    reloaded.load_full().unwrap();
    let embed = reloaded
        .get_embed("emb1")
        .expect("embed 必须从对象文件恢复");
    assert_eq!(
        embed.position,
        StarMapPoint { x: 500.0, y: 25.0 },
        "Embed 移动后 position 必须持久化在 embed 对象里"
    );
}

// ---------------------------------------------------------------------------
// 2/3. Graph 完整序列化 + 显示层字段退出 Core
// ---------------------------------------------------------------------------

/// `StarMapGraph` 带着全部 authored object 的 position/style 完整序列化，
/// 且节点/Embed/Portal 不再携带任何显示层状态。
#[test]
fn graph_serialization_carries_position_and_style_without_display_layer_state() {
    let (dir, host_sid) = setup();
    let child = writer_core::starmap::create_starmap(dir.path(), "子图", "", None).unwrap();
    let store = seed_full_graph(&dir, &host_sid, &child.starmap_id);

    let graph = store.to_starmap_graph();
    let json = serde_json::to_value(&graph).unwrap();

    let node_json = &json["nodes"][0];
    assert_eq!(
        node_json["position"],
        serde_json::json!({ "x": 30.0, "y": 40.0 }),
        "Graph 必须带节点 position"
    );
    assert_eq!(
        node_json["style"],
        serde_json::json!({ "fillColor": "#aabbcc" }),
        "Graph 必须带节点 style.fillColor"
    );
    assert!(
        node_json.get("displayPolicy").is_none() && node_json.get("openBehavior").is_none(),
        "节点的显示策略/打开行为不再是 Core 数据：{node_json}"
    );
    let portal_keys = sorted_keys(&node_json["portal"]);
    assert_eq!(
        portal_keys,
        vec![
            "destinationStarmapId".to_string(),
            "destinationTarget".to_string()
        ],
        "Portal 只保留 destination 语义"
    );

    let embed_json = &json["embeds"][0];
    assert_eq!(
        embed_json["position"],
        serde_json::json!({ "x": 700.0, "y": 90.0 }),
        "Graph 必须带 embed position"
    );
    for removed in [
        "width",
        "height",
        "scale",
        "zIndex",
        "collapsed",
        "viewport",
        "placement",
    ] {
        assert!(
            embed_json.get(removed).is_none(),
            "Embed 的显示字段 '{removed}' 必须退出 Core：{embed_json}"
        );
    }

    for removed in ["layout", "viewport", "displayPolicy", "motionPolicy"] {
        assert!(
            json.get(removed).is_none(),
            "Graph 顶层不应再有 '{removed}'：{json}"
        );
    }

    // 不依赖任何显示状态即可还原星图本身：position/style 经反序列化仍在。
    let restored: StarMapGraph = serde_json::from_value(json).unwrap();
    assert_eq!(restored.nodes.len(), 1);
    assert_eq!(
        restored.nodes[0].position,
        StarMapPoint { x: 30.0, y: 40.0 }
    );
    assert_eq!(
        restored.nodes[0].style.fill_color.as_deref(),
        Some("#aabbcc")
    );
    assert_eq!(
        restored.embeds[0].position,
        StarMapPoint { x: 700.0, y: 90.0 }
    );
    assert_eq!(restored.links.len(), 1);
    assert_eq!(restored.hyperlinks.len(), 1);
}

/// 移动对象不会产生任何 layout/viewport/phased 显示状态文件；
/// 分片存储 `graph.json` 也不再持有 layout revision。
#[test]
fn moving_objects_writes_no_layout_or_viewport_state() {
    let (dir, sid) = setup();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.add_node(node_at(make_node("n1", "甲"), 1.0, 2.0));
    store
        .update_node("n1", &move_and_style_patch(11.0, 12.0, "#334455"))
        .unwrap();
    store.flush().unwrap();

    let sid_dir = starmap_dir(&dir, &sid);
    assert!(
        !sid_dir.join("layouts").exists(),
        "Core 不再写 layouts/default/**"
    );
    assert!(
        !sid_dir.join("viewport.json").exists(),
        "Core 不再写 starmaps/{{id}}/viewport.json"
    );
    assert!(
        !dir.path().join("session").exists(),
        "Core 不再写 session/starmaps/{{id}}/viewport.json"
    );

    let graph_meta = read_json(&sid_dir.join("graph.json"));
    for removed in ["layoutRevision", "layout", "viewport", "layouts"] {
        assert!(
            graph_meta.get(removed).is_none(),
            "GraphMeta 不再持有 '{removed}'：{graph_meta}"
        );
    }
    assert_eq!(
        graph_meta["nodeIds"],
        serde_json::json!(["n1"]),
        "GraphMeta 只作为分片 manifest"
    );
}

// ---------------------------------------------------------------------------
// 4. Meta / Index 单一真相
// ---------------------------------------------------------------------------

/// `starmaps/index.json` 只有四个字段，节点/边统计不再回写 Meta，
/// 派生数字只能从对象文件重新算出来。
#[test]
fn meta_and_index_keep_one_truth_without_persisted_stats() {
    let (dir, sid) = setup();
    let mut store = StarMapStore::new(dir.path(), &sid);
    store.add_node(node_at(make_node("n1", "甲"), 0.0, 0.0));
    store.add_node(node_at(make_node("n2", "乙"), 0.0, 0.0));
    store
        .add_edge(StarMapEdge {
            id: "e1".to_string(),
            from: local_path(&sid, "n1"),
            to: local_path(&sid, "n2"),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        })
        .unwrap();
    store.flush().unwrap();

    let index = read_json(&dir.path().join("starmaps").join("index.json"));
    assert_eq!(
        sorted_keys(&index),
        vec![
            "mainStarmapByProject".to_string(),
            "schemaVersion".to_string(),
            "starmapIds".to_string(),
            "updatedAt".to_string(),
        ],
        "index.json 只保存 starmap_ids 与 main 映射"
    );

    let meta_json = read_json(&sid_dir_meta_path(&dir, &sid));
    assert_eq!(
        sorted_keys(&meta_json),
        vec![
            "accentColor".to_string(),
            "createdAt".to_string(),
            "description".to_string(),
            "projectId".to_string(),
            "starmapId".to_string(),
            "title".to_string(),
            "updatedAt".to_string(),
        ],
        "meta.json 是标题/描述/项目/强调色/时间戳的唯一事实源"
    );
    for derived in [
        "nodeCount",
        "edgeCount",
        "linkedChapterCount",
        "isMainForProject",
    ] {
        assert!(
            meta_json.get(derived).is_none(),
            "派生字段 '{derived}' 不再持久化：{meta_json}"
        );
    }

    // 派生数字仍可从对象文件重新算出来（meta 不是第二份业务真相）。
    let mut reloaded = StarMapStore::new(dir.path(), &sid);
    reloaded.load_full().unwrap();
    assert_eq!(reloaded.node_count(), 2);
    assert_eq!(reloaded.edge_count(), 1);
}

/// `set_main_starmap_for_project` 直接改 index 映射，不遍历改写其他星图的 meta。
#[test]
fn set_main_starmap_does_not_rewrite_other_starmap_metas() {
    let (dir, first_sid) = setup();
    let second = writer_core::starmap::create_starmap(dir.path(), "第二图", "", None).unwrap();
    let second_sid = second.starmap_id.clone();
    std::fs::create_dir_all(dir.path().join("projects").join("p1")).unwrap();

    writer_core::starmap::bind_starmap_to_project(dir.path(), &first_sid, "p1").unwrap();
    writer_core::starmap::bind_starmap_to_project(dir.path(), &second_sid, "p1").unwrap();

    let first_meta_path = sid_dir_meta_path(&dir, &first_sid);
    let second_meta_path = sid_dir_meta_path(&dir, &second_sid);
    let second_initial = std::fs::read_to_string(&second_meta_path).unwrap();

    writer_core::starmap::set_main_starmap_for_project(dir.path(), &first_sid, "p1").unwrap();
    assert_eq!(
        std::fs::read_to_string(&second_meta_path).unwrap(),
        second_initial,
        "设置 s1 为主星图不得改写 s2 的 meta"
    );

    let first_before = std::fs::read_to_string(&first_meta_path).unwrap();
    writer_core::starmap::set_main_starmap_for_project(dir.path(), &second_sid, "p1").unwrap();
    assert_eq!(
        std::fs::read_to_string(&first_meta_path).unwrap(),
        first_before,
        "设置 s2 为主星图不得改写 s1 的 meta"
    );

    let index = read_json(&dir.path().join("starmaps").join("index.json"));
    assert_eq!(
        index["mainStarmapByProject"]["p1"],
        serde_json::json!(second_sid),
        "main 映射只能来自 index.json"
    );
}

fn sid_dir_meta_path(dir: &TempDir, starmap_id: &str) -> std::path::PathBuf {
    dir.path()
        .join("starmaps")
        .join(format!("{starmap_id}.meta.json"))
}

// ---------------------------------------------------------------------------
// 5. DTO 解码 fail-closed
// ---------------------------------------------------------------------------

/// 未知 kind、缺失/空必填 ID、payload JSON 解析失败都必须返回 Err，
/// 不允许静默降级成空字符串或另一种语义。
#[test]
fn dto_decode_is_fail_closed() {
    // 未知 path segment kind 不再自动变成 EnterEmbed/EnterPortal。
    assert!(StarMapPathSegment::try_from(StarMapPathSegmentDto {
        kind: "enterChild".to_string(),
        instance_id: Some("emb1".to_string()),
        node_id: None,
    })
    .is_err());
    // 当前 kind 必填 ID 缺失或为空。
    assert!(StarMapPathSegment::try_from(StarMapPathSegmentDto {
        kind: "enterEmbed".to_string(),
        instance_id: Some(String::new()),
        node_id: None,
    })
    .is_err());
    assert!(StarMapPathSegment::try_from(StarMapPathSegmentDto {
        kind: "enterPortal".to_string(),
        instance_id: None,
        node_id: None,
    })
    .is_err());

    // 未知 target kind 不再自动变成 Starmap/Node/ChapterRange。
    assert!(StarMapTargetDetail::try_from(StarMapTargetDetailDto {
        kind: "wormhole".to_string(),
        ..Default::default()
    })
    .is_err());
    assert!(StarMapTargetDetail::try_from(StarMapTargetDetailDto {
        kind: "node".to_string(),
        node_id: Some(String::new()),
        ..Default::default()
    })
    .is_err());

    // 未知 node content kind；chapterRef 缺 chapter_id。
    assert!(StarMapNodeContent::try_from(StarMapNodeContentDto {
        kind: "bogus".to_string(),
        ..Default::default()
    })
    .is_err());
    assert!(StarMapNodeContent::try_from(StarMapNodeContentDto {
        kind: "chapterRef".to_string(),
        project_id: Some("p1".to_string()),
        chapter_id: None,
        ..Default::default()
    })
    .is_err());

    // payload 不是合法 JSON 时返回 Err（不接受被吞成空值）。
    assert!(StarMapNode::try_from(StarMapNodeDto {
        id: "n1".to_string(),
        title: "甲".to_string(),
        kind: StarMapNodeKindDto::Concept,
        payload: Some("{not json".to_string()),
        tags: vec![],
        content: StarMapNodeContentDto::default(),
        anchors: vec![],
        portal: None,
        position: StarMapPointDto::default(),
        style: StarMapNodeStyleDto::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    })
    .is_err());

    // 合法输入仍然正常解码（fail-closed 不等于全都失败）。
    let ok = StarMapNode::try_from(StarMapNodeDto {
        id: "n1".to_string(),
        title: "甲".to_string(),
        kind: StarMapNodeKindDto::Concept,
        payload: Some(r#"{"k":1}"#.to_string()),
        tags: vec![],
        content: StarMapNodeContentDto {
            kind: "inline".to_string(),
            summary: Some("摘要".to_string()),
            ..Default::default()
        },
        anchors: vec![],
        portal: None,
        position: StarMapPointDto { x: 3.0, y: 4.0 },
        style: StarMapNodeStyleDto {
            fill_color: Some("#ffffff".to_string()),
        },
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    })
    .unwrap();
    assert_eq!(ok.position, StarMapPoint { x: 3.0, y: 4.0 });
    let ok_segment = StarMapPathSegment::try_from(StarMapPathSegmentDto {
        kind: "enterPortal".to_string(),
        instance_id: None,
        node_id: Some("n1".to_string()),
    })
    .unwrap();
    assert_eq!(
        ok_segment,
        StarMapPathSegment::EnterPortal {
            node_id: "n1".to_string()
        }
    );
}
