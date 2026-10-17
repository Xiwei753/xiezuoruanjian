use super::*;
use crate::starmap::semantic::{StarMapNodeContent, StarMapPortal, StarMapProvenance};
use crate::starmap::types::{
    StarMapEmbed, StarMapGraph, StarMapNode, StarMapNodeKind, StarMapPoint,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn context(overlays: Vec<(&str, StarMapGraph)>) -> GraphResolverContext {
    GraphResolverContext {
        app_data_root: PathBuf::from("/tmp/test_starmap_path_planner"),
        overlays: overlays
            .into_iter()
            .map(|(id, g)| (id.to_string(), g))
            .collect::<HashMap<_, _>>(),
    }
}

fn graph(starmap_id: &str) -> StarMapGraph {
    StarMapGraph {
        starmap_id: starmap_id.to_string(),
        ..Default::default()
    }
}

fn embed(instance_id: &str, target_starmap_id: &str) -> StarMapEmbed {
    StarMapEmbed {
        instance_id: instance_id.to_string(),
        target_starmap_id: target_starmap_id.to_string(),
        label: None,
        position: StarMapPoint::default(),
        host_path: StarMapTargetPath::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    }
}

fn node(node_id: &str) -> StarMapNode {
    StarMapNode {
        id: node_id.to_string(),
        title: node_id.to_string(),
        kind: StarMapNodeKind::Note,
        payload: None,
        tags: vec![],
        content: StarMapNodeContent::Empty,
        anchors: vec![],
        portal: None,
        position: StarMapPoint::default(),
        style: Default::default(),
        provenance: StarMapProvenance::default(),
        created_at: 0,
        updated_at: 0,
    }
}

fn portal_node(node_id: &str, destination_starmap_id: &str) -> StarMapNode {
    let mut n = node(node_id);
    n.portal = Some(StarMapPortal {
        destination_starmap_id: destination_starmap_id.to_string(),
        destination_target: None,
    });
    n
}

fn item_path(root: &str, segments: Vec<StarMapPathSegment>, node_id: &str) -> StarMapTargetPath {
    StarMapTargetPath {
        starmap_id: root.to_string(),
        segments,
        target: StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        },
    }
}

fn embed_path(root: &str, segments: Vec<StarMapPathSegment>) -> StarMapTargetPath {
    // 容器端点：`target` 为 Starmap，最后一段是容器自身。
    StarMapTargetPath {
        starmap_id: root.to_string(),
        segments,
        target: StarMapTargetDetail::Starmap,
    }
}

fn enter(instance_id: &str) -> StarMapPathSegment {
    StarMapPathSegment::EnterEmbed {
        instance_id: instance_id.to_string(),
    }
}

fn enter_portal(node_id: &str) -> StarMapPathSegment {
    StarMapPathSegment::EnterPortal {
        node_id: node_id.to_string(),
    }
}

// -----------------------------------------------------------------------
// 同层
// -----------------------------------------------------------------------
#[test]
fn same_canvas_nodes_host_root() {
    let mut g = graph("root");
    g.nodes.push(node("a"));
    g.nodes.push(node("b"));
    let ctx = context(vec![("root", g)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "a"),
        &item_path("root", vec![], "b"),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert!(plan.host_segments.is_empty());
    assert_eq!(plan.from.starmap_id, "root");
    assert!(plan.from.segments.is_empty());
    assert!(plan.to.segments.is_empty());
}

// -----------------------------------------------------------------------
// 父 Node → 子 Node
// -----------------------------------------------------------------------
#[test]
fn parent_to_child_host_parent() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "child"));
    root.nodes.push(node("a"));
    let mut child = graph("child");
    child.nodes.push(node("b"));
    let ctx = context(vec![("root", root), ("child", child)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "a"),
        &item_path("root", vec![enter("e1")], "b"),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert!(plan.host_segments.is_empty());
    assert!(plan.from.segments.is_empty());
    assert_eq!(plan.to.segments, vec![enter("e1")]);
    assert_eq!(
        plan.to.target,
        StarMapTargetDetail::Node {
            node_id: "b".to_string()
        }
    );
}

