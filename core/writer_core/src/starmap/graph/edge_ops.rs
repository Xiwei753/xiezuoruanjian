use crate::error::{Error, Result};
use crate::starmap::types::*;
use crate::starmap::now_epoch;

pub fn add_starmap_edge(
    workspace: &std::path::Path,
    starmap_id: &str,
    edge: StarMapEdge,
) -> Result<StarMapEdge> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;

    let from_valid = edge.from_target.is_some()
        || edge.from_endpoint.is_some()
        || edge.from_endpoint_path.is_some()
        || edge
            .from
            .as_ref()
            .map(|id| graph.nodes.iter().any(|n| &n.id == id))
            .unwrap_or(false);
    let to_valid = edge.to_target.is_some()
        || edge.to_endpoint.is_some()
        || edge.to_endpoint_path.is_some()
        || edge
            .to
            .as_ref()
            .map(|id| graph.nodes.iter().any(|n| &n.id == id))
            .unwrap_or(false);

    if !from_valid || !to_valid {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Edge nodes do not exist and no deep target is provided",
        )));
    }

    let new_edge = edge.clone();
    graph.edges.push(new_edge.clone());
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(new_edge)
}

pub fn update_starmap_edge(
    workspace: &std::path::Path,
    starmap_id: &str,
    edge_id: &str,
    patch: StarMapEdgePatch,
) -> Result<StarMapEdge> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
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
        if let Some(fe) = patch.from_endpoint {
            edge.from_endpoint = fe;
        }
        if let Some(te) = patch.to_endpoint {
            edge.to_endpoint = te;
        }
        if let Some(fep) = patch.from_endpoint_path {
            edge.from_endpoint_path = fep;
        }
        if let Some(tep) = patch.to_endpoint_path {
            edge.to_endpoint_path = tep;
        }
        edge.updated_at = now_epoch();
        let updated_edge = edge.clone();
        graph.updated_at = now_epoch();
        super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_edge)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Edge not found",
        )))
    }
}

pub fn delete_starmap_edge(workspace: &std::path::Path, starmap_id: &str, edge_id: &str) -> Result<()> {
    let mut graph = super::ops::get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.edges.len();
    graph.edges.retain(|e| e.id != edge_id);
    if graph.edges.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Edge not found",
        )));
    }
    graph.updated_at = now_epoch();
    super::ops::save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}