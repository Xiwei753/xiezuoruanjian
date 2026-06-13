//! # 星图包存储模块
//!
//! 本模块实现了星图文档的拆分文件存储（Package Storage）。
//!
//! ## 存储结构
//!
//! ```text
//! starmaps/<id>/
//!   graph.json                        -- 图形元数据（schema_version, id, starmap_id, title, timestamps）
//!   nodes/<node_id>.json              -- 单个节点
//!   edges/<edge_id>.json              -- 单条边
//!   child_starmaps/<instance_id>.json -- 单个 embed（子星图放置）
//!   links/<link_id>.json              -- 单个 link
//!   hyperlinks/<hyperlink_id>.json    -- 单个超链接
//!   layouts/default.json              -- 默认布局
//!   viewport.json                     -- 视口
//! ```
//!
//! ## 设计原则
//!
//! - 改一个节点，只改 `nodes/<id>.json`
//! - 改一条线，只改 `edges/<id>.json`
//! - 移动布局，只改 `layouts/default.json`
//! - 超链接不进 edges
//! - 子星图 placement 不进 nodes
//! - 旧单文件 `starmap.json` 能迁移/兼容读取

use crate::error::{Error, Result};
use crate::starmap::types::*;
use crate::storage::atomic_write_string;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// 路径辅助函数
// ---------------------------------------------------------------------------

fn starmap_pkg_dir(workspace: &Path, starmap_id: &str) -> PathBuf {
    workspace
        .join("app-meta")
        .join("starmaps")
        .join(starmap_id)
}

fn graph_meta_path(dir: &Path) -> PathBuf {
    dir.join("graph.json")
}

fn nodes_dir(dir: &Path) -> PathBuf {
    dir.join("nodes")
}

fn node_path(dir: &Path, node_id: &str) -> PathBuf {
    dir.join("nodes").join(format!("{}.json", node_id))
}

fn edges_dir(dir: &Path) -> PathBuf {
    dir.join("edges")
}

fn edge_path(dir: &Path, edge_id: &str) -> PathBuf {
    dir.join("edges").join(format!("{}.json", edge_id))
}

fn child_starmaps_dir(dir: &Path) -> PathBuf {
    dir.join("child_starmaps")
}

fn child_starmap_path(dir: &Path, instance_id: &str) -> PathBuf {
    dir.join("child_starmaps").join(format!("{}.json", instance_id))
}

fn links_dir(dir: &Path) -> PathBuf {
    dir.join("links")
}

fn link_path(dir: &Path, link_id: &str) -> PathBuf {
    dir.join("links").join(format!("{}.json", link_id))
}

fn hyperlinks_dir(dir: &Path) -> PathBuf {
    dir.join("hyperlinks")
}

fn hyperlink_path(dir: &Path, hyperlink_id: &str) -> PathBuf {
    dir.join("hyperlinks").join(format!("{}.json", hyperlink_id))
}

fn layout_path(dir: &Path) -> PathBuf {
    dir.join("layouts").join("default.json")
}

fn viewport_path(dir: &Path) -> PathBuf {
    dir.join("viewport.json")
}

/// 旧格式单文件路径（用于兼容读取）
fn legacy_starmap_path(workspace: &Path, starmap_id: &str) -> PathBuf {
    workspace
        .join("app-meta")
        .join("starmaps")
        .join(format!("{}.json", starmap_id))
}

// ---------------------------------------------------------------------------
// 图形元数据（graph.json 只存元信息，不存 nodes/edges 列表）
// ---------------------------------------------------------------------------

