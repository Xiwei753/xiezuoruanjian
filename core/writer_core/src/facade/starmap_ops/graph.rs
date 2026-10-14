//! 星图 graph / 节点 / 边 / 嵌入 / 链接 / 超链接的 facade 方法。
//!
//! 与 `starmap_ops.rs` 属于同一个 `WriterCore` 类型，只是按关注点拆成两个固有
//! `impl`：星图本身的增删改查与 store 生命周期在 `starmap_ops.rs`，图数据操作
//! 在这里。Rust 允许同一类型的固有 `impl` 分散在同 crate 的多个模块中。

use super::patch::{
    apply_deletion_to_graph, apply_edge_patch_to_graph, apply_embed_patch_to_graph,
    apply_hyperlink_update_to_graph, apply_link_patch_to_graph, apply_node_patch_to_graph,
};
use super::*;

/// 跨画布连线创建结果：宿主星图 ID 和真正写入 Store 的 Edge。
///
/// 由 facade 的 [`WriterCore::create_cross_canvas_edge`] 返回。
/// `owner_starmap_id` 是 Edge 实际持久化所在的宿主星图，
/// `edge` 是完整写入的 `StarMapEdge`（含生成的 `id`、`from`/`to` 路径、时间戳）。
/// 平台端可直接用返回的 `edge.id` 对原始边做后续修改/删除。
#[derive(Debug, Clone)]
pub struct CreateCrossCanvasEdgeResult {
    pub owner_starmap_id: String,
    pub edge: crate::starmap::types::StarMapEdge,
}

