//! resolver 磁盘 fallback 解耦 load_full 的回归测试。
//!
//! 覆盖 Issue #772 评论 5849073336 的四个场景：
//! 1. 双向跨图引用不栈溢出（resolver 不再调 load_full）
//! 2. 目标星图 schema 版本不兼容时返回 UnsupportedVersion 而非 MissingNode
//! 3. embed.target_starmap_id 指向不存在的星图时报 DanglingReference
//! 4. owned path 起点不等于宿主图时报 DanglingReference

use super::super::meta::DeletedSinceLastSync;
use super::super::*;
use super::*;
use crate::starmap::graph::resolve::{resolve_target, GraphResolverContext};
use crate::starmap::semantic::{
    StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance, StarMapTargetDetail,
    StarMapTargetResolveStatus,
};
use crate::starmap::types::reference::{StarMapPathSegment, StarMapTargetPath};
use tempfile::TempDir;

/// 构造一个合法的 schema 3 GraphMeta。
fn make_graph_meta(
    starmap_id: &str,
    node_ids: &[&str],
    edge_ids: &[&str],
    embed_ids: &[&str],
) -> GraphMeta {
    GraphMeta {
        schema_version: "3".to_string(),
        starmap_id: starmap_id.to_string(),
        node_ids: node_ids.iter().map(|s| s.to_string()).collect(),
        edge_ids: edge_ids.iter().map(|s| s.to_string()).collect(),
        embed_instance_ids: embed_ids.iter().map(|s| s.to_string()).collect(),
        link_ids: vec![],
        hyperlink_ids: vec![],
        edge_relation_index: vec![],
        embed_host_index: vec![],
        link_relation_index: vec![],
        hyperlink_relation_index: vec![],
        node_kind_counts: HashMap::new(),
        package_revision: 1,
        updated_at: 0,
        deleted_since_last_sync: DeletedSinceLastSync::default(),
        ..Default::default()
    }
}

/// 写入 graph.json。
fn write_graph_json(starmap_dir: &std::path::Path, meta: &GraphMeta) {
    let json = serde_json::to_string_pretty(meta).unwrap();
    std::fs::write(starmap_dir.join("graph.json"), json).unwrap();
}

