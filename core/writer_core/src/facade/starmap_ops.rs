use crate::error::Result;
use crate::starmap::graph::resolve::GraphResolverContext;
use crate::starmap::graph::validation;
use crate::starmap::store::{LoadPhase, SaveQueueEntry, StarMapStore};

// 星图 facade 操作按关注点拆成三个模块：
//   - 本文件：星图本身的增删改查、store 生命周期、两个子模块共用的私有 helper
//   - graph.rs：graph / node / edge / embed / link / hyperlink 数据操作
//   - patch.rs：candidate graph patch 模拟（只被 graph.rs 调用）
mod graph;
mod patch;

impl super::WriterCore {
    pub fn list_starmaps(&self) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps(&self.app_data_root)
    }

    pub fn list_starmaps_bound_to_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps_bound_to_project(&self.app_data_root, project_id)
    }

    pub fn get_starmap(&self, starmap_id: &str) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::get_starmap(&self.app_data_root, starmap_id)
    }

    pub fn create_starmap(
        &self,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::create_starmap(&self.app_data_root, title, description, accent_color)
    }

    ///   create_starmap 的变更集版本。
    pub fn create_starmap_with_changes(
        &self,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::storage::workspace_git::WorkspaceChangeSet,
    )> {
        crate::starmap::create_starmap_with_changes(
            &self.app_data_root,
            title,
            description,
            accent_color,
        )
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::rename_starmap(&self.app_data_root, starmap_id, new_title)
    }

    ///   rename_starmap 的变更集版本。
    pub fn rename_starmap_with_changes(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::storage::workspace_git::WorkspaceChangeSet,
    )> {
        crate::starmap::rename_starmap_with_changes(&self.app_data_root, starmap_id, new_title)
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn delete_starmap(&self, starmap_id: &str) -> Result<()> {
        // Fix 6: 引用扫描前必须 flush 所有 dirty starmap stores，否则
        // find_starmap_references 读到的磁盘数据可能不含刚写入的引用，
        // 导致误删。先 flush 全部，再移除待删 store，最后落盘删除。
        self.flush_all_starmap_stores()?;
        {
            let mut stores = self
                .starmap_stores
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            stores.remove(starmap_id);
        }
        crate::starmap::delete_starmap(&self.app_data_root, starmap_id)
    }

    ///   delete_starmap 的变更集版本。
    pub fn delete_starmap_with_changes(
        &self,
        starmap_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        // 先 flush 全部 dirty stores（与 delete_starmap 同样的前置逻辑），
        // 再移除缓存并落盘删除。
        self.flush_all_starmap_stores()?;
        self.remove_starmap_store(starmap_id);
        crate::starmap::delete_starmap_with_changes(&self.app_data_root, starmap_id)
    }

    /// 从缓存中移除指定 starmap store（内部 helper）。
    fn remove_starmap_store(&self, starmap_id: &str) {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        stores.remove(starmap_id);
    }

    pub fn bind_starmap_to_project(&self, starmap_id: &str, project_id: &str) -> Result<()> {
        let _ =
            crate::starmap::bind_starmap_to_project(&self.app_data_root, starmap_id, project_id)?;
        Ok(())
    }

    ///   bind_starmap_to_project 的变更集版本。
    pub fn bind_starmap_to_project_with_changes(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        crate::starmap::bind_starmap_to_project_with_changes(
            &self.app_data_root,
            starmap_id,
            project_id,
        )
    }

    pub fn set_main_starmap_for_project(&self, starmap_id: &str, project_id: &str) -> Result<()> {
        let _ = crate::starmap::set_main_starmap_for_project(
            &self.app_data_root,
            starmap_id,
            project_id,
        )?;
        Ok(())
    }

    ///   set_main_starmap_for_project 的变更集版本。
    pub fn set_main_starmap_for_project_with_changes(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        crate::starmap::set_main_starmap_for_project_with_changes(
            &self.app_data_root,
            starmap_id,
            project_id,
        )
    }

    pub fn get_main_starmap_for_project(
        &self,
        project_id: &str,
    ) -> Result<Option<crate::starmap::StarMapMeta>> {
        crate::starmap::get_main_starmap_for_project(&self.app_data_root, project_id)
    }

    pub fn unbind_starmap_from_project(&self, starmap_id: &str) -> Result<()> {
        let _ = crate::starmap::unbind_starmap_from_project(&self.app_data_root, starmap_id)?;
        Ok(())
    }

    ///   unbind_starmap_from_project 的变更集版本。
    pub fn unbind_starmap_from_project_with_changes(
        &self,
        starmap_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        crate::starmap::unbind_starmap_from_project_with_changes(&self.app_data_root, starmap_id)
    }

    pub fn close_starmap_store(&self, starmap_id: &str) -> Result<Vec<std::path::PathBuf>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get_mut(starmap_id) {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                let paths = store.flush()?;
                stores.remove(starmap_id);
                return Ok(paths);
            }
        }
        stores.remove(starmap_id);
        Ok(Vec::new())
    }

    pub fn flush_starmap_store(&self, starmap_id: &str) -> Result<Vec<std::path::PathBuf>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get_mut(starmap_id) {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                return store.flush();
            }
        }
        Ok(Vec::new())
    }

    pub fn flush_all_starmap_stores(&self) -> Result<Vec<std::path::PathBuf>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut all_changed: Vec<std::path::PathBuf> = Vec::new();
        for store in stores.values_mut() {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                let paths = store.flush()?;
                all_changed.extend(paths);
            }
        }
        Ok(all_changed)
    }

    /// 从 stores 中获取 store 的不可变引用，若不存在则返回错误。
    /// 用于 add/update 函数中在 entry().or_insert_with() 之后安全获取 store。
    fn get_store_or_err<'a>(
        stores: &'a std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
    ) -> Result<&'a StarMapStore> {
        stores.get(starmap_id).ok_or_else(|| {
            crate::error::Error::Other(format!(
                "internal error: store not found for starmap_id: {}",
                starmap_id
            ))
        })
    }

    /// 从 stores 中获取 store 的可变引用，若不存在则返回错误。
    /// 用于 add/update 函数中在验证通过后安全获取 store 做实际修改。
    fn get_store_mut_or_err<'a>(
        stores: &'a mut std::collections::HashMap<String, StarMapStore>,
        starmap_id: &str,
    ) -> Result<&'a mut StarMapStore> {
        stores.get_mut(starmap_id).ok_or_else(|| {
            crate::error::Error::Other(format!(
                "internal error: store not found for starmap_id: {}",
                starmap_id
            ))
        })
    }

    /// 构建一个 `GraphResolverContext`，包含当前所有已完成后台全量加载的
    /// Store 的 `to_starmap_graph()` 快照作为 overlays。这样 resolver 在
    /// 校验 candidate graph 时可以看到其他星图内存中尚未 flush 的变更。
    ///
    /// 只把 `current_load_phase == Some(LoadPhase::BackgroundFullLoad)` 的
    /// Store 放进 overlays，避免部分加载的 Store 导致误报 MissingNode/
    /// MissingEmbed。candidate 永远单独放进去（不管其 phase）。
    ///
    /// 调用方必须在持有 `starmap_stores` 锁的上下文中调用此方法，
    /// 传入已获取的 stores 引用，避免重复 lock 导致死锁。
    fn build_resolver_context_from_stores(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        candidate: &crate::starmap::types::StarMapGraph,
    ) -> GraphResolverContext {
        let mut overlays = std::collections::HashMap::new();
        // 只放入已完成后台全量加载的 Store 的图快照，
        // 部分加载的 Store 可能缺少节点/嵌入，放入会导致误报。
        for (id, store) in stores.iter() {
            if store.current_load_phase() == Some(LoadPhase::BackgroundFullLoad) {
                overlays.insert(id.clone(), store.to_starmap_graph());
            }
        }
        // 最后用 candidate graph 覆盖对应 starmap_id 的 overlay，
        // 确保 candidate 的最新变更优先于 Store 快照。
        overlays.insert(candidate.starmap_id.clone(), candidate.clone());
        GraphResolverContext {
            app_data_root: self.app_data_root.clone(),
            overlays,
        }
    }
}
