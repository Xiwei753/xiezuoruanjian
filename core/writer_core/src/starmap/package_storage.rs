//! # 星图包存储模块
//!
//! 本模块提供星图对象的单元素增量读写函数。
//! 完整文档级加载/保存已迁移到 `store::StarMapStore`。
//!
//! 布局/视口/运动策略已退出 Core，由平台端自行管理。
//! 节点位置在 `node.position`，嵌入位置在 `embed.position`。

use crate::error::Result;
use crate::starmap::types::*;
use crate::storage::atomic_write_string;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn starmap_pkg_dir(app_data_root: &Path, starmap_id: &str) -> PathBuf {
    app_data_root.join("starmaps").join(starmap_id)
}

/// 星图包目录相对于 `app_data_root` 的路径：`starmaps/{starmap_id}`。
fn starmap_pkg_rel_dir(starmap_id: &str) -> PathBuf {
    PathBuf::from("starmaps").join(starmap_id)
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
    dir.join("nodes")
        .join(bucket_for_id(node_id))
        .join(format!("{}.json", node_id))
}

fn edge_path(dir: &Path, edge_id: &str) -> PathBuf {
    dir.join("edges")
        .join(bucket_for_id(edge_id))
        .join(format!("{}.json", edge_id))
}

fn embed_path(dir: &Path, instance_id: &str) -> PathBuf {
    dir.join("embeds")
        .join(bucket_for_id(instance_id))
        .join(format!("{}.json", instance_id))
}

fn link_path(dir: &Path, link_id: &str) -> PathBuf {
    dir.join("links")
        .join(bucket_for_id(link_id))
        .join(format!("{}.json", link_id))
}

fn hyperlink_path(dir: &Path, hyperlink_id: &str) -> PathBuf {
    dir.join("hyperlinks")
        .join(bucket_for_id(hyperlink_id))
        .join(format!("{}.json", hyperlink_id))
}

pub fn save_node(app_data_root: &Path, starmap_id: &str, node: &StarMapNode) -> Result<PathBuf> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    fs::create_dir_all(dir.join("nodes").join(bucket_for_id(&node.id)))?;
    let json = serde_json::to_string_pretty(node)?;
    atomic_write_string(&node_path(&dir, &node.id), &json)?;
    Ok(starmap_pkg_rel_dir(starmap_id)
        .join("nodes")
        .join(bucket_for_id(&node.id))
        .join(format!("{}.json", node.id)))
}

pub fn delete_node_file(
    app_data_root: &Path,
    starmap_id: &str,
    node_id: &str,
) -> Result<Vec<PathBuf>> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    let mut changed: Vec<PathBuf> = Vec::new();
    let path = node_path(&dir, node_id);
    if path.exists() {
        fs::remove_file(&path)?;
        changed.push(
            starmap_pkg_rel_dir(starmap_id)
                .join("nodes")
                .join(bucket_for_id(node_id))
                .join(format!("{}.json", node_id)),
        );
    }
    Ok(changed)
}

pub fn save_edge(app_data_root: &Path, starmap_id: &str, edge: &StarMapEdge) -> Result<PathBuf> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    fs::create_dir_all(dir.join("edges").join(bucket_for_id(&edge.id)))?;
    let json = serde_json::to_string_pretty(edge)?;
    atomic_write_string(&edge_path(&dir, &edge.id), &json)?;
    Ok(starmap_pkg_rel_dir(starmap_id)
        .join("edges")
        .join(bucket_for_id(&edge.id))
        .join(format!("{}.json", edge.id)))
}

pub fn delete_edge_file(
    app_data_root: &Path,
    starmap_id: &str,
    edge_id: &str,
) -> Result<Vec<PathBuf>> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    let mut changed: Vec<PathBuf> = Vec::new();
    let path = edge_path(&dir, edge_id);
    if path.exists() {
        fs::remove_file(&path)?;
        changed.push(
            starmap_pkg_rel_dir(starmap_id)
                .join("edges")
                .join(bucket_for_id(edge_id))
                .join(format!("{}.json", edge_id)),
        );
    }
    Ok(changed)
}

pub fn save_embed(app_data_root: &Path, starmap_id: &str, embed: &StarMapEmbed) -> Result<PathBuf> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    fs::create_dir_all(dir.join("embeds").join(bucket_for_id(&embed.instance_id)))?;
    let json = serde_json::to_string_pretty(embed)?;
    atomic_write_string(&embed_path(&dir, &embed.instance_id), &json)?;
    Ok(starmap_pkg_rel_dir(starmap_id)
        .join("embeds")
        .join(bucket_for_id(&embed.instance_id))
        .join(format!("{}.json", embed.instance_id)))
}

