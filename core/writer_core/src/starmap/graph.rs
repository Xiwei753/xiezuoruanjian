//! # 星图图形数据管理模块
//!
//! 本模块负责星图图形数据的持久化存储和操作，包括节点（Node）和边（Edge）的
//! 增删改查操作，以及布局（Layout）信息的管理。
//!
//! ## 主要功能
//! - **图形数据存储**：将星图的节点和边数据序列化为JSON格式存储
//! - **节点管理**：添加、更新、删除星图节点，自动同步布局信息
//! - **边管理**：添加、更新、删除星图边，验证节点存在性
//! - **布局管理**：存储和读取节点的位置、大小等布局信息
//! - **统计更新**：自动更新星图的节点数、边数和关联章节数
//!
//! ## 数据结构
//! - `StarMapGraph`：包含节点和边的图形数据结构
//! - `StarMapLayout`：包含节点布局信息的数据结构
//! - `StarMapNode`：星图节点，表示故事元素
//! - `StarMapEdge`：星图边，表示元素之间的关系
//!
//! ## 依赖关系
//! - `crate::starmap::types`：星图数据类型定义
//! - `crate::starmap`：星图元数据管理功能
//! - `crate::storage`：原子写入功能
//! - `crate::error`：错误处理
//!
//! ## 使用场景
//! - 可视化编辑器中的图形数据持久化
//! - 节点和边的CRUD操作
//! - 布局信息的保存和恢复

use crate::error::{Error, Result};
use crate::starmap::types::*;
use crate::starmap::{load_starmap_meta, now_epoch, starmaps_dir, update_starmap_stats};
use crate::storage::atomic_write_string;
use std::fs;
use std::path::Path;

fn graph_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("graph.json")
}

fn layout_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("layout.json")
}

pub fn get_starmap_graph(workspace: &Path, starmap_id: &str) -> Result<StarMapGraph> {
    let meta = load_starmap_meta(workspace, starmap_id)?;

    let path = graph_path(workspace, starmap_id);
    if !path.exists() {
        return Ok(StarMapGraph {
            schema_version: 1,
            id: starmap_id.to_string(),
            starmap_id: starmap_id.to_string(),
            title: meta.title.clone(),
            nodes: vec![],
            edges: vec![],
            embeds: vec![],
            links: vec![],
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
    }

    let json_str = fs::read_to_string(&path)?;
    let graph: StarMapGraph = serde_json::from_str(&json_str)?;
    Ok(graph)
}

pub fn save_starmap_graph(workspace: &Path, starmap_id: &str, graph: &StarMapGraph) -> Result<()> {
    validate_graph(workspace, graph)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(graph)?;
    atomic_write_string(&graph_path(workspace, starmap_id), &json_str)?;

    let node_count = graph.nodes.len() as u32;
    let edge_count = graph.edges.len() as u32;

    let mut linked_chapters = 0u32;
    for node in &graph.nodes {
        if node.kind == StarMapNodeKind::Chapter {
            linked_chapters += 1;
        }
    }

    update_starmap_stats(
        workspace,
        starmap_id,
        node_count,
        edge_count,
        linked_chapters,
    )?;
    Ok(())
}

pub fn get_starmap_layout(workspace: &Path, starmap_id: &str) -> Result<StarMapLayout> {
    let path = layout_path(workspace, starmap_id);
    if !path.exists() {
        return Ok(StarMapLayout::default());
    }

    let json_str = fs::read_to_string(&path)?;
    let layout: StarMapLayout = serde_json::from_str(&json_str)?;
    Ok(layout)
}

pub fn save_starmap_layout(
    workspace: &Path,
    starmap_id: &str,
    layout: &StarMapLayout,
) -> Result<()> {
    validate_layout(layout)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(layout)?;
    atomic_write_string(&layout_path(workspace, starmap_id), &json_str)?;
    Ok(())
}

pub fn add_starmap_node(
    workspace: &Path,
    starmap_id: &str,
    node: StarMapNode,
    default_x: f32,
    default_y: f32,
) -> Result<StarMapNode> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let new_node = node.clone();
    graph.nodes.push(new_node.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;

    if let Ok(mut layout) = get_starmap_layout(workspace, starmap_id) {
        layout.nodes.push(crate::starmap::types::StarMapLayoutNode {
            node_id: new_node.id.clone(),
            x: default_x,
            y: default_y,
            width: 150.0,
            height: 60.0,
            radius: 30.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        });
        let _ = save_starmap_layout(workspace, starmap_id, &layout);
    }

    Ok(new_node)
}

pub fn update_starmap_node(
    workspace: &Path,
    starmap_id: &str,
    node_id: &str,
    patch: StarMapNodePatch,
) -> Result<StarMapNode> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == node_id) {
        if let Some(t) = patch.title {
            node.title = t;
        }
        if let Some(k) = patch.kind {
            node.kind = k;
        }
        if let Some(p) = patch.payload {
            node.payload = p;
        }
        if let Some(t) = patch.tags {
            node.tags = t;
        }
        if let Some(c) = patch.content {
            node.content = c;
        }
        if let Some(a) = patch.anchors {
            node.anchors = a;
        }
        if let Some(p) = patch.portal {
            node.portal = p;
        }
        if let Some(dp) = patch.display_policy {
            node.display_policy = dp;
        }
        if let Some(ob) = patch.open_behavior {
            node.open_behavior = ob;
        }
        if let Some(p) = patch.provenance {
            node.provenance = p;
        }
        node.updated_at = now_epoch();
        let updated_node = node.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_node)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )))
    }
}

