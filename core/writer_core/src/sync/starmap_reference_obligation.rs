//! 星图派生引用索引的事务责任（#1062 评论 #7236446138 第 4 项）。
//!
//! 同步 commit 把画布实例/素材对象写回 live 后，`references.json` 必须反映
//! 本轮实例变化。旧实现每次都全量 `derive_reference_index_from_instances`
//! 重建：只改一个节点也要枚举所有画布，而且重建失败会让已经 durable 的正文/
//! 星图提交被上层当成“提交失败”。
//!
//! 这里把派生索引更新拆成两步：
//! 1. 提交前登记 durable 责任（dirty index 的 `starmap_references`），记录
//!    本轮受影响的实例身份；
//! 2. 提交成功后按受影响实例做 `references.json` 的 insert/remove；
//!    成功即结算责任，失败只保留责任供恢复重建，不再改写主数据提交回执。

use std::path::{Path, PathBuf};

use crate::starmap::types::StarMapNodeInstance;
use crate::sync::dirty_index;
use crate::sync::staging::CommitAction;

const OBLIGATION_KIND: &str = "starmap_references";

/// 一个实例文件相对 live root 的路径解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstanceTouch {
    pub starmap_id: String,
    pub instance_id: String,
    pub deleted: bool,
}

/// 提交前登记的本轮派生索引责任。
#[derive(Debug)]
pub(crate) struct PendingStarmapReferences {
    app_root: PathBuf,
    live_root: PathBuf,
    transaction_id: String,
    touches: Vec<InstanceTouch>,
}

impl PendingStarmapReferences {
    /// 从提交动作里登记受影响实例；没有实例变化时返回 `None`。
    pub(crate) fn begin(
        live_root: &Path,
        actions: &[CommitAction],
    ) -> crate::error::Result<Option<Self>> {
        let touches: Vec<InstanceTouch> = actions.iter().filter_map(instance_touch).collect();
        if touches.is_empty() {
            return Ok(None);
        }
        let app_root = crate::storage::WorkspaceWriteLock::current_root()
            .unwrap_or_else(|| live_root.to_path_buf());
        let transaction_id = uuid::Uuid::new_v4().to_string();
        register(&app_root, live_root, &transaction_id, &touches)?;
        Ok(Some(Self {
            app_root,
            live_root: live_root.to_path_buf(),
            transaction_id,
            touches,
        }))
    }

    /// 主数据已提交：按受影响实例增量更新引用索引并结算责任。
    ///
    /// 更新失败只保留责任（恢复时会据此重建或补齐），不影响调用方返回的
    /// 主数据提交成功。
    pub(crate) fn settle(self) {
        match apply_touches(&self.live_root, &self.touches) {
            Ok(()) => {
                if let Err(error) = dirty_index::clear_obligation(
                    &self.app_root,
                    OBLIGATION_KIND,
                    &self.live_root,
                    &self.transaction_id,
                ) {
                    log::warn!(
                        "[sync] starmap reference obligation clear failed: {error} (kept for recovery)"
                    );
                }
            }
            Err(error) => {
                log::warn!(
                    "[sync] starmap reference index update failed for {}: {error} — kept as \
                     recovery obligation",
                    self.live_root.display()
                );
            }
        }
    }

    /// 主数据提交失败：撤销责任，不需要为未提交内容补索引。
    pub(crate) fn cancel(self) {
        if let Err(error) = dirty_index::clear_obligation(
            &self.app_root,
            OBLIGATION_KIND,
            &self.live_root,
            &self.transaction_id,
        ) {
            log::warn!("[sync] starmap reference obligation cancel failed: {error}");
        }
    }
}

