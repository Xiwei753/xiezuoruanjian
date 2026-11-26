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
//! 画布入口只看 `graph.json` 声明的成员，素材库里尚未放到任何画布上的旧节点
//! 定义不会被扫描；[`migrate_library_node_tags`] 以库本体文件为入口补一次
//! 库范围迁移，归属只按库索引确认。
//!
//! 幂等：迁移成功后 `tags` 字段已不存在，重复执行是空操作。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::annotations::migration::MigrationConflict;
use crate::error::Result;
use crate::starmap::types::{StarMapAssetKind, StarMapAssetScope, StarMapLibraryIndex};

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
        log_conflict(conflict);
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

/// 迁移素材库中尚未被任何画布引用的旧节点定义标签。
///
/// 画布入口 `migrate_legacy_node_tags` 只扫描 `graph.json` 的 `nodeIds`，没被
/// 任何画布引用的库素材旧 `tags` 永远不会被看到；新 `StarMapNodeDefinition`
/// 也不再读取 `tags`，不迁移就会从界面消失。这里直接遍历
/// `starmaps/library/nodes/**` 的本体文件，并用库索引确认唯一归属：
/// 索引里没有该素材或同一素材有多个不同归属时记冲突、跳过，保留源 `tags`。
///
/// 幂等：迁移成功后本体文件已没有 `tags` 字段，重复执行是空操作。
pub(super) fn migrate_library_node_tags(app_data_root: &Path) -> Result<()> {
    let nodes_dir = app_data_root.join("starmaps").join("library").join("nodes");
    if !nodes_dir.is_dir() {
        return Ok(());
    }
    let definitions = collect_legacy_library_definitions(&nodes_dir)?;
    if definitions.is_empty() {
        return Ok(());
    }

    // 旧 schema 的库索引要等 `library::migrate_library` 升级后才能按当前结构
    // 读取；读不到就不能确认归属，本次保留全部源 `tags`，等索引可用后再迁移。
    let index = match crate::starmap::library::load_library_index(app_data_root) {
        Ok(index) => index,
        Err(err) => {
            log::warn!(
                "starmap migration: skip library tag migration, library index is not readable: {err}"
            );
            return Ok(());
        }
    };

    let mut legacy: Vec<(String, Option<String>, Vec<String>)> = Vec::new();
    let mut definition_paths: Vec<(String, PathBuf)> = Vec::new();
    let mut conflicts: Vec<MigrationConflict> = Vec::new();
    for (asset_id, path, tags) in definitions {
        let mut scopes = library_asset_scopes(&index, &asset_id);
        match scopes.len() {
            // 同一归属的多条目录条目去重后只剩一个 scope，按唯一归属处理。
            1 => {
                let project_hint = scopes.pop().flatten();
                legacy.push((asset_id.clone(), project_hint, tags));
                definition_paths.push((asset_id, path));
            }
            0 => conflicts.push(MigrationConflict {
                kind: "scopeUnresolved".to_string(),
                source_id: asset_id,
                detail: "素材不在库索引中，无法确认归属".to_string(),
            }),
            _ => conflicts.push(MigrationConflict {
                kind: "scopeAmbiguous".to_string(),
                source_id: asset_id,
                detail: format!("素材在库索引中有多个归属：{scopes:?}"),
            }),
        }
    }

    for conflict in &conflicts {
        log_conflict(conflict);
    }
    let mut conflicted: HashSet<String> = conflicts
        .iter()
        .map(|conflict| conflict.source_id.clone())
        .collect();
    if !legacy.is_empty() {
        let report =
            crate::annotations::migration::migrate_starmap_node_tags(app_data_root, &legacy)?;
        for conflict in &report.conflicts {
            log_conflict(conflict);
        }
        conflicted.extend(
            report
                .conflicts
                .iter()
                .map(|conflict| conflict.source_id.clone()),
        );
    }
    // 有冲突的素材保留源数据，只清理已成功迁移的本体字段。
    for (asset_id, path) in definition_paths {
        if conflicted.contains(&asset_id) {
            continue;
        }
        strip_legacy_tags(&path)?;
    }
    Ok(())
}

/// 一条待迁移的库素材旧标签：`(素材 ID, 本体文件, 旧标签名)`。
type LegacyLibraryDefinition = (String, PathBuf, Vec<String>);