/// 图形元数据：graph.json 中只保留元信息，nodes/edges/embeds/links 列表为空。
/// 实际数据拆分到各自的子目录中。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMeta {
    pub schema_version: u32,
    pub id: String,
    pub starmap_id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<&StarMapGraph> for GraphMeta {
    fn from(g: &StarMapGraph) -> Self {
        Self {
            schema_version: g.schema_version,
            id: g.id.clone(),
            starmap_id: g.starmap_id.clone(),
            title: g.title.clone(),
            created_at: g.created_at,
            updated_at: g.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Load: 从拆分文件组装 StarMapDocument
// ---------------------------------------------------------------------------

/// 从拆分文件存储加载完整的 StarMapDocument。
///
/// 如果检测到旧格式单文件（`starmap.json`），会自动迁移到新格式。
pub fn load_starmap_document(workspace: &Path, starmap_id: &str) -> Result<StarMapDocument> {
    let dir = starmap_pkg_dir(workspace, starmap_id);

    // 检查旧格式并迁移
    let legacy_path = legacy_starmap_path(workspace, starmap_id);
    if legacy_path.exists() && !graph_meta_path(&dir).exists() {
        migrate_from_legacy(workspace, starmap_id)?;
    }

    if !dir.exists() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("StarMap package not found: {}", starmap_id),
        )));
    }

    // 1. 读取图形元数据
    let meta_path = graph_meta_path(&dir);
    let graph_meta: GraphMeta = if meta_path.exists() {
        let json_str = fs::read_to_string(&meta_path)?;
        serde_json::from_str(&json_str)?
    } else {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("graph.json not found for starmap: {}", starmap_id),
        )));
    };

    // 2. 读取节点
    let mut nodes = Vec::new();
    let nodes_dir = nodes_dir(&dir);
    if nodes_dir.exists() {
        for entry in fs::read_dir(&nodes_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let json_str = fs::read_to_string(&path)?;
                if let Ok(node) = serde_json::from_str::<StarMapNode>(&json_str) {
                    nodes.push(node);
                }
            }
        }
    }

    // 3. 读取边
    let mut edges = Vec::new();
    let edges_dir_path = edges_dir(&dir);
    if edges_dir_path.exists() {
        for entry in fs::read_dir(&edges_dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let json_str = fs::read_to_string(&path)?;
                if let Ok(edge) = serde_json::from_str::<StarMapEdge>(&json_str) {
                    edges.push(edge);
                }
            }
        }
    }

    // 4. 读取 embeds（子星图放置）
    let mut embeds = Vec::new();
    let cs_dir = child_starmaps_dir(&dir);
    if cs_dir.exists() {
        for entry in fs::read_dir(&cs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let json_str = fs::read_to_string(&path)?;
                if let Ok(embed) = serde_json::from_str::<StarMapEmbed>(&json_str) {
                    embeds.push(embed);
                }
            }
        }
    }

    // 5. 读取 links
    let mut links = Vec::new();
    let links_dir_path = links_dir(&dir);
    if links_dir_path.exists() {
        for entry in fs::read_dir(&links_dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let json_str = fs::read_to_string(&path)?;
                if let Ok(link) = serde_json::from_str::<StarMapLink>(&json_str) {
                    links.push(link);
                }
            }
        }
    }

    // 6. 读取超链接
    let mut hyperlinks = Vec::new();
    let hl_dir = hyperlinks_dir(&dir);
    if hl_dir.exists() {
        for entry in fs::read_dir(&hl_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let json_str = fs::read_to_string(&path)?;
                if let Ok(hl) = serde_json::from_str::<StarMapHyperlink>(&json_str) {
                    hyperlinks.push(hl);
                }
            }
        }
    }

    // 7. 读取布局
    let layout = {
        let lp = layout_path(&dir);
        if lp.exists() {
            let json_str = fs::read_to_string(&lp)?;
            serde_json::from_str::<StarMapLayout>(&json_str)?
        } else {
            StarMapLayout::default()
        }
    };

    // 8. 读取视口
    let viewport = {
        let vp = viewport_path(&dir);
        if vp.exists() {
            let json_str = fs::read_to_string(&vp)?;
            serde_json::from_str::<StarMapViewport>(&json_str)?
        } else {
            StarMapViewport::default()
        }
    };

    // 9. 读取子星图放置信息（从 embeds 中提取）
    let child_map_placements: Vec<StarMapChildMapPlacement> = embeds
        .iter()
        .map(|e| StarMapChildMapPlacement {
            instance_id: e.instance_id.clone(),
            target_starmap_id: e.target_starmap_id.clone(),
            placement: e.placement.clone(),
            target_viewport: e.target_viewport.clone(),
            display_policy: e.display_policy.clone(),
            open_behavior: e.open_behavior.clone(),
        })
        .collect();

    // 10. 读取文档元信息
    let meta = crate::starmap::get_starmap(workspace, starmap_id)?;

    // 组装 StarMapDocument
    let graph = StarMapGraph {
        schema_version: graph_meta.schema_version,
        id: graph_meta.id,
        starmap_id: graph_meta.starmap_id,
        title: graph_meta.title,
        nodes,
        edges,
        embeds,
        links,
        created_at: graph_meta.created_at,
        updated_at: graph_meta.updated_at,
    };

    Ok(StarMapDocument {
        starmap_id: meta.starmap_id,
        title: meta.title,
        description: meta.description,
        project_id: meta.project_id,
        parent_starmap_id: meta.parent_starmap_id,
        is_main_for_project: meta.is_main_for_project,
        accent_color: meta.accent_color,
        graph,
        layout,
        viewport,
        child_map_placements,
        hyperlinks,
        created_at: meta.created_at,
        updated_at: meta.updated_at,
    })
}