pub fn delete_starmap_node(workspace: &Path, starmap_id: &str, node_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.nodes.len();
    graph.nodes.retain(|n| n.id != node_id);
    if graph.nodes.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )));
    }

    graph.edges.retain(|e| e.from != node_id && e.to != node_id);

    if let Ok(mut layout) = get_starmap_layout(workspace, starmap_id) {
        layout.nodes.retain(|n| n.node_id != node_id);
        let _ = save_starmap_layout(workspace, starmap_id, &layout);
    }

    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

pub fn add_starmap_edge(
    workspace: &Path,
    starmap_id: &str,
    edge: StarMapEdge,
) -> Result<StarMapEdge> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;

    let from_valid = edge.from_target.is_some() || graph.nodes.iter().any(|n| n.id == edge.from);
    let to_valid = edge.to_target.is_some() || graph.nodes.iter().any(|n| n.id == edge.to);

    if !from_valid || !to_valid {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Edge nodes do not exist and no deep target is provided",
        )));
    }

    let new_edge = edge.clone();
    graph.edges.push(new_edge.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(new_edge)
}

pub fn update_starmap_edge(
    workspace: &Path,
    starmap_id: &str,
    edge_id: &str,
    patch: StarMapEdgePatch,
) -> Result<StarMapEdge> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(edge) = graph.edges.iter_mut().find(|e| e.id == edge_id) {
        if let Some(k) = patch.kind {
            edge.kind = k;
        }
        if let Some(l) = patch.label {
            edge.label = l;
        }
        if let Some(p) = patch.payload {
            edge.payload = p;
        }
        if let Some(ft) = patch.from_target {
            edge.from_target = ft;
        }
        if let Some(tt) = patch.to_target {
            edge.to_target = tt;
        }
        edge.updated_at = now_epoch();
        let updated_edge = edge.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_edge)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Edge not found",
        )))
    }
}

pub fn delete_starmap_edge(workspace: &Path, starmap_id: &str, edge_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.edges.len();
    graph.edges.retain(|e| e.id != edge_id);
    if graph.edges.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Edge not found",
        )));
    }
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

pub fn add_starmap_embed(
    workspace: &Path,
    starmap_id: &str,
    embed: StarMapEmbed,
) -> Result<StarMapEmbed> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if graph.embeds.iter().any(|e| e.instance_id == embed.instance_id) {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Duplicate embed instance_id",
        )));
    }
    graph.embeds.push(embed.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(embed)
}

pub fn update_starmap_embed(
    workspace: &Path,
    starmap_id: &str,
    instance_id: &str,
    patch: crate::starmap::types::StarMapEmbedPatch,
) -> Result<StarMapEmbed> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(embed) = graph.embeds.iter_mut().find(|e| e.instance_id == instance_id) {
        if let Some(l) = patch.label { embed.label = l; }
        if let Some(dp) = patch.display_policy { embed.display_policy = dp; }
        if let Some(ob) = patch.open_behavior { embed.open_behavior = ob; }
        if let Some(vp) = patch.viewport { embed.viewport = vp; }
        if let Some(sni) = patch.source_node_id { embed.source_node_id = sni; }
        if let Some(ha) = patch.host_anchor { embed.host_anchor = ha; }
        embed.updated_at = now_epoch();
        let updated = embed.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated)
    } else {
        Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Embed not found")))
    }
}

pub fn delete_starmap_embed(workspace: &Path, starmap_id: &str, instance_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.embeds.len();
    graph.embeds.retain(|e| e.instance_id != instance_id);
    if graph.embeds.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Embed not found")));
    }
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

