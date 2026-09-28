//! Issue #789 评论 5868714244 回归测试 — Harmony 星图主链必须接通。
//!
//! 该评论指出 Harmony 端"新平台层已经建出来，但数据契约、持久化、Embed
//! 和父层导航还没接通"。其中 Core 侧要证明的行为契约：
//!
//! 1. `getStarMapGraph` 发出的节点/嵌入形状是当前契约：节点带 `position` /
//!    `style`，嵌入带 `position` / `hostPath`，且不再有旧显示层字段
//!    （`displayPolicy` / `openBehavior` / `placement` / `targetViewport`）。
//!    Harmony 侧 DTO/decoder 只有按这份形状才能解码成功。
//! 2. C ABI `writer_core_update_starmap_node` 写出的 position 是节点位置的
//!    唯一真相，flush + close 后重新读图仍是同一份数据（移动不会"重进页面就丢"）。
//! 3. C ABI `writer_core_add_starmap_edge` 接受 Harmony 桥接层构造的
//!    `StarMapEdgeDto`（target detail 全字段、显式 null），并在图里真实落地。
//! 4. C ABI `writer_core_update_starmap_embed` 写出嵌入 position，同样持久化。
//! 5. 非法 patch 返回失败 envelope，不静默成功。

#![cfg(feature = "harmony-ffi")]
#![allow(clippy::expect_used, clippy::undocumented_unsafe_blocks)]

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{json, Value};
use tempfile::TempDir;

use writer_core::ffi;
use writer_core::starmap::semantic::{StarMapNodeContent, StarMapTargetDetail};
use writer_core::starmap::store::StarMapStore;
use writer_core::starmap::types::reference::StarMapTargetPath;
use writer_core::starmap::types::{
    StarMapEmbed, StarMapNode, StarMapNodeKind, StarMapNodeStyle, StarMapPoint,
};

// ---------------------------------------------------------------------------
// FFI 调用辅助
// ---------------------------------------------------------------------------

fn cstr(s: &str) -> CString {
    CString::new(s.to_string()).expect("测试入参不含内嵌 NUL")
}

fn take_string(ptr: *mut c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: ptr 由本测试刚从 writer_core_* 取回，非空且指向 NUL 结尾 UTF-8。
    let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().to_string();
    // SAFETY: ptr 来自 writer_core_* 返回值，按 FFI 契约由调用方释放一次。
    unsafe { ffi::writer_core_free_string(ptr) };
    s
}

fn call1(f: unsafe extern "C" fn(*const c_char) -> *mut c_char, a: &CString) -> Value {
    // SAFETY: a 是本测试持有的合法 C 串。
    let raw = unsafe { f(a.as_ptr()) };
    serde_json::from_str(&take_string(raw)).expect("FFI 返回合法 JSON envelope")
}

fn call2(
    f: unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_char,
    a: &CString,
    b: &CString,
) -> Value {
    // SAFETY: a/b 都是本测试持有的合法 C 串。
    let raw = unsafe { f(a.as_ptr(), b.as_ptr()) };
    serde_json::from_str(&take_string(raw)).expect("FFI 返回合法 JSON envelope")
}

fn call3(
    f: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char) -> *mut c_char,
    a: &CString,
    b: &CString,
    c: &CString,
) -> Value {
    // SAFETY: 三个指针都是本测试持有的合法 C 串。
    let raw = unsafe { f(a.as_ptr(), b.as_ptr(), c.as_ptr()) };
    serde_json::from_str(&take_string(raw)).expect("FFI 返回合法 JSON envelope")
}

fn assert_success(env: &Value, endpoint: &str) {
    assert_eq!(
        env.get("success").and_then(Value::as_bool),
        Some(true),
        "{endpoint} 期望成功，实际: {env}"
    );
}