pub fn delete_embed_file(
    app_data_root: &Path,
    starmap_id: &str,
    instance_id: &str,
) -> Result<Vec<PathBuf>> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    let mut changed: Vec<PathBuf> = Vec::new();
    let path = embed_path(&dir, instance_id);
    if path.exists() {
        fs::remove_file(&path)?;
        changed.push(
            starmap_pkg_rel_dir(starmap_id)
                .join("embeds")
                .join(bucket_for_id(instance_id))
                .join(format!("{}.json", instance_id)),
        );
    }
    Ok(changed)
}

pub fn save_link(app_data_root: &Path, starmap_id: &str, link: &StarMapLink) -> Result<PathBuf> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    fs::create_dir_all(dir.join("links").join(bucket_for_id(&link.link_id)))?;
    let json = serde_json::to_string_pretty(link)?;
    atomic_write_string(&link_path(&dir, &link.link_id), &json)?;
    Ok(starmap_pkg_rel_dir(starmap_id)
        .join("links")
        .join(bucket_for_id(&link.link_id))
        .join(format!("{}.json", link.link_id)))
}

pub fn delete_link_file(
    app_data_root: &Path,
    starmap_id: &str,
    link_id: &str,
) -> Result<Vec<PathBuf>> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    let mut changed: Vec<PathBuf> = Vec::new();
    let path = link_path(&dir, link_id);
    if path.exists() {
        fs::remove_file(&path)?;
        changed.push(
            starmap_pkg_rel_dir(starmap_id)
                .join("links")
                .join(bucket_for_id(link_id))
                .join(format!("{}.json", link_id)),
        );
    }
    Ok(changed)
}

pub fn save_hyperlink(
    app_data_root: &Path,
    starmap_id: &str,
    hl: &StarMapHyperlink,
) -> Result<PathBuf> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    fs::create_dir_all(dir.join("hyperlinks").join(bucket_for_id(&hl.hyperlink_id)))?;
    let json = serde_json::to_string_pretty(hl)?;
    atomic_write_string(&hyperlink_path(&dir, &hl.hyperlink_id), &json)?;
    Ok(starmap_pkg_rel_dir(starmap_id)
        .join("hyperlinks")
        .join(bucket_for_id(&hl.hyperlink_id))
        .join(format!("{}.json", hl.hyperlink_id)))
}

pub fn delete_hyperlink_file(
    app_data_root: &Path,
    starmap_id: &str,
    hyperlink_id: &str,
) -> Result<Vec<PathBuf>> {
    let dir = starmap_pkg_dir(app_data_root, starmap_id);
    let mut changed: Vec<PathBuf> = Vec::new();
    let path = hyperlink_path(&dir, hyperlink_id);
    if path.exists() {
        fs::remove_file(&path)?;
        changed.push(
            starmap_pkg_rel_dir(starmap_id)
                .join("hyperlinks")
                .join(bucket_for_id(hyperlink_id))
                .join(format!("{}.json", hyperlink_id)),
        );
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::create_starmap;
    use tempfile::tempdir;

    fn setup_project_root() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("projects")).unwrap();
        dir
    }

    #[test]
    fn test_single_node_save_and_delete() {
        let dir = setup_project_root();
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
            position: StarMapPoint::default(),
            style: StarMapNodeStyle::default(),
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
        let dir = setup_project_root();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let now = crate::starmap::now_epoch();
        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: crate::starmap::types::reference::StarMapTargetPath {
                starmap_id: String::new(),
                segments: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Node {
                    node_id: "n1".to_string(),
                },
            },
            to: crate::starmap::types::reference::StarMapTargetPath {
                starmap_id: String::new(),
                segments: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Node {
                    node_id: "n2".to_string(),
                },
            },
            kind: StarMapEdgeKind::RelatedTo,
            label: Some("relates".to_string()),
            payload: None,
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
        let dir = setup_project_root();
        let meta = create_starmap(dir.path(), "Test", "", None).unwrap();
        let now = crate::starmap::now_epoch();
        let link = StarMapLink {
            link_id: "l1".to_string(),
            source: crate::starmap::types::reference::StarMapTargetPath {
                starmap_id: String::new(),
                segments: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            target: crate::starmap::types::reference::StarMapTargetPath {
                starmap_id: "other".to_string(),
                segments: vec![],
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
}
