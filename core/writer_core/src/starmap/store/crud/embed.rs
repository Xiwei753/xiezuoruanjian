use crate::error::Result;
use crate::starmap::store::relation_index::CascadeIds;
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_embed(&mut self, embed: StarMapEmbed) {
        let instance_id = embed.instance_id.clone();
        self.embeds.insert(instance_id.clone(), embed);
        self.dirty_embeds.insert(instance_id.clone());
        self.deleted_embed_ids.remove(&instance_id);
        self.dirty_graph_meta = true;
    }

    pub fn remove_embed(&mut self, instance_id: &str) {
        self.embeds.remove(instance_id);
        self.dirty_embeds.remove(instance_id);
        self.deleted_embed_ids.insert(instance_id.to_string());
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
        if let Some(ref p) = patch.position {
            embed.position = p.clone();
        }
        if let Some(ref hp) = patch.host_path {
            embed.host_path = hp.clone();
        }
        embed.updated_at = crate::starmap::now_epoch();
        let updated = embed.clone();
        self.dirty_embeds.insert(instance_id.to_string());
        self.dirty_graph_meta = true;
        Ok(updated)
    }

    /// 计算删除本地 embed instance 时需要级联删除的 edge/embed/link/hyperlink ID 集合。
    ///
    /// 这是 pub 方法，供 facade 的 candidate 模拟删除复用，保证 candidate
    /// 模拟和 store 真实删除产生相同的最终对象集合。内部调用
    /// `relation_index::embed_cascade_ids` 纯函数。
    pub fn embed_cascade_ids(&self, instance_id: &str) -> CascadeIds {
        let host = self.starmap_id.as_str();
        crate::starmap::store::relation_index::embed_cascade_ids(
            instance_id,
            host,
            self.edges.values(),
            self.embeds.values(),
            self.links.values(),
            self.hyperlinks.values(),
        )
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

        // 级联删除所有第一段 EnterEmbed 引用命中被删 instance 的 edge/link/hyperlink，
        // 以及 host_path 第一段 EnterEmbed 命中被删 instance 的其他 embed。
        // 级联 ID 计算复用 `embed_cascade_ids` 纯函数，candidate 模拟删除也用
        // 同一函数，避免两套逻辑漂移。
        let cascade = self.embed_cascade_ids(instance_id);

        self.remove_embed(instance_id);

        for eid in &cascade.edge_ids {
            self.remove_edge(eid);
        }

        for iid in &cascade.embed_ids {
            self.remove_embed(iid);
        }

        for lid in &cascade.link_ids {
            self.remove_link(lid);
        }

        for hlid in &cascade.hyperlink_ids {
            self.remove_hyperlink(hlid);
        }

        Ok(())
    }
}