/// 遍历素材本体目录，返回带旧 `tags` 的节点定义。
///
/// 同一素材只保留一次（按路径排序后的第一个文件），避免重复迁移与重复清理。
fn collect_legacy_library_definitions(nodes_dir: &Path) -> Result<Vec<LegacyLibraryDefinition>> {
    let mut definition_files = Vec::new();
    collect_definition_files(nodes_dir, &mut definition_files)?;
    let mut definitions = Vec::new();
    let mut seen_assets: HashSet<String> = HashSet::new();
    for path in definition_files {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&content) else {
            continue;
        };
        let asset_id = value
            .get("assetId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .or_else(|| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| !stem.is_empty())
                    .map(str::to_string)
            });
        let Some(asset_id) = asset_id else {
            continue;
        };
        if !seen_assets.insert(asset_id.clone()) {
            continue;
        }
        let tags = legacy_tags(&value);
        if tags.is_empty() {
            continue;
        }
        definitions.push((asset_id, path, tags));
    }
    Ok(definitions)
}

/// 库索引中一个素材节点定义的全部归属（去重；`None` 表示全局库）。
///
/// 空表示索引里没有该素材，多于一个表示归属有歧义：两种情况都不能迁移。
fn library_asset_scopes(index: &StarMapLibraryIndex, asset_id: &str) -> Vec<Option<String>> {
    let mut scopes: Vec<Option<String>> = Vec::new();
    for entry in &index.entries {
        if entry.target.kind != StarMapAssetKind::Node || entry.target.asset_id != asset_id {
            continue;
        }
        let scope = match &entry.scope {
            StarMapAssetScope::Global => None,
            StarMapAssetScope::Project { project_id } => Some(project_id.clone()),
        };
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    scopes
}

fn log_conflict(conflict: &MigrationConflict) {
    log::warn!(
        "starmap migration: asset tag conflict {} for {}: {}",
        conflict.kind,
        conflict.source_id,
        conflict.detail
    );
}

/// 递归收集素材本体目录下的全部 `*.json` 文件（按路径排序保证顺序稳定）。
fn collect_definition_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> =
        std::fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, std::io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_definition_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::AnnotationStore;

    fn write_json(path: &Path, value: &Value) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    fn definition(asset_id: &str, tags: &[&str]) -> Value {
        serde_json::json!({
            "assetId": asset_id,
            "title": "旧节点定义",
            "kind": "note",
            "tags": tags,
            "createdAt": 1,
            "updatedAt": 1,
        })
    }

    fn write_library_index(root: &Path, entries: Value) {
        let index = serde_json::json!({
            "schemaVersion": crate::starmap::types::CURRENT_LIBRARY_SCHEMA_VERSION,
            "entries": entries,
            "folders": [],
            "updatedAt": 1,
        });
        write_json(&root.join("starmaps/library/index.json"), &index);
    }

    fn read_definition(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn library_definition_with_unique_entry_migrates_and_strips_tags() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = crate::starmap::library::node_definition_abs_path(root, "asset_lib_1");
        write_json(&path, &definition("asset_lib_1", &["角色", "主角"]));
        write_library_index(
            root,
            serde_json::json!([{
                "entryId": "le_1",
                "scope": {"kind": "global"},
                "target": {"kind": "node", "assetId": "asset_lib_1"},
                "createdAt": 1,
                "updatedAt": 1
            }]),
        );

        migrate_library_node_tags(root).unwrap();

        let global = AnnotationStore::global(root);
        assert_eq!(global.list_tags().unwrap().len(), 2, "两个旧标签都应迁移");
        assert_eq!(global.list_bindings().unwrap().len(), 2);
        assert!(
            read_definition(&path).get("tags").is_none(),
            "迁移成功后删除本体旧 tags 字段"
        );

        // 幂等：重复执行不新增关系，也不报错。
        migrate_library_node_tags(root).unwrap();
        assert_eq!(global.list_tags().unwrap().len(), 2);
        assert_eq!(global.list_bindings().unwrap().len(), 2);
    }

    #[test]
    fn library_definition_without_entry_keeps_source_tags() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = crate::starmap::library::node_definition_abs_path(root, "asset_lib_2");
        write_json(&path, &definition("asset_lib_2", &["线索"]));
        write_library_index(root, serde_json::json!([]));

        migrate_library_node_tags(root).unwrap();

        assert_eq!(
            read_definition(&path).get("tags"),
            Some(&serde_json::json!(["线索"])),
            "无法确认归属时保留源 tags（记 scopeUnresolved）"
        );
        let global = AnnotationStore::global(root);
        assert!(global.list_tags().unwrap().is_empty(), "不猜测作用域写标签");
        assert!(global.list_bindings().unwrap().is_empty());
    }
}
