//! # 星图包存储模块
//!
//! 本模块提供星图对象的单元素增量读写函数。
//! 完整文档级加载/保存已迁移到 `store::StarMapStore`。

use crate::error::Result;
use crate::starmap::types::*;
use crate::storage::atomic_write_string;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn starmap_pkg_dir(workspace: &Path, starmap_id: &str) -> PathBuf {
    workspace.join("app-meta").join("starmaps").join(starmap_id)
}

pub(crate) fn bucket_for_id(id: &str) -> &str {
    let bytes = id.as_bytes();
    if bytes.is_empty() {
        return "00";
    }
    let b = bytes[0];
    let hi = (b >> 4) & 0x0F;
    match hi {
        0 => "00",
        1 => "01",
        2 => "02",
        3 => "03",
        4 => "04",
        5 => "05",
        6 => "06",
        7 => "07",
        8 => "08",
        9 => "09",
        10 => "0a",
        11 => "0b",
        12 => "0c",
        13 => "0d",
        14 => "0e",
        _ => "0f",
    }
}

fn node_path(dir: &Path, node_id: &str) -> PathBuf {
    dir.join("nodes").join(bucket_for_id(node_id)).join(format!("{}.json", node_id))
}

fn edge_path(dir: &Path, edge_id: &str) -> PathBuf {
    dir.join("edges").join(bucket_for_id(edge_id)).join(format!("{}.json", edge_id))
}

fn child_starmap_path(dir: &Path, instance_id: &str) -> PathBuf {
    dir.join("child_starmaps").join(bucket_for_id(instance_id)).join(format!("{}.json", instance_id))
}

fn link_path(dir: &Path, link_id: &str) -> PathBuf {
    dir.join("links").join(bucket_for_id(link_id)).join(format!("{}.json", link_id))
}

fn hyperlink_path(dir: &Path, hyperlink_id: &str) -> PathBuf {
    dir.join("hyperlinks").join(bucket_for_id(hyperlink_id)).join(format!("{}.json", hyperlink_id))
}

fn layout_dir(dir: &Path) -> PathBuf {
    dir.join("layouts").join("default")
}

fn legacy_layout_path(dir: &Path) -> PathBuf {
    dir.join("layouts").join("default.json")
}

fn layout_kind_path(dir: &Path) -> PathBuf {
    layout_dir(dir).join("kind.json")
}

fn layout_nodes_shard_path(dir: &Path, bucket: &str) -> PathBuf {
    layout_dir(dir).join("nodes").join(format!("{}.json", bucket))
}

fn session_dir(workspace: &Path, starmap_id: &str) -> PathBuf {
    workspace.join("session").join("starmaps").join(starmap_id)
}

fn viewport_path(workspace: &Path, starmap_id: &str) -> PathBuf {
    session_dir(workspace, starmap_id).join("viewport.json")
}

pub fn save_node(workspace: &Path, starmap_id: &str, node: &StarMapNode) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(dir.join("nodes").join(bucket_for_id(&node.id)))?;
    let json = serde_json::to_string_pretty(node)?;
    atomic_write_string(&node_path(&dir, &node.id), &json)?;
    Ok(())
}

