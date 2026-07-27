use crate::error::Result;
use crate::starmap::types::*;

use super::super::relation_index::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_node(&mut self, node: StarMapNode) {
        let node_id = node.id.clone();
        let kind_key = format!("{:?}", node.kind);
        let is_new = !self.nodes.contains_key(&node_id);
        self.nodes.insert(node_id.clone(), node);
        self.dirty_nodes.insert(node_id.clone());
        if is_new {
            if self.graph_meta.is_none() {
                self.ensure_graph_meta_initialized();
            }
            if let Some(ref mut meta) = self.graph_meta {
                if !meta.node_ids.contains(&node_id) {
                    meta.node_ids.push(node_id);
                }
                *meta.node_kind_counts.entry(kind_key).or_insert(0u32) += 1;
            }
            self.dirty_graph_meta = true;
        }
    }

    pub fn remove_node(&mut self, node_id: &str) {
        if let Some(node) = self.nodes.get(node_id) {
            let kind_key = format!("{:?}", node.kind);
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(count) = meta.node_kind_counts.get_mut(&kind_key) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        self.nodes.remove(node_id);
        self.dirty_nodes.remove(node_id);
        self.deleted_node_ids.insert(node_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.node_ids.retain(|id| id != node_id);
        }
        self.dirty_graph_meta = true;
    }

    pub fn add_node(
        &mut self,
        node: StarMapNode,
        default_x: f32,
        default_y: f32,
    ) -> StarMapNode {
        let result = node.clone();
        self.upsert_node(node);
        if let Some(ref mut layout) = self.layout {
            layout.nodes.push(StarMapLayoutNode {
                node_id: result.id.clone(),
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
            self.dirty_layout = true;
        }
        result
    }

    pub fn update_node(&mut self, node_id: &str, patch: &StarMapNodePatch) -> Result<StarMapNode> {
        if !self.nodes.contains_key(node_id) {
            self.ensure_object_loaded(node_id)?;
        }
        let node = self.nodes.get_mut(node_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            ))
        })?;
        if let Some(ref t) = patch.title { node.title = t.clone(); }
        if let Some(ref k) = patch.kind {
            let old_kind_key = format!("{:?}", node.kind);
            let new_kind_key = format!("{:?}", k);
            if old_kind_key != new_kind_key {
                if let Some(ref mut meta) = self.graph_meta {
                    if let Some(count) = meta.node_kind_counts.get_mut(&old_kind_key) {
                        *count = count.saturating_sub(1);
                    }
                    *meta.node_kind_counts.entry(new_kind_key).or_insert(0u32) += 1;
                }
                self.dirty_graph_meta = true;
            }
            node.kind = k.clone();
        }
        if let Some(ref p) = patch.payload { node.payload = p.clone(); }
        if let Some(ref t) = patch.tags { node.tags = t.clone(); }
        if let Some(ref c) = patch.content { node.content = c.clone(); }
        if let Some(ref a) = patch.anchors { node.anchors = a.clone(); }
        if let Some(ref p) = patch.portal { node.portal = p.clone(); }
        if let Some(ref dp) = patch.display_policy { node.display_policy = dp.clone(); }
        if let Some(ref ob) = patch.open_behavior { node.open_behavior = ob.clone(); }
        if let Some(ref p) = patch.provenance { node.provenance = p.clone(); }
        node.updated_at = crate::starmap::now_epoch();
        let updated = node.clone();
        self.dirty_nodes.insert(node_id.to_string());
        Ok(updated)
    }

    pub fn delete_node(&mut self, node_id: &str) -> Result<()> {
        if !self.nodes.contains_key(node_id) {
            self.ensure_object_loaded(node_id)?;
        }
        if !self.nodes.contains_key(node_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            )));
        }

        let edge_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.edge_relation_index.iter()
                .filter(|eri| extract_eri_node_refs(eri).contains(&node_id))
                .map(|eri| eri.edge_id.clone())
                .collect())
            .unwrap_or_default();

        let embed_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.embed_host_index.iter()
                .filter(|ehi| extract_ehi_node_refs(ehi).contains(&node_id))
                .map(|ehi| ehi.instance_id.clone())
                .collect())
            .unwrap_or_default();

        let link_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.link_relation_index.iter()
                .filter(|lri| lri.source_node_id == node_id)
                .map(|lri| lri.link_id.clone())
                .collect())
            .unwrap_or_default();

        let hyperlink_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_relation_index.iter()
                .filter(|hri| hri.source_node_id == node_id)
                .map(|hri| hri.hyperlink_id.clone())
                .collect())
            .unwrap_or_default();

        self.remove_node(node_id);

        for eid in &edge_ids_to_remove {
            self.remove_edge(eid);
        }

        for iid in &embed_ids_to_remove {
            self.remove_embed(iid);
        }

        for lid in &link_ids_to_remove {
            self.remove_link(lid);
        }

        for hlid in &hyperlink_ids_to_remove {
            self.remove_hyperlink(hlid);
        }

        if let Some(ref mut layout) = self.layout {
            layout.nodes.retain(|n| n.node_id != node_id);
            self.dirty_layout = true;
        }

        Ok(())
    }
}
