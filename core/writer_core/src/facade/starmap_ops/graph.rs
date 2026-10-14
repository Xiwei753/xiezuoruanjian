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

    // -----------------------------------------------------------------------
    // 跨图引用检查（删除保护）
    // -----------------------------------------------------------------------

    /// 检查删除节点前是否存在跨图引用。
    ///
    /// 遍历所有星图（不含当前图）的 Edge，解析每条 Edge 的 from/to 路径，
    /// 检查是否有一端解析后指向 `(starmap_id, Node { node_id })`。
    /// 有跨图关联时拒绝删除并返回引用列表。
    ///
    /// **不能只扫描本图**：祖先图中的 Edge 可能通过 Embed/Portal 路径
    /// 指向当前图的节点，删除该节点会使祖先 Edge 变悬空。
    #[allow(clippy::excessive_nesting)]
    fn check_cross_canvas_references_for_node_delete(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
        node_id: &str,
    ) -> Result<()> {
        use crate::starmap::graph::resolve::resolve_target;
        use crate::starmap::store::relation_index::CrossCanvasReferenceInfo;

        let context = self.build_resolver_context(stores);
        let target_detail = crate::starmap::semantic::StarMapTargetDetail::Node {
            node_id: node_id.to_string(),
        };

        let mut refs = Vec::new();

        // 遍历所有星图的 Edge，检查是否有跨图引用指向当前图的该节点。
        // 使用 starmap index 获取所有星图 ID。
        let idx = crate::starmap::load_index(&self.app_data_root)?;
        for other_starmap_id in &idx.starmap_ids {
            if other_starmap_id == starmap_id {
                continue;
            }

            // 加载该星图的边列表
            let edges = if let Some(graph) = context.overlays.get(other_starmap_id) {
                graph.edges.clone()
            } else {
                load_edges_from_disk(&self.app_data_root, other_starmap_id)
            };

            for edge in &edges {
                let from_resolved = resolve_target(&context, &edge.from);
                let to_resolved = resolve_target(&context, &edge.to);

                let from_matches =
                    matches_resolved_target(&from_resolved, starmap_id, &target_detail);
                let to_matches = matches_resolved_target(&to_resolved, starmap_id, &target_detail);

                if from_matches || to_matches {
                    refs.push(CrossCanvasReferenceInfo {
                        owner_starmap_id: other_starmap_id.clone(),
                        edge_id: edge.id.clone(),
                        from_path: edge.from.clone(),
                        to_path: edge.to.clone(),
                        kind: edge.kind.clone(),
                        label: edge.label.clone(),
                    });
                }
            }
        }

        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "Cannot delete node '{}': cross-canvas references exist: {:?}",
                node_id,
                refs.iter()
                    .map(|r| format!(
                        "(owner={}, edge={}, from={:?}, to={:?})",
                        r.owner_starmap_id, r.edge_id, r.from_path, r.to_path
                    ))
                    .collect::<Vec<_>>()
            )));
        }

        Ok(())
    }

    /// 检查删除 Embed 入口前是否存在跨图引用。
    ///
    /// 遍历所有星图（不含当前图）的 Edge，解析每条 Edge 的 from/to 路径，
    /// 检查是否有路径穿越了当前图的该 Embed 实例。有跨图关联时拒绝删除
    /// 并返回引用列表。
    ///
    /// **不能只扫描本图**：祖先图中的 Edge 可能通过当前图的 Embed 入口
    /// 穿越到子画布，删除该 Embed 会使祖先 Edge 变悬空。
    #[allow(clippy::excessive_nesting)]
    fn check_cross_canvas_references_for_embed_delete(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
        instance_id: &str,
    ) -> Result<()> {
        use crate::starmap::store::relation_index::CrossCanvasReferenceInfo;

        let context = self.build_resolver_context(stores);

        let mut refs = Vec::new();

        // 遍历所有星图的 Edge，检查是否有路径穿越了当前图的该 Embed 实例。
        let idx = crate::starmap::load_index(&self.app_data_root)?;
        for other_starmap_id in &idx.starmap_ids {
            if other_starmap_id == starmap_id {
                continue;
            }

            let edges = if let Some(graph) = context.overlays.get(other_starmap_id) {
                graph.edges.clone()
            } else {
                load_edges_from_disk(&self.app_data_root, other_starmap_id)
            };

            for edge in &edges {
                // 检查 from 和 to 路径是否穿越了当前图的该 Embed 实例
                let from_traverses =
                    path_traverses_embed(&context, &edge.from, starmap_id, instance_id);
                let to_traverses =
                    path_traverses_embed(&context, &edge.to, starmap_id, instance_id);

                if from_traverses || to_traverses {
                    refs.push(CrossCanvasReferenceInfo {
                        owner_starmap_id: other_starmap_id.clone(),
                        edge_id: edge.id.clone(),
                        from_path: edge.from.clone(),
                        to_path: edge.to.clone(),
                        kind: edge.kind.clone(),
                        label: edge.label.clone(),
                    });
                }
            }
        }

        if !refs.is_empty() {
            return Err(crate::error::Error::Other(format!(
                "Cannot delete embed '{}': cross-canvas references exist: {:?}",
                instance_id,
                refs.iter()
                    .map(|r| format!(
                        "(owner={}, edge={}, from={:?}, to={:?})",
                        r.owner_starmap_id, r.edge_id, r.from_path, r.to_path
                    ))
                    .collect::<Vec<_>>()
            )));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 跨图引用检查 - 模块级辅助函数
// ---------------------------------------------------------------------------

/// 检查解析后的目标是否匹配特定画布和目标详情。
fn matches_resolved_target(
    resolved: &std::result::Result<
        crate::starmap::graph::resolve::ResolvedTarget,
        crate::starmap::semantic::StarMapTargetResolveStatus,
    >,
    starmap_id: &str,
    target: &crate::starmap::semantic::StarMapTargetDetail,
) -> bool {
    match resolved {
        Ok(r) => r.final_starmap_id == starmap_id && &r.target == target,
        Err(_) => false,
    }
}

/// 检查路径是否穿越了指定星图中的指定 Embed 实例。
///
/// 通过解析路径获取 `traversed_starmap_ids`，找到 `starmap_id` 在穿越链中的
/// 位置，检查对应的路径段是否为 `EnterEmbed { instance_id }`。
fn path_traverses_embed(
    context: &crate::starmap::graph::resolve::GraphResolverContext,
    path: &crate::starmap::types::reference::StarMapTargetPath,
    starmap_id: &str,
    instance_id: &str,
) -> bool {
    use crate::starmap::types::reference::StarMapPathSegment;

    let resolved = match crate::starmap::graph::resolve::resolve_target(context, path) {
        Ok(r) => r,
        Err(_) => return false,
    };

    // traversed_starmap_ids[i] 是 segments[i] 之前的星图
    // segments[i] 从 traversed_starmap_ids[i] 穿越到 traversed_starmap_ids[i+1]
    // 所以如果 traversed_starmap_ids[i] == starmap_id 且 segments[i] 是
    // EnterEmbed { instance_id }，则路径穿越了该 Embed
    resolved
        .traversed_starmap_ids
        .iter()
        .enumerate()
        .any(|(i, sid)| {
            sid == starmap_id
                && matches!(
                    path.segments.get(i),
                    Some(StarMapPathSegment::EnterEmbed { instance_id: ref eid })
                        if eid == instance_id
                )
        })
}

/// 从磁盘加载星图的边列表。
fn load_edges_from_disk(
    app_data_root: &std::path::Path,
    starmap_id: &str,
) -> Vec<crate::starmap::types::StarMapEdge> {
    use crate::starmap::package_storage::bucket_for_id;
    use crate::starmap::store::meta::GraphMeta;

    let graph_json_path = app_data_root
        .join("starmaps")
        .join(starmap_id)
        .join("graph.json");
    if !graph_json_path.exists() {
        return Vec::new();
    }

    let content = match std::fs::read_to_string(&graph_json_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let meta: GraphMeta = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    let mut edges = Vec::new();
    for edge_id in &meta.edge_ids {
        let bucket_dir = app_data_root
            .join("starmaps")
            .join(starmap_id)
            .join("edges")
            .join(bucket_for_id(edge_id));
        let edge_path = bucket_dir.join(format!("{}.json", edge_id));
        if let Ok(edge_content) = std::fs::read_to_string(&edge_path) {
            if let Ok(edge) =
                serde_json::from_str::<crate::starmap::types::StarMapEdge>(&edge_content)
            {
                edges.push(edge);
            }
        }
    }

    edges
}