fn node_at(id: &str, title: &str, x: f32, y: f32) -> StarMapNode {
    StarMapNode {
        id: id.to_string(),
        title: title.to_string(),
        kind: StarMapNodeKind::Concept,
        payload: None,
        tags: vec![],
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: StarMapPoint { x, y },
        style: StarMapNodeStyle::default(),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
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

fn embed_at(
    instance_id: &str,
    host: &str,
    host_node: &str,
    child: &str,
    x: f32,
    y: f32,
) -> StarMapEmbed {
    StarMapEmbed {
        instance_id: instance_id.to_string(),
        target_starmap_id: child.to_string(),
        label: Some("子星图".to_string()),
        position: StarMapPoint { x, y },
        host_path: local_path(host, host_node),
        provenance: Default::default(),
        created_at: 0,
        updated_at: 0,
    }
}

/// Harmony `NativeStarMapBridge.nodeTargetPath()` 构造的同形 target path：
/// target detail 的所有键都出现，未使用的显式 null。
fn harmony_target_path(starmap_id: &str, node_id: &str) -> Value {
    json!({
        "starmapId": starmap_id,
        "segments": [],
        "target": {
            "type": "node",
            "nodeId": node_id,
            "anchorId": Value::Null,
            "projectId": Value::Null,
            "volumeId": Value::Null,
            "chapterId": Value::Null,
            "rangeStart": Value::Null,
            "rangeEnd": Value::Null,
            "entityType": Value::Null,
            "entityId": Value::Null,
            "uri": Value::Null,
        }
    })
}

fn position_of(node: &Value) -> (f64, f64) {
    let x = node["position"]["x"].as_f64().expect("position.x 是数字");
    let y = node["position"]["y"].as_f64().expect("position.y 是数字");
    (x, y)
}

// ---------------------------------------------------------------------------
// 测试流程
// ---------------------------------------------------------------------------

/// 建图 + FFI 初始化。Harmony FFI 没有"建节点"入口（节点由平台端生成 ID 后经
/// Core 的 add_starmap_node 写入），这里按同样的数据形态在存储层落盘。
fn seed_and_init() -> (TempDir, String) {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("projects")).expect("projects 目录");
    let meta = writer_core::starmap::create_starmap(dir.path(), "星图", "", None).expect("建星图");
    let sid = meta.starmap_id.clone();
    let child = writer_core::starmap::create_starmap(dir.path(), "子星图", "", None)
        .expect("建子星图")
        .starmap_id;

    let mut store = StarMapStore::new(dir.path(), &sid);
    store.add_node(node_at("n1", "甲", 10.0, 20.0));
    store.add_node(node_at("n2", "乙", 30.0, 40.0));
    store
        .add_embed(embed_at("emb1", &sid, "n1", &child, 200.0, 100.0))
        .expect("加嵌入");
    store.flush().expect("flush 种子数据");
    drop(store);

    // FFI 初始化（Harmony 运行时的真实入口）。
    let root = cstr(&dir.path().to_string_lossy());
    // SAFETY: root 是本测试持有的合法 NUL 结尾 UTF-8 路径。
    let code = unsafe { ffi::writer_core_init(root.as_ptr()) };
    assert_eq!(code, 0, "writer_core_init 失败: {code}");
    (dir, sid)
}

/// 评论问题 1/4：图负载必须带 authored position/style/position(embed)，且没有旧显示层字段。
fn assert_graph_contract(sid: &str) {
    let graph = call1(ffi::starmap_ops::writer_core_get_starmap_graph, &cstr(sid));
    assert_success(&graph, "getStarMapGraph");
    let nodes = graph["data"]["nodes"].as_array().expect("nodes 是数组");
    assert_eq!(nodes.len(), 2, "图里有两个节点");

    let n1 = nodes.iter().find(|n| n["id"] == "n1").expect("n1 在图里");
    assert_eq!(position_of(n1), (10.0, 20.0), "节点带 authored position");
    assert!(
        n1.get("style").is_some(),
        "节点必须带 style（Harmony decoder 按非空解码）"
    );
    assert!(
        n1.get("displayPolicy").is_none() && n1.get("openBehavior").is_none(),
        "旧显示层字段不得再出现: {n1}"
    );

    let embeds = graph["data"]["embeds"].as_array().expect("embeds 是数组");
    assert_eq!(embeds.len(), 1, "图里有一个嵌入");
    assert_eq!(position_of(&embeds[0]), (200.0, 100.0), "嵌入带 position");
    assert!(
        embeds[0].get("placement").is_none() && embeds[0].get("targetViewport").is_none(),
        "嵌入不得再有 placement/targetViewport: {}",
        embeds[0]
    );
    assert!(
        embeds[0].get("hostPath").is_some(),
        "嵌入必须带 hostPath（Harmony decoder 按非空解码）"
    );
}

/// 评论问题 3/4：节点移动、拉线建关系、嵌入移动三条写路径都走 C ABI。
fn apply_mutations(sid: &str) {
    let sid_c = cstr(sid);

    let moved = call3(
        ffi::starmap_ops::writer_core_update_starmap_node,
        &sid_c,
        &cstr("n1"),
        &cstr(r#"{"position":{"x":123.5,"y":-7.25}}"#),
    );
    assert_success(&moved, "updateStarMapNode");
    assert_eq!(
        position_of(&moved["data"]),
        (123.5, -7.25),
        "position patch 必须落到节点上"
    );

    let edge = json!({
        "id": "edge-harmony-1",
        "from": harmony_target_path(sid, "n1"),
        "to": harmony_target_path(sid, "n2"),
        "kind": "RelatedTo",
        "label": Value::Null,
        "payload": Value::Null,
        "createdAt": 1_700_000_000_000u64,
        "updatedAt": 1_700_000_000_000u64,
    });
    let edge_env = call2(
        ffi::starmap_ops::writer_core_add_starmap_edge,
        &sid_c,
        &cstr(&edge.to_string()),
    );
    assert_success(&edge_env, "addStarMapEdge");
    assert_eq!(edge_env["data"]["id"], "edge-harmony-1");
    assert_eq!(edge_env["data"]["from"]["target"]["nodeId"], "n1");
    assert_eq!(edge_env["data"]["to"]["target"]["nodeId"], "n2");

    let moved_embed = call3(
        ffi::starmap_ops::writer_core_update_starmap_embed,
        &sid_c,
        &cstr("emb1"),
        &cstr(r#"{"position":{"x":640,"y":48}}"#),
    );
    assert_success(&moved_embed, "updateStarMapEmbed");
    assert_eq!(
        position_of(&moved_embed["data"]),
        (640.0, 48.0),
        "embed position patch 必须落到嵌入上"
    );
}

/// 落盘往返：flush + close 后重新读图，位置与关系都还在（"重进页面不丢"）。
fn assert_mutations_persisted(sid: &str) {
    let sid_c = cstr(sid);
    let _ = call1(ffi::starmap_ops::writer_core_flush_starmap_store, &sid_c);
    let _ = call1(ffi::starmap_ops::writer_core_close_starmap_store, &sid_c);

    let reloaded = call1(ffi::starmap_ops::writer_core_get_starmap_graph, &sid_c);
    assert_success(&reloaded, "getStarMapGraph（重新加载）");
    let reloaded_nodes = reloaded["data"]["nodes"].as_array().expect("nodes 是数组");
    let reloaded_n1 = reloaded_nodes
        .iter()
        .find(|n| n["id"] == "n1")
        .expect("n1 仍在图里");
    assert_eq!(
        position_of(reloaded_n1),
        (123.5, -7.25),
        "节点位置经落盘往返不丢"
    );
    let reloaded_embeds = reloaded["data"]["embeds"]
        .as_array()
        .expect("embeds 是数组");
    assert_eq!(
        position_of(&reloaded_embeds[0]),
        (640.0, 48.0),
        "嵌入位置经落盘往返不丢"
    );
    let reloaded_edges = reloaded["data"]["edges"].as_array().expect("edges 是数组");
    assert!(
        reloaded_edges.iter().any(|e| e["id"] == "edge-harmony-1"),
        "新建关系经落盘往返不丢: {reloaded_edges:?}"
    );
}

/// 失败路径不静默成功：缺 y 的 position patch 解析失败，未知节点归属失败。
fn assert_failure_paths(sid: &str) {
    let sid_c = cstr(sid);
    let bad_patch = call3(
        ffi::starmap_ops::writer_core_update_starmap_node,
        &sid_c,
        &cstr("n1"),
        &cstr(r#"{"position":{"x":1}}"#),
    );
    assert_eq!(
        bad_patch.get("success").and_then(Value::as_bool),
        Some(false),
        "非法 patch 必须返回失败: {bad_patch}"
    );
    let missing_node = call3(
        ffi::starmap_ops::writer_core_update_starmap_node,
        &sid_c,
        &cstr("nope"),
        &cstr(r#"{"position":{"x":1.0,"y":2.0}}"#),
    );
    assert_eq!(
        missing_node.get("success").and_then(Value::as_bool),
        Some(false),
        "未知节点必须返回失败: {missing_node}"
    );
}

#[test]
fn harmony_starmap_graph_contract_and_mutations_roundtrip() {
    let (_dir, sid) = seed_and_init();
    assert_graph_contract(&sid);
    apply_mutations(&sid);
    assert_mutations_persisted(&sid);
    assert_failure_paths(&sid);
}