pub fn delete_node_file(workspace: &Path, starmap_id: &str, node_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = node_path(&dir, node_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn save_edge(workspace: &Path, starmap_id: &str, edge: &StarMapEdge) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(dir.join("edges").join(bucket_for_id(&edge.id)))?;
    let json = serde_json::to_string_pretty(edge)?;
    atomic_write_string(&edge_path(&dir, &edge.id), &json)?;
    Ok(())
}

pub fn delete_edge_file(workspace: &Path, starmap_id: &str, edge_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = edge_path(&dir, edge_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn save_embed(workspace: &Path, starmap_id: &str, embed: &StarMapEmbed) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(dir.join("child_starmaps").join(bucket_for_id(&embed.instance_id)))?;
    let json = serde_json::to_string_pretty(embed)?;
    atomic_write_string(&child_starmap_path(&dir, &embed.instance_id), &json)?;
    Ok(())
}

pub fn delete_embed_file(workspace: &Path, starmap_id: &str, instance_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = child_starmap_path(&dir, instance_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn save_link(workspace: &Path, starmap_id: &str, link: &StarMapLink) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(dir.join("links").join(bucket_for_id(&link.link_id)))?;
    let json = serde_json::to_string_pretty(link)?;
    atomic_write_string(&link_path(&dir, &link.link_id), &json)?;
    Ok(())
}

pub fn delete_link_file(workspace: &Path, starmap_id: &str, link_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = link_path(&dir, link_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn save_hyperlink(workspace: &Path, starmap_id: &str, hl: &StarMapHyperlink) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    fs::create_dir_all(dir.join("hyperlinks").join(bucket_for_id(&hl.hyperlink_id)))?;
    let json = serde_json::to_string_pretty(hl)?;
    atomic_write_string(&hyperlink_path(&dir, &hl.hyperlink_id), &json)?;
    Ok(())
}

pub fn delete_hyperlink_file(workspace: &Path, starmap_id: &str, hyperlink_id: &str) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    let path = hyperlink_path(&dir, hyperlink_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn save_layout(workspace: &Path, starmap_id: &str, layout: &StarMapLayout) -> Result<()> {
    let dir = starmap_pkg_dir(workspace, starmap_id);
    save_layout_sharded(&dir, layout)
}

pub(crate) fn save_layout_sharded(dir: &Path, layout: &StarMapLayout) -> Result<()> {
    let ld = layout_dir(dir);
    fs::create_dir_all(ld.join("nodes"))?;

    let kind_json = serde_json::to_string_pretty(&layout.kind)?;
    atomic_write_string(&layout_kind_path(dir), &kind_json)?;

    let mut buckets: HashMap<&str, Vec<&StarMapLayoutNode>> = HashMap::new();
    for node in &layout.nodes {
        let b = bucket_for_id(&node.node_id);
        buckets.entry(b).or_default().push(node);
    }
    for (bucket, nodes) in &buckets {
        let json = serde_json::to_string_pretty(nodes)?;
        atomic_write_string(&layout_nodes_shard_path(dir, bucket), &json)?;
    }

    let all_buckets = [
        "00","01","02","03","04","05","06","07",
        "08","09","0a","0b","0c","0d","0e","0f",
    ];
    for b in &all_buckets {
        if !buckets.contains_key(b) {
            let path = layout_nodes_shard_path(dir, b);
            if path.exists() {
                let _ = fs::remove_file(&path);
            }
        }
    }

    Ok(())
}

pub(crate) fn load_layout_sharded(dir: &Path) -> Option<StarMapLayout> {
    let kind_path = layout_kind_path(dir);
    if !kind_path.exists() {
        return None;
    }
    let kind_content = std::fs::read_to_string(&kind_path).ok()?;
    let kind: StarMapLayoutKind = serde_json::from_str(&kind_content).ok()?;

    let nodes_dir = layout_dir(dir).join("nodes");
    let mut all_nodes: Vec<StarMapLayoutNode> = Vec::new();
    if let Ok(entries) = fs::read_dir(&nodes_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(nodes) = serde_json::from_str::<Vec<StarMapLayoutNode>>(&content) {
                        all_nodes.extend(nodes);
                    }
                }
            }
        }
    }

    Some(StarMapLayout {
        kind,
        nodes: all_nodes,
    })
}

pub(crate) fn load_legacy_layout(dir: &Path) -> Option<StarMapLayout> {
    let path = legacy_layout_path(dir);
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn save_viewport(workspace: &Path, starmap_id: &str, viewport: &StarMapViewport) -> Result<()> {
    let dir = session_dir(workspace, starmap_id);
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(viewport)?;
    atomic_write_string(&viewport_path(workspace, starmap_id), &json)?;
    Ok(())
}

pub fn load_viewport(workspace: &Path, starmap_id: &str) -> Option<StarMapViewport> {
    let path = viewport_path(workspace, starmap_id);
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
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
    fn test_single_node_save_and_delete() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let now = crate::starmap::now_epoch();
        let node = StarMapNode {
            id: "n1".to_string(),
            title: "Node 1".to_string(),
            kind: StarMapNodeKind::Concept,
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
        };
        save_node(dir.path(), &meta.starmap_id, &node).unwrap();
        let node_file = node_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "n1");
        assert!(node_file.exists());
        delete_node_file(dir.path(), &meta.starmap_id, "n1").unwrap();
        assert!(!node_file.exists());
    }

    #[test]
    fn test_single_edge_save_and_delete() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let now = crate::starmap::now_epoch();
        let edge = StarMapEdge {
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
        };
        save_edge(dir.path(), &meta.starmap_id, &edge).unwrap();
        let edge_file = edge_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "e1");
        assert!(edge_file.exists());
        delete_edge_file(dir.path(), &meta.starmap_id, "e1").unwrap();
        assert!(!edge_file.exists());
    }

    #[test]
    fn test_single_link_save_and_delete() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let now = crate::starmap::now_epoch();
        let link = StarMapLink {
            link_id: "l1".to_string(),
            source: StarMapEndpoint::Starmap,
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: "other".to_string(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: Some("link".to_string()),
            created_at: now,
            updated_at: now,
        };
        save_link(dir.path(), &meta.starmap_id, &link).unwrap();
        let link_file = link_path(&starmap_pkg_dir(dir.path(), &meta.starmap_id), "l1");
        assert!(link_file.exists());
        delete_link_file(dir.path(), &meta.starmap_id, "l1").unwrap();
        assert!(!link_file.exists());
    }

    #[test]
    fn test_layout_sharded_save_and_load() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let layout = StarMapLayout {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![
                StarMapLayoutNode {
                    node_id: "n1".to_string(),
                    x: 10.0, y: 20.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
                StarMapLayoutNode {
                    node_id: "n2".to_string(),
                    x: 200.0, y: 100.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 1,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
            ],
        };
        save_layout(dir.path(), &meta.starmap_id, &layout).unwrap();

        let pkg_dir = starmap_pkg_dir(dir.path(), &meta.starmap_id);
        assert!(layout_kind_path(&pkg_dir).exists());

        let loaded = load_layout_sharded(&pkg_dir).unwrap();
        assert_eq!(loaded.kind, StarMapLayoutKind::Freeform);
        assert_eq!(loaded.nodes.len(), 2);

        let mut loaded_ids: Vec<&str> = loaded.nodes.iter().map(|n| n.node_id.as_str()).collect();
        loaded_ids.sort();
        assert_eq!(loaded_ids, vec!["n1", "n2"]);
    }

    #[test]
    fn test_layout_sharded_bucket_distribution() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let layout = StarMapLayout {
            kind: StarMapLayoutKind::AutoRadial,
            nodes: vec![
                StarMapLayoutNode {
                    node_id: "anode".to_string(),
                    x: 0.0, y: 0.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
                StarMapLayoutNode {
                    node_id: "Anode".to_string(),
                    x: 0.0, y: 0.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
            ],
        };
        save_layout(dir.path(), &meta.starmap_id, &layout).unwrap();

        let pkg_dir = starmap_pkg_dir(dir.path(), &meta.starmap_id);
        let a_bucket = bucket_for_id("anode");
        let upper_a_bucket = bucket_for_id("Anode");
        assert_ne!(a_bucket, upper_a_bucket, "lowercase a and uppercase A should be in different buckets");
        assert!(layout_nodes_shard_path(&pkg_dir, a_bucket).exists());
        assert!(layout_nodes_shard_path(&pkg_dir, upper_a_bucket).exists());
    }

    #[test]
    fn test_layout_legacy_fallback() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let pkg_dir = starmap_pkg_dir(dir.path(), &meta.starmap_id);

        let legacy_layout = StarMapLayout {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![
                StarMapLayoutNode {
                    node_id: "legacy_n1".to_string(),
                    x: 50.0, y: 50.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
            ],
        };
        fs::create_dir_all(pkg_dir.join("layouts")).unwrap();
        let json = serde_json::to_string_pretty(&legacy_layout).unwrap();
        atomic_write_string(&legacy_layout_path(&pkg_dir), &json).unwrap();

        let loaded = load_legacy_layout(&pkg_dir).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].node_id, "legacy_n1");

        assert!(load_layout_sharded(&pkg_dir).is_none());
    }

    #[test]
    fn test_layout_sharded_removes_empty_buckets() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let pkg_dir = starmap_pkg_dir(dir.path(), &meta.starmap_id);

        let a_bucket = bucket_for_id("anode");
        let upper_a_bucket = bucket_for_id("Anode");
        assert_ne!(a_bucket, upper_a_bucket);

        let layout_full = StarMapLayout {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![
                StarMapLayoutNode {
                    node_id: "anode".to_string(),
                    x: 0.0, y: 0.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
                StarMapLayoutNode {
                    node_id: "Anode".to_string(),
                    x: 0.0, y: 0.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
            ],
        };
        save_layout(dir.path(), &meta.starmap_id, &layout_full).unwrap();
        assert!(layout_nodes_shard_path(&pkg_dir, a_bucket).exists());
        assert!(layout_nodes_shard_path(&pkg_dir, upper_a_bucket).exists());

        let layout_reduced = StarMapLayout {
            kind: StarMapLayoutKind::Freeform,
            nodes: vec![
                StarMapLayoutNode {
                    node_id: "anode".to_string(),
                    x: 0.0, y: 0.0, width: 150.0, height: 60.0,
                    radius: 30.0, collapsed: false, z_index: 0,
                    scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
                },
            ],
        };
        save_layout(dir.path(), &meta.starmap_id, &layout_reduced).unwrap();
        assert!(layout_nodes_shard_path(&pkg_dir, a_bucket).exists());
        assert!(!layout_nodes_shard_path(&pkg_dir, upper_a_bucket).exists());
    }
}
