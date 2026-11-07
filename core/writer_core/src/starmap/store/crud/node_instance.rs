//! 画布节点实例 CRUD：实例身份、位置与共享本体的解析。
//!
//! 画布文件只保存 `StarMapNodeInstance`；共享内容永远写
//! `starmaps/library/nodes/<bucket>/<asset_id>.json`。

use crate::error::Result;
use crate::starmap::types::*;

use super::super::StarMapStore;

impl StarMapStore {
    /// 新增一个画布实例，复用共享本体并登记反向引用。
    pub fn create_node_instance(
        &mut self,
        instance_id: &str,
        definition: &StarMapNodeDefinition,
        position: StarMapPoint,
        project_id: Option<&str>,
    ) -> Result<StarMapNode> {
        let node = definition.instantiate(instance_id, position);
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
    ) -> Result<StarMapNode> {
        let patch = StarMapNodePatch {
            title: None,
            kind: None,
            payload: None,
            tags: None,
            content: None,
            anchors: None,
            portal: None,
            position: Some(position),
            style: None,
            provenance: None,
        };
        self.update_node(instance_id, &patch)
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