// -----------------------------------------------------------------------
// 子 Node → 父 Node（方向对称）
// -----------------------------------------------------------------------
#[test]
fn child_to_parent_symmetric() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "child"));
    root.nodes.push(node("a"));
    let mut child = graph("child");
    child.nodes.push(node("b"));
    let ctx = context(vec![("root", root), ("child", child)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![enter("e1")], "b"),
        &item_path("root", vec![], "a"),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert_eq!(plan.from.segments, vec![enter("e1")]);
    assert!(plan.to.segments.is_empty());
}

// -----------------------------------------------------------------------
// 同一 Embed 里的两个子 Node：宿主下沉到该子图
// -----------------------------------------------------------------------
#[test]
fn two_nodes_same_child_host_child() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "child"));
    let mut child = graph("child");
    child.nodes.push(node("a"));
    child.nodes.push(node("b"));
    let ctx = context(vec![("root", root), ("child", child)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![enter("e1")], "a"),
        &item_path("root", vec![enter("e1")], "b"),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "child");
    assert_eq!(plan.host_segments, vec![enter("e1")]);
    assert!(plan.from.segments.is_empty());
    assert!(plan.to.segments.is_empty());
}

// -----------------------------------------------------------------------
// 两个兄弟子图里的 Node：宿主是根，两端各带一段
// -----------------------------------------------------------------------
#[test]
fn sibling_child_nodes_host_root() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "b"));
    root.embeds.push(embed("e2", "c"));
    let mut b = graph("b");
    b.nodes.push(node("nb"));
    let mut c = graph("c");
    c.nodes.push(node("nc"));
    let ctx = context(vec![("root", root), ("b", b), ("c", c)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![enter("e1")], "nb"),
        &item_path("root", vec![enter("e2")], "nc"),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert!(plan.host_segments.is_empty());
    assert_eq!(plan.from.segments, vec![enter("e1")]);
    assert_eq!(plan.to.segments, vec![enter("e2")]);
}

// -----------------------------------------------------------------------
// 更深一层：子 Node → 更深一层的子 Node
// -----------------------------------------------------------------------
#[test]
fn deep_sibling_host_middle() {
    // root --e1--> mid --e2--> deep
    let mut root = graph("root");
    root.embeds.push(embed("e1", "mid"));
    let mut mid = graph("mid");
    mid.embeds.push(embed("e2", "deep"));
    mid.nodes.push(node("nm"));
    let mut deep = graph("deep");
    deep.nodes.push(node("nd"));
    let ctx = context(vec![("root", root), ("mid", mid), ("deep", deep)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![enter("e1")], "nm"),
        &item_path("root", vec![enter("e1"), enter("e2")], "nd"),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "mid");
    assert_eq!(plan.host_segments, vec![enter("e1")]);
    assert!(plan.from.segments.is_empty());
    assert_eq!(plan.to.segments, vec![enter("e2")]);
}

// -----------------------------------------------------------------------
// 容器端点：Node → Embed
// -----------------------------------------------------------------------
#[test]
fn node_to_embed_endpoint() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "child"));
    root.nodes.push(node("a"));
    let child = graph("child");
    let ctx = context(vec![("root", root), ("child", child)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "a"),
        &embed_path("root", vec![enter("e1")]),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert_eq!(plan.to.segments, vec![enter("e1")]);
    assert_eq!(plan.to.target, StarMapTargetDetail::Starmap);
}

