//! 星图 graph / 节点 / 边 / 嵌入 / 链接 / 超链接的 facade 方法。
//!
//! 与 `starmap_ops.rs` 属于同一个 `WriterCore` 类型，只是按关注点拆成两个固有
//! `impl`：星图本身的增删改查与 store 生命周期在 `starmap_ops.rs`，图数据操作
//! 在这里。Rust 允许同一类型的固有 `impl` 分散在同 crate 的多个模块中。

use super::patch::{
    apply_edge_patch_to_graph, apply_embed_deletion_to_graph, apply_embed_patch_to_graph,
    apply_hyperlink_update_to_graph, apply_link_patch_to_graph, apply_node_deletion_to_graph,
    apply_node_patch_to_graph,
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
        {
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
        }

        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_node(node_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> Result<()> {
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
            store.ensure_object_loaded(node_id)?;
        }

        // 跨图引用检查：在本地级联检查之后、实际删除之前执行。
        // 检查是否有其他 graph 持有的跨层引用指向当前图的该节点。
        // 有跨图关联时拒绝破坏性删除并明确返回引用列表。
        self.check_cross_canvas_references_for_node_delete(&stores, starmap_id, node_id)?;

        // 先在 candidate graph 上模拟删除（含级联），跑 validate_graph，再真正改 Store。
        // 级联 ID 复用 store.node_cascade_ids 纯函数，保证 candidate 模拟和 store
        // 真实删除产生相同的最终对象集合。
        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let cascade = store.node_cascade_ids(node_id);
            let mut g = store.to_starmap_graph();
            apply_node_deletion_to_graph(&mut g, node_id, &cascade);
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        store.delete_node(node_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteNode);
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(())
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_edge(edge)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_edge(edge_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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
        store.ensure_loaded()?;
        store.ensure_edge_loaded(edge_id)?;
        store.delete_edge(edge_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_embed(embed)?;
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_embed(instance_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> Result<()> {
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
            store.ensure_embed_loaded(instance_id)?;
        }

        // 跨图引用检查：在本地级联检查之后、实际删除之前执行。
        // 检查是否有其他 graph 持有的跨层引用通过该 Embed 入口穿越。
        // 有跨图关联时拒绝破坏性删除并明确返回引用列表。
        self.check_cross_canvas_references_for_embed_delete(&stores, starmap_id, instance_id)?;

        // 先在 candidate graph 上模拟删除（含级联），跑 validate_graph，再真正改 Store。
        // 级联 ID 复用 store.embed_cascade_ids 纯函数，保证 candidate 模拟和 store
        // 真实删除产生相同的最终对象集合。
        let candidate = {
            let store = Self::get_store_or_err(&stores, starmap_id)?;
            let cascade = store.embed_cascade_ids(instance_id);
            let mut g = store.to_starmap_graph();
            apply_embed_deletion_to_graph(&mut g, instance_id, &cascade);
            g
        };
        validation::validate_graph(
            &self.build_resolver_context_from_stores(&stores, &candidate),
            &candidate,
        )?;

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        store.delete_embed(instance_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_link(link)?;
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_link(link_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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
        store.ensure_loaded()?;
        store.ensure_link_loaded(link_id)?;
        store.delete_link(link_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.add_hyperlink(hl)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        let store = Self::get_store_mut_or_err(&mut stores, starmap_id)?;
        let result = store.update_hyperlink(hyperlink_id, patch)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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
        store.ensure_loaded()?;
        store.ensure_hyperlink_loaded(hyperlink_id)?;
        store.delete_hyperlink(hyperlink_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
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

        // 4. 真正写入宿主图 Store，进入 Edge + GraphMeta 保存链。
        let store = Self::get_store_mut_or_err(&mut stores, &owner_starmap_id)?;
        let written_edge = store.add_edge(edge)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        Ok(CreateCrossCanvasEdgeResult {
            owner_starmap_id,
            edge: written_edge,
        })
    }

    // -----------------------------------------------------------------------
    // 跨图引用检查（删除保护）
    //
    // 修改4：两个检查函数都走同一个 PathDependencyIndex 查询入口，
    // 不再各自遍历全图、手写匹配逻辑。
    // -----------------------------------------------------------------------

    /// 检查删除节点前是否存在跨图引用。
    ///
    /// 通过 [`PathDependencyIndex`] 查询所有外部图对本图该节点的依赖：
    /// - `NodeTarget`：路径终点落在该节点
    /// - `AnchorTarget`：路径终点落在该节点的某个锚点（删节点连带删锚点）
    /// - `PortalTraversal`：路径途中穿越该节点（作为 Portal 入口，修改2）
    ///
    /// 本图自身的引用由本地级联删除（`node_cascade_ids`）处理，只拦截外部跨图依赖。
    /// 读取错误（磁盘 IO、schema 不兼容等）通过 `?` 向上传播，不吞成"无引用"。
    fn check_cross_canvas_references_for_node_delete(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
        node_id: &str,
    ) -> Result<()> {
        use crate::starmap::graph::resolve::PathDependencyKind;
        use crate::starmap::store::relation_index::PathDependencyIndex;

        let context = self.build_resolver_context(stores);
        let idx = crate::starmap::load_index(&self.app_data_root)?;
        let dep_index = PathDependencyIndex::build(&context, &idx.starmap_ids)?;

        // 删除节点连带删除其所有锚点，且节点本身可能是 Portal 入口（修改2），
        // 因此同时检查 NodeTarget、AnchorTarget、PortalTraversal 三类外部依赖。
        let mut refs = Vec::new();
        for kind in [
            PathDependencyKind::NodeTarget,
            PathDependencyKind::AnchorTarget,
            PathDependencyKind::PortalTraversal,
        ] {
            refs.extend(collect_external_refs(
                &dep_index, starmap_id, &kind, node_id,
            ));
        }

        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "Cannot delete node '{}': cross-canvas references exist: {:?}",
                node_id,
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

    /// 检查删除 Embed 入口前是否存在跨图引用。
    ///
    /// 通过 [`PathDependencyIndex`] 查询所有外部图路径对本图该 Embed 实例的
    /// `EmbedTraversal` 依赖（路径途中穿越该 Embed 入口）。
    ///
    /// 本图自身的引用由本地级联删除（`embed_cascade_ids`）处理，只拦截外部跨图依赖。
    /// 读取错误通过 `?` 向上传播，不吞成"无引用"。
    fn check_cross_canvas_references_for_embed_delete(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
        instance_id: &str,
    ) -> Result<()> {
        use crate::starmap::graph::resolve::PathDependencyKind;
        use crate::starmap::store::relation_index::PathDependencyIndex;

        let context = self.build_resolver_context(stores);
        let idx = crate::starmap::load_index(&self.app_data_root)?;
        let dep_index = PathDependencyIndex::build(&context, &idx.starmap_ids)?;

        let refs = collect_external_refs(
            &dep_index,
            starmap_id,
            &PathDependencyKind::EmbedTraversal,
            instance_id,
        );

        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "Cannot delete embed '{}': cross-canvas references exist: {:?}",
                instance_id,
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
        use crate::starmap::store::relation_index::PathDependencyIndex;

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

        let context = self.build_resolver_context(stores);
        let idx = crate::starmap::load_index(&self.app_data_root)?;
        let dep_index = PathDependencyIndex::build(&context, &idx.starmap_ids)?;

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
