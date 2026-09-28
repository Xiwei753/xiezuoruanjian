//! 在 candidate `StarMapGraph` 上模拟 store update 的辅助函数。
//!
//! 这些函数在真正写 Store 之前先跑 `validate_graph`，字段更新语义必须与
//! `store/crud/*.rs` 保持一致。

use super::*;

// ---------------------------------------------------------------------------
// Candidate graph patch 应用辅助函数
//
// 这些函数在 candidate `StarMapGraph` 上模拟 store 的 update 操作，
// 用于在真正修改 Store 前跑 `validate_graph`。它们必须与 store CRUD 的
// 字段更新语义保持一致（见 store/crud/*.rs）。
// ---------------------------------------------------------------------------

pub(super) fn apply_node_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    node_id: &str,
    patch: &crate::starmap::types::StarMapNodePatch,
) -> Result<()> {
    let node = graph
        .nodes
        .iter_mut()
        .find(|n| n.id == node_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            ))
        })?;
    if let Some(ref t) = patch.title {
        node.title = t.clone();
    }
    if let Some(ref k) = patch.kind {
        node.kind = k.clone();
    }
    if let Some(ref p) = patch.payload {
        node.payload = p.clone();
    }
    if let Some(ref t) = patch.tags {
        node.tags = t.clone();
    }
    if let Some(ref c) = patch.content {
        node.content = c.clone();
    }
    if let Some(ref a) = patch.anchors {
        node.anchors = a.clone();
    }
    if let Some(ref p) = patch.portal {
        node.portal = p.clone();
    }
    if let Some(ref p) = patch.position {
        node.position = p.clone();
    }
    if let Some(ref s) = patch.style {
        node.style = s.clone();
    }
    if let Some(ref p) = patch.provenance {
        node.provenance = p.clone();
    }
    Ok(())
}

pub(super) fn apply_edge_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    edge_id: &str,
    patch: &crate::starmap::types::StarMapEdgePatch,
) -> Result<()> {
    let edge = graph
        .edges
        .iter_mut()
        .find(|e| e.id == edge_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            ))
        })?;
    if let Some(ref k) = patch.kind {
        edge.kind = k.clone();
    }
    if let Some(ref l) = patch.label {
        edge.label = l.clone();
    }
    if let Some(ref p) = patch.payload {
        edge.payload = p.clone();
    }
    if let Some(ref f) = patch.from {
        edge.from = f.clone();
    }
    if let Some(ref t) = patch.to {
        edge.to = t.clone();
    }
    Ok(())
}

pub(super) fn apply_embed_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    instance_id: &str,
    patch: &crate::starmap::types::StarMapEmbedPatch,
) -> Result<()> {
    let embed = graph
        .embeds
        .iter_mut()
        .find(|e| e.instance_id == instance_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Embed not found",
            ))
        })?;
    if let Some(ref l) = patch.label {
        embed.label = l.clone();
    }
    if let Some(ref p) = patch.position {
        embed.position = p.clone();
    }
    if let Some(ref hp) = patch.host_path {
        embed.host_path = hp.clone();
    }
    Ok(())
}

pub(super) fn apply_link_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    link_id: &str,
    patch: &crate::starmap::types::StarMapLinkPatch,
) -> Result<()> {
    let link = graph
        .links
        .iter_mut()
        .find(|l| l.link_id == link_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            ))
        })?;
    if let Some(ref s) = patch.source {
        link.source = s.clone();
    }
    if let Some(ref t) = patch.target {
        link.target = t.clone();
    }
    if let Some(ref l) = patch.label {
        link.label = l.clone();
    }
    Ok(())
}

pub(super) fn apply_hyperlink_update_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    hyperlink_id: &str,
    patch: &crate::starmap::types::StarMapHyperlinkPatch,
) -> Result<()> {
    let hl = graph
        .hyperlinks
        .iter_mut()
        .find(|h| h.hyperlink_id == hyperlink_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Hyperlink not found",
            ))
        })?;
    if let Some(ref l) = patch.label {
        hl.label = l.clone();
    }
    if let Some(ref u) = patch.target_uri {
        hl.target_uri = u.clone();
    }
    if let Some(ref s) = patch.source {
        hl.source = s.clone();
    }
    Ok(())
}

/// 在 candidate graph 上模拟 node 删除（含级联）。
///
/// `cascade` 由 `store.node_cascade_ids(node_id)` 计算得出，和 store 真实删除
/// 用同一纯函数，保证 candidate 模拟和 store 真实删除产生相同的最终对象集合。
pub(super) fn apply_node_deletion_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    node_id: &str,
    cascade: &crate::starmap::store::CascadeIds,
) {
    graph.nodes.retain(|n| n.id != node_id);
    graph.edges.retain(|e| !cascade.edge_ids.contains(&e.id));
    graph
        .embeds
        .retain(|em| !cascade.embed_ids.contains(&em.instance_id));
    graph
        .links
        .retain(|l| !cascade.link_ids.contains(&l.link_id));
    graph
        .hyperlinks
        .retain(|hl| !cascade.hyperlink_ids.contains(&hl.hyperlink_id));
}

/// 在 candidate graph 上模拟 embed 删除（含级联）。
///
/// `cascade` 由 `store.embed_cascade_ids(instance_id)` 计算得出，和 store 真实删除
/// 用同一纯函数，保证 candidate 模拟和 store 真实删除产生相同的最终对象集合。
pub(super) fn apply_embed_deletion_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    instance_id: &str,
    cascade: &crate::starmap::store::CascadeIds,
) {
    graph.embeds.retain(|em| em.instance_id != instance_id);
    graph.edges.retain(|e| !cascade.edge_ids.contains(&e.id));
    // cascade.embed_ids 已排除被删 instance 自己（见 embed_cascade_ids），
    // 但这里 retain 已经移除了自己，再 retain cascade.embed_ids 安全。
    graph
        .embeds
        .retain(|em| !cascade.embed_ids.contains(&em.instance_id));
    graph
        .links
        .retain(|l| !cascade.link_ids.contains(&l.link_id));
    graph
        .hyperlinks
        .retain(|hl| !cascade.hyperlink_ids.contains(&hl.hyperlink_id));
}
