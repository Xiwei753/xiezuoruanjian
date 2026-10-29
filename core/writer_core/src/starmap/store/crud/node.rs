use crate::error::Result;
use crate::starmap::store::relation_index::{CascadeIds, CascadeOrigin};
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
    /// `node.position` 是节点初始位置，由调用方在写入前设置好。
    /// 节点移动以后就是更新 `node.position`，不再另外创建 layout record。
    /// 不再二次覆盖 position：facade 的 `add_starmap_node` 负责把
    /// `default_x`/`default_y` 合进 `node.position`，store 只信任传入的 node。
    pub fn add_node(&mut self, node: StarMapNode) -> StarMapNode {
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
        node.updated_at = crate::starmap::now_epoch().max(node.updated_at.saturating_add(1));
        let updated = node.clone();
        self.dirty_nodes.insert(node_id.to_string());
        self.dirty_graph_meta = true;
        Ok(updated)
    }

    /// 计算删除本地 node 时需要级联删除的 edge/embed/link ID 集合。
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
        )
    }

    /// 统一级联闭包规划：计算删除 Node 或 Embed 后，传递级联会删除的全部对象。
    ///
    /// 与 `node_cascade_ids` / `embed_cascade_ids` 的区别：本方法循环扩展
    /// 被删 Embed 的下一层级联，直到不动点，避免遗漏传递级联。
    /// pub 方法，供 facade 的 candidate 模拟删除和 store 真实删除复用，
    /// 保证两者产生相同的最终对象集合。内部调用
    /// `relation_index::cascade_closure_plan` 纯函数。
    pub fn cascade_closure_plan(&self, origin: &CascadeOrigin) -> CascadeIds {
        let host = self.starmap_id.as_str();
        crate::starmap::store::relation_index::cascade_closure_plan(
            origin,
            host,
            self.edges.values(),
            self.embeds.values(),
            self.links.values(),
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

        // 统一级联闭包规划：循环扩展被删 Embed 的下一层级联，直到不动点。
        // 旧实现只做一次级联（node_cascade_ids），会遗漏传递级联——
        // 删 node n 级联删 embed e，但 e 的删除又应级联删以其为第一段
        // EnterEmbed 的其他 embed/edge，旧实现不会继续扩展。
        // 级联 ID 计算复用 `cascade_closure_plan` 纯函数，candidate 模拟删除
        // 也用同一函数，避免两套逻辑漂移。
        let cascade = self.cascade_closure_plan(&CascadeOrigin::Node(node_id.to_string()));

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

        Ok(())
    }
}
