use crate::error::Result;
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_node(&mut self, node: StarMapNode) {
        let node_id = node.id.clone();
        self.nodes.insert(node_id.clone(), node);
        self.dirty_nodes.insert(node_id.clone());
        self.deleted_node_ids.remove(&node_id);
        self.dirty_graph_meta = true;
    }

    pub fn remove_node(&mut self, node_id: &str) {
        self.nodes.remove(node_id);
        self.dirty_nodes.remove(node_id);
        self.deleted_node_ids.insert(node_id.to_string());
        self.dirty_graph_meta = true;
    }

    pub fn add_node(&mut self, node: StarMapNode, default_x: f32, default_y: f32) -> StarMapNode {
        let result = node.clone();
        self.upsert_node(node);
        let layout = self.layout.get_or_insert_with(StarMapLayout::default);
        if !layout.nodes.iter().any(|n| n.node_id == result.id) {
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
        if let Some(ref t) = patch.title {
            node.title = t.clone();
        }
        if let Some(ref k) = patch.kind {
            node.kind = k.clone();
        }
        if let Some(ref p) = patch.payload {
            node.payload = p.clone();
        }
        if let Some(ref t) = patch.tags {
            node.tags = t.clone();
        }
        if let Some(ref c) = patch.content {
            node.content = c.clone();
        }
        if let Some(ref a) = patch.anchors {
            node.anchors = a.clone();
        }
        if let Some(ref p) = patch.portal {
            node.portal = p.clone();
        }
        if let Some(ref dp) = patch.display_policy {
            node.display_policy = dp.clone();
        }
        if let Some(ref ob) = patch.open_behavior {
            node.open_behavior = ob.clone();
        }
        if let Some(ref p) = patch.provenance {
            node.provenance = p.clone();
        }
        node.updated_at = crate::starmap::now_epoch();
        let updated = node.clone();
        self.dirty_nodes.insert(node_id.to_string());
        self.dirty_graph_meta = true;
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

        // Collect IDs of edges, embeds, links, hyperlinks that reference this node
        // **as a local node reference** (path.starmap_id == host && segments.is_empty()).
        // 跨层路径的终点 node_id 属于另一张星图，绝不参与本图的级联删除。
        // These are derived from the in-memory objects directly, not from the relation index.
        let host = self.starmap_id.as_str();
        let edge_ids_to_remove: Vec<String> = self
            .edges
            .values()
            .filter(|e| {
                let from_refs =
                    crate::starmap::store::relation_index::target_path_node_id(&e.from, host);
                let to_refs =
                    crate::starmap::store::relation_index::target_path_node_id(&e.to, host);
                from_refs == Some(node_id) || to_refs == Some(node_id)
            })
            .map(|e| e.id.clone())
            .collect();

        let embed_ids_to_remove: Vec<String> = self
            .embeds
            .values()
            .filter(|em| {
                crate::starmap::store::relation_index::target_path_node_id(&em.host_path, host)
                    == Some(node_id)
            })
            .map(|em| em.instance_id.clone())
            .collect();

        let link_ids_to_remove: Vec<String> = self
            .links
            .values()
            .filter(|l| {
                crate::starmap::store::relation_index::target_path_node_id(&l.source, host)
                    == Some(node_id)
            })
            .map(|l| l.link_id.clone())
            .collect();

        let hyperlink_ids_to_remove: Vec<String> = self
            .hyperlinks
            .values()
            .filter(|hl| {
                crate::starmap::store::relation_index::target_path_node_id(&hl.source, host)
                    == Some(node_id)
            })
            .map(|hl| hl.hyperlink_id.clone())
            .collect();

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