pub fn add_starmap_link(
    workspace: &Path,
    starmap_id: &str,
    link: StarMapLink,
) -> Result<StarMapLink> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if graph.links.iter().any(|l| l.link_id == link.link_id) {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Duplicate link_id",
        )));
    }
    graph.links.push(link.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(link)
}

pub fn update_starmap_link(
    workspace: &Path,
    starmap_id: &str,
    link_id: &str,
    patch: crate::starmap::types::StarMapLinkPatch,
) -> Result<StarMapLink> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(link) = graph.links.iter_mut().find(|l| l.link_id == link_id) {
        if let Some(s) = patch.source { link.source = s; }
        if let Some(t) = patch.target { link.target = t; }
        if let Some(l) = patch.label { link.label = l; }
        link.updated_at = now_epoch();
        let updated = link.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated)
    } else {
        Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Link not found")))
    }
}

pub fn delete_starmap_link(workspace: &Path, starmap_id: &str, link_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.links.len();
    graph.links.retain(|l| l.link_id != link_id);
    if graph.links.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "Link not found")));
    }
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

fn validate_graph(workspace: &Path, graph: &StarMapGraph) -> Result<()> {
    let mut node_ids = std::collections::HashSet::new();
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate node ID",
            )));
        }

        if let crate::starmap::semantic::StarMapNodeContent::ChapterRef {
            range_start,
            range_end,
            ..
        } = &node.content
        {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Content range_start cannot be greater than range_end",
                    )));
                }
            }
        }

        let mut anchor_ids = std::collections::HashSet::new();
        for anchor in &node.anchors {
            if !anchor_ids.insert(&anchor.anchor_id) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Duplicate anchor ID in node",
                )));
            }
            if let crate::starmap::semantic::StarMapAnchorTarget::ChapterRange {
                range_start,
                range_end,
                ..
            } = &anchor.target
            {
                if let (Some(s), Some(e)) = (range_start, range_end) {
                    if s > e {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Anchor range_start cannot be greater than range_end",
                        )));
                    }
                }
            }
        }

        if let Some(portal) = &node.portal {
            if portal.mode == crate::starmap::semantic::StarMapPortalMode::EnterChild {
                let target_id = portal
                    .deep_target
                    .as_ref()
                    .map(|t| t.starmap_id.clone())
                    .unwrap_or_else(|| portal.target_starmap_id.clone());
                
                if crate::starmap::load_starmap_meta(workspace, &target_id).is_err() {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Portal target starmap does not exist",
                    )));
                }

                if let Some(dt) = &portal.deep_target {
                    let status = resolve_deep_target(workspace, dt);
                    use crate::starmap::semantic::StarMapTargetResolveStatus::*;
                    match status {
                        CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor | InvalidRange => {
                            return Err(Error::Io(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Deep target resolve failed: {:?}", status),
                            )));
                        }
                        _ => {}
                    }
                }
            }
        }

        let dp = &node.display_policy;
        if dp.importance.is_nan()
            || dp.importance < 0.0
            || dp.min_visible_scale.is_nan()
            || dp.min_visible_scale < 0.0
            || dp.title_scale.is_nan()
            || dp.title_scale < 0.0
            || dp.summary_scale.is_nan()
            || dp.summary_scale < 0.0
            || dp.detail_scale.is_nan()
            || dp.detail_scale < 0.0
            || dp.min_readable_px.is_nan()
            || dp.min_readable_px < 0.0
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid display policy values",
            )));
        }

        // min_visible_scale <= title_scale <= summary_scale <= detail_scale
        if !(dp.min_visible_scale <= dp.title_scale
            && dp.title_scale <= dp.summary_scale
            && dp.summary_scale <= dp.detail_scale)
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Display policy scales must be ordered: min_visible <= title <= summary <= detail",
            )));
        }
    }

    for edge in &graph.edges {
        if let Some(ft) = &edge.from_target {
            let status = resolve_deep_target(workspace, ft);
            use crate::starmap::semantic::StarMapTargetResolveStatus::*;
            match status {
                CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor | InvalidRange => {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Deep target resolve failed: {:?}", status),
                    )));
                }
                _ => {}
            }
        } else {
            if !node_ids.contains(&edge.from) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Edge references non-existent from node",
                )));
            }
        }

        if let Some(tt) = &edge.to_target {
            let status = resolve_deep_target(workspace, tt);
            use crate::starmap::semantic::StarMapTargetResolveStatus::*;
            match status {
                CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor | InvalidRange => {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Deep target resolve failed: {:?}", status),
                    )));
                }
                _ => {}
            }
        } else {
            if !node_ids.contains(&edge.to) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Edge references non-existent to node",
                )));
            }
        }
    }

    let mut instance_ids = std::collections::HashSet::new();
    for embed in &graph.embeds {
        if !instance_ids.insert(&embed.instance_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate embed instance_id",
            )));
        }
        if crate::starmap::load_starmap_meta(workspace, &embed.target_starmap_id).is_err() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Embed target starmap does not exist",
            )));
        }
        if let Some(vp) = &embed.viewport {
            if vp.scale <= 0.0 || vp.scale.is_nan() || vp.width.is_nan() || vp.height.is_nan() || vp.offset_x.is_nan() || vp.offset_y.is_nan() {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid viewport values",
                )));
            }
        }
        if let Some(sni) = &embed.source_node_id {
            if !node_ids.contains(sni) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Embed source_node_id does not exist",
                )));
            }
        }
        if let Some(ha) = &embed.host_anchor {
            let mut anchor_found = false;
            for node in &graph.nodes {
                if node.anchors.iter().any(|a| &a.anchor_id == ha) {
                    anchor_found = true;
                    break;
                }
            }
            if !anchor_found {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Embed host_anchor does not exist",
                )));
            }
        }
    }

    let mut link_ids = std::collections::HashSet::new();
    for link in &graph.links {
        if !link_ids.insert(&link.link_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate link_id",
            )));
        }
        match &link.source {
            crate::starmap::types::StarMapEndpoint::Node { node_id } => {
                if !node_ids.contains(node_id) {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Link source node does not exist",
                    )));
                }
            }
            crate::starmap::types::StarMapEndpoint::Anchor { node_id, anchor_id } => {
                if let Some(n) = graph.nodes.iter().find(|n| &n.id == node_id) {
                    if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                        return Err(Error::Io(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "Link source anchor does not exist",
                        )));
                    }
                } else {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Link source node for anchor does not exist",
                    )));
                }
            }
            crate::starmap::types::StarMapEndpoint::Starmap => {}
        }

        let status = resolve_deep_target(workspace, &link.target);
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        match status {
            CycleDetected | TooDeep | MissingStarmap | MissingNode | MissingAnchor | InvalidRange => {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Link deep target resolve failed: {:?}", status),
                )));
            }
            _ => {}
        }
    }

    Ok(())
}


