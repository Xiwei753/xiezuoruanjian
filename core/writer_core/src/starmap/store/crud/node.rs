use crate::error::Result;
use crate::starmap::store::relation_index::{CascadeIds, CascadeOrigin};
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    pub fn upsert_node(&mut self, node: StarMapNode) {
        let node_id = node.id.clone();
        // Issue #1062 评论 #7236446138 第 2 项：显式新建（本入口的创建路径）
        // 是唯一允许从读模型建立共享本体的业务入口。已有画布实例缺本体是
        // "本体尚未到达"的占位视图，不能反推成真实共享素材。
        self.create_shared_definition_for_new_node(&node);
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

    /// 提交前把实例刷新为磁盘上的持久版本，并检测外部更新。
    ///
    /// 同步或另一个进程可能已经改过坐标：
    /// - 未 dirty 的实例直接以磁盘记录替换缓存，后续 expected 校验读到的就是
    ///   持久 revision，而不是缓存里的旧值；
    /// - dirty 的实例保留内存版本，但磁盘 revision 更新时返回显式冲突，
    ///   绝不能让下一次 flush 静默覆盖外部已落盘的坐标。
    /// 新对象尚未落盘时没有持久 revision，不构成冲突。
    fn refresh_instance_for_write(&mut self, instance_id: &str) -> Result<()> {
        let instance_rel =
            crate::starmap::package_storage::node_rel_path(&self.starmap_id, instance_id);
        if !self.app_data_root.join(&instance_rel).is_file() {
            // 新对象尚未落盘：没有持久 revision 可冲突。
            return Ok(());
        }
        let current_revision = self.nodes.get(instance_id).map(|node| node.updated_at);
        match self.try_load_node(instance_id) {
            Ok(persisted) => {
                if self.dirty_nodes.contains(instance_id) {
                    let current = current_revision.unwrap_or(0);
                    if persisted.updated_at > current {
                        return Err(revision_conflict(
                            "instance",
                            instance_id,
                            current,
                            persisted.updated_at,
                        ));
                    }
                } else {
                    self.nodes.insert(instance_id.to_string(), persisted);
                }
                Ok(())
            }
            Err(crate::error::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// 落盘前校验对象写入的两个 expected revision。
    ///
    /// 定义 revision 直接读素材本体文件；实例 revision 以磁盘实例文件为准
    /// （未 dirty 时先刷新缓存）。任何一项不匹配都在写盘前返回冲突，避免
    /// 定义已写入、实例校验才失败的半提交。
    pub fn check_node_write_revisions(
        &mut self,
        instance_id: &str,
        expected_definition_revision: Option<u64>,
        expected_instance_revision: Option<u64>,
    ) -> Result<()> {
        if !self.nodes.contains_key(instance_id) {
            self.ensure_object_loaded(instance_id)?;
        }
        self.refresh_instance_for_write(instance_id)?;
        if let Some(expected) = expected_definition_revision {
            let asset_id = self.asset_id_for_node(instance_id);
            let current =
                crate::starmap::library::load_node_definition(&self.app_data_root, &asset_id)?
                    .map(|definition| definition.revision())
                    .unwrap_or(0);
            if current != expected {
                return Err(revision_conflict(
                    "definition",
                    &asset_id,
                    expected,
                    current,
                ));
            }
        }
        if let Some(expected) = expected_instance_revision {
            let current = self
                .nodes
                .get(instance_id)
                .ok_or_else(node_not_found)?
                .updated_at;
            if current != expected {
                return Err(revision_conflict(
                    "instance",
                    instance_id,
                    expected,
                    current,
                ));
            }
        }
        Ok(())
    }

    /// 应用实例补丁：只改实例事实，并单调推进实例 revision。
    ///
    /// Issue #1062 评论 #7236446138 第 2 项：本方法只写
    /// [`crate::starmap::types::StarMapNodeInstance`]（位置等实例事实）。
    /// `dirty_nodes` 的语义只覆盖"实例自身变化"，不再隐含"需要补写共享
    /// definition"——即使读模型是 [`NodeDefinitionState::ProvisionalMissingDefinition`]
    /// 的占位视图，移动节点也只落实例文件，不会让 flush 把占位内容上推成
    /// 素材库里的真实共享本体。
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
        // 未 dirty 的实例以磁盘持久 revision 为准，不能拿缓存里的旧版本
        // 通过 expected 校验后覆盖外部已落盘的新坐标。
        self.refresh_instance_for_write(instance_id)?;
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

    /// 当前节点读模型的共享本体到达状态（只读；不写任何文件）。
    ///
    /// Issue #1062 评论 #7236446138 第 2 项：状态按权威材料显式给出，不用
    /// `title`/`kind`/`content` 猜测：
    ///
    /// - 素材库里有该 `asset_id` 的本体 → [`NodeDefinitionState::Resolved`]；
    /// - 没有本体、但画布实例文件已落盘 → 实例先于本体存在，
    ///   [`NodeDefinitionState::ProvisionalMissingDefinition`]；
    /// - 没有本体、画布实例也还没有文件 → 本地新建，
    ///   [`NodeDefinitionState::LocalNew`]（本轮 upsert 的显式创建路径）。
    ///
    /// 状态只属于读模型，不进入节点 JSON / 星图快照 / 同步 manifest。
    pub fn node_definition_state(&self, instance_id: &str) -> NodeDefinitionState {
        let asset_id = self
            .nodes
            .get(instance_id)
            .map(|node| node.resolved_asset_id().to_string())
            .unwrap_or_else(|| self.asset_id_for_node(instance_id));
        if self.shared_node_definition_exists(&asset_id) {
            return NodeDefinitionState::Resolved;
        }
        if self.persisted_node_instance_exists(instance_id) {
            NodeDefinitionState::ProvisionalMissingDefinition
        } else {
            NodeDefinitionState::LocalNew
        }
    }

    /// 读取节点读模型并同时给出共享本体到达状态。
    pub fn current_node_read(&self, instance_id: &str) -> Result<StarMapNodeRead> {
        let node = self
            .nodes
            .get(instance_id)
            .cloned()
            .ok_or_else(node_not_found)?;
        let definition_state = self.node_definition_state(instance_id);
        Ok(StarMapNodeRead::new(node, definition_state))
    }

    /// 共享本体到达后按 `asset_id` 重新解析实例视图。
    ///
    /// 只把真实本体内容应用到内存读模型，保留实例位置与实例 revision；
    /// 不写回素材库、不产生任何上推，占位视图因此直接变回
    /// [`NodeDefinitionState::Resolved`]。
    pub fn refresh_node_definition(&mut self, instance_id: &str) -> Result<StarMapNode> {
        if !self.nodes.contains_key(instance_id) {
            self.ensure_object_loaded(instance_id)?;
        }
        let asset_id = self
            .nodes
            .get(instance_id)
            .map(|node| node.resolved_asset_id().to_string())
            .unwrap_or_else(|| self.asset_id_for_node(instance_id));
        if let Some(definition) =
            crate::starmap::library::load_node_definition(&self.app_data_root, &asset_id)?
        {
            if let Some(node) = self.nodes.get_mut(instance_id) {
                definition.apply_to_node(node);
            }
        }
        self.current_node_view(instance_id)
    }

    /// 素材库里是否已有该 `asset_id` 的共享本体。
    fn shared_node_definition_exists(&self, asset_id: &str) -> bool {
        matches!(
            crate::starmap::library::load_node_definition(&self.app_data_root, asset_id),
            Ok(Some(_))
        )
    }

    /// 画布实例文件是否已落盘（本地新建、尚未 flush 时为 `false`）。
    fn persisted_node_instance_exists(&self, instance_id: &str) -> bool {
        self.app_data_root
            .join(crate::starmap::package_storage::node_rel_path(
                &self.starmap_id,
                instance_id,
            ))
            .is_file()
    }

    /// 显式创建入口：为本地新建节点建立共享本体。
    ///
    /// 只处理"画布实例尚未落盘 + 素材库没有同 `asset_id` 本体"的新节点。
    /// 已有实例缺本体属于"本体尚未到达"（旧数据/跨端未同步），此时读模型是
    /// `title = asset_id`、`kind = Note`、内容为空的占位视图，把它升级成真实
    /// 共享素材会污染跨画布复用与 LWW 判定，因此直接跳过。
    ///
    /// 本体已存在时不覆盖：共享内容的后续编辑归 `library` 的独立事务。
    /// 落盘失败只记恢复诊断，不伪造成功；节点仍留在 `dirty_nodes` 里，
    /// 实例文件照常按实例事实写入，下次显式 upsert 会重试本体创建。
    fn create_shared_definition_for_new_node(&mut self, node: &StarMapNode) {
        let asset_id = node.resolved_asset_id().to_string();
        match crate::starmap::library::load_node_definition(&self.app_data_root, &asset_id) {
            Ok(Some(_)) => return,
            Ok(None) => {}
            Err(error) => {
                self.record_definition_create_failure(&node.id, &error);
                return;
            }
        }
        if self.persisted_node_instance_exists(&node.id) {
            return;
        }
        let definition = StarMapNodeDefinition::from_node(asset_id, node);
        if let Err(error) =
            crate::starmap::library::save_node_definition(&self.app_data_root, &definition)
        {
            self.record_definition_create_failure(&node.id, &error);
        }
    }

    fn record_definition_create_failure(&mut self, instance_id: &str, error: &crate::error::Error) {
        log::error!(
            "starmap explicit node definition create failed: starmap_id={} instance_id={}: {:?}",
            self.starmap_id,
            instance_id,
            error
        );
        self.recovery_log
            .push(crate::starmap::store::types::LoadDiagnostic {
                kind: crate::starmap::store::types::LoadDiagnosticKind::Corrupt,
                object_type: "node definition".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("explicit node definition create failed: {error:?}"),
            });
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

    pub fn delete_node(
        &mut self,
        node_id: &str,
    ) -> Result<crate::starmap::store::DeletedStarmapObjects> {
        if !self.nodes.contains_key(node_id) {
            self.ensure_object_loaded(node_id)?;
        }
        if !self.nodes.contains_key(node_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            )));
        }

        // 级联闭包依赖链接投影：先按仓库版本刷新，避免用过期投影漏删/误删。
        self.refresh_link_projection()?;

        // 统一级联闭包规划：循环扩展被删 Embed 的下一层级联，直到不动点。
        // 旧实现只做一次级联（node_cascade_ids），会遗漏传递级联——
        // 删 node n 级联删 embed e，但 e 的删除又应级联删以其为第一段
        // EnterEmbed 的其他 embed/edge，旧实现不会继续扩展。
        // 级联 ID 计算复用 `cascade_closure_plan` 纯函数，candidate 模拟删除
        // 也用同一函数，避免两套逻辑漂移。
        let origin = CascadeOrigin::Node(node_id.to_string());
        let cascade = self.cascade_closure_plan(&origin);
        let deleted = crate::starmap::store::DeletedStarmapObjects::from_cascade(&origin, &cascade);

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
            self.delete_link_projected(lid)?;
        }
        // 本批 tombstone 已写仓库：失效版本，下一次读取按仓库重建投影。
        self.invalidate_link_projection_version();

        Ok(deleted)
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
