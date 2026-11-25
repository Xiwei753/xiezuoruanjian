//! 旧节点/素材标签迁移到 annotations 关系仓库（Issue #1060）。
//!
//! 旧格式把标签名直接存在共享素材本体（`StarMapNodeDefinition.tags`）和
//! 更早的画布节点文件里。新模型只有 [`crate::annotations::TagBinding`] 一条
//! 关系，节点/素材不再保存标签。
//!
//! 迁移顺序：
//! 1. 从画布实例文件解析素材 ID（旧数据回退到实例 ID）；
//! 2. 从素材本体文件（或旧画布节点文件）读取旧 `tags`；
//! 3. 交给 `annotations::migration::migrate_starmap_node_tags` 按素材实际归属
//!    写入项目域或全局域；归属有多个候选时记冲突、不猜；
//! 4. **只在迁移没有为某素材记录冲突时**重写本体文件并删除 `tags` 字段，
//!    保证失败/冲突时源数据仍在。
//!
//! 幂等：迁移成功后 `tags` 字段已不存在，重复执行是空操作。

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use crate::error::Result;

/// 迁移某个星图内所有节点引用的素材标签。
pub(super) fn migrate_legacy_node_tags(
    app_data_root: &Path,
    starmap_id: &str,
    graph_dir: &Path,
) -> Result<()> {
    let graph_json_path = graph_dir.join("graph.json");
    let node_ids = match std::fs::read_to_string(&graph_json_path) {
        Ok(content) => match serde_json::from_str::<Value>(&content) {
            Ok(value) => super::read_declared_member_ids(&value, "nodeIds")?,
            Err(_) => return Ok(()),
        },
        Err(_) => return Ok(()),
    };
    let project_hint = starmap_project_id(app_data_root, starmap_id);

    let mut legacy: Vec<(String, Option<String>, Vec<String>)> = Vec::new();
    // 记录每个素材本体文件里旧 tags 的原始位置，供迁移成功后清理字段。
    let mut definition_paths: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut seen_assets: HashSet<String> = HashSet::new();

    for instance_id in node_ids {
        let bucket = crate::starmap::package_storage::bucket_for_id(&instance_id);
        let node_path = graph_dir
            .join("nodes")
            .join(bucket)
            .join(format!("{instance_id}.json"));
        let Ok(content) = std::fs::read_to_string(&node_path) else {
            continue;
        };
        let Ok(node_value) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        let asset_id = node_value
            .get("assetId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or(instance_id.as_str())
            .to_string();
        if !seen_assets.insert(asset_id.clone()) {
            continue;
        }

        let definition_rel = crate::starmap::library::node_definition_rel_path(&asset_id);
        let definition_path = app_data_root.join(&definition_rel);
        let (tags, source_path) = match std::fs::read_to_string(&definition_path) {
            Ok(definition_content) => match serde_json::from_str::<Value>(&definition_content) {
                Ok(definition_value) => (
                    legacy_tags(&definition_value),
                    Some(definition_path.clone()),
                ),
                Err(_) => continue,
            },
            // 旧格式的画布节点文件本身就是完整节点内容，标签也在里面。
            Err(_) => (legacy_tags(&node_value), None),
        };
        if tags.is_empty() {
            continue;
        }
        legacy.push((asset_id.clone(), project_hint.clone(), tags));
        if let Some(path) = source_path {
            definition_paths.push((asset_id, path));
        }
    }

    if legacy.is_empty() {
        return Ok(());
    }

    let report = crate::annotations::migration::migrate_starmap_node_tags(app_data_root, &legacy)?;
    for conflict in &report.conflicts {
        log::warn!(
            "starmap migration: asset tag conflict {} for {}: {}",
            conflict.kind,
            conflict.source_id,
            conflict.detail
        );
    }
    // 有冲突的素材保留源数据，只清理已成功迁移的本体字段。
    let conflicted: HashSet<&str> = report
        .conflicts
        .iter()
        .map(|conflict| conflict.source_id.as_str())
        .collect();
    for (asset_id, path) in definition_paths {
        if conflicted.contains(asset_id.as_str()) {
            continue;
        }
        strip_legacy_tags(&path)?;
    }
    Ok(())
}

fn legacy_tags(value: &Value) -> Vec<String> {
    value
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn starmap_project_id(app_data_root: &Path, starmap_id: &str) -> Option<String> {
    let meta_path = app_data_root
        .join("starmaps")
        .join(format!("{starmap_id}.meta.json"));
    std::fs::read_to_string(meta_path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|meta| {
            meta.get("projectId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
}

/// 重写素材本体，删除旧 `tags` 字段；内容不变时不写盘。
fn strip_legacy_tags(path: &Path) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let mut value: Value = serde_json::from_str(&content)?;
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    if object.remove("tags").is_none() {
        return Ok(());
    }
    let rewritten = serde_json::to_string_pretty(&value)?;
    crate::storage::atomic_write_string(path, &rewritten)?;
    Ok(())
}