pub fn resolve_deep_target(
    workspace: &Path,
    dt: &crate::starmap::semantic::StarMapDeepTarget,
) -> crate::starmap::semantic::StarMapTargetResolveStatus {
    use crate::starmap::semantic::StarMapTargetResolveStatus::*;

    if dt.path.len() > 32 {
        return TooDeep;
    }

    if crate::starmap::load_starmap_meta(workspace, &dt.starmap_id).is_err() {
        return MissingStarmap;
    }

    let mut current_starmap_id = dt.starmap_id.clone();
    let mut visited = std::collections::HashSet::new();
    visited.insert(current_starmap_id.clone());

    for segment in &dt.path {
        match segment {
            crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id } => {
                current_starmap_id = starmap_id.clone();
                if !visited.insert(current_starmap_id.clone()) {
                    return CycleDetected;
                }
                if crate::starmap::load_starmap_meta(workspace, &current_starmap_id).is_err() {
                    return MissingStarmap;
                }
            }
            crate::starmap::semantic::StarMapPathSegment::EnterNode { node_id } => {
                let target_graph_path = crate::starmap::starmap_graph_path(workspace, &current_starmap_id);
                if target_graph_path.exists() {
                    if let Ok(json_str) = std::fs::read_to_string(&target_graph_path) {
                        if let Ok(target_graph) = serde_json::from_str::<crate::starmap::types::StarMapGraph>(&json_str) {
                            if !target_graph.nodes.iter().any(|n| &n.id == node_id) {
                                return MissingNode;
                            }
                        } else {
                            return Unresolved;
                        }
                    } else {
                        return Unresolved;
                    }
                } else {
                    return MissingNode;
                }
            }
        }
    }

    match &dt.target {
        crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => {
            let target_graph_path = crate::starmap::starmap_graph_path(workspace, &current_starmap_id);
            if target_graph_path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&target_graph_path) {
                    if let Ok(target_graph) = serde_json::from_str::<crate::starmap::types::StarMapGraph>(&json_str) {
                        if !target_graph.nodes.iter().any(|n| &n.id == node_id) {
                            return MissingNode;
                        }
                    }
                }
            } else {
                return MissingNode;
            }
        }
        crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            let target_graph_path = crate::starmap::starmap_graph_path(workspace, &current_starmap_id);
            if target_graph_path.exists() {
                if let Ok(json_str) = std::fs::read_to_string(&target_graph_path) {
                    if let Ok(target_graph) = serde_json::from_str::<crate::starmap::types::StarMapGraph>(&json_str) {
                        if let Some(n) = target_graph.nodes.iter().find(|n| &n.id == node_id) {
                            if !n.anchors.iter().any(|a| &a.anchor_id == anchor_id) {
                                return MissingAnchor;
                            }
                        } else {
                            return MissingNode;
                        }
                    }
                }
            } else {
                return MissingNode;
            }
        }
        crate::starmap::semantic::StarMapTargetDetail::ChapterRange {
            range_start,
            range_end,
            ..
        } => {
            if let (Some(s), Some(e)) = (range_start, range_end) {
                if s > e {
                    return InvalidRange;
                }
            }
        }
        _ => {}
    }

    Resolved
}

