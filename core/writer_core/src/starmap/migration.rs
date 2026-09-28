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
//!   （GraphMeta 声明的成员列表必须可解析、每个 node/embed 文件都必须存在，
//!   否则 Err 并保留旧 layout——那是 position 的唯一来源）
//! - 最后把 GraphMeta schema 写成 "4"

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};

/// 旧 index schema 版本。
const OLD_INDEX_SCHEMA_VERSION: u64 = 1;
/// 新 index schema 版本。
pub(crate) const NEW_INDEX_SCHEMA_VERSION: u32 = 2;

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
/// 已经是 schema 2 则跳过（`Ok(())`）。
///
/// **Fail-closed 版本策略**：未知 / 缺失 / 非法版本（既不是 1 也不是 2）
/// 直接返回 `Err(UnsupportedVersion)`，不把未来格式当 schema 2 静默接受。
///
/// 同时重写每个星图的 `starmaps/{id}.meta.json`，只保留当前唯一结构字段
/// `starmapId / title / description / projectId / accentColor / createdAt / updatedAt`，
/// 删除 `isMainForProject / nodeCount / edgeCount / linkedChapterCount` 等废弃字段。
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

    if schema_version == u64::from(NEW_INDEX_SCHEMA_VERSION) {
        // 已经是 schema 2，无需迁移。
        return Ok(());
    }
    if schema_version != OLD_INDEX_SCHEMA_VERSION {
        // 未知/缺失/非法版本，fail-closed：不把未来格式当 schema 2 静默接受。
        return Err(Error::UnsupportedVersion {
            version: schema_version.to_string(),
        });
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

        // 重写 meta JSON，只保留当前唯一结构字段。
        rewrite_meta_to_current_shape(app_data_root, starmap_id, meta)?;
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

/// 把旧 meta JSON 重写成当前唯一结构，只保留
/// `starmapId / title / description / projectId / accentColor / createdAt / updatedAt`。
///
/// 缺失字段使用默认值：description=""、projectId=null、accentColor="#7B8CDE"。
/// meta 文件不存在时跳过（不报错）。
fn rewrite_meta_to_current_shape(
    app_data_root: &Path,
    starmap_id: &str,
    fallback_meta: &Value,
) -> Result<()> {
    let meta_path = app_data_root
        .join("starmaps")
        .join(format!("{}.meta.json", starmap_id));
    // 优先用磁盘上已存在的 meta 文件作为来源，否则用 index 里的 fallback。
    let source: Value = if meta_path.exists() {
        let content = std::fs::read_to_string(&meta_path)?;
        serde_json::from_str(&content)?
    } else {
        // meta 文件不存在，跳过（不报错）。
        return Ok(());
    };

    let title = source
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| fallback_meta.get("title").and_then(|v| v.as_str()))
        .unwrap_or("");
    let description = source
        .get("description")
        .and_then(|v| v.as_str())
        .or_else(|| fallback_meta.get("description").and_then(|v| v.as_str()))
        .unwrap_or("");
    let project_id = source
        .get("projectId")
        .and_then(|v| v.as_str())
        .or_else(|| fallback_meta.get("projectId").and_then(|v| v.as_str()));
    let accent_color = source
        .get("accentColor")
        .and_then(|v| v.as_str())
        .or_else(|| fallback_meta.get("accentColor").and_then(|v| v.as_str()))
        .unwrap_or("#7B8CDE");
    let created_at = source
        .get("createdAt")
        .and_then(|v| v.as_u64())
        .or_else(|| fallback_meta.get("createdAt").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let updated_at = source
        .get("updatedAt")
        .and_then(|v| v.as_u64())
        .or_else(|| fallback_meta.get("updatedAt").and_then(|v| v.as_u64()))
        .unwrap_or_else(super::now_epoch);

    let new_meta = serde_json::json!({
        "starmapId": starmap_id,
        "title": title,
        "description": description,
        "projectId": project_id,
        "accentColor": accent_color,
        "createdAt": created_at,
        "updatedAt": updated_at,
    });

    let new_content = serde_json::to_string_pretty(&new_meta)?;
    crate::storage::atomic_write_string(&meta_path, &new_content)?;
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
///
/// **Fail-closed 源数据保护**：GraphMeta 声明的每个 node / embed 都必须迁移成功
/// （成员列表必须可解析、对象文件必须存在），否则返回 `Err` 并保留旧
/// `layouts/default/**`——旧 layout 是所有缺失 position 的唯一来源，
/// 不允许在没迁完的情况下删除。
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
    //    成员集合来自 GraphMeta，必须严格解析并逐个迁移成功：声明的 node 文件缺失
    //    就是 Err，不能当成"没有这个成员"跳过，否则会在没迁完时删掉旧 layout。
    let node_ids = read_declared_member_ids(&value, "nodeIds")?;
    let mut migrated_node_ids: Vec<String> = Vec::new();
    for node_id in node_ids {
        if !migrate_node_json(&graph_dir, &node_id, &layout_positions)? {
            return Err(Error::Other(format!(
                "node '{}' is declared in graph.json but has no node file to migrate",
                node_id
            )));
        }
        migrated_node_ids.push(node_id);
    }

    // 3. 迁移 embed JSON：placement.x/y -> position，删除旧显示层字段。
    let embed_instance_ids = read_declared_member_ids(&value, "embedInstanceIds")?;
    let mut migrated_embed_ids: Vec<String> = Vec::new();
    for instance_id in embed_instance_ids {
        if !migrate_embed_json(&graph_dir, &instance_id)? {
            return Err(Error::Other(format!(
                "embed '{}' is declared in graph.json but has no embed file to migrate",
                instance_id
            )));
        }
        migrated_embed_ids.push(instance_id);
    }

    // 4. 删除旧 layouts/default/** 和 session/starmaps/{id}/viewport.json。
    //    只有上面所有声明的 node / embed 都迁移成功（没有提前 return Err）才走到这里。
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

    // 5. 把 GraphMeta schema 写成 "4"，删除 layoutRevision，推进 package revision。
    //    一次 schema 迁移就是一次真实的数据事务：文件内容变了 revision 必须变。
    let mut new_value = value.clone();
    if let Some(obj) = new_value.as_object_mut() {
        obj.insert(
            "schemaVersion".to_string(),
            Value::String(NEW_GRAPH_META_SCHEMA_VERSION.to_string()),
        );
        obj.remove("layoutRevision");

        let old_package_revision = obj
            .get("packageRevision")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let next = old_package_revision.saturating_add(1);
        obj.insert("packageRevision".to_string(), Value::from(next));

        let now = super::now_epoch();
        obj.insert("updatedAt".to_string(), Value::from(now));

        // 所有本轮实际迁过的 node/embed 的 revision 设为 next。
        set_revisions_for_ids(obj, "nodeRevisions", &migrated_node_ids, next);
        set_revisions_for_ids(obj, "embedRevisions", &migrated_embed_ids, next);
    }
    let new_content = serde_json::to_string_pretty(&new_value)?;
    crate::storage::atomic_write_string(&graph_json_path, &new_content)?;

    Ok(())
}

/// 读取旧 GraphMeta 的成员 ID 数组（`nodeIds` / `embedInstanceIds`）。
///
/// 迁移必须在"声明的成员全部迁完"之后才能删旧 layout，所以成员列表本身
/// 也 fail-closed：字段缺失、不是数组、条目不是字符串都返回 `Err`，
/// 不能静默当成空集合继续（那会在什么都没迁的情况下删掉 position 的唯一来源）。
fn read_declared_member_ids(value: &Value, field: &str) -> Result<Vec<String>> {
    let entries = value.get(field).and_then(|v| v.as_array()).ok_or_else(|| {
        Error::Other(format!(
            "legacy graph.json member list '{}' is missing or not an array",
            field
        ))
    })?;
    let mut ids = Vec::with_capacity(entries.len());
    for entry in entries {
        let id = entry.as_str().ok_or_else(|| {
            Error::Other(format!(
                "legacy graph.json member list '{}' contains a non-string id",
                field
            ))
        })?;
        ids.push(id.to_string());
    }
    Ok(ids)
}

/// 把 `ids` 中每个 id 的 revision 设为 `next`，写入 `obj[field]` 的 map。
fn set_revisions_for_ids(
    obj: &mut serde_json::Map<String, Value>,
    field: &str,
    ids: &[String],
    next: u64,
) {
    if ids.is_empty() {
        return;
    }
    let rev_map = obj
        .entry(field.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Some(map) = rev_map.as_object_mut() {
        for id in ids {
            map.insert(id.clone(), Value::from(next));
        }
    }
}

/// 读取旧 `layouts/default/nodes/*.json`，构建 nodeId -> (x, y) 映射。
///
/// layout shard JSON 解析失败必须返回 `Err`：不能把损坏 layout 当成"没有位置"
/// 然后继续删源数据。
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
        // 旧 layout node shard 是 Vec<StarMapLayoutNode>。解析失败必须 Err，
        // 不能 unwrap_or_default 把损坏 layout 当成空。
        let nodes: Vec<Value> = serde_json::from_str(&content)?;
        for node in nodes {
            // fail-closed：layout node shard 里每条记录都是 layout node，
            // 必须有合法 nodeId/x/y，否则直接 Err，不能静默跳过坏记录。
            let node_id = node
                .get("nodeId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::Other("layout node entry has no nodeId".to_string()))?;
            // fail-closed：有 nodeId 但 x/y 缺失/非数字/非 finite → Err，
            // 不能静默跳过坏 x/y（那是 layout 损坏，不是"没有位置"）。
            let x = node.get("x").and_then(|v| v.as_f64()).ok_or_else(|| {
                Error::Other(format!(
                    "layout node '{}' has nodeId but x is missing or not a number",
                    node_id
                ))
            })?;
            let y = node.get("y").and_then(|v| v.as_f64()).ok_or_else(|| {
                Error::Other(format!(
                    "layout node '{}' has nodeId but y is missing or not a number",
                    node_id
                ))
            })?;
            let xf = x as f32;
            let yf = y as f32;
            if !xf.is_finite() || !yf.is_finite() {
                return Err(Error::Other(format!(
                    "layout node '{}' x/y not finite after f32 truncation",
                    node_id
                )));
            }
            positions.insert(node_id.to_string(), (xf, yf));
        }
    }
    Ok(positions)
}

/// 判断 JSON `position` 字段是否合法（x、y 都是 finite f32）。
#[allow(clippy::cast_possible_truncation)]
fn position_is_valid_finite(pos: &Value) -> bool {
    let obj = match pos.as_object() {
        Some(o) => o,
        None => return false,
    };
    let x = match obj.get("x").and_then(|v| v.as_f64()) {
        Some(x) => x,
        None => return false,
    };
    let y = match obj.get("y").and_then(|v| v.as_f64()) {
        Some(y) => y,
        None => return false,
    };
    // as f32 截断后也要 finite。
    let xf = x as f32;
    let yf = y as f32;
    xf.is_finite() && yf.is_finite()
}

/// 迁移单个 node JSON 文件。
///
/// 幂等对象迁移：如果 node JSON 已有合法 `position`（x、y 都是 finite f32），
/// 保留，不再用 layout 覆盖；只有没有 position 时才从旧 layout 提取；
/// 没有 layout 也没有 position 时直接 `Err`，不能猜 (0,0) 伪装成用户
/// authored position——这是数据损坏，必须 fail-closed。
///
/// 删除 displayPolicy/openBehavior 和 portal 的 mode/previewPolicy 仍然执行
/// （这些是旧字段清理，幂等）。
///
/// 返回 `true` 表示文件被实际重写。返回 `false` 只表示"这个对象没有文件"；
/// 调用方对 GraphMeta 声明的成员必须把 `false` 当 `Err` 处理，
/// 不能把缺文件当成"没有这个成员"，否则会提前删除旧 layout。
fn migrate_node_json(
    graph_dir: &Path,
    node_id: &str,
    layout_positions: &HashMap<String, (f32, f32)>,
) -> Result<bool> {
    let bucket = crate::starmap::package_storage::bucket_for_id(node_id);
    let node_path = graph_dir
        .join("nodes")
        .join(bucket)
        .join(format!("{}.json", node_id));
    if !node_path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&node_path)?;
    let mut value: Value = serde_json::from_str(&content)?;
    if let Some(obj) = value.as_object_mut() {
        // 删除旧显示层字段。
        obj.remove("displayPolicy");
        obj.remove("openBehavior");

        // 幂等 position 迁移：已有合法 position 则保留，否则从 layout 提取，
        // 否则用 (0, 0)。
        let has_valid_position = obj
            .get("position")
            .map(position_is_valid_finite)
            .unwrap_or(false);
        if !has_valid_position {
            // fail-closed：没有合法 position 且没有 legacy layout 位置可迁移时，
            // 不能猜 (0,0) 伪装成用户 authored position——这是数据损坏，必须 Err。
            let (x, y) = layout_positions.get(node_id).copied().ok_or_else(|| {
                Error::Other(format!(
                    "node '{}' has no position and no legacy layout position to migrate from",
                    node_id
                ))
            })?;
            obj.insert(
                "position".to_string(),
                serde_json::json!({ "x": x, "y": y }),
            );
        }

        // portal 删除 mode/previewPolicy。
        if let Some(portal) = obj.get_mut("portal").and_then(|v| v.as_object_mut()) {
            portal.remove("mode");
            portal.remove("previewPolicy");
        }
    }
    let new_content = serde_json::to_string_pretty(&value)?;
    crate::storage::atomic_write_string(&node_path, &new_content)?;
    Ok(true)
}

