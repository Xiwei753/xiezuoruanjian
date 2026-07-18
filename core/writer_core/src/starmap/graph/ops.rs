use crate::error::Result;
use crate::starmap::types::*;
use crate::starmap::{load_starmap_meta, now_epoch, starmaps_dir, update_starmap_stats};
use crate::storage::atomic_write_string;
use std::fs;
use std::path::Path;

pub(crate) fn graph_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("graph.json")
}

pub(crate) fn layout_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("layout.json")
}

pub(crate) fn viewport_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace)
        .join(starmap_id)
        .join("viewport.json")
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

#[allow(clippy::cast_possible_truncation)]
pub fn save_starmap_graph(workspace: &Path, starmap_id: &str, graph: &StarMapGraph) -> Result<()> {
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
    super::validation::validate_layout(layout)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(layout)?;
    atomic_write_string(&layout_path(workspace, starmap_id), &json_str)?;
    Ok(())
}

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
