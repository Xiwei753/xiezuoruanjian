use crate::error::Result;
use crate::starmap::types::*;
use crate::starmap::{load_starmap_meta, now_epoch, starmaps_dir, update_starmap_stats};
use crate::storage::atomic_write_string;
use std::fs;
use std::path::Path;

/// 星图数据文件路径：`{starmaps_dir}/{starmap_id}/graph.json`
pub(crate) fn graph_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("graph.json")
}

/// 星图布局文件路径：`{starmaps_dir}/{starmap_id}/layout.json`
pub(crate) fn layout_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("layout.json")
}

/// 星图视口文件路径：`{starmaps_dir}/{starmap_id}/viewport.json`
pub(crate) fn viewport_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace)
        .join(starmap_id)
        .join("viewport.json")
}

/// 获取星图数据（迁移兼容入口，新代码应使用 StarMapStore）。
///
/// 若 `graph.json` 不存在，返回空图（保留 meta 标题），
/// 不返回错误——首次创建节点/边时 `save_starmap_graph` 会写入文件。
pub(crate) fn get_starmap_graph(workspace: &Path, starmap_id: &str) -> Result<StarMapGraph> {
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

/// 保存星图数据到 `graph.json`（迁移兼容入口，新代码应使用 StarMapStore）。
///
/// 保存前调用 `validate_graph` 校验引用完整性；校验失败阻止写入。
/// 写入后同步更新 `starmap.json` 中的 `node_count`/`edge_count`/`linked_chapters`
/// 统计字段——这些统计是 `list_starmaps` 列表视图的唯一数据来源，
/// 必须与 `graph.json` 保持一致，否则列表显示的节点/边数会过时。
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn save_starmap_graph(workspace: &Path, starmap_id: &str, graph: &StarMapGraph) -> Result<()> {
    super::validation::validate_graph(workspace, graph)?;

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

/// 获取星图布局。若 `layout.json` 不存在，返回默认布局（所有字段零值）。
/// 布局缺失不影响语义完整性——平台端可重新触发布局计算。
pub fn get_starmap_layout(workspace: &Path, starmap_id: &str) -> Result<StarMapLayout> {
    let path = layout_path(workspace, starmap_id);
    if !path.exists() {
        return Ok(StarMapLayout::default());
    }

    let json_str = fs::read_to_string(&path)?;
    let layout: StarMapLayout = serde_json::from_str(&json_str)?;
    Ok(layout)
}

/// 保存星图布局到 `layout.json`。保存前调用 `validate_layout` 校验
/// scale/depth/focus_weight 数值合法性。布局是可重建的派生数据，
/// 损坏后平台端可重新触发布局计算。
pub fn save_starmap_layout(
    workspace: &Path,
    starmap_id: &str,
    layout: &StarMapLayout,
) -> Result<()> {
    super::validation::validate_layout(layout)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(layout)?;
    atomic_write_string(&layout_path(workspace, starmap_id), &json_str)?;
    Ok(())
}

/// 获取星图视口。若 `viewport.json` 不存在，返回默认视口（scale=1, offset=0）。
/// 保存/加载时均调用 `validate_viewport` 校验数值合法性。
pub fn get_starmap_viewport(workspace: &Path, starmap_id: &str) -> Result<StarMapViewport> {
    let path = viewport_path(workspace, starmap_id);
    if !path.exists() {
        return Ok(StarMapViewport::default());
    }

    let json_str = fs::read_to_string(&path)?;
    let viewport: StarMapViewport = serde_json::from_str(&json_str)?;
    super::validation::validate_viewport(&viewport)?;
    Ok(viewport)
}

/// 保存星图视口到 `viewport.json`。保存前校验 scale 为有限正值、
/// 偏移/尺寸为有限数。视口是平台端可重建的 UI 状态，损坏不影响语义。
pub fn save_starmap_viewport(
    workspace: &Path,
    starmap_id: &str,
    viewport: &StarMapViewport,
) -> Result<()> {
    super::validation::validate_viewport(viewport)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(viewport)?;
    atomic_write_string(&viewport_path(workspace, starmap_id), &json_str)?;
    Ok(())
}
