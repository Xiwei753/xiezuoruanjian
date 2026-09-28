use crate::error::{Error, Result};

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn load_full(&mut self) -> Result<StarMapStoreResult> {
        self.recovery_log.clear();
        let mut diagnostics = Vec::new();

        self.load_recovery_from_disk();

        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        match super::phased::load_current_graph_meta(&graph_json_path, &self.starmap_id) {
            Ok(Some(meta)) => {
                self.graph_meta = Some(meta);
            }
            Ok(None) => {
                // graph.json 不存在：允许新空图，保持空图，不调 scan_objects_from_disk。
            }
            Err(Error::UnsupportedVersion { version }) => {
                return Err(Error::UnsupportedVersion { version });
            }
            Err(e) => {
                // JSON corrupt / IO read failed：fail-closed 直接返回 Err，
                // 不再记 Corrupt diagnostic 后继续 scan_objects_from_disk。
                return Err(e);
            }
        }

        let node_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.node_ids.clone())
            .unwrap_or_default();

        for node_id in &node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let edge_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();

        for edge_id in &edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    self.edges.insert(edge_id.clone(), edge);
                }
            }
        }

        let embed_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();

        for instance_id in &embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    self.embeds.insert(instance_id.clone(), embed);
                }
            }
        }

        let hl_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                }
            }
        }

        let link_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();

        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link_path) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link_path);
                }
            }
        }

        // 收死最终成员集合：非 dirty 的内存对象如果不在 GraphMeta 成员列表里，
        // 视为 orphan（partial cache 残留或磁盘 revision 切换留下的旧对象），移除。
        // dirty 对象保留：刚被 CRUD 修改、还没 flush 的合法对象（如刚 add 的 node
        // 还没进 GraphMeta.node_ids，但它在 dirty_nodes 里）。
        // 这样 to_starmap_graph() 只返回 GraphMeta 声明的完整集合 + dirty 对象，不含 orphan。
        {
            let declared_node_ids: std::collections::HashSet<String> = self
                .graph_meta
                .as_ref()
                .map(|m| m.node_ids.iter().cloned().collect())
                .unwrap_or_default();
            let declared_edge_ids: std::collections::HashSet<String> = self
                .graph_meta
                .as_ref()
                .map(|m| m.edge_ids.iter().cloned().collect())
                .unwrap_or_default();
            let declared_embed_ids: std::collections::HashSet<String> = self
                .graph_meta
                .as_ref()
                .map(|m| m.embed_instance_ids.iter().cloned().collect())
                .unwrap_or_default();
            let declared_link_ids: std::collections::HashSet<String> = self
                .graph_meta
                .as_ref()
                .map(|m| m.link_ids.iter().cloned().collect())
                .unwrap_or_default();
            let declared_hl_ids: std::collections::HashSet<String> = self
                .graph_meta
                .as_ref()
                .map(|m| m.hyperlink_ids.iter().cloned().collect())
                .unwrap_or_default();

            self.nodes
                .retain(|id, _| self.dirty_nodes.contains(id) || declared_node_ids.contains(id));
            self.edges
                .retain(|id, _| self.dirty_edges.contains(id) || declared_edge_ids.contains(id));
            self.embeds
                .retain(|id, _| self.dirty_embeds.contains(id) || declared_embed_ids.contains(id));
            self.links
                .retain(|id, _| self.dirty_links.contains(id) || declared_link_ids.contains(id));
            self.hyperlinks
                .retain(|id, _| self.dirty_hyperlinks.contains(id) || declared_hl_ids.contains(id));
        }

        self.detect_dangling_references(&mut diagnostics);
        self.detect_orphan_objects(&mut diagnostics);

        self.package_revision = self
            .graph_meta
            .as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

        self.current_load_phase = Some(LoadPhase::BackgroundFullLoad);

        diagnostics.append(&mut self.recovery_log);
        self.recovery_log = diagnostics.clone();

        Ok(StarMapStoreResult {
            diagnostics,
            loaded_node_count: self.nodes.len(),
            loaded_edge_count: self.edges.len(),
            loaded_embed_count: self.embeds.len(),
            loaded_link_count: self.links.len(),
            loaded_hyperlink_count: self.hyperlinks.len(),
        })
    }
}
