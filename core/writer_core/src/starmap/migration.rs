//! 旧星图数据一次性迁移。
//!
//! 在正常 Store/Meta loader 之前执行。迁移完成后只保留新格式，
//! 不保留 runtime 双轨兼容。
//!
//! ## 迁移内容
//!
//! ### index schema 1 -> 2
//! - 旧 `starmaps: Vec<StarMapMeta>` 提取 `starmap_ids`
//! - 旧 `isMainForProject + projectId` 生成 `main_starmap_by_project`
//! - 写回新 index 后版本改成 2
//!
//! ### 星图对象存储 schema "3" -> "4"
//! - 读取旧 `layouts/default/nodes/*.json`，按 nodeId 找到 x/y，写进对应 node JSON 的 `position`
//! - 读取旧 embed JSON，把 `placement.x/y` 写成 `position`
//! - 删除 embed 的 `placement / targetViewport / displayPolicy / openBehavior`
//! - node 删除 `displayPolicy / openBehavior`
//! - portal 删除 `mode / previewPolicy`
//! - 全部对象写成功以后，再删除旧 `layouts/default/**` 和 `session/starmaps/{id}/viewport.json`
//! - 最后把 GraphMeta schema 写成 "4"

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};

/// 旧 index schema 版本。
const OLD_INDEX_SCHEMA_VERSION: u64 = 1;
/// 新 index schema 版本。
const NEW_INDEX_SCHEMA_VERSION: u32 = 2;

/// 旧 GraphMeta schema 版本。
const OLD_GRAPH_META_SCHEMA_VERSION: &str = "3";
/// 新 GraphMeta schema 版本（等于 `store::meta::CURRENT_SCHEMA_VERSION`）。
const NEW_GRAPH_META_SCHEMA_VERSION: &str = "4";

/// 迁移入口：在 `load_index` / `load_current_graph_meta` 之前调用。
///
/// 幂等：已经是新格式的数据会被跳过，不会重复迁移。
pub fn migrate_starmap_data(app_data_root: &Path) -> Result<()> {
    migrate_index(app_data_root)?;
    migrate_all_starmap_graphs(app_data_root)?;
    Ok(())
}

/// index schema 1 -> 2。
///
/// 读取 `starmaps/index.json`，如果是旧 schema 1 格式则迁移为新 schema 2。
/// 已经是 schema 2 或文件不存在则跳过。
pub fn migrate_index(app_data_root: &Path) -> Result<()> {
    let index_path = app_data_root.join("starmaps").join("index.json");
    if !index_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&index_path)?;
    let value: Value = serde_json::from_str(&content)?;

    let schema_version = value
        .get("schemaVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if schema_version != OLD_INDEX_SCHEMA_VERSION {
        // 已经是新格式或未知格式，跳过。
        return Ok(());
    }

    // 旧格式：{ schemaVersion: 1, starmaps: [StarMapMeta], updatedAt }
    let starmaps = value
        .get("starmaps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Other("old index schema 1 missing 'starmaps' array".to_string()))?;

    let mut starmap_ids = Vec::new();
    let mut main_starmap_by_project: HashMap<String, String> = HashMap::new();

    for meta in starmaps {
        let starmap_id = meta
            .get("starmapId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Other("old index meta missing 'starmapId' field".to_string()))?;
        starmap_ids.push(starmap_id.to_string());

        let is_main = meta
            .get("isMainForProject")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let project_id = meta.get("projectId").and_then(|v| v.as_str());
        if is_main {
            if let Some(pid) = project_id {
                main_starmap_by_project.insert(pid.to_string(), starmap_id.to_string());
            }
        }
    }

    let updated_at = value
        .get("updatedAt")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(super::now_epoch);

    let new_index = serde_json::json!({
        "schemaVersion": NEW_INDEX_SCHEMA_VERSION,
        "starmapIds": starmap_ids,
        "mainStarmapByProject": main_starmap_by_project,
        "updatedAt": updated_at,
    });

    let new_content = serde_json::to_string_pretty(&new_index)?;
    crate::storage::atomic_write_string(&index_path, &new_content)?;
    Ok(())
}

/// 对所有星图执行对象存储迁移 schema "3" -> "4"。
fn migrate_all_starmap_graphs(app_data_root: &Path) -> Result<()> {
    let starmaps_dir = app_data_root.join("starmaps");
    if !starmaps_dir.exists() {
        return Ok(());
    }

    let entries: Vec<_> = std::fs::read_dir(&starmaps_dir)?
        .collect::<std::result::Result<Vec<_>, std::io::Error>>()?;
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let starmap_id = entry.file_name().to_string_lossy().to_string();
        migrate_one_starmap_graph(app_data_root, &starmap_id)?;
    }
    Ok(())
}