/// 构造一个 embed 实例。
fn make_embed(instance_id: &str, target_starmap_id: &str, host_starmap_id: &str) -> StarMapEmbed {
    StarMapEmbed {
        instance_id: instance_id.to_string(),
        target_starmap_id: target_starmap_id.to_string(),
        label: None,
        display_policy: StarMapDisplayPolicy::default(),
        open_behavior: StarMapOpenBehavior::default(),
        placement: StarMapEmbedPlacement::default(),
        target_viewport: StarMapEmbedViewport::default(),
        host_path: StarMapTargetPath {
            starmap_id: host_starmap_id.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    }
}

/// 构造一条 edge，from/to 通过 EnterEmbed 进入目标图的节点。
fn make_cross_edge(
    edge_id: &str,
    host_starmap_id: &str,
    embed_instance_id: &str,
    target_node_id: &str,
) -> StarMapEdge {
    let cross_path = StarMapTargetPath {
        starmap_id: host_starmap_id.to_string(),
        segments: vec![StarMapPathSegment::EnterEmbed {
            instance_id: embed_instance_id.to_string(),
        }],
        target: StarMapTargetDetail::Node {
            node_id: target_node_id.to_string(),
        },
    };
    StarMapEdge {
        id: edge_id.to_string(),
        from: cross_path,
        to: StarMapTargetPath {
            starmap_id: host_starmap_id.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    }
}

// ---------------------------------------------------------------------------
// 场景 1：双向跨图引用不栈溢出
// ---------------------------------------------------------------------------
//
// sm_a 有 embed_a→sm_b + edge_a 经 embed_a 指向 sm_b 的 node_b；
// sm_b 有 embed_b→sm_a + edge_b 经 embed_b 指向 sm_a 的 node_a。
// 旧 resolver 的 lookup_node 会调 load_full(sm_b) → detect_dangling(sm_b)
// → resolve edge_b → lookup_node(sm_a) → load_full(sm_a) → 无限递归。
// 新 resolver 用 ResolverGraphProvider 直接读对象文件，不递归。

#[test]
fn load_full_does_not_recurse_on_bidirectional_cross_graph_refs() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();

    let sm_a = crate::starmap::create_starmap(dir.path(), "A", "", None).unwrap();
    let sm_b = crate::starmap::create_starmap(dir.path(), "B", "", None).unwrap();
    let id_a = &sm_a.starmap_id;
    let id_b = &sm_b.starmap_id;

    let dir_a = dir.path().join("starmaps").join(id_a);
    let dir_b = dir.path().join("starmaps").join(id_b);
    for d in [&dir_a, &dir_b] {
        std::fs::create_dir_all(d.join("nodes")).unwrap();
        std::fs::create_dir_all(d.join("edges")).unwrap();
        std::fs::create_dir_all(d.join("embeds")).unwrap();
    }

    // sm_a: node_a + embed_a(target=sm_b) + edge_a(经 embed_a → node_b)
    let node_a = make_test_node("node_a", "A");
    write_to_bucket(
        &dir_a,
        "nodes",
        "node_a",
        &serde_json::to_string_pretty(&node_a).unwrap(),
    );
    let embed_a = make_embed("emb_a", id_b, id_a);
    write_to_bucket(
        &dir_a,
        "embeds",
        "emb_a",
        &serde_json::to_string_pretty(&embed_a).unwrap(),
    );
    let edge_a = make_cross_edge("edge_a", id_a, "emb_a", "node_b");
    write_to_bucket(
        &dir_a,
        "edges",
        "edge_a",
        &serde_json::to_string_pretty(&edge_a).unwrap(),
    );
    write_graph_json(
        &dir_a,
        &make_graph_meta(id_a, &["node_a"], &["edge_a"], &["emb_a"]),
    );

    // sm_b: node_b + embed_b(target=sm_a) + edge_b(经 embed_b → node_a)
    let node_b = make_test_node("node_b", "B");
    write_to_bucket(
        &dir_b,
        "nodes",
        "node_b",
        &serde_json::to_string_pretty(&node_b).unwrap(),
    );
    let embed_b = make_embed("emb_b", id_a, id_b);
    write_to_bucket(
        &dir_b,
        "embeds",
        "emb_b",
        &serde_json::to_string_pretty(&embed_b).unwrap(),
    );
    let edge_b = make_cross_edge("edge_b", id_b, "emb_b", "node_a");
    write_to_bucket(
        &dir_b,
        "edges",
        "edge_b",
        &serde_json::to_string_pretty(&edge_b).unwrap(),
    );
    write_graph_json(
        &dir_b,
        &make_graph_meta(id_b, &["node_b"], &["edge_b"], &["emb_b"]),
    );

    // load_full(sm_a) 应成功完成，不栈溢出。
    let mut store = StarMapStore::new(dir.path(), id_a);
    let result = store.load_full();
    assert!(
        result.is_ok(),
        "load_full should complete without stack overflow on bidirectional cross-graph refs, got: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// 场景 2：目标星图 schema 版本不兼容时返回 UnsupportedVersion
// ---------------------------------------------------------------------------

#[test]
fn resolve_target_returns_unsupported_version_for_schema_2() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();

    let sm_b = crate::starmap::create_starmap(dir.path(), "B", "", None).unwrap();
    let id_b = &sm_b.starmap_id;
    let dir_b = dir.path().join("starmaps").join(id_b);
    std::fs::create_dir_all(dir_b.join("nodes")).unwrap();

    // 写一个 schema 版本为 "2" 的 graph.json（不兼容当前版本 "3"）。
    let bad_meta = serde_json::json!({
        "schemaVersion": "2",
        "starmapId": id_b,
        "nodeIds": [],
        "edgeIds": [],
        "embedInstanceIds": [],
        "linkIds": [],
        "hyperlinkIds": [],
        "packageRevision": 1,
        "updatedAt": 0,
    });
    std::fs::write(dir_b.join("graph.json"), bad_meta.to_string()).unwrap();

    // resolve 一条指向 sm_b 的 Node 路径。无 overlays，走磁盘 provider。
    let context = GraphResolverContext::new_disk_only(dir.path());
    let path = StarMapTargetPath {
        starmap_id: id_b.to_string(),
        segments: vec![],
        target: StarMapTargetDetail::Node {
            node_id: "any_node".to_string(),
        },
    };
    let status = resolve_target(&context, &path);
    assert!(
        matches!(status, Err(StarMapTargetResolveStatus::UnsupportedVersion)),
        "expected UnsupportedVersion, got: {:?}",
        status
    );
}

// ---------------------------------------------------------------------------
// 场景 3：embed.target_starmap_id 指向不存在的星图时报 DanglingReference
// ---------------------------------------------------------------------------

#[test]
fn detect_dangling_reports_embed_target_starmap_missing() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();

    let sm_a = crate::starmap::create_starmap(dir.path(), "A", "", None).unwrap();
    let id_a = &sm_a.starmap_id;
    let dir_a = dir.path().join("starmaps").join(id_a);
    std::fs::create_dir_all(dir_a.join("nodes")).unwrap();
    std::fs::create_dir_all(dir_a.join("edges")).unwrap();
    std::fs::create_dir_all(dir_a.join("embeds")).unwrap();

    let node_a = make_test_node("n1", "A");
    write_to_bucket(
        &dir_a,
        "nodes",
        "n1",
        &serde_json::to_string_pretty(&node_a).unwrap(),
    );

    // embed 的 target_starmap_id 指向一个不存在的星图。
    let embed = make_embed("emb1", "nonexistent_starmap", id_a);
    write_to_bucket(
        &dir_a,
        "embeds",
        "emb1",
        &serde_json::to_string_pretty(&embed).unwrap(),
    );
    write_graph_json(&dir_a, &make_graph_meta(id_a, &["n1"], &[], &["emb1"]));

    let mut store = StarMapStore::new(dir.path(), id_a);
    let result = store.load_full().unwrap();
    let dangling: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == LoadDiagnosticKind::DanglingReference
                && d.object_type == "embed"
                && d.detail.contains("target_starmap_id")
        })
        .collect();
    assert!(
        !dangling.is_empty(),
        "expected DanglingReference for embed target_starmap_id pointing to nonexistent starmap, diagnostics: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// 场景 4：owned path 起点不等于宿主图时报 DanglingReference
// ---------------------------------------------------------------------------

#[test]
fn detect_dangling_reports_owned_path_starmap_id_mismatch() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();

    let sm_a = crate::starmap::create_starmap(dir.path(), "A", "", None).unwrap();
    let id_a = &sm_a.starmap_id;
    let dir_a = dir.path().join("starmaps").join(id_a);
    std::fs::create_dir_all(dir_a.join("nodes")).unwrap();
    std::fs::create_dir_all(dir_a.join("edges")).unwrap();
    std::fs::create_dir_all(dir_a.join("embeds")).unwrap();

    let node_a = make_test_node("n1", "A");
    write_to_bucket(
        &dir_a,
        "nodes",
        "n1",
        &serde_json::to_string_pretty(&node_a).unwrap(),
    );

    // edge.from.starmap_id = "sm_other" != host graph id_a（磁盘坏数据）。
    let edge = StarMapEdge {
        id: "e1".to_string(),
        from: StarMapTargetPath {
            starmap_id: "sm_other".to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Node {
                node_id: "n1".to_string(),
            },
        },
        to: StarMapTargetPath {
            starmap_id: id_a.to_string(),
            segments: vec![],
            target: StarMapTargetDetail::Starmap,
        },
        kind: StarMapEdgeKind::RelatedTo,
        label: None,
        payload: None,
        created_at: 0,
        updated_at: 0,
    };
    write_to_bucket(
        &dir_a,
        "edges",
        "e1",
        &serde_json::to_string_pretty(&edge).unwrap(),
    );
    write_graph_json(&dir_a, &make_graph_meta(id_a, &["n1"], &["e1"], &[]));

    let mut store = StarMapStore::new(dir.path(), id_a);
    let result = store.load_full().unwrap();
    let dangling: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.kind == LoadDiagnosticKind::DanglingReference
                && d.object_type == "edge"
                && d.detail.contains("invalid host path")
        })
        .collect();
    assert!(
        !dangling.is_empty(),
        "expected DanglingReference for edge owned path starmap_id mismatch, diagnostics: {:?}",
        result.diagnostics
    );
}
