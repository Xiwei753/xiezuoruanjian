//! 画布节点实例 CRUD：实例身份、位置与共享本体的解析。
//!
//! 画布文件只保存 `StarMapNodeInstance`；共享内容永远写
//! `starmaps/library/nodes/<bucket>/<asset_id>.json`。

use crate::error::Result;
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    /// 新增一个画布实例，复用共享本体并登记反向引用。
    ///
    /// 实例创建时间与实例 revision 属于画布，不继承素材本体的时间戳。
    pub fn create_node_instance(
        &mut self,
        instance_id: &str,
        definition: &StarMapNodeDefinition,
        position: StarMapPoint,
        project_id: Option<&str>,
    ) -> Result<StarMapNode> {
        let now = crate::starmap::now_epoch().max(1);
        let instance = StarMapNodeInstance {
            instance_id: instance_id.to_string(),
            asset_id: definition.asset_id.clone(),
            position,
            starmap_id: self.starmap_id.clone(),
            project_id: project_id.map(str::to_string),
            created_at: now,
            updated_at: now,
        };
        let node = definition.instantiate_instance(&instance);
        self.set_node_asset(instance_id, &definition.asset_id);
        crate::starmap::library::record_node_reference(
            &self.app_data_root,
            &self.starmap_id,
            project_id,
            instance_id,
            &definition.asset_id,
        )?;
        Ok(self.add_node(node))
    }

    /// 只移动当前画布实例；不触碰共享定义或其它画布。
    pub fn move_node_instance(
        &mut self,
        instance_id: &str,
        position: StarMapPoint,
        expected_instance_revision: Option<u64>,
    ) -> Result<StarMapNode> {
        self.update_node_instance(
            instance_id,
            &NodeInstancePatch {
                position: Some(position),
            },
            expected_instance_revision,
        )
    }

    /// 从当前画布移除实例引用；共享本体和库归属保持不变。
    pub fn unlink_node_instance(&mut self, instance_id: &str) -> Result<()> {
        self.remove_node(instance_id);
        self.node_assets.remove(instance_id);
        crate::starmap::library::remove_instance_records(
            &self.app_data_root,
            &self.starmap_id,
            &[instance_id.to_string()],
        )
    }

    /// 解析实例当前指向的共享本体 ID。
    pub fn resolve_node_instance(&self, instance_id: &str) -> String {
        self.asset_id_for_node(instance_id)
    }
}
