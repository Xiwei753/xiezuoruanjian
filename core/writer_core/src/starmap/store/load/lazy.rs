use crate::error::Result;

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn ensure_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::PrefetchNearbyObjects) {
            // 已加载：链接仓库可能被 MCP/同步/另一个实例改写，按版本刷新投影。
            self.refresh_link_projection()?;
            return Ok(());
        }
        self.load_phased(LoadPhase::PrefetchNearbyObjects)?;
        Ok(())
    }

    pub fn ensure_fully_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::BackgroundFullLoad) {
            // 已加载：链接仓库可能被 MCP/同步/另一个实例改写，按版本刷新投影。
            self.refresh_link_projection()?;
            return Ok(());
        }
        self.load_full()?;
        Ok(())
    }

    pub fn ensure_object_loaded(&mut self, node_id: &str) -> Result<()> {
        if self.nodes.contains_key(node_id) {
            return Ok(());
        }
        let node = self.try_load_node(node_id)?;
        self.nodes.insert(node_id.to_string(), node);
        Ok(())
    }

    pub fn ensure_edge_loaded(&mut self, edge_id: &str) -> Result<()> {
        if self.edges.contains_key(edge_id) {
            return Ok(());
        }
        let edge = self.try_load_edge(edge_id)?;
        self.edges.insert(edge_id.to_string(), edge);
        Ok(())
    }

    pub fn ensure_embed_loaded(&mut self, instance_id: &str) -> Result<()> {
        if self.embeds.contains_key(instance_id) {
            return Ok(());
        }
        let embed = self.try_load_embed(instance_id)?;
        self.embeds.insert(instance_id.to_string(), embed);
        Ok(())
    }

    /// 按需把一条 annotations 仓库链接装入画布投影。
    ///
    /// 仓库没有该链接（未迁移 / 已删除 / 属于其它画布）时保持投影不变，
    /// 由调用方按「链接不存在」处理，不 fail-closed。
    pub fn ensure_link_loaded(&mut self, link_id: &str) -> Result<()> {
        if self.links.contains_key(link_id) {
            return Ok(());
        }
        if let Some(link) = self.try_load_link(link_id)? {
            self.links.insert(link.link_id.clone(), link);
        }
        Ok(())
    }
}
