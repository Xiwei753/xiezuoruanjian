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
        default_x: f32,
        default_y: f32,
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

    /// 原子组合操作：创建子星图并嵌入父图。
    ///
    /// 内部顺序：
    /// 1. `create_starmap(title, "", DEFAULT_ACCENT_COLOR)` 创建目标 StarMap
    /// 2. 构造 StarMapEmbed 并调用 `add_starmap_embed` 在宿主图创建 Embed
    /// 3. 第二步失败时在 Core 内回滚刚创建的目标 StarMap
    ///
    /// 锁顺序：`create_starmap` 不持 `starmap_stores` 锁（纯磁盘操作），
    /// `add_starmap_embed` 持锁。两步之间无锁竞争，不会死锁。
    pub fn create_starmap_child_embed(
        &self,
        host_starmap_id: &str,
        title: &str,
        position: crate::starmap::types::StarMapPoint,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::starmap::types::StarMapEmbed,
    )> {
        // Step 1: 创建子星图（无锁，纯磁盘操作）
        let child_meta = self.create_starmap(title, "", None)?;

        // Step 2: 构造 Embed 并添加到宿主图
        let now = crate::starmap::now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: format!("em_{}", uuid::Uuid::new_v4()),
            target_starmap_id: child_meta.starmap_id.clone(),
            label: None,
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

        match self.add_starmap_embed(host_starmap_id, embed) {
            Ok(created_embed) => Ok((child_meta, created_embed)),
            Err(e) => {
                // Step 3: 回滚——删除刚创建的子星图
                log::warn!(
                    "create_starmap_child_embed: add_starmap_embed failed, rolling back \
                     created starmap {}: {}",
                    child_meta.starmap_id,
                    e
                );
                if let Err(rollback_err) = self.delete_starmap(&child_meta.starmap_id) {
                    log::error!(
                        "create_starmap_child_embed: rollback failed for starmap {}: {}",
                        child_meta.starmap_id,
                        rollback_err
                    );
                }
                Err(e)
            }
        }
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapReference>> {
        crate::starmap::find_starmap_references(&self.app_data_root, target_starmap_id)
    }
}
