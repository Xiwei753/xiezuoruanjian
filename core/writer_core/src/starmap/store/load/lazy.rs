use crate::error::Result;

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn ensure_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::PrefetchNearbyObjects) {
            return Ok(());
        }
        self.load_phased(LoadPhase::PrefetchNearbyObjects)?;
        Ok(())
    }

    pub fn ensure_fully_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::BackgroundFullLoad) {
            return Ok(());
        }
        self.load_full()?;
        Ok(())
    }

    pub fn ensure_object_loaded(&mut self, node_id: &str) -> Result<()> {
        if self.nodes.contains_key(node_id) {
            return Ok(());
        }
        if let Some(node) = self.try_load_node(node_id) {
            self.nodes.insert(node_id.to_string(), node);
        }
        Ok(())
    }

    pub fn ensure_edge_loaded(&mut self, edge_id: &str) -> Result<()> {
        if self.edges.contains_key(edge_id) {
            return Ok(());
        }
        if let Some(edge) = self.try_load_edge(edge_id) {
            self.edges.insert(edge_id.to_string(), edge);
        }
        Ok(())
    }

    pub fn ensure_embed_loaded(&mut self, instance_id: &str) -> Result<()> {
        if self.embeds.contains_key(instance_id) {
            return Ok(());
        }
        if let Some(embed) = self.try_load_embed(instance_id) {
            self.embeds.insert(instance_id.to_string(), embed);
        }
        Ok(())
    }

    pub fn ensure_link_loaded(&mut self, link_id: &str) -> Result<()> {
        if self.links.contains_key(link_id) {
            return Ok(());
        }
        if let Some(link) = self.try_load_link(link_id) {
            self.links.insert(link_id.to_string(), link);
        }
        Ok(())
    }

    pub fn ensure_hyperlink_loaded(&mut self, hyperlink_id: &str) -> Result<()> {
        if self.hyperlinks.contains_key(hyperlink_id) {
            return Ok(());
        }
        if let Some(hl) = self.try_load_hyperlink(hyperlink_id) {
            self.hyperlinks.insert(hyperlink_id.to_string(), hl);
        }
        Ok(())
    }
}
