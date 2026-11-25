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

    /// 修改共享素材本体（标题/内容/样式/portal/anchors…）。
    ///
    /// Issue #1060：本体 revision 独立递增，实例 revision 不动；画布文件
    /// 不因内容修改而重写。返回更新后的读模型。
    pub fn update_node_definition(
        &mut self,
        instance_id: &str,
        patch: &NodeDefinitionPatch,
        expected_definition_revision: Option<u64>,
    ) -> Result<StarMapNode> {
        if patch.is_empty() {
            return self.current_node_view(instance_id);
        }
        if !self.nodes.contains_key(instance_id) {
            self.ensure_object_loaded(instance_id)?;
        }
        let asset_id = self.asset_id_for_node(instance_id);
        let mut definition =
            match crate::starmap::library::load_node_definition(&self.app_data_root, &asset_id)? {
                Some(definition) => definition,
                None => {
                    let node = self.nodes.get(instance_id).ok_or_else(node_not_found)?;
                    crate::starmap::types::StarMapNodeDefinition::from_node(asset_id.clone(), node)
                }
            };
        if let Some(expected) = expected_definition_revision {
            if definition.revision() != expected {
                return Err(revision_conflict(
                    "definition",
                    &asset_id,
                    expected,
                    definition.revision(),
                ));
            }
        }
        if let Some(ref title) = patch.title {
            definition.title = title.clone();
        }
        if let Some(ref kind) = patch.kind {
            definition.kind = kind.clone();
        }
        if let Some(ref payload) = patch.payload {
            definition.payload = payload.clone();
        }
        if let Some(ref content) = patch.content {
            definition.content = content.clone();
        }
        if let Some(ref anchors) = patch.anchors {
            definition.anchors = anchors.clone();
        }
        if let Some(ref portal) = patch.portal {
            definition.portal = portal.clone();
        }
        if let Some(ref style) = patch.style {
            definition.style = style.clone();
        }
        if let Some(ref provenance) = patch.provenance {
            definition.provenance = provenance.clone();
        }
        definition.updated_at = crate::starmap::now_epoch()
            .max(definition.updated_at.saturating_add(1))
            .max(1);
        crate::starmap::library::save_node_definition(&self.app_data_root, &definition)?;
        if let Some(node) = self.nodes.get_mut(instance_id) {
            definition.apply_to_node(node);
        }
        self.current_node_view(instance_id)
    }

    /// 应用实例补丁：只改实例事实，并单调推进实例 revision。
    pub fn update_node_instance(
        &mut self,
        instance_id: &str,
        patch: &NodeInstancePatch,
        expected_instance_revision: Option<u64>,
    ) -> Result<StarMapNode> {
        if patch.is_empty() {
            return self.current_node_view(instance_id);
        }
        if !self.nodes.contains_key(instance_id) {
            self.ensure_object_loaded(instance_id)?;
        }
        let node = self.nodes.get_mut(instance_id).ok_or_else(node_not_found)?;
        if let Some(expected) = expected_instance_revision {
            if node.updated_at != expected {
                return Err(revision_conflict(
                    "instance",
                    instance_id,
                    expected,
                    node.updated_at,
                ));
            }
        }
        let mut changed = false;
        if let Some(ref position) = patch.position {
            if node.position != *position {
                node.position = position.clone();
                changed = true;
            }
        }
        if !changed {
            return self.current_node_view(instance_id);
        }
        node.updated_at = crate::starmap::now_epoch().max(node.updated_at.saturating_add(1));
        self.dirty_nodes.insert(instance_id.to_string());
        self.dirty_graph_meta = true;
        self.current_node_view(instance_id)
    }

    /// 当前节点的只读投影（共享本体 + 实例）。
    pub fn current_node_view(&self, instance_id: &str) -> Result<StarMapNode> {
        self.nodes
            .get(instance_id)
            .cloned()
            .ok_or_else(node_not_found)
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

        // 级联删除的链接必须写 annotations 仓库 tombstone（唯一持久化所有者），
        // 不能只从投影移除，否则仓库里的活动链接会在下次加载时复活。
        let link_ids: Vec<String> = cascade.link_ids.clone();
        for lid in &link_ids {
            self.delete_link(lid)?;
        }

        Ok(())
    }
}

fn node_not_found() -> crate::error::Error {
    crate::error::Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Node not found",
    ))
}

/// 版本冲突：`target` 是 "definition" 或 "instance"。
pub(super) fn revision_conflict(
    target: &str,
    object_id: &str,
    expected: u64,
    actual: u64,
) -> crate::error::Error {
    crate::error::Error::Other(format!(
        "{target} revision conflict for '{object_id}': expected {expected}, actual {actual}"
    ))
}
