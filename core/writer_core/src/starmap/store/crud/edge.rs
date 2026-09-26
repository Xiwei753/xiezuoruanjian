use crate::error::Result;
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_edge(&mut self, edge: StarMapEdge) {
        let edge_id = edge.id.clone();
        self.edges.insert(edge_id.clone(), edge);
        self.dirty_edges.insert(edge_id.clone());
        self.deleted_edge_ids.remove(&edge_id);
        self.dirty_graph_meta = true;
    }

    pub fn remove_edge(&mut self, edge_id: &str) {
        self.edges.remove(edge_id);
        self.dirty_edges.remove(edge_id);
        self.deleted_edge_ids.insert(edge_id.to_string());
        self.dirty_graph_meta = true;
    }

    pub fn add_edge(&mut self, edge: StarMapEdge) -> Result<StarMapEdge> {
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
        edge.updated_at = crate::starmap::now_epoch();
        let updated = edge.clone();
        self.dirty_edges.insert(edge_id.to_string());
        self.dirty_graph_meta = true;
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