// ---------------------------------------------------------------------------
// Save: 将 StarMapDocument 拆分写入文件
// ---------------------------------------------------------------------------

/// 将 StarMapDocument 拆分写入文件存储。
///
/// 每个节点、边、embed、link、超链接各自独立存储，
/// 修改单个元素只需写入对应的单个文件。
pub fn save_starmap_document(workspace: &Path, doc: &StarMapDocument) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, &doc.starmap_id);
    fs::create_dir_all(&dir)?;

    // 1. 写入图形元数据
    let graph_meta = GraphMeta::from(&doc.graph);
    let meta_json = serde_json::to_string_pretty(&graph_meta)?;
    atomic_write_string(&graph_meta_path(&dir), &meta_json)?;

    // 2. 写入节点
    fs::create_dir_all(nodes_dir(&dir))?;
    for node in &doc.graph.nodes {
        let json = serde_json::to_string_pretty(node)?;
        atomic_write_string(&node_path(&dir, &node.id), &json)?;
    }
    // 清理已删除的节点文件
    cleanup_removed_files(&nodes_dir(&dir), doc.graph.nodes.iter().map(|n| n.id.as_str()))?;

    // 3. 写入边
    fs::create_dir_all(edges_dir(&dir))?;
    for edge in &doc.graph.edges {
        let json = serde_json::to_string_pretty(edge)?;
        atomic_write_string(&edge_path(&dir, &edge.id), &json)?;
    }
    cleanup_removed_files(&edges_dir(&dir), doc.graph.edges.iter().map(|e| e.id.as_str()))?;

    // 4. 写入 embeds（子星图放置）
    fs::create_dir_all(child_starmaps_dir(&dir))?;
    for embed in &doc.graph.embeds {
        let json = serde_json::to_string_pretty(embed)?;
        atomic_write_string(&child_starmap_path(&dir, &embed.instance_id), &json)?;
    }
    cleanup_removed_files(
        &child_starmaps_dir(&dir),
        doc.graph.embeds.iter().map(|e| e.instance_id.as_str()),
    )?;

    // 5. 写入 links
    fs::create_dir_all(links_dir(&dir))?;
    for link in &doc.graph.links {
        let json = serde_json::to_string_pretty(link)?;
        atomic_write_string(&link_path(&dir, &link.link_id), &json)?;
    }
    cleanup_removed_files(
        &links_dir(&dir),
        doc.graph.links.iter().map(|l| l.link_id.as_str()),
    )?;

    // 6. 写入超链接
    fs::create_dir_all(hyperlinks_dir(&dir))?;
    for hl in &doc.hyperlinks {
        let json = serde_json::to_string_pretty(hl)?;
        atomic_write_string(&hyperlink_path(&dir, &hl.hyperlink_id), &json)?;
    }
    cleanup_removed_files(
        &hyperlinks_dir(&dir),
        doc.hyperlinks.iter().map(|h| h.hyperlink_id.as_str()),
    )?;

    // 7. 写入布局
    fs::create_dir_all(dir.join("layouts"))?;
    let layout_json = serde_json::to_string_pretty(&doc.layout)?;
    atomic_write_string(&layout_path(&dir), &layout_json)?;

    // 8. 写入视口
    let viewport_json = serde_json::to_string_pretty(&doc.viewport)?;
    atomic_write_string(&viewport_path(&dir), &viewport_json)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// 单元素 CRUD（增量写入，只改对应文件）
// ---------------------------------------------------------------------------

/// 保存单个节点（只写 `nodes/<id>.json`）
pub fn save_node(workspace: &Path, starmap_id: &str, node: &StarMapNode) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(nodes_dir(&dir))?;
    let json = serde_json::to_string_pretty(node)?;
    atomic_write_string(&node_path(&dir, &node.id), &json)?;
    Ok(())
}