/// 单个星图的对象存储迁移。
///
/// 读取 `starmaps/{id}/graph.json`，如果 schemaVersion 是 "3" 则执行迁移。
/// 已经是 "4" 或文件不存在则跳过。
pub fn migrate_one_starmap_graph(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    let graph_dir = app_data_root.join("starmaps").join(starmap_id);
    if !graph_dir.is_dir() {
        return Ok(());
    }
    let graph_json_path = graph_dir.join("graph.json");
    if !graph_json_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&graph_json_path)?;
    let value: Value = serde_json::from_str(&content)?;

    let schema_version = value
        .get("schemaVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if schema_version != OLD_GRAPH_META_SCHEMA_VERSION {
        // 已经是新格式或未知格式，跳过。
        return Ok(());
    }

    // 1. 读取旧 layout，构建 nodeId -> (x, y) 映射。
    let layout_positions = read_layout_positions(&graph_dir)?;

    // 2. 迁移 node JSON：写入 position，删除 displayPolicy/openBehavior，portal 删除 mode/previewPolicy。
    if let Some(node_ids) = value.get("nodeIds").and_then(|v| v.as_array()) {
        for nid in node_ids {
            if let Some(id) = nid.as_str() {
                migrate_node_json(&graph_dir, id, &layout_positions)?;
            }
        }
    }

    // 3. 迁移 embed JSON：placement.x/y -> position，删除旧显示层字段。
    if let Some(embed_ids) = value.get("embedInstanceIds").and_then(|v| v.as_array()) {
        for eid in embed_ids {
            if let Some(id) = eid.as_str() {
                migrate_embed_json(&graph_dir, id)?;
            }
        }
    }

    // 4. 删除旧 layouts/default/** 和 session/starmaps/{id}/viewport.json。
    let layouts_dir = graph_dir.join("layouts");
    if layouts_dir.exists() {
        std::fs::remove_dir_all(&layouts_dir)?;
    }
    let viewport_path = app_data_root
        .join("session")
        .join("starmaps")
        .join(starmap_id)
        .join("viewport.json");
    if viewport_path.exists() {
        std::fs::remove_file(&viewport_path)?;
    }
    // 尝试删除空的 session/starmaps/{id} 目录（非空则忽略）。
    let session_starmap_dir = app_data_root
        .join("session")
        .join("starmaps")
        .join(starmap_id);
    if session_starmap_dir.exists() {
        let _ = std::fs::remove_dir(&session_starmap_dir);
    }

    // 5. 把 GraphMeta schema 写成 "4"，删除 layoutRevision。
    let mut new_value = value.clone();
    if let Some(obj) = new_value.as_object_mut() {
        obj.insert(
            "schemaVersion".to_string(),
            Value::String(NEW_GRAPH_META_SCHEMA_VERSION.to_string()),
        );
        obj.remove("layoutRevision");
    }
    let new_content = serde_json::to_string_pretty(&new_value)?;
    crate::storage::atomic_write_string(&graph_json_path, &new_content)?;

    Ok(())
}

