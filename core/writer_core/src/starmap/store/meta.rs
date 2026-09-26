use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::relation_index::{
    EdgeRelationIndex, EmbedHostIndex, HyperlinkRelationIndex, LinkRelationIndex,
};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMeta {
    pub schema_version: String,
    pub starmap_id: String,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub embed_instance_ids: Vec<String>,
    pub link_ids: Vec<String>,
    pub hyperlink_ids: Vec<String>,
    #[serde(default)]
    pub edge_relation_index: Vec<EdgeRelationIndex>,
    #[serde(default)]
    pub embed_host_index: Vec<EmbedHostIndex>,
    #[serde(default)]
    pub link_relation_index: Vec<LinkRelationIndex>,
    #[serde(default)]
    pub hyperlink_relation_index: Vec<HyperlinkRelationIndex>,
    #[serde(default)]
    pub node_kind_counts: HashMap<String, u32>,
    /// 每个节点最近一次被写入的事务 revision。用于增量快照：只返回
    /// `node_revisions[id] > since_revision` 的节点。
    #[serde(default)]
    pub node_revisions: HashMap<String, u64>,
    /// 每条边最近一次被写入的事务 revision。
    #[serde(default)]
    pub edge_revisions: HashMap<String, u64>,
    /// 每个嵌入实例最近一次被写入的事务 revision。
    #[serde(default)]
    pub embed_revisions: HashMap<String, u64>,
    /// 每条链接最近一次被写入的事务 revision。
    #[serde(default)]
    pub link_revisions: HashMap<String, u64>,
    /// 每条超链接最近一次被写入的事务 revision。
    #[serde(default)]
    pub hyperlink_revisions: HashMap<String, u64>,
    /// 布局最近一次被写入的事务 revision。
    #[serde(default)]
    pub layout_revision: u64,
    pub package_revision: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub deleted_since_last_sync: DeletedSinceLastSync,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedSinceLastSync {
    #[serde(default)]
    pub entries: Vec<DeletionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionEntry {
    pub object_type: String,
    pub object_id: String,
    pub deleted_at_revision: u64,
}

impl DeletedSinceLastSync {
    pub fn add_entry(&mut self, object_type: &str, object_id: &str, revision: u64) {
        self.entries.push(DeletionEntry {
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            deleted_at_revision: revision,
        });
    }

    pub fn remove_entry(&mut self, object_type: &str, object_id: &str) {
        self.entries
            .retain(|e| !(e.object_type == object_type && e.object_id == object_id));
    }

    pub fn entries_since(&self, since_revision: u64) -> impl Iterator<Item = &DeletionEntry> {
        self.entries
            .iter()
            .filter(move |e| e.deleted_at_revision > since_revision)
    }

    /// 确认到 `acknowledged_revision`（含）为止的删除都已被同步方持久化，
    /// 可以安全清理对应的 tombstone。保留 `deleted_at_revision > acknowledged_revision`
    /// 的条目，删除 `deleted_at_revision <= acknowledged_revision` 的条目。
    pub fn acknowledge(&mut self, acknowledged_revision: u64) {
        self.entries
            .retain(|e| e.deleted_at_revision > acknowledged_revision);
    }

    pub fn compact(&mut self, keep_since_revision: u64) {
        self.entries
            .retain(|e| e.deleted_at_revision >= keep_since_revision);
    }
}