// -----------------------------------------------------------------------
// 容器端点：Embed → Embed（宿主为父层）
// -----------------------------------------------------------------------
#[test]
fn embed_to_nested_embed_endpoint() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "child"));
    let mut child = graph("child");
    child.embeds.push(embed("e2", "grand"));
    let grand = graph("grand");
    let ctx = context(vec![("root", root), ("child", child), ("grand", grand)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &embed_path("root", vec![enter("e1")]),
        &embed_path("root", vec![enter("e1"), enter("e2")]),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert_eq!(plan.from.segments, vec![enter("e1")]);
    assert_eq!(plan.to.segments, vec![enter("e1"), enter("e2")]);
}

// -----------------------------------------------------------------------
// 旧 Portal 容器端点保留终端段类型
// -----------------------------------------------------------------------
#[test]
fn legacy_portal_terminal_is_preserved() {
    let mut root = graph("root");
    root.nodes.push(node("a"));
    root.nodes.push(portal_node("p1", "child"));
    let child = graph("child");
    let ctx = context(vec![("root", root), ("child", child)]);

    let plan = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "a"),
        &embed_path("root", vec![enter_portal("p1")]),
    )
    .unwrap();

    assert_eq!(plan.host_starmap_id, "root");
    assert_eq!(plan.to.segments, vec![enter_portal("p1")]);
    assert_eq!(plan.to.target, StarMapTargetDetail::Starmap);
}

// -----------------------------------------------------------------------
// 拒绝：自环
// -----------------------------------------------------------------------
#[test]
fn self_loop_node_rejected() {
    let mut g = graph("root");
    g.nodes.push(node("a"));
    let ctx = context(vec![("root", g)]);

    let result = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "a"),
        &item_path("root", vec![], "a"),
    );
    assert_eq!(result, Err(CrossLayerConnectionError::SelfLoop));
}

#[test]
fn self_loop_embed_rejected() {
    let mut root = graph("root");
    root.embeds.push(embed("e1", "child"));
    let child = graph("child");
    let ctx = context(vec![("root", root), ("child", child)]);

    let result = plan_cross_layer_connection(
        &ctx,
        &embed_path("root", vec![enter("e1")]),
        &embed_path("root", vec![enter("e1")]),
    );
    assert_eq!(result, Err(CrossLayerConnectionError::SelfLoop));
}

// -----------------------------------------------------------------------
// 拒绝：不同根星图
// -----------------------------------------------------------------------
#[test]
fn different_root_rejected() {
    let mut a = graph("a");
    a.nodes.push(node("na"));
    let mut b = graph("b");
    b.nodes.push(node("nb"));
    let ctx = context(vec![("a", a), ("b", b)]);

    let result = plan_cross_layer_connection(
        &ctx,
        &item_path("a", vec![], "na"),
        &item_path("b", vec![], "nb"),
    );
    assert_eq!(result, Err(CrossLayerConnectionError::NotSameRoot));
}

#[test]
fn empty_root_rejected() {
    let ctx = context(vec![]);
    let result = plan_cross_layer_connection(
        &ctx,
        &item_path("", vec![], "a"),
        &item_path("", vec![], "b"),
    );
    assert_eq!(result, Err(CrossLayerConnectionError::NotSameRoot));
}

// -----------------------------------------------------------------------
// 拒绝：不可解析的端点路径
// -----------------------------------------------------------------------
#[test]
fn missing_from_node_rejected() {
    let mut g = graph("root");
    g.nodes.push(node("b"));
    let ctx = context(vec![("root", g)]);

    let result = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "missing"),
        &item_path("root", vec![], "b"),
    );
    assert_eq!(
        result,
        Err(CrossLayerConnectionError::FromResolveFailed(
            StarMapTargetResolveStatus::MissingNode
        ))
    );
}

#[test]
fn missing_to_embed_rejected() {
    let mut g = graph("root");
    g.nodes.push(node("a"));
    let ctx = context(vec![("root", g)]);

    let result = plan_cross_layer_connection(
        &ctx,
        &item_path("root", vec![], "a"),
        &embed_path("root", vec![enter("missing")]),
    );
    assert_eq!(
        result,
        Err(CrossLayerConnectionError::ToResolveFailed(
            StarMapTargetResolveStatus::MissingEmbed
        ))
    );
}
