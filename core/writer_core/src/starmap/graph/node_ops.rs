use crate::error::{Error, Result};
use crate::starmap::types::*;
use crate::starmap::now_epoch;

pub fn add_starmap_node(
    workspace: &std::path::Path,
    starmap_id: &str,
    node: StarMapNode,
    default_x: f32,
    default_y: f32,
) -> Result<StarMapNode> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let new_node = node.clone();
    graph.nodes.push(new_node.clone());
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;

    if let Ok(mut layout) = super::ops::get_starmap_layout(workspace, starmap_id) {
        layout.nodes.push(StarMapLayoutNode {
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
        let _ = super::ops::save_starmap_layout(workspace, starmap_id, &layout);
    }

    Ok(new_node)
}

pub fn update_starmap_node(
    workspace: &std::path::Path,
    starmap_id: &str,
    node_id: &str,
    patch: StarMapNodePatch,
) -> Result<StarMapNode> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
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
        super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_node)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )))
    }
}

pub fn delete_starmap_node(workspace: &std::path::Path, starmap_id: &str, node_id: &str) -> Result<()> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.nodes.len();
    graph.nodes.retain(|n| n.id != node_id);
    if graph.nodes.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )));
    }

    graph.edges.retain(|e| {
        let mut keep = true;
        if let Some(id) = &e.from {
            if id == node_id {
                keep = false;
            }
        }
        if let Some(id) = &e.to {
            if id == node_id {
                keep = false;
            }
        }
        if let Some(ep) = &e.from_endpoint {
            match ep {
                StarMapEdgeEndpoint::Node { node_id: id } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                StarMapEdgeEndpoint::Anchor { node_id: id, .. } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                _ => {}
            }
        }
        if let Some(ep) = &e.to_endpoint {
            match ep {
                StarMapEdgeEndpoint::Node { node_id: id } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                StarMapEdgeEndpoint::Anchor { node_id: id, .. } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                _ => {}
            }
        }
        keep
    });

    graph.embeds.retain(|e| {
        let mut keep = true;
        if let Some(id) = &e.source_node_id {
            if id == node_id {
                keep = false;
            }
        }
        if let Some(ep) = &e.host_endpoint {
            match ep {
                StarMapEndpoint::Node { node_id: id } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                StarMapEndpoint::Anchor { node_id: id, .. } => {
                    if id == node_id {
                        keep = false;
                    }
                }
                _ => {}
            }
        }
        keep
    });

    graph.links.retain(|l| {
        let mut keep = true;
        match &l.source {
            StarMapEndpoint::Node { node_id: id } => {
                if id == node_id {
                    keep = false;
                }
            }
            StarMapEndpoint::Anchor { node_id: id, .. } => {
                if id == node_id {
                    keep = false;
                }
            }
            _ => {}
        }
        keep
    });

    if let Ok(mut layout) = super::ops::get_starmap_layout(workspace, starmap_id) {
        layout.nodes.retain(|n| n.node_id != node_id);
        let _ = super::ops::save_starmap_layout(workspace, starmap_id, &layout);
    }

    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}