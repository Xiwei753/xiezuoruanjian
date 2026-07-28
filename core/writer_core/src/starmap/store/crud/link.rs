use crate::error::Result;
use crate::starmap::types::*;

use super::super::relation_index::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_link(&mut self, link: StarMapLink) {
        let link_id = link.link_id.clone();
        let is_new = !self.links.contains_key(&link_id);
        let source_node_id = endpoint_node_id(&link.source).unwrap_or_default().to_string();
        self.links.insert(link_id.clone(), link);
        self.dirty_links.insert(link_id.clone());
        if is_new {
            if self.graph_meta.is_none() {
                self.ensure_graph_meta_initialized();
            }
            if let Some(ref mut meta) = self.graph_meta {
                if !meta.link_ids.contains(&link_id) {
                    meta.link_ids.push(link_id.clone());
                }
                meta.link_relation_index.push(LinkRelationIndex {
                    link_id: link_id.clone(),
                    source_node_id,
                });
            }
            self.dirty_graph_meta = true;
        } else {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(lri) = meta.link_relation_index.iter_mut().find(|lri| lri.link_id == link_id) {
                    lri.source_node_id = source_node_id;
                }
            }
            self.dirty_graph_meta = true;
        }
    }

    pub fn remove_link(&mut self, link_id: &str) {
        self.links.remove(link_id);
        self.dirty_links.remove(link_id);
        self.deleted_link_ids.insert(link_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.link_ids.retain(|id| id != link_id);
            meta.link_relation_index.retain(|lri| lri.link_id != link_id);
            if !meta.deleted_since_last_sync.links.contains(&link_id.to_string()) {
                meta.deleted_since_last_sync.links.push(link_id.to_string());
            }
        }
        self.dirty_graph_meta = true;
    }

    pub fn add_link(&mut self, link: StarMapLink) -> Result<StarMapLink> {
        if self.links.contains_key(&link.link_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate link_id",
            )));
        }
        let result = link.clone();
        self.upsert_link(link);
        Ok(result)
    }

    pub fn update_link(&mut self, link_id: &str, patch: &StarMapLinkPatch) -> Result<StarMapLink> {
        if !self.links.contains_key(link_id) {
            self.ensure_link_loaded(link_id)?;
        }
        let link = self.links.get_mut(link_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            ))
        })?;
        if let Some(ref s) = patch.source { link.source = s.clone(); }
        if let Some(ref t) = patch.target { link.target = t.clone(); }
        if let Some(ref l) = patch.label { link.label = l.clone(); }
        link.updated_at = crate::starmap::now_epoch();
        let updated = link.clone();
        self.dirty_links.insert(link_id.to_string());
        Ok(updated)
    }

    pub fn delete_link(&mut self, link_id: &str) -> Result<()> {
        if !self.links.contains_key(link_id) {
            self.ensure_link_loaded(link_id)?;
        }
        if !self.links.contains_key(link_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            )));
        }
        self.remove_link(link_id);
        Ok(())
    }
}