/// 读取旧 `layouts/default/nodes/*.json`，构建 nodeId -> (x, y) 映射。
#[allow(clippy::cast_possible_truncation)]
fn read_layout_positions(graph_dir: &Path) -> Result<HashMap<String, (f32, f32)>> {
    let mut positions = HashMap::new();
    let nodes_dir = graph_dir.join("layouts").join("default").join("nodes");
    if !nodes_dir.exists() {
        return Ok(positions);
    }
    let entries: Vec<_> =
        std::fs::read_dir(&nodes_dir)?.collect::<std::result::Result<Vec<_>, std::io::Error>>()?;
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        // 旧 layout node shard 是 Vec<StarMapLayoutNode>。
        let nodes: Vec<Value> = serde_json::from_str(&content).unwrap_or_default();
        for node in nodes {
            let node_id = node.get("nodeId").and_then(|v| v.as_str());
            let x = node.get("x").and_then(|v| v.as_f64());
            let y = node.get("y").and_then(|v| v.as_f64());
            if let (Some(id), Some(x), Some(y)) = (node_id, x, y) {
                positions.insert(id.to_string(), (x as f32, y as f32));
            }
        }
    }
    Ok(positions)
}

/// 迁移单个 node JSON 文件。
fn migrate_node_json(
    graph_dir: &Path,
    node_id: &str,
    layout_positions: &HashMap<String, (f32, f32)>,
) -> Result<()> {
    let bucket = crate::starmap::package_storage::bucket_for_id(node_id);
    let node_path = graph_dir
        .join("nodes")
        .join(bucket)
        .join(format!("{}.json", node_id));
    if !node_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&node_path)?;
    let mut value: Value = serde_json::from_str(&content)?;
    if let Some(obj) = value.as_object_mut() {
        // 删除旧显示层字段。
        obj.remove("displayPolicy");
        obj.remove("openBehavior");

        // 写入 position：优先用 layout 位置，否则默认 (0, 0)。
        let (x, y) = layout_positions.get(node_id).copied().unwrap_or((0.0, 0.0));
        obj.insert(
            "position".to_string(),
            serde_json::json!({ "x": x, "y": y }),
        );

        // portal 删除 mode/previewPolicy。
        if let Some(portal) = obj.get_mut("portal").and_then(|v| v.as_object_mut()) {
            portal.remove("mode");
            portal.remove("previewPolicy");
        }
    }
    let new_content = serde_json::to_string_pretty(&value)?;
    crate::storage::atomic_write_string(&node_path, &new_content)?;
    Ok(())
}

/// 迁移单个 embed JSON 文件。
#[allow(clippy::cast_possible_truncation)]
fn migrate_embed_json(graph_dir: &Path, instance_id: &str) -> Result<()> {
    let bucket = crate::starmap::package_storage::bucket_for_id(instance_id);
    let embed_path = graph_dir
        .join("embeds")
        .join(bucket)
        .join(format!("{}.json", instance_id));
    if !embed_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&embed_path)?;
    let mut value: Value = serde_json::from_str(&content)?;
    if let Some(obj) = value.as_object_mut() {
        // 从 placement.x/y 提取 position。
        let (x, y) = if let Some(p) = obj.get("placement").and_then(|v| v.as_object()) {
            let x = p.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y = p.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
            (x as f32, y as f32)
        } else {
            (0.0, 0.0)
        };

        obj.insert(
            "position".to_string(),
            serde_json::json!({ "x": x, "y": y }),
        );

        // 删除旧显示层字段。
        obj.remove("placement");
        obj.remove("targetViewport");
        obj.remove("displayPolicy");
        obj.remove("openBehavior");
    }
    let new_content = serde_json::to_string_pretty(&value)?;
    crate::storage::atomic_write_string(&embed_path, &new_content)?;
    Ok(())
}

#[cfg(test)]
mod tests;
