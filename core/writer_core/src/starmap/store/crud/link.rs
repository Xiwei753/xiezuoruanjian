//! 星图导航链接 CRUD（Issue #1060 第 3 节）。
//!
//! 链接的唯一持久化所有者是 annotations 仓库；`StarMapStore.links` 只是按
//! 画布过滤的读投影。add/update/delete 直接写仓库 tombstone/记录，不再产生
//! `starmaps/<id>/links/*.json`、写队列或链接索引所有权。

use crate::annotations::{AnnotationMutation, LinkTarget};
use crate::error::{Error, Result};
use crate::starmap::types::*;

use super::super::StarMapStore;

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn link_not_found() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Link not found",
    ))
}

impl StarMapStore {
    /// 只改内存投影（级联删除外的调用方不得依赖它持久化）。
    pub fn upsert_link(&mut self, link: StarMapLink) {
        let link_id = link.link_id.clone();
        self.links.insert(link_id.clone(), link);
        self.deleted_link_ids.remove(&link_id);
        self.dirty_graph_meta = true;
    }

    /// 只改内存投影。级联删除必须走 [`StarMapStore::delete_link`] 写仓库 tombstone。
    pub fn remove_link(&mut self, link_id: &str) {
        self.links.remove(link_id);
        self.deleted_link_ids.insert(link_id.to_string());
        self.dirty_graph_meta = true;
    }

    pub fn add_link(&mut self, link: StarMapLink) -> Result<StarMapLink> {
        if self.links.contains_key(&link.link_id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate link_id",
            )));
        }
        let object_link = self.star_map_link_to_object_link(&link)?;
        let store = self.annotation_store_for_canvas()?;
        let (stored, _changed) = store.attach_link(object_link)?;
        let projected = self.object_link_to_star_map_link(&stored).ok_or_else(|| {
            Error::Other("annotations link is not a canvas navigation link".to_string())
        })?;
        // 同一 (source,target) 已有链接时仓库幂等返回现有记录：投影跟随
        // 仓库里真正持久化的 link_id，不保留请求里未落盘的第二个 ID。
        self.links
            .insert(projected.link_id.clone(), projected.clone());
        self.deleted_link_ids.remove(&projected.link_id);
        self.dirty_graph_meta = true;
        Ok(projected)
    }

    pub fn update_link(&mut self, link_id: &str, patch: &StarMapLinkPatch) -> Result<StarMapLink> {
        if !self.links.contains_key(link_id) {
            self.ensure_link_loaded(link_id)?;
        }
        let store = self.annotation_store_for_canvas()?;
        let mut object_link = match store.get_link(link_id)? {
            Some(link) if link.deleted_at_ms.is_none() => link,
            // 仓库里没有活动记录（未迁移 / 已被 tombstone）：用投影转换兜底，
            // 保证旧画布上的既有链接仍可编辑。
            _ => {
                let projected = self.links.get(link_id).ok_or_else(link_not_found)?;
                self.star_map_link_to_object_link(projected)?
            }
        };
        if let Some(ref source) = patch.source {
            object_link.source = self.source_path_to_object_ref(source)?;
        }
        if let Some(ref target) = patch.target {
            object_link.target = LinkTarget::CanvasNavigation(target.clone());
        }
        if let Some(ref label) = patch.label {
            object_link.label = label.clone();
        }
        object_link.updated_at_ms = now_millis().max(object_link.updated_at_ms.saturating_add(1));
        object_link.deleted_at_ms = None;
        store.apply_mutation(AnnotationMutation::UpsertLink(object_link.clone()))?;
        let projected = self
            .object_link_to_star_map_link(&object_link)
            .ok_or_else(link_not_found)?;
        self.links
            .insert(projected.link_id.clone(), projected.clone());
        self.deleted_link_ids.remove(&projected.link_id);
        self.dirty_graph_meta = true;
        Ok(projected)
    }

    pub fn delete_link(&mut self, link_id: &str) -> Result<()> {
        if !self.links.contains_key(link_id) {
            self.ensure_link_loaded(link_id)?;
        }
        let link = self
            .links
            .get(link_id)
            .cloned()
            .ok_or_else(link_not_found)?;
        match self.star_map_link_to_object_link(&link) {
            Ok(object_link) => {
                let store = self.annotation_store_for_canvas()?;
                store.detach_link(&object_link.source, &object_link.target, now_millis())?;
            }
            Err(_) => {
                // 源不属于本画布的链接只可能来自旧的脏投影：注解仓库里没有
                // 它的记录，删除只需清掉投影。合法投影的源一定归本画布所有。
            }
        }
        self.links.remove(link_id);
        // 保留 deleted_link_ids 供快照 tombstone；链接对象文件已不存在，
        // 不再进入对象删除 journal / flush 删除分支。
        self.deleted_link_ids.insert(link_id.to_string());
        self.dirty_graph_meta = true;
        Ok(())
    }
}
