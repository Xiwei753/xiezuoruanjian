use crate::error::Result;
use crate::starmap::store::relation_index::CascadeIds;
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

    /// 添加节点。
    ///
    /// `default_x`/`default_y` 作为节点初始位置写入 `node.position`。
    /// 节点移动以后就是更新 `node.position`，不再另外创建 layout record。
    pub fn add_node(
        &mut self,
        mut node: StarMapNode,
        default_x: f32,
        default_y: f32,
    ) -> StarMapNode {
        node.position = StarMapPoint {
            x: default_x,
            y: default_y,
        };
        let result = node.clone();
        self.upsert_node(node);
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
        if let Some(ref p) = patch.position {
            node.position = p.clone();
        }
        if let Some(ref s) = patch.style {
            node.style = s.clone();
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

    /// 计算删除本地 node 时需要级联删除的 edge/embed/link/hyperlink ID 集合。
    ///
    /// 这是 pub 方法，供 facade 的 candidate 模拟删除复用，保证 candidate
    /// 模拟和 store 真实删除产生相同的最终对象集合。内部调用
    /// `relation_index::node_cascade_ids` 纯函数。
    pub fn node_cascade_ids(&self, node_id: &str) -> CascadeIds {
        let host = self.starmap_id.as_str();
        crate::starmap::store::relation_index::node_cascade_ids(
            node_id,
            host,
            self.edges.values(),
            self.embeds.values(),
            self.links.values(),
            self.hyperlinks.values(),
        )
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
        // **as a local node reference** (path.starmap_id == host && segments.is_empty())
        // OR via first-segment EnterPortal (path.starmap_id == host && segments[0]
        // is EnterPortal { node_id })。跨层路径的终点 node_id 属于另一张星图，
        // 绝不参与本图的级联删除；但第一段 EnterPortal 引用的 portal 节点属于
        // 本图，删除 portal 节点必须级联删所有穿越它的路径。
        //
        // 级联 ID 计算复用 `node_cascade_ids` 纯函数，candidate 模拟删除也用
        // 同一函数，避免两套逻辑漂移。
        let cascade = self.node_cascade_ids(node_id);

        self.remove_node(node_id);

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