fn validate_layout(layout: &StarMapLayout) -> Result<()> {
    for node in &layout.nodes {
        if node.scale <= 0.0 || node.scale.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout scale must be > 0",
            )));
        }
        if node.depth.is_nan() || node.focus_weight.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout depth or focus_weight cannot be NaN",
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::create_starmap;
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    fn setup_workspace() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        create_workspace(dir.path()).unwrap();
        dir
    }

    #[test]
    fn test_starmap_graph_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);

        let node1 = StarMapNode {
            id: "n1".to_string(),
            title: "Node 1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node1.clone(), 0.0, 0.0).unwrap();

        let node2 = StarMapNode {
            id: "n2".to_string(),
            title: "Node 2".to_string(),
            kind: StarMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node2.clone(), 0.0, 0.0).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 2);

        let refreshed_meta = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta.node_count, 2);

        update_starmap_node(
            dir.path(),
            &meta.starmap_id,
            "n1",
            StarMapNodePatch {
                title: Some("Updated N1".to_string()),
                kind: Some(StarMapNodeKind::Chapter),
                payload: None,
                tags: None,
                content: None,
                anchors: None,
                portal: None,
                display_policy: None,
                open_behavior: None,
                provenance: None,
            },
        )
        .unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(
            graph.nodes.iter().find(|n| n.id == "n1").unwrap().title,
            "Updated N1"
        );

        let refreshed_meta2 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta2.linked_chapter_count, 1);

        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: "n1".to_string(),
            to: "n2".to_string(),
            kind: StarMapEdgeKind::RelatedTo,
            label: Some("relates".to_string()),
            payload: None,
            from_target: None,
            to_target: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_edge(dir.path(), &meta.starmap_id, edge.clone()).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges.len(), 1);

        update_starmap_edge(
            dir.path(),
            &meta.starmap_id,
            "e1",
            StarMapEdgePatch {
                kind: Some(StarMapEdgeKind::Causes),
                label: Some(Some("causes".to_string())),
                payload: None,
                from_target: None,
                to_target: None,
            },
        )
        .unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges[0].label.as_deref(), Some("causes"));

        delete_starmap_node(dir.path(), &meta.starmap_id, "n1").unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 0);

        let refreshed_meta3 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta3.node_count, 1);
        assert_eq!(refreshed_meta3.edge_count, 0);
        assert_eq!(refreshed_meta3.linked_chapter_count, 0);
    }

    #[test]
    fn test_starmap_layout_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Layout Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut layout = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(layout.nodes.len(), 0);

        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 100.0,
            y: 200.0,
            width: 50.0,
            height: 50.0,
            radius: 25.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        });

        save_starmap_layout(dir.path(), &meta.starmap_id, &layout).unwrap();

        let loaded = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].x, 100.0);
    }

    #[test]
    fn test_embed_and_link_crud_and_validation() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let meta_b = create_starmap(dir.path(), "Map B", "", None).unwrap();
        
        // 1. add/update/delete embed 正常, delete embed 不删除 target StarMap
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: "inst1".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            viewport: None,
            source_node_id: None,
            host_anchor: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        assert!(crate::starmap::graph::add_starmap_embed(dir.path(), &meta_a.starmap_id, embed.clone()).is_ok());
        
        // duplicate embed instance_id 失败
        assert!(crate::starmap::graph::add_starmap_embed(dir.path(), &meta_a.starmap_id, embed.clone()).is_err());
        
        assert!(crate::starmap::graph::update_starmap_embed(dir.path(), &meta_a.starmap_id, "inst1", crate::starmap::types::StarMapEmbedPatch {
            label: Some(Some("updated".to_string())),
            display_policy: None,
            open_behavior: None,
            viewport: None,
            source_node_id: None,
            host_anchor: None,
        }).is_ok());

        assert!(crate::starmap::graph::delete_starmap_embed(dir.path(), &meta_a.starmap_id, "inst1").is_ok());
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());

        // 2. embed target_starmap_id 不存在失败
        let embed_missing = crate::starmap::types::StarMapEmbed {
            instance_id: "inst_missing".to_string(),
            target_starmap_id: "non-existent".to_string(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            viewport: None,
            source_node_id: None,
            host_anchor: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g.embeds.push(embed_missing);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g).is_err());

        // 3. embed source_node_id 不存在失败
        let embed_missing_node = crate::starmap::types::StarMapEmbed {
            instance_id: "inst2".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            viewport: None,
            source_node_id: Some("missing_node".to_string()),
            host_anchor: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g2 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g2.embeds.push(embed_missing_node);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g2).is_err());

        // 4. add/update/delete link 正常
        let link = crate::starmap::types::StarMapLink {
            link_id: "link1".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        assert!(crate::starmap::graph::add_starmap_link(dir.path(), &meta_a.starmap_id, link.clone()).is_ok());
        
        // duplicate link_id 失败
        assert!(crate::starmap::graph::add_starmap_link(dir.path(), &meta_a.starmap_id, link.clone()).is_err());

        assert!(crate::starmap::graph::update_starmap_link(dir.path(), &meta_a.starmap_id, "link1", crate::starmap::types::StarMapLinkPatch {
            source: None,
            target: None,
            label: Some(Some("updated_link".to_string())),
        }).is_ok());

        assert!(crate::starmap::graph::delete_starmap_link(dir.path(), &meta_a.starmap_id, "link1").is_ok());
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());

        // 5. link source endpoint 不存在失败
        let link_missing_src = crate::starmap::types::StarMapLink {
            link_id: "link_bad_src".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Node { node_id: "missing_node".to_string() },
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g3 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g3.links.push(link_missing_src);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g3).is_err());

        // 6. link target missing starmap/node/anchor 失败
        let link_missing_tgt = crate::starmap::types::StarMapLink {
            link_id: "link_bad_tgt".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: "missing_starmap".to_string(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g4 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g4.links.push(link_missing_tgt);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g4).is_err());

        // 7. semantic edge 可以指向 DeepTarget，不需要 dummy to node
        let n1 = StarMapNode { id: "n1".to_string(), title: "n1".to_string(), kind: StarMapNodeKind::Note, payload: None, tags: vec![], content: Default::default(), anchors: vec![], portal: None, display_policy: Default::default(), open_behavior: Default::default(), provenance: Default::default(), created_at: now_epoch(), updated_at: now_epoch() };
        let mut g5 = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g5.nodes.push(n1);
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &g5).unwrap();

        let edge = StarMapEdge {
            id: "e_semantic".to_string(),
            from: "n1".to_string(),
            to: "dummy_missing".to_string(), // node does not exist
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: Some(crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            }),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        // This should pass because `to_target` is present, bypassing `dummy_missing` check
        assert!(crate::starmap::graph::add_starmap_edge(dir.path(), &meta_a.starmap_id, edge).is_ok());

        // 8. find_starmap_references 能返回同一 host graph 中多个引用 & DeepTarget path 中间 starmap
        let meta_c = create_starmap(dir.path(), "Map C", "", None).unwrap();
        let link_deep = crate::starmap::types::StarMapLink {
            link_id: "link_deep".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(), // Target is B
                path: vec![crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id: meta_c.starmap_id.clone() }], // Path goes through C
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        crate::starmap::graph::add_starmap_link(dir.path(), &meta_a.starmap_id, link_deep).unwrap();
        
        let refs_to_b = crate::starmap::find_starmap_references(dir.path(), &meta_b.starmap_id).unwrap();
        // A has an edge to B and a link to B
        assert_eq!(refs_to_b.len(), 2);

        let refs_to_c = crate::starmap::find_starmap_references(dir.path(), &meta_c.starmap_id).unwrap();
        // A has a link whose path goes through C
        assert_eq!(refs_to_c.len(), 1);
        assert_eq!(refs_to_c[0].ref_type, "link");
    }

    #[test]
    fn test_starmap_deep_target_validation() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let meta_b = create_starmap(dir.path(), "Map B", "", None).unwrap();
        
        let mut graph_a = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        
        let node_a1 = StarMapNode {
            id: "a1".to_string(),
            title: "Node A1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        graph_a.nodes.push(node_a1.clone());
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).unwrap();

        // 1. Portal point to non-existent starmap -> fail
        let mut node_with_portal = node_a1.clone();
        node_with_portal.portal = Some(crate::starmap::semantic::StarMapPortal {
            target_starmap_id: "non-existent".to_string(),
            deep_target: None,
            mode: crate::starmap::semantic::StarMapPortalMode::EnterChild,
            preview_policy: Default::default(),
        });
        let mut g = graph_a.clone();
        g.nodes[0] = node_with_portal;
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g).is_err());

        // 2. Edge from current to another StarMap
        let edge_to_b = StarMapEdge {
            id: "e1".to_string(),
            from: "a1".to_string(),
            to: "dummy".to_string(),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: Some(crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            }),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g2 = graph_a.clone();
        g2.edges.push(edge_to_b);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g2).is_ok());

        // 3. Edge with deep path (Cycle detection)
        let mut dt_cycle = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_b.starmap_id.clone(),
            path: vec![
                crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id: meta_b.starmap_id.clone() }
            ],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        let edge_cycle = StarMapEdge {
            id: "e2".to_string(),
            from: "a1".to_string(),
            to: "dummy".to_string(),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: Some(dt_cycle),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        let mut g3 = graph_a.clone();
        g3.edges.push(edge_cycle);
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g3).is_err()); // Cycle detected

        // 4. Invalid display policy
        let mut node_dp = node_a1.clone();
        node_dp.display_policy = crate::starmap::semantic::StarMapDisplayPolicy {
            importance: -1.0,
            ..Default::default()
        };
        let mut g4 = graph_a.clone();
        g4.nodes[0] = node_dp;
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g4).is_err());

        // 5. Invalid Anchor range
        let mut node_anchor = node_a1.clone();
        node_anchor.anchors.push(crate::starmap::semantic::StarMapAnchor {
            anchor_id: "anc1".to_string(),
            target: crate::starmap::semantic::StarMapAnchorTarget::ChapterRange {
                project_id: None,
                volume_id: None,
                chapter_id: "chap1".to_string(),
                range_start: Some(100),
                range_end: Some(50),
            },
            label: None,
            role: Default::default(),
        });
        let mut g5 = graph_a.clone();
        g5.nodes[0] = node_anchor;
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &g5).is_err());
    }

    #[test]
    fn test_starmap_embed_and_link_semantics() {
        let dir = setup_workspace();
        // 新建 A/B/C 三个独立 StarMap
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let meta_b = create_starmap(dir.path(), "Map B", "", None).unwrap();
        let meta_c = create_starmap(dir.path(), "Map C", "", None).unwrap();
        
        let mut graph_a = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        let mut graph_b = get_starmap_graph(dir.path(), &meta_b.starmap_id).unwrap();
        let mut graph_c = get_starmap_graph(dir.path(), &meta_c.starmap_id).unwrap();

        // B 是空 StarMap，也可以被 embed
        // A embed B
        graph_a.embeds.push(crate::starmap::types::StarMapEmbed {
            instance_id: "inst1".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: Some("B in A".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            viewport: None,
            source_node_id: None,
            host_anchor: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });

        // 一个 StarMap 只有 embed，没有普通 node，合法
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).is_ok());

        // C 也能 embed B
        graph_c.embeds.push(crate::starmap::types::StarMapEmbed {
            instance_id: "inst2".to_string(),
            target_starmap_id: meta_b.starmap_id.clone(),
            label: Some("B in C".to_string()),
            display_policy: Default::default(),
            open_behavior: Default::default(),
            viewport: None,
            source_node_id: None,
            host_anchor: None,
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
        assert!(save_starmap_graph(dir.path(), &meta_c.starmap_id, &graph_c).is_ok());

        // A link 到 B，不会创建 embed instance
        graph_a.links.push(crate::starmap::types::StarMapLink {
            link_id: "link1".to_string(),
            source: crate::starmap::types::StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta_b.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).is_ok());

        // B 本体仍然独立存在
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());
        
        // 测试 find_starmap_references
        let refs = crate::starmap::find_starmap_references(dir.path(), &meta_b.starmap_id).unwrap();
        assert_eq!(refs.len(), 3);
        let has_a = refs.iter().any(|r| r.host_starmap_id == meta_a.starmap_id && r.ref_type == "embed");
        let has_c = refs.iter().any(|r| r.host_starmap_id == meta_c.starmap_id && r.ref_type == "embed");
        assert!(has_a);
        assert!(has_c);

        // 删除 A 中的 B embed，不删除 B
        graph_a.embeds.clear();
        assert!(save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).is_ok());
        assert!(crate::starmap::load_starmap_meta(dir.path(), &meta_b.starmap_id).is_ok());
    }

    #[test]
    fn test_starmap_edge_patching() {
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        let mut graph_a = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();

        // Add 2 nodes
        let n1 = StarMapNode { id: "n1".to_string(), title: "n1".to_string(), kind: StarMapNodeKind::Note, payload: None, tags: vec![], content: Default::default(), anchors: vec![], portal: None, display_policy: Default::default(), open_behavior: Default::default(), provenance: Default::default(), created_at: now_epoch(), updated_at: now_epoch() };
        let n2 = StarMapNode { id: "n2".to_string(), title: "n2".to_string(), kind: StarMapNodeKind::Note, payload: None, tags: vec![], content: Default::default(), anchors: vec![], portal: None, display_policy: Default::default(), open_behavior: Default::default(), provenance: Default::default(), created_at: now_epoch(), updated_at: now_epoch() };
        
        graph_a.nodes.push(n1);
        graph_a.nodes.push(n2);
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &graph_a).unwrap();

        // add edge
        let edge = add_starmap_edge(dir.path(), &meta_a.starmap_id, StarMapEdge {
            id: "e1".to_string(),
            from: "n1".to_string(),
            to: "n2".to_string(),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        }).unwrap();

        // 1. 设置 to_target (Some(Some(target)))
        let dt = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Node { node_id: "n2".to_string() },
        };
        update_starmap_edge(dir.path(), &meta_a.starmap_id, "e1", StarMapEdgePatch {
            kind: None, label: None, payload: None, from_target: None,
            to_target: Some(Some(dt.clone()))
        }).unwrap();
        let g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        assert!(g.edges[0].to_target.is_some());

        // 2. 清空 to_target (Some(None))
        update_starmap_edge(dir.path(), &meta_a.starmap_id, "e1", StarMapEdgePatch {
            kind: None, label: None, payload: None, from_target: None,
            to_target: Some(None)
        }).unwrap();
        let g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        assert!(g.edges[0].to_target.is_none());

        // 3. 不改 to_target (None)
        update_starmap_edge(dir.path(), &meta_a.starmap_id, "e1", StarMapEdgePatch {
            kind: None, label: None, payload: None, from_target: None,
            to_target: None
        }).unwrap();
        let g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        assert!(g.edges[0].to_target.is_none());
    }

    #[test]
    fn test_deep_target_resolution() {
        use crate::starmap::semantic::StarMapTargetResolveStatus::*;
        let dir = setup_workspace();
        let meta_a = create_starmap(dir.path(), "Map A", "", None).unwrap();
        
        // MissingStarmap
        let dt_missing_sm = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: "missing".to_string(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_missing_sm), MissingStarmap);

        // MissingNode
        let dt_missing_node = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Node { node_id: "non-existent".to_string() },
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_missing_node), MissingNode);
        
        let mut g = get_starmap_graph(dir.path(), &meta_a.starmap_id).unwrap();
        g.nodes.push(StarMapNode { 
            id: "n1".to_string(), title: "n1".to_string(), kind: StarMapNodeKind::Note, payload: None, tags: vec![], content: Default::default(), 
            anchors: vec![crate::starmap::semantic::StarMapAnchor {
                anchor_id: "a1".to_string(),
                target: crate::starmap::semantic::StarMapAnchorTarget::Project { project_id: "p".to_string() },
                label: None,
                role: Default::default(),
            }], 
            portal: None, display_policy: Default::default(), open_behavior: Default::default(), provenance: Default::default(), created_at: now_epoch(), updated_at: now_epoch() 
        });
        save_starmap_graph(dir.path(), &meta_a.starmap_id, &g).unwrap();

        // Node Exists
        let dt_node_exists = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Node { node_id: "n1".to_string() },
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_node_exists), Resolved);

        // MissingAnchor
        let dt_missing_anchor = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id: "n1".to_string(), anchor_id: "missing".to_string() },
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_missing_anchor), MissingAnchor);

        // Anchor Exists
        let dt_anchor_exists = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![],
            target: crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id: "n1".to_string(), anchor_id: "a1".to_string() },
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_anchor_exists), Resolved);

        // Path > 32 => TooDeep
        let dt_too_deep = crate::starmap::semantic::StarMapDeepTarget {
            starmap_id: meta_a.starmap_id.clone(),
            path: vec![crate::starmap::semantic::StarMapPathSegment::EnterNode { node_id: "n1".to_string() }; 33],
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        assert_eq!(resolve_deep_target(dir.path(), &dt_too_deep), TooDeep);
    }
}