/// 删除单个节点（只删 `nodes/<id>.json`）
pub fn delete_node_file(workspace: &Path, starmap_id: &str, node_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = node_path(&dir, node_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 保存单条边（只写 `edges/<id>.json`）
pub fn save_edge(workspace: &Path, starmap_id: &str, edge: &StarMapEdge) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(edges_dir(&dir))?;
    let json = serde_json::to_string_pretty(edge)?;
    atomic_write_string(&edge_path(&dir, &edge.id), &json)?;
    Ok(())
}

/// 删除单条边（只删 `edges/<id>.json`）
pub fn delete_edge_file(workspace: &Path, starmap_id: &str, edge_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = edge_path(&dir, edge_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 保存单个 embed（只写 `child_starmaps/<instance_id>.json`）
pub fn save_embed(workspace: &Path, starmap_id: &str, embed: &StarMapEmbed) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(child_starmaps_dir(&dir))?;
    let json = serde_json::to_string_pretty(embed)?;
    atomic_write_string(&child_starmap_path(&dir, &embed.instance_id), &json)?;
    Ok(())
}

/// 删除单个 embed（只删 `child_starmaps/<instance_id>.json`）
pub fn delete_embed_file(workspace: &Path, starmap_id: &str, instance_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = child_starmap_path(&dir, instance_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 保存单个 link（只写 `links/<link_id>.json`）
pub fn save_link(workspace: &Path, starmap_id: &str, link: &StarMapLink) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(links_dir(&dir))?;
    let json = serde_json::to_string_pretty(link)?;
    atomic_write_string(&link_path(&dir, &link.link_id), &json)?;
    Ok(())
}

/// 删除单个 link（只删 `links/<link_id>.json`）
pub fn delete_link_file(workspace: &Path, starmap_id: &str, link_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = link_path(&dir, link_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 保存单个超链接（只写 `hyperlinks/<hyperlink_id>.json`）
pub fn save_hyperlink(workspace: &Path, starmap_id: &str, hl: &StarMapHyperlink) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(hyperlinks_dir(&dir))?;
    let json = serde_json::to_string_pretty(hl)?;
    atomic_write_string(&hyperlink_path(&dir, &hl.hyperlink_id), &json)?;
    Ok(())
}

/// 删除单个超链接（只删 `hyperlinks/<hyperlink_id>.json`）
pub fn delete_hyperlink_file(
    workspace: &Path,
    starmap_id: &str,
    hyperlink_id: &str,
) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = hyperlink_path(&dir, hyperlink_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 保存布局（只写 `layouts/default.json`）
pub fn save_layout(workspace: &Path, starmap_id: &str, layout: &StarMapLayout) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(dir.join("layouts"))?;
    let json = serde_json::to_string_pretty(layout)?;
    atomic_write_string(&layout_path(&dir), &json)?;
    Ok(())
}

/// 保存视口（只写 `viewport.json`）
pub fn save_viewport(workspace: &Path, starmap_id: &str, viewport: &StarMapViewport) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let json = serde_json::to_string_pretty(viewport)?;
    atomic_write_string(&viewport_path(&dir), &json)?;
    Ok(())
}

/// 更新图形元数据的时间戳（只写 `graph.json`）
pub fn touch_graph_updated_at(workspace: &Path, starmap_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let meta_path = graph_meta_path(&dir);
    if meta_path.exists() {
        let json_str = fs::read_to_string(&meta_path)?;
        let mut meta: GraphMeta = serde_json::from_str(&json_str)?;
        meta.updated_at = crate::starmap::now_epoch();
        let new_json = serde_json::to_string_pretty(&meta)?;
        atomic_write_string(&meta_path, &new_json)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 旧格式迁移
// ---------------------------------------------------------------------------

/// 从旧格式单文件（`starmap.json`）迁移到拆分文件存储。
///
/// 迁移完成后，旧文件会被重命名为 `starmap.json.migrated.bak`。
fn migrate_from_legacy(workspace: &Path, starmap_id: &str) -> Result<()> {
    let legacy_path = legacy_starmap_path(workspace, starmap_id);
    if !legacy_path.exists() {
        return Ok(());
    }

    let json_str = fs::read_to_string(&legacy_path)?;
    let doc: StarMapDocument = serde_json::from_str(&json_str)?;

    save_starmap_document(workspace, &doc)?;

    // 重命名旧文件为备份
    let backup_path = PathBuf::from(format!("{}.migrated.bak", legacy_path.display()));
    let _ = fs::rename(&legacy_path, &backup_path);

    Ok(())
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 清理目录中不属于当前 ID 集合的 JSON 文件。
/// 用于删除已从文档中移除的元素对应的文件。
fn cleanup_removed_files<'a>(
    dir: &Path,
    current_ids: impl Iterator<Item = &'a str>,
) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    let current_set: std::collections::HashSet<String> =
        current_ids.map(|s| format!("{}.json", s)).collect();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if !current_set.contains(file_name) {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

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

    fn make_test_document(starmap_id: &str) -> StarMapDocument {
        let now = crate::starmap::now_epoch();
        StarMapDocument {
            starmap_id: starmap_id.to_string(),
            title: "Test Doc".to_string(),
            description: "A test document".to_string(),
            project_id: None,
            parent_starmap_id: None,
            is_main_for_project: false,
            accent_color: "#7B8CDE".to_string(),
            graph: StarMapGraph {
                schema_version: 1,
                id: starmap_id.to_string(),
                starmap_id: starmap_id.to_string(),
                title: "Test Doc".to_string(),
                nodes: vec![
                    StarMapNode {
                        id: "n1".to_string(),
                        title: "Node 1".to_string(),
                        kind: StarMapNodeKind::Character,
                        payload: None,
                        tags: vec!["tag1".to_string()],
                        content: Default::default(),
                        anchors: vec![],
                        portal: None,
                        display_policy: Default::default(),
                        open_behavior: Default::default(),
                        provenance: Default::default(),
                        created_at: now,
                        updated_at: now,
                    },
                    StarMapNode {
                        id: "n2".to_string(),
                        title: "Node 2".to_string(),
                        kind: StarMapNodeKind::Event,
                        payload: None,
                        tags: vec![],
                        content: Default::default(),
                        anchors: vec![],
                        portal: None,
                        display_policy: Default::default(),
                        open_behavior: Default::default(),
                        provenance: Default::default(),
                        created_at: now,
                        updated_at: now,
                    },
                ],
                edges: vec![StarMapEdge {
                    id: "e1".to_string(),
                    from: Some("n1".to_string()),
                    to: Some("n2".to_string()),
                    kind: StarMapEdgeKind::RelatedTo,
                    label: Some("relates".to_string()),
                    payload: None,
                    from_target: None,
                    to_target: None,
                    from_endpoint: None,
                    to_endpoint: None,
                    from_endpoint_path: None,
                    to_endpoint_path: None,
                    created_at: now,
                    updated_at: now,
                }],
                embeds: vec![StarMapEmbed {
                    instance_id: "inst1".to_string(),
                    target_starmap_id: "sm_child".to_string(),
                    label: Some("Child Map".to_string()),
                    display_policy: Default::default(),
                    open_behavior: Default::default(),
                    placement: Default::default(),
                    target_viewport: Default::default(),
                    source_node_id: None,
                    host_endpoint: None,
                    provenance: Default::default(),
                    created_at: now,
                    updated_at: now,
                }],
                links: vec![StarMapLink {
                    link_id: "link1".to_string(),
                    source: StarMapEndpoint::Starmap,
                    target: crate::starmap::semantic::StarMapDeepTarget {
                        starmap_id: "sm_other".to_string(),
                        path: vec![],
                        target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                    },
                    label: Some("link".to_string()),
                    created_at: now,
                    updated_at: now,
                }],
                created_at: now,
                updated_at: now,
            },
            layout: StarMapLayout {
                kind: StarMapLayoutKind::Freeform,
                nodes: vec![StarMapLayoutNode {
                    node_id: "n1".to_string(),
                    x: 100.0,
                    y: 200.0,
                    width: 150.0,
                    height: 60.0,
                    radius: 30.0,
                    collapsed: false,
                    z_index: 0,
                    scale: 1.0,
                    depth: 0.0,
                    focus_weight: 0.0,
                    orbit_group: None,
                }],
            },
            viewport: StarMapViewport {
                scale: 1.5,
                offset_x: 10.0,
                offset_y: 20.0,
                width: 800.0,
                height: 600.0,
            },
            child_map_placements: vec![StarMapChildMapPlacement {
                instance_id: "inst1".to_string(),
                target_starmap_id: "sm_child".to_string(),
                placement: Default::default(),
                target_viewport: Default::default(),
                display_policy: Default::default(),
                open_behavior: Default::default(),
            }],
            hyperlinks: vec![StarMapHyperlink {
                hyperlink_id: "hl1".to_string(),
                source: StarMapEndpointPath {
                    segments: vec![],
                    endpoint: StarMapEdgeEndpoint::Node {
                        node_id: "n1".to_string(),
                    },
                },
                target_uri: "https://example.com".to_string(),
                label: Some("Example".to_string()),
                target_starmap_id: None,
                created_at: now,
                updated_at: now,
            }],
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn test_save_and_load_starmap_document() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        let loaded = load_starmap_document(dir.path(), &meta.starmap_id).unwrap();

        assert_eq!(loaded.starmap_id, doc.starmap_id);
        assert_eq!(loaded.graph.nodes.len(), 2);
        assert_eq!(loaded.graph.edges.len(), 1);
        assert_eq!(loaded.graph.embeds.len(), 1);
        assert_eq!(loaded.graph.links.len(), 1);
        assert_eq!(loaded.hyperlinks.len(), 1);
        assert_eq!(loaded.layout.nodes.len(), 1);
        assert_eq!(loaded.viewport.scale, 1.5);

        // 验证节点内容（不依赖顺序）
        let n1 = loaded.graph.nodes.iter().find(|n| n.id == "n1").unwrap();
        assert_eq!(n1.title, "Node 1");
        assert_eq!(n1.kind, StarMapNodeKind::Character);
        let n2 = loaded.graph.nodes.iter().find(|n| n.id == "n2").unwrap();
        assert_eq!(n2.id, "n2");

        // 验证边内容
        assert_eq!(loaded.graph.edges[0].id, "e1");
        assert_eq!(loaded.graph.edges[0].label, Some("relates".to_string()));

        // 验证超链接
        assert_eq!(loaded.hyperlinks[0].hyperlink_id, "hl1");
        assert_eq!(loaded.hyperlinks[0].target_uri, "https://example.com");
    }

    #[test]
    fn test_single_node_save_only_touches_node_file() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        // 记录 edge 文件的修改时间
        let edge_file = edge_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "e1");
        let edge_meta_before = fs::metadata(&edge_file).unwrap();
        let edge_modified_before = edge_meta_before.modified().unwrap();

        // 修改节点并保存
        let mut updated_node = doc.graph.nodes[0].clone();
        updated_node.title = "Updated Node 1".to_string();
        save_node(dir.path(), &meta.starmap_id, &updated_node).unwrap();

        // edge 文件不应被修改
        let edge_meta_after = fs::metadata(&edge_file).unwrap();
        let edge_modified_after = edge_meta_after.modified().unwrap();
        assert_eq!(edge_modified_before, edge_modified_after);

        // 节点文件应该已更新
        let loaded_node_json = fs::read_to_string(
            node_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "n1"),
        )
        .unwrap();
        let loaded_node: StarMapNode = serde_json::from_str(&loaded_node_json).unwrap();
        assert_eq!(loaded_node.title, "Updated Node 1");
    }

    #[test]
    fn test_single_edge_save_only_touches_edge_file() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        // 记录 node 文件的修改时间
        let node_file = node_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "n1");
        let node_meta_before = fs::metadata(&node_file).unwrap();
        let node_modified_before = node_meta_before.modified().unwrap();

        // 修改边并保存
        let mut updated_edge = doc.graph.edges[0].clone();
        updated_edge.label = Some("updated label".to_string());
        save_edge(dir.path(), &meta.starmap_id, &updated_edge).unwrap();

        // node 文件不应被修改
        let node_meta_after = fs::metadata(&node_file).unwrap();
        let node_modified_after = node_meta_after.modified().unwrap();
        assert_eq!(node_modified_before, node_modified_after);

        // 边文件应该已更新
        let loaded_edge_json = fs::read_to_string(
            edge_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "e1"),
        )
        .unwrap();
        let loaded_edge: StarMapEdge = serde_json::from_str(&loaded_edge_json).unwrap();
        assert_eq!(loaded_edge.label, Some("updated label".to_string()));
    }

    #[test]
    fn test_layout_save_only_touches_layout_file() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        // 记录 node 文件的修改时间
        let node_file = node_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "n1");
        let node_meta_before = fs::metadata(&node_file).unwrap();
        let node_modified_before = node_meta_before.modified().unwrap();

        // 修改布局并保存
        let mut updated_layout = doc.layout.clone();
        updated_layout.nodes[0].x = 500.0;
        save_layout(dir.path(), &meta.starmap_id, &updated_layout).unwrap();

        // node 文件不应被修改
        let node_meta_after = fs::metadata(&node_file).unwrap();
        let node_modified_after = node_meta_after.modified().unwrap();
        assert_eq!(node_modified_before, node_modified_after);

        // 布局文件应该已更新
        let loaded_layout_json = fs::read_to_string(layout_path(&starmap_pkg_dir(
            dir.path(),
            &meta.starmap_id,
        )))
        .unwrap();
        let loaded_layout: StarMapLayout = serde_json::from_str(&loaded_layout_json).unwrap();
        assert_eq!(loaded_layout.nodes[0].x, 500.0);
    }

    #[test]
    fn test_hyperlink_not_in_edges() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        let loaded = load_starmap_document(dir.path(), &meta.starmap_id).unwrap();

        // 超链接不进 edges
        assert_eq!(loaded.graph.edges.len(), 1);
        assert_eq!(loaded.graph.edges[0].id, "e1"); // 只有原始边
        assert_eq!(loaded.hyperlinks.len(), 1);
        assert_eq!(loaded.hyperlinks[0].hyperlink_id, "hl1");
    }

    #[test]
    fn test_child_starmap_placement_not_in_nodes() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        let loaded = load_starmap_document(dir.path(), &meta.starmap_id).unwrap();

        // 子星图 placement 不进 nodes
        assert_eq!(loaded.graph.nodes.len(), 2);
        assert_eq!(loaded.child_map_placements.len(), 1);
        assert_eq!(loaded.child_map_placements[0].instance_id, "inst1");
    }

    #[test]
    fn test_delete_node_removes_file() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        let node_file = node_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "n1");
        assert!(node_file.exists());

        delete_node_file(dir.path(), &meta.starmap_id, "n1").unwrap();
        assert!(!node_file.exists());
    }

    #[test]
    fn test_delete_edge_removes_file() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        let edge_file = edge_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "e1");
        assert!(edge_file.exists());

        delete_edge_file(dir.path(), &meta.starmap_id, "e1").unwrap();
        assert!(!edge_file.exists());
    }

    #[test]
    fn test_legacy_migration() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        // 写入旧格式单文件
        let doc = make_test_document(&meta.starmap_id);
        let legacy_path = legacy_starmap_path(dir.path(), &meta.starmap_id);
        let legacy_json = serde_json::to_string_pretty(&doc).unwrap();
        fs::write(&legacy_path, &legacy_json).unwrap();

        // 确保新格式目录不存在
        let pkg_dir = starmap_pkg_dir(dir.path(), &meta.starmap_id);
        if pkg_dir.exists() {
            fs::remove_dir_all(&pkg_dir).unwrap();
        }

        // 加载时应该自动迁移
        let loaded = load_starmap_document(dir.path(), &meta.starmap_id).unwrap();

        assert_eq!(loaded.graph.nodes.len(), 2);
        assert_eq!(loaded.graph.edges.len(), 1);
        assert_eq!(loaded.hyperlinks.len(), 1);

        // 旧文件应该被重命名
        assert!(!legacy_path.exists());
        let backup_path = PathBuf::from(format!("{}.migrated.bak", legacy_path.display()));
        assert!(backup_path.exists());

        // 新格式文件应该存在
        assert!(graph_meta_path(&pkg_dir).exists());
        assert!(node_path(&pkg_dir, "n1").exists());
        assert!(edge_path(&pkg_dir, "e1").exists());
    }

    #[test]
    fn test_cleanup_removed_files() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();

        let doc = make_test_document(&meta.starmap_id);
        save_starmap_document(dir.path(), &doc).unwrap();

        // 验证 n1 和 n2 都存在
        let pkg_dir = starmap_pkg_dir(dir.path(), &meta.starmap_id);
        assert!(node_path(&pkg_dir, "n1").exists());
        assert!(node_path(&pkg_dir, "n2").exists());

        // 保存一个只有 n2 的文档（n1 被删除）
        let mut doc_without_n1 = doc.clone();
        doc_without_n1.graph.nodes.retain(|n| n.id != "n1");
        save_starmap_document(dir.path(), &doc_without_n1).unwrap();

        // n1 文件应该被清理
        assert!(!node_path(&pkg_dir, "n1").exists());
        assert!(node_path(&pkg_dir, "n2").exists());
    }
}