/// 从旧 placement JSON 提取 finite (x, y)。placement 不存在或 x/y 不合法 → Err。
#[allow(clippy::cast_possible_truncation)]
fn extract_position_from_placement(
    instance_id: &str,
    obj: &serde_json::Map<String, Value>,
) -> Result<(f32, f32)> {
    let placement = obj
        .get("placement")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            Error::Other(format!(
                "embed '{}' has no position and no placement to migrate from",
                instance_id
            ))
        })?;
    let x = placement.get("x").and_then(|v| v.as_f64()).ok_or_else(|| {
        Error::Other(format!(
            "embed '{}' placement missing valid x for migration",
            instance_id
        ))
    })?;
    let y = placement.get("y").and_then(|v| v.as_f64()).ok_or_else(|| {
        Error::Other(format!(
            "embed '{}' placement missing valid y for migration",
            instance_id
        ))
    })?;
    let xf = x as f32;
    let yf = y as f32;
    if !xf.is_finite() || !yf.is_finite() {
        return Err(Error::Other(format!(
            "embed '{}' placement x/y not finite for migration",
            instance_id
        )));
    }
    Ok((xf, yf))
}

/// 迁移单个 embed JSON 文件。
///
/// 幂等对象迁移：如果 embed JSON 已有合法 `position`（x、y 都是 finite），
/// 保留，不再要求 placement；只有没有 position 时才从旧 placement.x/y 提取；
/// 旧 schema 3 的 embed 既没有 position 又没有合法 placement（placement 字段
/// 不存在或 x/y 不是合法数字）时直接 `Err`，不能猜 (0,0)。
///
/// 删除 placement/targetViewport/displayPolicy/openBehavior 仍然执行（幂等清理）。
///
/// 返回 `true` 表示文件被实际重写。返回 `false` 只表示"这个对象没有文件"；
/// 调用方对 GraphMeta 声明的成员必须把 `false` 当 `Err` 处理，
/// 不能把缺文件当成"没有这个成员"，否则会提前删除旧 layout。
fn migrate_embed_json(graph_dir: &Path, instance_id: &str) -> Result<bool> {
    let bucket = crate::starmap::package_storage::bucket_for_id(instance_id);
    let embed_path = graph_dir
        .join("embeds")
        .join(bucket)
        .join(format!("{}.json", instance_id));
    if !embed_path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&embed_path)?;
    let mut value: Value = serde_json::from_str(&content)?;
    if let Some(obj) = value.as_object_mut() {
        // 幂等 position 迁移：已有合法 position 则保留。
        let has_valid_position = obj
            .get("position")
            .map(position_is_valid_finite)
            .unwrap_or(false);

        if !has_valid_position {
            let (x, y) = extract_position_from_placement(instance_id, obj)?;
            obj.insert(
                "position".to_string(),
                serde_json::json!({ "x": x, "y": y }),
            );
        }

        // 删除旧显示层字段。
        obj.remove("placement");
        obj.remove("targetViewport");
        obj.remove("displayPolicy");
        obj.remove("openBehavior");
    }
    let new_content = serde_json::to_string_pretty(&value)?;
    crate::storage::atomic_write_string(&embed_path, &new_content)?;
    Ok(true)
}

#[cfg(test)]
mod tests;