impl super::super::WriterCore {
    pub fn get_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapGraph> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        Ok(store.to_starmap_graph())
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::starmap::types::StarMapNode,
        default_x: f64,
        default_y: f64,
    ) -> Result<crate::starmap::types::StarMapNode> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
        }

        // 先把 default_x/default_y 合进 node.position，再用同一个 node 做校验和写入。
        // 这样 validator 检查的就是真正要写入的同一份数据，不会出现 DTO 自带 position
        // 校验通过但 default_x=NaN 被写进 node.position 的情况。
        let mut node = node;
        node.position = crate::starmap::types::StarMapPoint {
            x: default_x,
            y: default_y,
        };

        // 先在 candidate graph 上模拟 add，跑 validate_graph，再真正改 Store。
        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            g.nodes.push(node.clone());
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_node(node);
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::starmap::types::StarMapNodePatch,
    ) -> Result<crate::starmap::types::StarMapNode> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
            store.ensure_object_loaded(node_id)?;
        }

        // 先在 candidate graph 上模拟 update，跑 validate_graph，再真正改 Store。
        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            apply_node_patch_to_graph(&mut g, node_id, &patch)?;
            g
        };

        // 修改1：Portal/Anchor 身份变更前检查外部图持有的路径依赖。
        // 只在 Portal destination 改变或 Anchor 被删除时检查；title/position/style
        // 等不影响目标身份的变更不触发。存在外部依赖则拒绝，不允许先改再让外部路径悬空。
        let needs_index_invalidation = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let old_node = store
                .get_node(node_id)
                .ok_or_else(|| {
                    crate::error::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Node not found",
                    ))
                })?
                .clone();
            let new_node = candidate
                .nodes
                .iter()
                .find(|n| n.id == node_id)
                .ok_or_else(|| {
                    crate::error::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "Node not found",
                    ))
                })?;
            self.check_external_dependencies_for_node_update(
                &stores, starmap_id, node_id, &old_node, new_node,
            )?;

            // Portal destination 变更或 Anchor 集合变更会影响路径解析结果，
            // 需要失效持久化索引。title/position/style 等不影响路径解析的变更不触发。
            let portal_changed = match (&old_node.portal, &new_node.portal) {
                (None, None) | (None, Some(_)) => false,
                (Some(_), None) => true,
                (Some(o), Some(n)) => {
                    o.destination_starmap_id != n.destination_starmap_id
                        || o.destination_target != n.destination_target
                }
            };
            // Anchor 身份变更 = anchor_id 集合不同。anchor 的 position 等非身份字段
            // 变更不影响路径依赖，只有 anchor_id 新增/删除才改变路径解析结果。
            let old_anchor_ids: std::collections::HashSet<&str> = old_node
                .anchors
                .iter()
                .map(|a| a.anchor_id.as_str())
                .collect();
            let new_anchor_ids: std::collections::HashSet<&str> = new_node
                .anchors
                .iter()
                .map(|a| a.anchor_id.as_str())
                .collect();
            let anchors_changed = old_anchor_ids != new_anchor_ids;
            portal_changed || anchors_changed
        };

        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_node(node_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // Portal/Anchor 变更会改变路径解析结果，失效持久化索引。
        // 下次查询时从真实数据惰性重建。
        if needs_index_invalidation {
            self.invalidate_path_dependency_index();
        }

        Ok(result)
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> Result<()> {
        self.delete_starmap_object(
            starmap_id,
            crate::starmap::store::CascadeOrigin::Node(node_id.to_string()),
        )
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::starmap::types::StarMapEdge,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            g.edges.push(edge.clone());
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare from/to 的事实（可失败，不修改
        // 任何状态）。成功后才写 Store；写 Store 后 replace（不可失败）。
        // prepare 失败时 Store 和索引都不变，不会分叉。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let from_facts = extract_path_dependency_facts(&context, &edge.from)?;
        let to_facts = extract_path_dependency_facts(&context, &edge.to)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_edge(edge)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实一次性替换该来源的全部索引 entry。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::EdgeFrom,
            &result.id,
            &result.from,
            &result.label,
            from_facts,
        );
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::EdgeTo,
            &result.id,
            &result.to,
            &result.label,
            to_facts,
        );

        Ok(result)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::starmap::types::StarMapEdgePatch,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
            store.ensure_edge_loaded(edge_id)?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            apply_edge_patch_to_graph(&mut g, edge_id, &patch)?;
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare from/to 的事实（可失败，不修改状态）。
        // 用 candidate 中 patch 应用后的 edge 路径，与写 Store 后 result 的路径一致。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let updated_edge = candidate
            .edges
            .iter()
            .find(|e| e.id == edge_id)
            .ok_or_else(|| {
                crate::error::Error::Other(format!(
                    "internal error: updated edge not found in candidate: {}",
                    edge_id
                ))
            })?;
        let from_facts = extract_path_dependency_facts(&context, &updated_edge.from)?;
        let to_facts = extract_path_dependency_facts(&context, &updated_edge.to)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_edge(edge_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实一次性替换索引 entry。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::EdgeFrom,
            &result.id,
            &result.from,
            &result.label,
            from_facts,
        );
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::EdgeTo,
            &result.id,
            &result.to,
            &result.label,
            to_facts,
        );

        Ok(result)
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_edge_loaded(edge_id)?;
        store.delete_edge(edge_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // 增量更新持久化路径依赖索引：移除被删 Edge 的 from/to 两端依赖事实。
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        self.remove_source_from_path_dependency_index(
            starmap_id,
            &CrossCanvasRefSourceKind::EdgeFrom,
            edge_id,
        );
        self.remove_source_from_path_dependency_index(
            starmap_id,
            &CrossCanvasRefSourceKind::EdgeTo,
            edge_id,
        );

        Ok(())
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::starmap::types::StarMapEmbed,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            g.embeds.push(embed.clone());
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare host_path 的事实（可失败，不修改状态）。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let host_path_facts = extract_path_dependency_facts(&context, &embed.host_path)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_embed(embed)?;
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::EmbedHostPath,
            &result.instance_id,
            &result.host_path,
            &result.label,
            host_path_facts,
        );

        Ok(result)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::starmap::types::StarMapEmbedPatch,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
            store.ensure_embed_loaded(instance_id)?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            apply_embed_patch_to_graph(&mut g, instance_id, &patch)?;
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare host_path 的事实（可失败，不修改状态）。
        // 用 candidate 中 patch 应用后的 embed host_path。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let updated_embed = candidate
            .embeds
            .iter()
            .find(|e| e.instance_id == instance_id)
            .ok_or_else(|| {
                crate::error::Error::Other(format!(
                    "internal error: updated embed not found in candidate: {}",
                    instance_id
                ))
            })?;
        let host_path_facts = extract_path_dependency_facts(&context, &updated_embed.host_path)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_embed(instance_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::EmbedHostPath,
            &result.instance_id,
            &result.host_path,
            &result.label,
            host_path_facts,
        );

        Ok(result)
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> Result<()> {
        self.delete_starmap_object(
            starmap_id,
            crate::starmap::store::CascadeOrigin::Embed(instance_id.to_string()),
        )
    }

    /// 统一删除流程：先得到完整删除集合，再做一次依赖检查，最后执行同一份删除计划。
    ///
    /// `delete_starmap_node` 和 `delete_starmap_embed` 共享这套规划/检查/执行逻辑，
    /// 避免一种删除安全、另一种还会留下孤儿路径。
    ///
    /// 流程：
    /// 1. 完整加载 store，确保级联计算看到全部对象。
    /// 2. 用 `cascade_closure_plan` 计算传递级联闭包（含被删 Embed 引起的下一层级联）。
    /// 3. 跨图引用检查：对每个被删 Node 查询 NodeTarget/AnchorTarget/PortalTraversal，
    ///    对每个被删 Embed 查询 EmbedTraversal。只拦截外部跨图依赖
    ///    （`owner_starmap_id != starmap_id`），本图自引用由本地级联处理。
    /// 4. candidate 模拟删除（含传递级联）+ `validate_graph`。
    /// 5. 执行 store 删除（内部复用同一 `cascade_closure_plan`）。
    fn delete_starmap_object(
        &self,
        starmap_id: &str,
        origin: crate::starmap::store::CascadeOrigin,
    ) -> Result<()> {
        use crate::starmap::store::CascadeOrigin;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            // 级联删除需要完整加载，否则会漏掉跨对象的引用关系。
            store.ensure_fully_loaded()?;
            match &origin {
                CascadeOrigin::Node(node_id) => store.ensure_object_loaded(node_id)?,
                CascadeOrigin::Embed(instance_id) => store.ensure_embed_loaded(instance_id)?,
            }
        }

        // 统一级联闭包规划：循环扩展被删 Embed 的下一层级联，直到不动点。
        let cascade = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            store.cascade_closure_plan(&origin)
        };

        // 跨图引用检查：对所有被删对象（Node + 级联删除的 Embed）检查外部依赖。
        // 旧实现只检查被点中的对象本身，不检查级联删除的其他对象——
        // 比如删 node n 级联删 embed e，但不查 e 的 EmbedTraversal，导致外部路径被破坏。
        self.check_cross_canvas_references_for_delete(&stores, starmap_id, &origin, &cascade)?;

        // 先在 candidate graph 上模拟删除（含传递级联），跑 validate_graph，再真正改 Store。
        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            apply_deletion_to_graph(&mut g, &origin, &cascade);
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 执行删除。store.delete_node/delete_embed 内部复用同一 cascade_closure_plan，
        // 保证与 candidate 模拟产生相同的最终对象集合。
        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        match &origin {
            CascadeOrigin::Node(node_id) => {
                store.delete_node(node_id)?;
                store.enqueue_save(SaveQueueEntry::DeleteNode);
            }
            CascadeOrigin::Embed(instance_id) => {
                store.delete_embed(instance_id)?;
            }
        }
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // 增量更新持久化路径依赖索引：移除所有被删来源的依赖事实。
        // 避免下次查询时从零扫描全库重建索引。
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        for eid in &cascade.edge_ids {
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::EdgeFrom,
                eid,
            );
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::EdgeTo,
                eid,
            );
        }
        // 被删 Embed：origin embed + 级联删除的 embed_ids
        if let CascadeOrigin::Embed(instance_id) = &origin {
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::EmbedHostPath,
                instance_id,
            );
        }
        for iid in &cascade.embed_ids {
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::EmbedHostPath,
                iid,
            );
        }
        for lid in &cascade.link_ids {
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::LinkSource,
                lid,
            );
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::LinkTarget,
                lid,
            );
        }
        for hlid in &cascade.hyperlink_ids {
            self.remove_source_from_path_dependency_index(
                starmap_id,
                &CrossCanvasRefSourceKind::HyperlinkSource,
                hlid,
            );
        }

        Ok(())
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::starmap::types::StarMapLink,
    ) -> Result<crate::starmap::types::StarMapLink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            g.links.push(link.clone());
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare source/target 的事实（可失败，不修改状态）。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let source_facts = extract_path_dependency_facts(&context, &link.source)?;
        let target_facts = extract_path_dependency_facts(&context, &link.target)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_link(link)?;
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::LinkSource,
            &result.link_id,
            &result.source,
            &result.label,
            source_facts,
        );
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::LinkTarget,
            &result.link_id,
            &result.target,
            &result.label,
            target_facts,
        );

        Ok(result)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::starmap::types::StarMapLinkPatch,
    ) -> Result<crate::starmap::types::StarMapLink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
            store.ensure_link_loaded(link_id)?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            apply_link_patch_to_graph(&mut g, link_id, &patch)?;
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare source/target 的事实（可失败，不修改状态）。
        // 用 candidate 中 patch 应用后的 link 路径。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let updated_link = candidate
            .links
            .iter()
            .find(|l| l.link_id == link_id)
            .ok_or_else(|| {
                crate::error::Error::Other(format!(
                    "internal error: updated link not found in candidate: {}",
                    link_id
                ))
            })?;
        let source_facts = extract_path_dependency_facts(&context, &updated_link.source)?;
        let target_facts = extract_path_dependency_facts(&context, &updated_link.target)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_link(link_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::LinkSource,
            &result.link_id,
            &result.source,
            &result.label,
            source_facts,
        );
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::LinkTarget,
            &result.link_id,
            &result.target,
            &result.label,
            target_facts,
        );

        Ok(result)
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_link_loaded(link_id)?;
        store.delete_link(link_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // 增量更新持久化路径依赖索引：移除被删 Link 的 source/target 两端依赖事实。
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        self.remove_source_from_path_dependency_index(
            starmap_id,
            &CrossCanvasRefSourceKind::LinkSource,
            link_id,
        );
        self.remove_source_from_path_dependency_index(
            starmap_id,
            &CrossCanvasRefSourceKind::LinkTarget,
            link_id,
        );

        Ok(())
    }

    pub fn list_starmap_hyperlinks(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapHyperlink>>
    {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.list_hyperlinks_with_diagnostics()
    }

    pub fn add_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hl: crate::starmap::types::StarMapHyperlink,
    ) -> Result<crate::starmap::types::StarMapHyperlink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
        }

        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            g.hyperlinks.push(hl.clone());
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare source 的事实（可失败，不修改状态）。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let source_facts = extract_path_dependency_facts(&context, &hl.source)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_hyperlink(hl)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::HyperlinkSource,
            &result.hyperlink_id,
            &result.source,
            &result.label,
            source_facts,
        );

        Ok(result)
    }

    pub fn update_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hyperlink_id: &str,
        patch: &crate::starmap::types::StarMapHyperlinkPatch,
    ) -> Result<crate::starmap::types::StarMapHyperlink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
            store.ensure_fully_loaded()?;
            store.ensure_hyperlink_loaded(hyperlink_id)?;
        }

        // hyperlink update 统一走 validate 以保持引用完整性（source 路径可能变）。
        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let mut g = store.to_starmap_graph();
            apply_hyperlink_update_to_graph(&mut g, hyperlink_id, patch)?;
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare source 的事实（可失败，不修改状态）。
        // 用 candidate 中 patch 应用后的 hyperlink source。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let updated_hl = candidate
            .hyperlinks
            .iter()
            .find(|h| h.hyperlink_id == hyperlink_id)
            .ok_or_else(|| {
                crate::error::Error::Other(format!(
                    "internal error: updated hyperlink not found in candidate: {}",
                    hyperlink_id
                ))
            })?;
        let source_facts = extract_path_dependency_facts(&context, &updated_hl.source)?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_hyperlink(hyperlink_id, patch)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            starmap_id,
            CrossCanvasRefSourceKind::HyperlinkSource,
            &result.hyperlink_id,
            &result.source,
            &result.label,
            source_facts,
        );

        Ok(result)
    }

    pub fn delete_starmap_hyperlink(&self, starmap_id: &str, hyperlink_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_hyperlink_loaded(hyperlink_id)?;
        store.delete_hyperlink(hyperlink_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // 增量更新持久化路径依赖索引：移除被删 Hyperlink 的 source 依赖事实。
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        self.remove_source_from_path_dependency_index(
            starmap_id,
            &CrossCanvasRefSourceKind::HyperlinkSource,
            hyperlink_id,
        );

        Ok(())
    }

    pub fn list_starmap_links(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapLink>>
    {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.list_links_with_diagnostics()
    }

    pub fn get_starmap_phased_snapshot(
        &self,
        starmap_id: &str,
        request: &crate::starmap::store::PhasedSnapshotRequest,
    ) -> Result<crate::starmap::store::StarMapPhasedSnapshot> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.get_phased_snapshot(request)
    }

    /// 确认星图删除 tombstone 已被同步方持久化，清理 `deleted_at_revision <=
    /// acknowledged_revision` 的 tombstone。清理结果在下次 flush 时写回磁盘。
    pub fn ack_starmap_deletions(
        &self,
        starmap_id: &str,
        acknowledged_revision: u64,
    ) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.acknowledge_deletions(acknowledged_revision)
    }

    /// 原子组合操作：创建子星图并嵌入父图（crash-safe）。
    ///
    /// 使用 journal 事务确保 crash-safe：
    /// 1. 先生成确定的 child_starmap_id
    /// 2. 写 pending journal（原子写盘）
    /// 3. 用确定 ID 创建 child meta/index
    /// 4. 在 host Store 写 Embed
    /// 5. flush host Store，确认 Embed 已落盘
    ///
    /// **不** complete/cleanup journal——把 tx_id 返回给调用方，
    /// 由调用方在 `record_workspace_change_set_history` 成功后
    /// 调 `ack_child_embed_history` 推进 journal 到 Completed 并清理。
    ///
    /// 返回 `(StarMapMeta, StarMapEmbed, WorkspaceChangeSet, tx_id)`，
    /// WorkspaceChangeSet 包含 child meta 路径、starmap index 路径、host Embed/graph meta 路径。
    pub fn create_starmap_child_embed(
        &self,
        host_starmap_id: &str,
        title: &str,
        position: crate::starmap::types::StarMapPoint,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::starmap::types::StarMapEmbed,
        crate::storage::workspace_git::WorkspaceChangeSet,
        String,
    )> {
        // Step 1: 生成确定的 child_starmap_id
        let child_starmap_id = format!("sm_{}", uuid::Uuid::new_v4());

        // Step 2: 写 pending journal（原子写盘）
        let mut tx = crate::storage::journal::StarMapChildEmbedTransaction::new(
            host_starmap_id,
            &child_starmap_id,
            title,
            position.clone(),
            &self.app_data_root,
        );
        tx.prepare()?;

        // Step 3: 用确定 ID 创建 child meta/index。
        // 走 nested entry 创建路径：只登记 starmap_ids，不写 root_starmap_ids。
        // 子星图身份在创建入口就是"嵌套"，正常一级列表不靠 Embed 关系过滤。
        let child_meta = match crate::starmap::create_nested_starmap_with_id(
            &self.app_data_root,
            &child_starmap_id,
            title,
            "",
            None,
        ) {
            Ok(meta) => meta,
            Err(e) => {
                // 创建失败：清 journal（child 未创建，Pending phase）
                log::warn!(
                    "create_starmap_child_embed: create_starmap_with_id failed, \
                     clearing journal tx_id={}: {}",
                    tx.tx_id(),
                    e
                );
                // journal 在 Pending phase，恢复时会清除
                return Err(e);
            }
        };
        tx.mark_child_created()?;

        // Step 4: 构造 Embed 并添加到宿主图（使用 journal 中预生成的 embed_instance_id）
        let now = crate::starmap::now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: tx.embed_instance_id().to_string(),
            target_starmap_id: child_meta.starmap_id.clone(),
            label: Some(title.to_string()),
            position,
            host_path: crate::starmap::types::StarMapTargetPath {
                starmap_id: host_starmap_id.to_string(),
                segments: Vec::new(),
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            provenance: crate::starmap::semantic::StarMapProvenance::default(),
            created_at: now,
            updated_at: now,
        };

        let created_embed = match self.add_starmap_embed(host_starmap_id, embed) {
            Ok(embed) => embed,
            Err(e) => {
                // Embed 添加失败：journal 保留在 ChildCreated phase，
                // 下次启动恢复时会补 Embed 或清理。
                log::warn!(
                    "create_starmap_child_embed: add_starmap_embed failed, \
                     journal retained at ChildCreated tx_id={}: {}",
                    tx.tx_id(),
                    e
                );
                return Err(e);
            }
        };

        // Step 5: flush host Store，确认 Embed 已落盘
        let host_changed_paths = self.flush_starmap_store(host_starmap_id)?;
        tx.mark_embed_added()?;

        // 不 complete/cleanup journal——由调用方在 history 记录成功后 ack。
        let tx_id = tx.tx_id().to_string();

        // 构造 WorkspaceChangeSet：
        // - child meta 路径 + starmap index 路径（create_starmap_with_id 写的）
        // - host changed paths（add_starmap_embed + flush 写的真实文件路径）
        let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(
                std::path::PathBuf::from("starmaps")
                    .join(format!("{}.meta.json", child_meta.starmap_id)),
            )
            .add_upsert(std::path::PathBuf::from("starmaps").join("index.json"));

        for path in host_changed_paths {
            change_set = change_set.add_upsert(path);
        }

        Ok((child_meta, created_embed, change_set, tx_id))
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapReference>> {
        crate::starmap::find_starmap_references(&self.app_data_root, target_starmap_id)
    }

    /// 创建跨画布边并真正写入宿主图 Store（事务操作）。
    ///
    /// 同一次写操作内完成：
    /// 1. 解析两端 `StarMapEndpointSelection`（route + target），确认对象存在。
    /// 2. 调用 [`plan_cross_canvas_edge`] 求共同宿主图、`edge_id`、`from`/`to` 路径。
    /// 3. 构造完整 `StarMapEdge`（含 `kind`、`label`、`payload`、时间戳）。
    /// 4. 在 candidate graph 上模拟添加并跑 `validate_graph`。
    /// 5. 调用既有 `store.add_edge` 写入宿主图 Store，进入 Edge + GraphMeta 保存链。
    ///
    /// 返回 [`CreateCrossCanvasEdgeResult`]（`owner_starmap_id` + 完整 `edge`），
    /// 供平台端后续修改/删除原始边。**不**让 UI 自己拿规划结果再分两步拼装写入。
    ///
    /// [`plan_cross_canvas_edge`]: crate::starmap::connections::plan_cross_canvas_edge
    pub fn create_cross_canvas_edge(
        &self,
        from: crate::starmap::connections::StarMapEndpointSelection,
        to: crate::starmap::connections::StarMapEndpointSelection,
        kind: crate::starmap::types::StarMapEdgeKind,
        label: Option<String>,
        payload: Option<serde_json::Value>,
    ) -> Result<CreateCrossCanvasEdgeResult> {
        let root_starmap_id = from.route.root_starmap_id.clone();
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        // 确保 root starmap 完全加载，plan 能看到尚未 flush 的 Embed/Portal 变更。
        {
            let store = stores
                .entry(root_starmap_id.clone())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, &root_starmap_id));
            store.ensure_fully_loaded()?;
        }

        let context = self.build_resolver_context(&stores);

        // 1. plan：求共同宿主图、edge_id、from/to 路径（纯计算，不写 Store）。
        let (edge_id, from_path, to_path) = crate::starmap::connections::plan_cross_canvas_edge(
            &context, &from, &to,
        )
        .map_err(|e| {
            crate::error::Error::Other(format!("create_cross_canvas_edge: plan failed: {e:?}"))
        })?;

        // plan 保证 from_path/to_path 的 starmap_id 都是宿主图 ID。
        let owner_starmap_id = from_path.starmap_id.clone();

        // 确保宿主图 Store 完全加载，以便 candidate 模拟和写入看到完整图数据。
        {
            let store = stores
                .entry(owner_starmap_id.clone())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, &owner_starmap_id));
            store.ensure_fully_loaded()?;
        }

        // 2. 构造完整 StarMapEdge。
        let now = crate::starmap::now_epoch();
        let edge = crate::starmap::types::StarMapEdge {
            id: edge_id,
            from: from_path,
            to: to_path,
            kind,
            label,
            payload,
            created_at: now,
            updated_at: now,
        };

        // 3. candidate 模拟 + validate_graph。
        let candidate = {
            let store = Self::get_store_or_err(&stores, &owner_starmap_id)?;
            let mut g = store.to_starmap_graph();
            g.edges.push(edge.clone());
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        // 两阶段索引更新（问题2）：先 prepare from/to 的事实（可失败，不修改状态）。
        let context = self.build_resolver_context(&stores);
        use crate::starmap::graph::resolve::extract_path_dependency_facts;
        use crate::starmap::store::relation_index::CrossCanvasRefSourceKind;
        let from_facts = extract_path_dependency_facts(&context, &edge.from)?;
        let to_facts = extract_path_dependency_facts(&context, &edge.to)?;

        // 4. 真正写入宿主图 Store，进入 Edge + GraphMeta 保存链。
        let store = Self::get_store_mut_or_err(&mut stores, &owner_starmap_id)?;
        let written_edge = store.add_edge(edge)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        // replace（不可失败）：用预计算的事实更新索引。
        self.replace_source_facts_in_path_dependency_index(
            &owner_starmap_id,
            CrossCanvasRefSourceKind::EdgeFrom,
            &written_edge.id,
            &written_edge.from,
            &written_edge.label,
            from_facts,
        );
        self.replace_source_facts_in_path_dependency_index(
            &owner_starmap_id,
            CrossCanvasRefSourceKind::EdgeTo,
            &written_edge.id,
            &written_edge.to,
            &written_edge.label,
            to_facts,
        );

        Ok(CreateCrossCanvasEdgeResult {
            owner_starmap_id,
            edge: written_edge,
        })
    }

    // -----------------------------------------------------------------------
    // 跨图引用检查（删除保护）
    //
    // 统一检查函数走同一个 PathDependencyIndex 查询入口，对所有被删对象
    // （含级联删除的 Embed）做一次完整检查，不再各自遍历全图、手写匹配逻辑。
    // -----------------------------------------------------------------------

    /// 检查删除 Node 或 Embed 前是否存在跨图引用（含级联删除的对象）。
    ///
    /// 旧实现只检查被点中的对象本身：
    /// - `check_cross_canvas_references_for_node_delete` 只查被删节点的
    ///   NodeTarget/AnchorTarget/PortalTraversal，不查级联删除的 Embed。
    /// - `check_cross_canvas_references_for_embed_delete` 只查被删 Embed 本身。
    ///
    /// 本方法对完整删除集合做一次检查：
    /// - 对每个被删 Node 查询 NodeTarget/AnchorTarget/PortalTraversal
    /// - 对每个被删 Embed（含 origin Embed 和级联删除的 Embed）查询 EmbedTraversal
    ///
    /// 只拦截外部跨图依赖（`owner_starmap_id != starmap_id`）；
    /// 本图自引用（`owner_starmap_id == starmap_id`）由本地级联删除处理，跳过。
    /// 读取错误（磁盘 IO、schema 不兼容等）通过 `?` 向上传播，不吞成"无引用"。
    fn check_cross_canvas_references_for_delete(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
        origin: &crate::starmap::store::CascadeOrigin,
        cascade: &crate::starmap::store::CascadeIds,
    ) -> Result<()> {
        use crate::starmap::graph::resolve::PathDependencyKind;

        // 使用 WriterCore 持有的持久化路径依赖索引，不再每次从零扫描全库 build。
        let index_guard = self.get_or_init_path_dependency_index(stores)?;
        let dep_index = index_guard.as_ref().ok_or_else(|| {
            crate::error::Error::Other("path dependency index initialization failed".to_string())
        })?;

        let mut refs = Vec::new();

        // 被删 Node：查询 NodeTarget/AnchorTarget/PortalTraversal。
        // 删除节点连带删除其所有锚点，且节点本身可能是 Portal 入口，
        // 因此同时检查三类外部依赖。
        if let crate::starmap::store::CascadeOrigin::Node(node_id) = origin {
            for kind in [
                PathDependencyKind::NodeTarget,
                PathDependencyKind::AnchorTarget,
                PathDependencyKind::PortalTraversal,
            ] {
                refs.extend(collect_external_refs(dep_index, starmap_id, &kind, node_id));
            }
        }

        // 被删 Embed：查询 EmbedTraversal。
        // 包含 origin Embed（如果是删 Embed）和级联删除的所有 Embed。
        let mut deleted_embed_ids: Vec<String> = Vec::new();
        if let crate::starmap::store::CascadeOrigin::Embed(instance_id) = origin {
            deleted_embed_ids.push(instance_id.clone());
        }
        deleted_embed_ids.extend(cascade.embed_ids.iter().cloned());
        for embed_id in &deleted_embed_ids {
            refs.extend(collect_external_refs(
                dep_index,
                starmap_id,
                &PathDependencyKind::EmbedTraversal,
                embed_id,
            ));
        }

        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "Cannot delete {:?} in '{}': cross-canvas references exist: {:?}",
                origin,
                starmap_id,
                refs.iter()
                    .map(|r| format!(
                        "(owner={}, kind={:?}, source={}, path={:?})",
                        r.owner_starmap_id, r.source_kind, r.source_id, r.referenced_path
                    ))
                    .collect::<Vec<_>>()
            )));
        }

        Ok(())
    }

    /// 检查更新节点时是否破坏了外部图持有的路径依赖（修改1）。
    ///
    /// 只在 Portal 身份（destination）改变或 Anchor 被删除时检查：
    /// - Portal 删除/目标改变：查询所有穿越该 Portal 的外部路径（`PortalTraversal`）
    /// - Anchor 删除：查询所有终点落在该 anchor 的外部路径（`AnchorTarget`）
    ///
    /// title/position/style 等不影响目标身份的变更不触发检查。
    /// 存在依赖时返回具体宿主、引用类型、引用 ID，不允许先改再让外部路径悬空。
    fn check_external_dependencies_for_node_update(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
        node_id: &str,
        old_node: &crate::starmap::types::StarMapNode,
        new_node: &crate::starmap::types::StarMapNode,
    ) -> Result<()> {
        use crate::starmap::graph::resolve::PathDependencyKind;

        // Portal 身份改变 = destination_starmap_id 或 destination_target 变化。
        // 新增 Portal（None→Some）不破坏既有外部路径，无需检查。
        let portal_changed = match (&old_node.portal, &new_node.portal) {
            (None, None) | (None, Some(_)) => false,
            (Some(_), None) => true,
            (Some(o), Some(n)) => {
                o.destination_starmap_id != n.destination_starmap_id
                    || o.destination_target != n.destination_target
            }
        };

        // 被删除的 anchor：旧有新无。anchor_id 改变即视为删除旧 anchor。
        let removed_anchor_ids: Vec<String> = old_node
            .anchors
            .iter()
            .filter(|a| {
                !new_node
                    .anchors
                    .iter()
                    .any(|na| na.anchor_id == a.anchor_id)
            })
            .map(|a| a.anchor_id.clone())
            .collect();

        if !portal_changed && removed_anchor_ids.is_empty() {
            return Ok(());
        }

        // 使用 WriterCore 持有的持久化路径依赖索引，不再每次从零扫描全库 build。
        let index_guard = self.get_or_init_path_dependency_index(stores)?;
        let dep_index = index_guard.as_ref().ok_or_else(|| {
            crate::error::Error::Other("path dependency index initialization failed".to_string())
        })?;

        let mut refs = Vec::new();
        if portal_changed {
            for dep in dep_index
                .query_dependents(starmap_id, &PathDependencyKind::PortalTraversal, node_id)
                .iter()
                .filter(|dep| dep.owner_starmap_id != starmap_id)
            {
                refs.push(format!(
                    "(owner={}, kind=PortalTraversal, source={}, path={:?})",
                    dep.owner_starmap_id, dep.source_id, dep.referenced_path
                ));
            }
        }
        for removed_anchor_id in &removed_anchor_ids {
            for dep in dep_index
                .query_dependents(starmap_id, &PathDependencyKind::AnchorTarget, node_id)
                .iter()
                .filter(|dep| dep.owner_starmap_id != starmap_id)
                .filter(|dep| dep.target_anchor_id.as_deref() == Some(removed_anchor_id.as_str()))
            {
                refs.push(format!(
                    "(owner={}, kind=AnchorTarget(anchor={}), source={}, path={:?})",
                    dep.owner_starmap_id, removed_anchor_id, dep.source_id, dep.referenced_path
                ));
            }
        }

        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "Cannot update node '{}': external dependencies would dangle: {:?}",
                node_id, refs
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 跨图引用检查 - 模块级辅助函数
// ---------------------------------------------------------------------------

/// 收集依赖指定对象的外部（非本图）路径引用。
///
/// 查询 `PathDependencyIndex` 并过滤掉宿主图等于 `starmap_id` 的本图自引用
/// （本图级联删除/validate_graph 已处理），只返回外部跨图依赖。
fn collect_external_refs(
    dep_index: &crate::starmap::store::relation_index::PathDependencyIndex,
    starmap_id: &str,
    kind: &crate::starmap::graph::resolve::PathDependencyKind,
    object_id: &str,
) -> Vec<crate::starmap::store::relation_index::AllCrossCanvasReferenceInfo> {
    use crate::starmap::store::relation_index::AllCrossCanvasReferenceInfo;

    dep_index
        .query_dependents(starmap_id, kind, object_id)
        .iter()
        .filter(|dep| dep.owner_starmap_id != starmap_id)
        .map(|dep| AllCrossCanvasReferenceInfo {
            owner_starmap_id: dep.owner_starmap_id.clone(),
            source_kind: dep.source_kind.clone(),
            source_id: dep.source_id.clone(),
            referenced_path: dep.referenced_path.clone(),
            label: dep.label.clone(),
        })
        .collect()
}
