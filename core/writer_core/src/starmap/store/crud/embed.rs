use crate::error::Result;
use crate::starmap::types::*;

use super::super::relation_index::*;
use super::super::StarMapStore;

impl StarMapStore {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn upsert_embed(&mut self, embed: StarMapEmbed) {
        let instance_id = embed.instance_id.clone();
        let is_new = !self.embeds.contains_key(&instance_id);
        let host_node_id = embed.source_node_id.clone().unwrap_or_default();
        let host_endpoint = embed.host_endpoint.clone();
        self.embeds.insert(instance_id.clone(), embed);
        self.dirty_embeds.insert(instance_id.clone());
        self.deleted_embed_ids.remove(&instance_id);
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.deleted_since_last_sync
                .remove_entry("embed", &instance_id);
            if is_new {
                if !meta.embed_instance_ids.contains(&instance_id) {
                    meta.embed_instance_ids.push(instance_id.clone());
                }
                meta.embed_host_index.push(EmbedHostIndex {
                    instance_id: instance_id.clone(),
                    host_node_id: host_node_id.clone(),
                    host_endpoint,
                });
            } else {
                if let Some(ehi) = meta
                    .embed_host_index
                    .iter_mut()
                    .find(|e| e.instance_id == instance_id)
                {
                    ehi.host_node_id = host_node_id;
                    ehi.host_endpoint = host_endpoint;
                }
            }
        }
        self.dirty_graph_meta = true;
    }

    pub fn remove_embed(&mut self, instance_id: &str) {
        self.embeds.remove(instance_id);
        self.dirty_embeds.remove(instance_id);
        self.deleted_embed_ids.insert(instance_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.embed_instance_ids.retain(|id| id != instance_id);
            meta.embed_host_index
                .retain(|ehi| ehi.instance_id != instance_id);
            meta.deleted_since_last_sync.add_entry(
                "embed",
                instance_id,
                self.package_revision.saturating_add(1),
            );
        }
        self.dirty_graph_meta = true;
    }

    pub fn add_embed(&mut self, embed: StarMapEmbed) -> Result<StarMapEmbed> {
        if self.embeds.contains_key(&embed.instance_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate embed instance_id",
            )));
        }
        let result = embed.clone();
        self.upsert_embed(embed);
        Ok(result)
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn update_embed(
        &mut self,
        instance_id: &str,
        patch: &StarMapEmbedPatch,
    ) -> Result<StarMapEmbed> {
        if !self.embeds.contains_key(instance_id) {
            self.ensure_embed_loaded(instance_id)?;
        }
        let embed = self.embeds.get_mut(instance_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Embed not found",
            ))
        })?;
        if let Some(ref l) = patch.label {
            embed.label = l.clone();
        }
        if let Some(ref dp) = patch.display_policy {
            embed.display_policy = dp.clone();
        }
        if let Some(ref ob) = patch.open_behavior {
            embed.open_behavior = ob.clone();
        }
        if let Some(Some(ref pl)) = patch.placement {
            embed.placement = pl.clone();
        }
        if let Some(Some(ref vp)) = patch.target_viewport {
            embed.target_viewport = vp.clone();
        }
        if let Some(Some(ref vp)) = patch.viewport {
            embed.placement.width = vp.width;
            embed.placement.height = vp.height;
            embed.target_viewport.scale = vp.scale;
            embed.target_viewport.offset_x = vp.offset_x;
            embed.target_viewport.offset_y = vp.offset_y;
        }
        let host_changed = patch.source_node_id.is_some()
            || patch.host_endpoint.is_some()
            || patch.host_anchor.is_some();
        if let Some(ref sni) = patch.source_node_id {
            embed.source_node_id = sni.clone();
        }
        if let Some(ref ep) = patch.host_endpoint {
            embed.host_endpoint = ep.clone();
        }
        if let Some(Some(ref anchor_id)) = patch.host_anchor {
            if let Some(ref node_id) = embed.source_node_id {
                embed.host_endpoint = Some(StarMapEndpoint::Anchor {
                    node_id: node_id.clone(),
                    anchor_id: anchor_id.clone(),
                });
            }
        }
        embed.updated_at = crate::starmap::now_epoch();
        let updated = embed.clone();
        self.dirty_embeds.insert(instance_id.to_string());
        if host_changed {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(ehi) = meta
                    .embed_host_index
                    .iter_mut()
                    .find(|e| e.instance_id == instance_id)
                {
                    ehi.host_node_id = updated.source_node_id.clone().unwrap_or_default();
                    ehi.host_endpoint = updated.host_endpoint.clone();
                }
            }
            self.dirty_graph_meta = true;
        }
        Ok(updated)
    }

    pub fn delete_embed(&mut self, instance_id: &str) -> Result<()> {
        if !self.embeds.contains_key(instance_id) {
            self.ensure_embed_loaded(instance_id)?;
        }
        if !self.embeds.contains_key(instance_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Embed not found",
            )));
        }
        self.remove_embed(instance_id);
        Ok(())
    }
}
