use crate::error::Result;
use crate::starmap::types::*;

use super::super::relation_index::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_edge(&mut self, edge: StarMapEdge) {
        let edge_id = edge.id.clone();
        let is_new = !self.edges.contains_key(&edge_id);
        let from = edge.from.clone().unwrap_or_default();
        let to = edge.to.clone().unwrap_or_default();
        let from_endpoint = edge.from_endpoint.clone();
        let to_endpoint = edge.to_endpoint.clone();
        let from_endpoint_path = edge.from_endpoint_path.clone();
        let to_endpoint_path = edge.to_endpoint_path.clone();
        self.edges.insert(edge_id.clone(), edge);
        self.dirty_edges.insert(edge_id.clone());
        self.deleted_edge_ids.remove(&edge_id);
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.deleted_since_last_sync.remove_entry("edge", &edge_id);
            if is_new {
                if !meta.edge_ids.contains(&edge_id) {
                    meta.edge_ids.push(edge_id.clone());
                }
                meta.edge_relation_index.push(EdgeRelationIndex {
                    edge_id: edge_id.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    from_endpoint,
                    to_endpoint,
                    from_endpoint_path,
                    to_endpoint_path,
                });
            } else {
                if let Some(eri) = meta.edge_relation_index.iter_mut().find(|e| e.edge_id == edge_id) {
                    eri.from = from;
                    eri.to = to;
                    eri.from_endpoint = from_endpoint;
                    eri.to_endpoint = to_endpoint;
                    eri.from_endpoint_path = from_endpoint_path;
                    eri.to_endpoint_path = to_endpoint_path;
                }
            }
        }
        self.dirty_graph_meta = true;
    }

    pub fn remove_edge(&mut self, edge_id: &str) {
        self.edges.remove(edge_id);
        self.dirty_edges.remove(edge_id);
        self.deleted_edge_ids.insert(edge_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.edge_ids.retain(|id| id != edge_id);
            meta.edge_relation_index.retain(|eri| eri.edge_id != edge_id);
            meta.deleted_since_last_sync.add_entry("edge", edge_id, self.package_revision.saturating_add(1));
        }
        self.dirty_graph_meta = true;
    }

    pub fn add_edge(&mut self, edge: StarMapEdge) -> Result<StarMapEdge> {
        if let Some(ref from_id) = edge.from {
            if !self.nodes.contains_key(from_id) {
                let _ = self.ensure_object_loaded(from_id);
            }
        }
        if let Some(ref to_id) = edge.to {
            if !self.nodes.contains_key(to_id) {
                let _ = self.ensure_object_loaded(to_id);
            }
        }
        let node_id_exists = |id: &str| -> bool {
            self.nodes.contains_key(id)
                || self.graph_meta.as_ref().map_or(false, |m| m.node_ids.contains(&id.to_string()))
        };
        let from_valid = edge.from_target.is_some()
            || edge.from_endpoint.is_some()
            || edge.from_endpoint_path.is_some()
            || edge.from.as_ref()
                .map(|id| node_id_exists(id))
                .unwrap_or(false);
        let to_valid = edge.to_target.is_some()
            || edge.to_endpoint.is_some()
            || edge.to_endpoint_path.is_some()
            || edge.to.as_ref()
                .map(|id| node_id_exists(id))
                .unwrap_or(false);

        if !from_valid || !to_valid {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Edge nodes do not exist and no deep target is provided",
            )));
        }

        let result = edge.clone();
        self.upsert_edge(edge);
        Ok(result)
    }

    pub fn update_edge(&mut self, edge_id: &str, patch: &StarMapEdgePatch) -> Result<StarMapEdge> {
        if !self.edges.contains_key(edge_id) {
            self.ensure_edge_loaded(edge_id)?;
        }
        let edge = self.edges.get_mut(edge_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            ))
        })?;
        if let Some(ref k) = patch.kind { edge.kind = k.clone(); }
        if let Some(ref l) = patch.label { edge.label = l.clone(); }
        if let Some(ref p) = patch.payload { edge.payload = p.clone(); }
        let endpoints_changed = patch.from_target.is_some()
            || patch.to_target.is_some()
            || patch.from_endpoint.is_some()
            || patch.to_endpoint.is_some()
            || patch.from_endpoint_path.is_some()
            || patch.to_endpoint_path.is_some();
        if let Some(ref ft) = patch.from_target {
            edge.from_target = ft.clone();
            edge.from = ft.as_ref().and_then(|t| match &t.target {
                crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Some(node_id.clone()),
                crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, .. } => Some(node_id.clone()),
                _ => None,
            });
        }
        if let Some(ref tt) = patch.to_target {
            edge.to_target = tt.clone();
            edge.to = tt.as_ref().and_then(|t| match &t.target {
                crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Some(node_id.clone()),
                crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, .. } => Some(node_id.clone()),
                _ => None,
            });
        }
        if let Some(ref fe) = patch.from_endpoint { edge.from_endpoint = fe.clone(); }
        if let Some(ref te) = patch.to_endpoint { edge.to_endpoint = te.clone(); }
        if let Some(ref fep) = patch.from_endpoint_path { edge.from_endpoint_path = fep.clone(); }
        if let Some(ref tep) = patch.to_endpoint_path { edge.to_endpoint_path = tep.clone(); }
        edge.updated_at = crate::starmap::now_epoch();
        let updated = edge.clone();
        self.dirty_edges.insert(edge_id.to_string());
        if endpoints_changed {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(eri) = meta.edge_relation_index.iter_mut().find(|e| e.edge_id == edge_id) {
                    eri.from = updated.from.clone().unwrap_or_default();
                    eri.to = updated.to.clone().unwrap_or_default();
                    eri.from_endpoint = updated.from_endpoint.clone();
                    eri.to_endpoint = updated.to_endpoint.clone();
                    eri.from_endpoint_path = updated.from_endpoint_path.clone();
                    eri.to_endpoint_path = updated.to_endpoint_path.clone();
                }
            }
            self.dirty_graph_meta = true;
        }
        Ok(updated)
    }

    pub fn delete_edge(&mut self, edge_id: &str) -> Result<()> {
        if !self.edges.contains_key(edge_id) {
            self.ensure_edge_loaded(edge_id)?;
        }
        if !self.edges.contains_key(edge_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            )));
        }
        self.remove_edge(edge_id);
        Ok(())
    }
}
