use crate::error::Result;
use crate::starmap::graph::path_planner::CrossLayerConnectionPlan;
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

    /// 列出一级（根）星图。
    ///
    /// 直接读 index 里显式持久化的 `root_starmap_ids`，再取对应 meta：
    /// 不再为了列一级页加载全部 `StarMapStore` 扫描 Embed 反推根身份。
    /// 根/嵌套身份在创建入口写定（普通新建 → root，嵌套子星图 → 只进
    /// starmap_ids），一级列表不需要图数据参与。
    pub fn list_root_starmaps(&self) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_root_starmaps(&self.app_data_root)
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

    pub fn create_starmap_with_id_with_changes(
        &self,
        starmap_id: &str,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::storage::workspace_git::WorkspaceChangeSet,
    )> {
        crate::starmap::create_starmap_with_id_with_changes(
            &self.app_data_root,
            starmap_id,
            title,
            description,
            accent_color,
        )
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

    /// 从缓存中移除指定 starmap store（内部 helper）。
    pub(crate) fn remove_starmap_store(&self, starmap_id: &str) {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        stores.remove(starmap_id);
        // 整图从缓存移除后，持久化索引可能仍持有该图的依赖事实（owner 或 host
        // 指向已移除的图）。失效索引以避免下次查询读到过期 entry（问题1）。
        // 锁顺序：先 starmap_stores 后 path_dependency_index，与全仓库一致。
        self.invalidate_path_dependency_index();
    }

    /// 同步 commit 后 evict Store：只移除缓存，不 flush dirty。
    ///
    /// commit 前已通过 `flush_all_starmap_stores` 把 dirty 写入 live 磁盘，
    /// 此处只需移除 Store 缓存，下次访问从 live 磁盘重新加载合并后的结果。
    /// 不再 flush dirty 避免覆盖刚提交的远端内容（问题1）。
    pub(crate) fn evict_starmap_store_after_commit(&self, starmap_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        stores.remove(starmap_id);
        Ok(())
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

    pub fn pending_starmap_flush_paths(&self, starmap_id: &str) -> Vec<std::path::PathBuf> {
        let stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        stores
            .get(starmap_id)
            .map(|store| {
                store
                    .pending_flush_paths()
                    .into_iter()
                    .map(|path| self.app_data_root.join(path))
                    .collect()
            })
            .unwrap_or_default()
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

    /// 按 `root starmap + 路径段` 解析当前层星图。
    ///
    /// 这是"同一画布里的无级星图套星图"的当前层加载入口：层级身份由
    /// root starmap 与逐段穿越（`EnterEmbed` / `EnterPortal`）决定，
    /// 而不是由界面点击事件传来的裸目标 ID 决定。解析失败时返回 `Err`，
    /// 调用方不得回退到裸 target id 继续加载。
    ///
    /// 起点星图先 `ensure_fully_loaded`，这样它尚未 flush 的 Embed/Portal
    /// 变更会作为 overlay 进入 resolver；其余已全量加载的 Store 同样作为
    /// overlay，避免读到旧磁盘状态。
    pub fn resolve_starmap_path(
        &self,
        root_starmap_id: &str,
        segments: Vec<crate::starmap::types::reference::StarMapPathSegment>,
    ) -> Result<crate::starmap::graph::resolve::ResolvedTarget> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(root_starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, root_starmap_id));
            store.ensure_fully_loaded()?;
        }

        let context = self.build_resolver_context(&stores);
        // 目标固定为 Starmap：下钻只要求"路径逐段可达，并落在最终星图"，
        // 不额外要求落到具体节点/锚点。
        let path = crate::starmap::types::reference::StarMapTargetPath {
            starmap_id: root_starmap_id.to_string(),
            segments,
            target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
        };
        crate::starmap::graph::resolve::resolve_target(&context, &path)
            .map_err(|status| crate::error::Error::Other(format!("星图路径解析失败: {status:?}")))
    }

    /// 规划跨层连线应当落在哪张图上（Core 唯一事实来源）。
    ///
    /// 两端必须是**同一条根星图**的绝对端点路径（`starmap_id` 相同且非空），
    /// `segments` 是从根星图逐层穿越到端点所在 Scene 的绝对路径。先确保根星图
    /// Store 完全加载，再构建 resolver context（含其它已全量加载 Store 的
    /// overlay），交给 `path_planner::plan_cross_layer_connection` 统一算宿主。
    pub fn plan_cross_layer_connection(
        &self,
        from: &crate::starmap::types::reference::StarMapTargetPath,
        to: &crate::starmap::types::reference::StarMapTargetPath,
    ) -> Result<CrossLayerConnectionPlan> {
        if from.starmap_id.is_empty() || from.starmap_id != to.starmap_id {
            return Err(crate::error::Error::Other(
                "plan_cross_layer_connection: endpoints must share the same root star map"
                    .to_string(),
            ));
        }

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(from.starmap_id.clone())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, &from.starmap_id));
            store.ensure_fully_loaded()?;
        }

        let context = self.build_resolver_context(&stores);
        crate::starmap::graph::path_planner::plan_cross_layer_connection(&context, from, to)
            .map_err(|e| crate::error::Error::Other(format!("plan_cross_layer_connection: {e:?}")))
    }

    /// 构建一个 `GraphResolverContext`，包含当前所有已完成后台全量加载的
    /// Store 的 `to_starmap_graph()` 快照作为 overlays。这样 resolver
    /// 可以看到其他星图内存中尚未 flush 的变更。
    ///
    /// 只把 `current_load_phase == Some(LoadPhase::BackgroundFullLoad)` 的
    /// Store 放进 overlays，避免部分加载的 Store 导致误报 MissingNode/
    /// MissingEmbed。
    ///
    /// 调用方必须在持有 `starmap_stores` 锁的上下文中调用此方法，
    /// 传入已获取的 stores 引用，避免重复 lock 导致死锁。
    fn build_resolver_context(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
    ) -> GraphResolverContext {
        let mut overlays = std::collections::HashMap::new();
        // 只放入已完成后台全量加载的 Store 的图快照，
        // 部分加载的 Store 可能缺少节点/嵌入，放入会导致误报。
        for (id, store) in stores.iter() {
            if store.current_load_phase() == Some(LoadPhase::BackgroundFullLoad) {
                overlays.insert(id.clone(), store.to_starmap_graph());
            }
        }
        GraphResolverContext {
            app_data_root: self.app_data_root.clone(),
            overlays,
        }
    }

    /// 构建 candidate 校验用的 `GraphResolverContext`：在通用 overlays 之上
    /// 用 candidate graph 覆盖其 starmap_id 的 overlay，确保 candidate 的
    /// 最新变更优先于 Store 快照。
    fn build_resolver_context_from_stores(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
        candidate: &crate::starmap::types::StarMapGraph,
    ) -> GraphResolverContext {
        let mut context = self.build_resolver_context(stores);
        context
            .overlays
            .insert(candidate.starmap_id.clone(), candidate.clone());
        context
    }

    /// 获取或惰性构建持久化路径依赖索引。
    ///
    /// 如果索引为 None（未初始化），从真实星图数据 `PathDependencyIndex::build`。
    /// 首次访问后复用，Edge/Link/Hyperlink/Embed/Portal 增删改后通过
    /// `add_or_replace_source` / `remove_source` 增量更新，不再每次从零扫描全库。
    ///
    /// 调用方必须在持有 `starmap_stores` 锁的上下文中调用此方法，
    /// 传入已获取的 stores 引用，避免重复 lock 导致死锁。
    /// 返回的 `MutexGuard` 保证内部为 `Some`（build 成功时）。
    /// 读取错误（磁盘 IO、schema 不兼容等）通过 `?` 向上传播。
    fn get_or_init_path_dependency_index(
        &self,
        stores: &std::collections::HashMap<String, StarMapStore>,
    ) -> Result<std::sync::MutexGuard<'_, Option<crate::starmap::store::PathDependencyIndex>>> {
        let mut guard = self
            .path_dependency_index
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            let context = self.build_resolver_context(stores);
            let idx = crate::starmap::load_index(&self.app_data_root)?;
            *guard = Some(crate::starmap::store::PathDependencyIndex::build(
                &context,
                &idx.starmap_ids,
            )?);
        }
        Ok(guard)
    }

    /// 增量更新持久化路径依赖索引：移除指定来源的旧依赖事实。
    ///
    /// 删除 Edge/Link/Hyperlink/Embed 后调用，从索引中移除该来源产生的全部 entry。
    /// 调用方必须在持有 `starmap_stores` 锁的上下文中调用。
    fn remove_source_from_path_dependency_index(
        &self,
        owner_starmap_id: &str,
        source_kind: &crate::starmap::store::relation_index::CrossCanvasRefSourceKind,
        source_id: &str,
    ) {
        let mut guard = self
            .path_dependency_index
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut index) = *guard {
            index.remove_source(owner_starmap_id, source_kind, source_id);
        }
        // 如果索引为 None（未初始化），无需移除——下次惰性构建时会从真实数据重建。
    }

    /// 失效持久化路径依赖索引，下次查询时从真实数据惰性重建。
    ///
    /// Node 的 Portal destination 或 Anchor 变更会改变路径解析结果，影响所有穿越
    /// 该 Portal 或指向该 Anchor 的路径的依赖事实。精确增量更新需要找到所有受影响
    /// 的来源并重新计算，复杂度高；此处采用失效重建策略——设为 None，下次查询时
    /// 从真实数据 build。仍避免了"每次查询都 build"的问题：只有 Portal/Anchor
    /// 变更才失效，title/position/style 等不影响路径解析的变更不触发。
    ///
    /// `pub(crate)` 暴露给 api/service 层：整图物理删除（`delete_starmap`）、同步、
    /// 恢复等批量改变磁盘数据的操作后调用，确保索引不持有已失效的 entry（问题1）。
    pub(crate) fn invalidate_path_dependency_index(&self) {
        let mut guard = self
            .path_dependency_index
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    /// 两阶段更新的第二阶段：用预计算的依赖事实替换索引中该来源的全部 entry。
    ///
    /// 接收由 `extract_path_dependency_facts` 预先计算好的 facts，只做不可失败的
    /// 内存替换（[`PathDependencyIndex::replace_source_facts`]）。调用方在写 Store
    /// **之前**已 prepare 好事实，写 Store 成功后调本方法；prepare 失败时不写
    /// Store，索引和 Store 都不变（问题2）。
    ///
    /// 调用方必须在持有 `starmap_stores` 锁的上下文中调用。
    fn replace_source_facts_in_path_dependency_index(
        &self,
        owner_starmap_id: &str,
        source_kind: crate::starmap::store::relation_index::CrossCanvasRefSourceKind,
        source_id: &str,
        path: &crate::starmap::types::reference::StarMapTargetPath,
        label: &Option<String>,
        facts: Vec<crate::starmap::graph::resolve::PathDependencyFact>,
    ) {
        let mut guard = self
            .path_dependency_index
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut index) = *guard {
            index.replace_source_facts(
                owner_starmap_id,
                source_kind,
                source_id,
                path,
                label,
                facts,
            );
        }
        // 如果索引为 None（未初始化），无需更新——下次惰性构建时会从真实数据重建。
    }

    /// 基于持久化路径依赖索引查询与特定画布相关的所有连接（问题4）。
    ///
    /// 封装 `get_or_init_path_dependency_index` + `query_connections_for_canvas`，
    /// 供 api/service 层的 `list_starmap_connections`（target=None 分支）调用，
    /// 替代逐 Edge 加载所有祖先图的旧扫描实现。
    ///
    /// 先确保 root starmap 完全加载，再构建 resolver context 并获取/初始化索引，
    /// 最后调用 [`PathDependencyIndex::query_connections_for_canvas`] 从索引获取
    /// 候选 Edge 来源，只加载相关祖先图的边文件。
    ///
    /// 读取错误通过 `?` 向上传播，不吞成"没有连接"。
    pub(crate) fn query_starmap_connections_for_canvas(
        &self,
        route: &crate::starmap::navigation::StarMapNavigationRoute,
        root_starmap_id: &str,
    ) -> crate::error::Result<Vec<crate::starmap::connections::StarMapConnectionInfo>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(root_starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, root_starmap_id));
            store.ensure_fully_loaded()?;
        }
        let context = self.build_resolver_context(&stores);
        let index_guard = self.get_or_init_path_dependency_index(&stores)?;
        let index = index_guard.as_ref().ok_or_else(|| {
            crate::error::Error::Other("path dependency index initialization failed".to_string())
        })?;
        index.query_connections_for_canvas(route, &context)
    }

    /// 按终点 target 查询指向该 target 的所有连接（持久化索引版）。
    ///
    /// 替代逐 Edge 加载所有祖先图的旧扫描实现。
    /// 先确保 root starmap 完全加载，再构建 resolver context 并获取/初始化索引，
    /// 最后调用 [`PathDependencyIndex::query_connections_for_target`] 从索引获取
    /// 候选 Edge 来源，只加载相关祖先图的边文件。
    ///
    /// 读取错误通过 `?` 向上传播，不吞成"没有连接"。
    pub(crate) fn query_starmap_connections_for_target(
        &self,
        route: &crate::starmap::navigation::StarMapNavigationRoute,
        target: &crate::starmap::semantic::StarMapTargetDetail,
        root_starmap_id: &str,
    ) -> crate::error::Result<Vec<crate::starmap::connections::StarMapConnectionInfo>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        {
            let store = stores
                .entry(root_starmap_id.to_string())
                .or_insert_with(|| StarMapStore::new(&self.app_data_root, root_starmap_id));
            store.ensure_fully_loaded()?;
        }
        let context = self.build_resolver_context(&stores);
        let index_guard = self.get_or_init_path_dependency_index(&stores)?;
        let index = index_guard.as_ref().ok_or_else(|| {
            crate::error::Error::Other("path dependency index initialization failed".to_string())
        })?;
        index.query_connections_for_target(route, target, &context)
    }
}

#[cfg(test)]
mod tests;