fn register(
    app_root: &Path,
    live_root: &Path,
    transaction_id: &str,
    touches: &[InstanceTouch],
) -> crate::error::Result<()> {
    let detail = serde_json::to_string(
        &touches
            .iter()
            .map(|touch| {
                serde_json::json!({
                    "starmapId": touch.starmap_id,
                    "instanceId": touch.instance_id,
                    "deleted": touch.deleted,
                })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(crate::Error::Json)?;
    dirty_index::record_obligation(
        app_root,
        dirty_index::RecoveryObligation {
            kind: OBLIGATION_KIND.to_string(),
            root: dirty_index::root_key(live_root),
            transaction_id: transaction_id.to_string(),
            detail: Some(detail),
        },
    )
}

fn apply_touches(live_root: &Path, touches: &[InstanceTouch]) -> crate::error::Result<()> {
    for touch in touches {
        if touch.deleted {
            crate::starmap::library::remove_instance_records(
                live_root,
                &touch.starmap_id,
                &[touch.instance_id.clone()],
            )?;
            continue;
        }
        let rel = instance_rel(&touch.starmap_id, &touch.instance_id);
        let path = live_root.join(&rel);
        let bytes = std::fs::read(&path).map_err(|error| {
            crate::Error::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "starmap instance {} unreadable while updating reference index: {error}",
                    path.display()
                ),
            ))
        })?;
        let instance: StarMapNodeInstance = serde_json::from_slice(&bytes)?;
        if instance.instance_id != touch.instance_id {
            return Err(crate::Error::Other(format!(
                "starmap instance {} declares instance_id={} (expected {})",
                path.display(),
                instance.instance_id,
                touch.instance_id
            )));
        }
        if instance.starmap_id != touch.starmap_id {
            return Err(crate::Error::Other(format!(
                "starmap instance {} declares starmap_id={} (expected {})",
                path.display(),
                instance.starmap_id,
                touch.starmap_id
            )));
        }
        crate::starmap::library::record_node_reference(
            live_root,
            &touch.starmap_id,
            instance.project_id.as_deref(),
            &touch.instance_id,
            &instance.asset_id,
        )?;
    }
    Ok(())
}

fn instance_touch(action: &CommitAction) -> Option<InstanceTouch> {
    let (rel, deleted) = match action {
        CommitAction::Apply { rel_path, .. } => (rel_path, false),
        CommitAction::Delete { rel_path } => (rel_path, true),
    };
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect();
    if parts.len() != 5 || parts[0] != "starmaps" || parts[2] != "nodes" {
        return None;
    }
    let file = parts[4].strip_suffix(".json")?;
    Some(InstanceTouch {
        starmap_id: parts[1].to_string(),
        instance_id: file.to_string(),
        deleted,
    })
}

fn instance_rel(starmap_id: &str, instance_id: &str) -> String {
    format!(
        "starmaps/{starmap_id}/nodes/{}/{instance_id}.json",
        crate::starmap::package_storage::bucket_for_id(instance_id)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_instance(root: &Path, starmap_id: &str, instance_id: &str, asset_id: &str) {
        let rel = instance_rel(starmap_id, instance_id);
        let path = root.join(&rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let instance = StarMapNodeInstance {
            instance_id: instance_id.to_string(),
            asset_id: asset_id.to_string(),
            position: Default::default(),
            starmap_id: starmap_id.to_string(),
            project_id: Some("p1".to_string()),
            created_at: 1,
            updated_at: 2,
        };
        std::fs::write(&path, serde_json::to_vec(&instance).unwrap()).unwrap();
    }

    #[test]
    fn settle_updates_only_touched_instances_and_clears_obligation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let live = tmp.path();
        std::fs::create_dir_all(crate::starmap::library::library_entries_dir(live)).unwrap();
        write_instance(live, "m1", "i1", "asset-1");
        write_instance(live, "m1", "i2", "asset-2");
        crate::starmap::library::record_node_reference(live, "m1", Some("p1"), "i2", "asset-2")
            .unwrap();

        let action = CommitAction::Apply {
            rel_path: PathBuf::from(instance_rel("m1", "i1")),
            content: Vec::new(),
        };
        // 模拟事务已经写入 live。
        let pending = PendingStarmapReferences::begin(live, &[action])
            .unwrap()
            .expect("实例变化必须登记责任");
        assert!(
            !dirty_index::obligations(live).is_empty(),
            "提交前必须已有 durable 责任"
        );
        pending.settle();

        let index = crate::starmap::library::load_reference_index(live).unwrap();
        assert!(
            index
                .nodes
                .iter()
                .any(|record| record.instance_id == "i1" && record.asset_id == "asset-1"),
            "受影响实例必须进入引用索引，实际 {:?}",
            index.nodes
        );
        assert!(
            index.nodes.iter().any(|record| record.instance_id == "i2"),
            "未受影响实例不得被清空"
        );
        assert!(
            dirty_index::obligations(live).is_empty(),
            "成功后必须结算责任"
        );
    }

    #[test]
    fn unreadable_instance_keeps_obligation_and_does_not_fail_commit() {
        let tmp = tempfile::TempDir::new().unwrap();
        let live = tmp.path();
        let action = CommitAction::Apply {
            rel_path: PathBuf::from(instance_rel("m1", "missing")),
            content: Vec::new(),
        };
        let pending = PendingStarmapReferences::begin(live, &[action])
            .unwrap()
            .expect("实例变化必须登记责任");
        let transaction_id = pending.transaction_id.clone();
        pending.settle();
        let obligations = dirty_index::obligations(live);
        assert!(
            obligations
                .iter()
                .any(|obligation| obligation.transaction_id == transaction_id),
            "索引更新失败必须保留恢复责任，实际 {obligations:?}"
        );
    }
}
