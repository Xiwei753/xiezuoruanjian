//! 全局/作品素材库目录与共享节点头。
//!
//! 目录只保存“归属与分类”，素材本体按稳定 `asset_id` 存放：
//! `starmaps/library/index.json` 与
//! `starmaps/library/nodes/<bucket>/<asset_id>.json`。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::starmap::types::*;
use crate::storage::atomic_write_string;

pub(crate) fn library_dir(app_data_root: &Path) -> PathBuf {
    app_data_root.join("starmaps").join("library")
}

pub(crate) fn library_index_rel_path() -> PathBuf {
    PathBuf::from("starmaps").join("library").join("index.json")
}

pub(crate) fn library_index_abs_path(app_data_root: &Path) -> PathBuf {
    app_data_root.join(library_index_rel_path())
}

pub(crate) fn node_definition_rel_path(asset_id: &str) -> PathBuf {
    PathBuf::from("starmaps")
        .join("library")
        .join("nodes")
        .join(crate::starmap::package_storage::bucket_for_id(asset_id))
        .join(format!("{asset_id}.json"))
}

pub(crate) fn node_definition_abs_path(app_data_root: &Path, asset_id: &str) -> PathBuf {
    app_data_root.join(node_definition_rel_path(asset_id))
}

pub fn load_library_index(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let path = library_index_abs_path(app_data_root);
    if !path.exists() {
        return Ok(StarMapLibraryIndex::default());
    }
    let content = fs::read_to_string(&path)?;
    let index: StarMapLibraryIndex = serde_json::from_str(&content)?;
    if index.schema_version != CURRENT_LIBRARY_SCHEMA_VERSION {
        return Err(Error::UnsupportedVersion {
            version: index.schema_version.to_string(),
        });
    }
    Ok(index)
}

pub fn save_library_index(app_data_root: &Path, index: &StarMapLibraryIndex) -> Result<()> {
    fs::create_dir_all(library_dir(app_data_root))?;
    let mut index = index.clone();
    index.schema_version = CURRENT_LIBRARY_SCHEMA_VERSION;
    index.updated_at = crate::starmap::now_epoch();
    let content = serde_json::to_string_pretty(&index)?;
    atomic_write_string(&library_index_abs_path(app_data_root), &content)
}

/// 确保库索引存在。首次打开旧数据时把已有节点/星图登记为素材本体。
pub fn ensure_library_index(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let path = library_index_abs_path(app_data_root);
    if path.exists() {
        let index = load_library_index(app_data_root)?;
        return Ok(index);
    }
    migrate_library(app_data_root)
}

/// 旧数据迁移：每个旧节点的 `id` 直接成为共享本体 ID，画布实例保留原名。
///
/// 迁移只写库索引、共享定义和实例映射，不改 graph.json；节点缓存文件仍是
/// 画布实例的完整读缓存，读取时以共享定义覆盖内容字段。
pub fn migrate_library(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let starmaps_dir = app_data_root.join("starmaps");
    if !starmaps_dir.exists() {
        let index = StarMapLibraryIndex::default();
        save_library_index(app_data_root, &index)?;
        return Ok(index);
    }

    let now = crate::starmap::now_epoch();
    let mut index = StarMapLibraryIndex {
        schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
        entries: Vec::new(),
        folders: Vec::new(),
        node_instances: Vec::new(),
        updated_at: now,
    };
    let mut seen_assets = HashSet::new();

    let mut starmap_ids = Vec::new();
    for entry in fs::read_dir(&starmaps_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".meta.json") || name == "index.json" {
            continue;
        }
        let Some(starmap_id) = name.strip_suffix(".meta.json") else {
            continue;
        };
        starmap_ids.push(starmap_id.to_string());
    }

    for starmap_id in &starmap_ids {
        let meta_path = starmaps_dir.join(format!("{starmap_id}.meta.json"));
        let Ok(content) = fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<crate::starmap::StarMapMeta>(&content) else {
            continue;
        };
        let scope = StarMapAssetScope::from_project_id(meta.project_id.as_deref());

        if seen_assets.insert(format!("starmap:{starmap_id}")) {
            index.entries.push(StarMapLibraryEntry {
                entry_id: new_entry_id(),
                parent_folder_id: None,
                scope: scope.clone(),
                target: StarMapAssetRef::starmap(starmap_id.clone()),
                created_at: meta.created_at,
                updated_at: now,
            });
        }

        let nodes_dir = starmaps_dir.join(starmap_id).join("nodes");
        let mut node_paths = Vec::new();
        collect_json_files(&nodes_dir, &mut node_paths);
        for node_path in node_paths {
            let Ok(content) = fs::read_to_string(&node_path) else {
                continue;
            };
            let Ok(node) = serde_json::from_str::<StarMapNode>(&content) else {
                continue;
            };
            let asset_id = node.id.clone();
            let definition =
                StarMapNodeDefinition::from_node(asset_id.clone(), &node);
            let definition_path = node_definition_abs_path(app_data_root, &asset_id);
            if !definition_path.exists() {
                save_node_definition(app_data_root, &definition)?;
            }
            if seen_assets.insert(format!("node:{asset_id}")) {
                index.entries.push(StarMapLibraryEntry {
                    entry_id: new_entry_id(),
                    parent_folder_id: None,
                    scope: scope.clone(),
                    target: StarMapAssetRef::node(asset_id.clone()),
                    created_at: node.created_at,
                    updated_at: now,
                });
            }
            index.node_instances.push(StarMapNodeInstance {
                instance_id: node.id.clone(),
                target_node_id: asset_id,
                position: node.position.clone(),
                starmap_id: starmap_id.clone(),
                project_id: meta.project_id.clone(),
                created_at: node.created_at,
                updated_at: node.updated_at,
            });
        }
    }

    save_library_index(app_data_root, &index)?;
    Ok(index)
}

fn collect_json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push(path);
        }
    }
}

pub fn save_node_definition(
    app_data_root: &Path,
    definition: &StarMapNodeDefinition,
) -> Result<PathBuf> {
    let path = node_definition_abs_path(app_data_root, &definition.asset_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(definition)?;
    atomic_write_string(&path, &content)?;
    Ok(node_definition_rel_path(&definition.asset_id))
}

pub fn load_node_definition(
    app_data_root: &Path,
    asset_id: &str,
) -> Result<Option<StarMapNodeDefinition>> {
    let path = node_definition_abs_path(app_data_root, asset_id);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    let definition = serde_json::from_str(&content)?;
    Ok(Some(definition))
}

pub fn delete_node_definition(app_data_root: &Path, asset_id: &str) -> Result<()> {
    let path = node_definition_abs_path(app_data_root, asset_id);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) fn new_entry_id() -> String {
    format!("le_{}", uuid::Uuid::new_v4())
}

pub(crate) fn new_folder_id() -> String {
    format!("lf_{}", uuid::Uuid::new_v4())
}

pub fn ensure_library_entry(
    index: &mut StarMapLibraryIndex,
    scope: &StarMapAssetScope,
    target: StarMapAssetRef,
    parent_folder_id: Option<String>,
    timestamp: u64,
) -> StarMapLibraryEntry {
    if let Some(existing) = index
        .entries
        .iter()
        .find(|entry| entry.target == target)
        .cloned()
    {
        return existing;
    }
    let entry = StarMapLibraryEntry {
        entry_id: new_entry_id(),
        parent_folder_id,
        scope: scope.clone(),
        target,
        created_at: timestamp,
        updated_at: timestamp,
    };
    index.entries.push(entry.clone());
    entry
}

/// 新增或复用共享节点头，并登记实例和库归属。
pub fn ensure_node_asset(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    asset_id: &str,
    instance_id: &str,
    node: &StarMapNode,
    position: &StarMapPoint,
) -> Result<String> {
    let mut index = ensure_library_index(app_data_root)?;
    let definition_path = node_definition_abs_path(app_data_root, asset_id);
    if !definition_path.exists() {
        save_node_definition(
            app_data_root,
            &StarMapNodeDefinition::from_node(asset_id.to_string(), node),
        )?;
    }
    let scope = StarMapAssetScope::from_project_id(project_id);
    ensure_library_entry(
        &mut index,
        &scope,
        StarMapAssetRef::node(asset_id),
        None,
        crate::starmap::now_epoch(),
    );
    record_node_instance(
        &mut index,
        starmap_id,
        project_id,
        instance_id,
        &asset_id,
        position,
        node.created_at,
        node.updated_at,
    );
    save_library_index(app_data_root, &index)?;
    Ok(asset_id.to_string())
}

pub fn ensure_starmap_asset(
    app_data_root: &Path,
    meta: &crate::starmap::StarMapMeta,
) -> Result<StarMapLibraryEntry> {
    let mut index = ensure_library_index(app_data_root)?;
    let scope = StarMapAssetScope::from_project_id(meta.project_id.as_deref());
    let entry = ensure_library_entry(
        &mut index,
        &scope,
        StarMapAssetRef::starmap(meta.starmap_id.clone()),
        None,
        crate::starmap::now_epoch(),
    );
    save_library_index(app_data_root, &index)?;
    Ok(entry)
}

/// 把已有或新建的素材 entry 放到指定 scope/文件夹；只改归属，不改本体 ID。
pub fn assign_entry_scope(
    app_data_root: &Path,
    target: StarMapAssetRef,
    scope: &StarMapAssetScope,
    parent_folder_id: Option<String>,
) -> Result<StarMapLibraryEntry> {
    let mut index = ensure_library_index(app_data_root)?;
    let timestamp = crate::starmap::now_epoch();
    let entry = if let Some(existing) = index
        .entries
        .iter_mut()
        .find(|entry| entry.target == target)
    {
        existing.scope = scope.clone();
        existing.parent_folder_id = parent_folder_id;
        existing.updated_at = timestamp;
        existing.clone()
    } else {
        let entry = StarMapLibraryEntry {
            entry_id: new_entry_id(),
            parent_folder_id,
            scope: scope.clone(),
            target,
            created_at: timestamp,
            updated_at: timestamp,
        };
        index.entries.push(entry.clone());
        entry
    };
    save_library_index(app_data_root, &index)?;
    Ok(entry)
}

#[allow(clippy::too_many_arguments)]
pub fn record_node_instance(
    index: &mut StarMapLibraryIndex,
    starmap_id: &str,
    project_id: Option<&str>,
    instance_id: &str,
    target_node_id: &str,
    position: &StarMapPoint,
    created_at: u64,
    updated_at: u64,
) {
    if let Some(record) = index.node_instances.iter_mut().find(|record| {
        record.starmap_id == starmap_id && record.instance_id == instance_id
    }) {
        record.target_node_id = target_node_id.to_string();
        record.position = position.clone();
        record.project_id = project_id.map(ToString::to_string);
        record.updated_at = updated_at;
        return;
    }
    index.node_instances.push(StarMapNodeInstance {
        instance_id: instance_id.to_string(),
        target_node_id: target_node_id.to_string(),
        position: position.clone(),
        starmap_id: starmap_id.to_string(),
        project_id: project_id.map(ToString::to_string),
        created_at,
        updated_at,
    });
}

pub fn ensure_instance_record(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    instance_id: &str,
    target_node_id: &str,
    position: &StarMapPoint,
    created_at: u64,
    updated_at: u64,
) -> Result<()> {
    let mut index = ensure_library_index(app_data_root)?;
    record_node_instance(
        &mut index,
        starmap_id,
        project_id,
        instance_id,
        target_node_id,
        position,
        created_at,
        updated_at,
    );
    save_library_index(app_data_root, &index)
}

pub fn update_instance_position(
    app_data_root: &Path,
    starmap_id: &str,
    instance_id: &str,
    position: &StarMapPoint,
) -> Result<()> {
    let mut index = load_library_index(app_data_root)?;
    for record in &mut index.node_instances {
        if record.starmap_id == starmap_id && record.instance_id == instance_id {
            record.position = position.clone();
            record.updated_at = crate::starmap::now_epoch();
        }
    }
    save_library_index(app_data_root, &index)
}

pub fn remove_instance_record(
    app_data_root: &Path,
    starmap_id: &str,
    instance_id: &str,
) -> Result<()> {
    remove_instance_records(app_data_root, starmap_id, &[instance_id.to_string()])
}

pub fn remove_instance_records(
    app_data_root: &Path,
    starmap_id: &str,
    instance_ids: &[String],
) -> Result<()> {
    let mut index = load_library_index(app_data_root)?;
    let before = index.node_instances.len();
    index.node_instances.retain(|record| {
        !(record.starmap_id == starmap_id
            && instance_ids.iter().any(|id| id == &record.instance_id))
    });
    if index.node_instances.len() != before {
        save_library_index(app_data_root, &index)?;
    }
    Ok(())
}

/// 整张画布删除后清理该画布的实例反向索引；共享节点本体与库归属保留，
/// 以便未嵌入的素材仍可继续被其它画布引用。
pub fn remove_starmap_instance_records(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    let mut index = load_library_index(app_data_root)?;
    let before = index.node_instances.len();
    index
        .node_instances
        .retain(|record| record.starmap_id != starmap_id);
    if index.node_instances.len() != before {
        save_library_index(app_data_root, &index)?;
    }
    Ok(())
}

/// 把共享定义写进尚未加载的画布实例缓存文件。
///
/// 已加载 Store 由调用方在内存中应用定义并等待 flush；这里只处理其余画布，
/// 避免绕过 Store 写队列。
pub fn sync_definition_to_instance_files(
    app_data_root: &Path,
    definition: &StarMapNodeDefinition,
    loaded_starmaps: &HashSet<String>,
) -> Result<()> {
    let index = load_library_index(app_data_root)?;
    for record in index
        .node_instances
        .iter()
        .filter(|record| record.target_node_id == definition.asset_id)
    {
        if loaded_starmaps.contains(&record.starmap_id) {
            continue;
        }
        let path = crate::starmap::package_storage::starmap_pkg_dir(
            app_data_root,
            &record.starmap_id,
        )
        .join("nodes")
        .join(crate::starmap::package_storage::bucket_for_id(
            &record.instance_id,
        ))
        .join(format!("{}.json", record.instance_id));
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut node) = serde_json::from_str::<StarMapNode>(&content) else {
            continue;
        };
        definition.apply_to_node(&mut node);
        crate::starmap::package_storage::save_node(
            app_data_root,
            &record.starmap_id,
            &node,
        )?;
    }
    Ok(())
}

pub fn asset_id_for_instance(
    app_data_root: &Path,
    starmap_id: &str,
    instance_id: &str,
) -> Result<Option<String>> {
    let index = load_library_index(app_data_root)?;
    Ok(index
        .node_instances
        .iter()
        .find(|record| record.starmap_id == starmap_id && record.instance_id == instance_id)
        .map(|record| record.target_node_id.clone()))
}

pub fn instances_for_asset(
    app_data_root: &Path,
    asset_id: &str,
) -> Result<Vec<StarMapNodeInstance>> {
    let index = load_library_index(app_data_root)?;
    Ok(index
        .node_instances
        .into_iter()
        .filter(|record| record.target_node_id == asset_id)
        .collect())
}

/// 返回某画布中全部节点实例到共享本体的映射，供 Graph DTO 一次带到平台端。
/// 尚未登记映射的旧实例由调用方回退使用 instance_id。
pub fn instance_asset_bindings_for_starmap(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<Vec<(String, String)>> {
    let index = load_library_index(app_data_root)?;
    Ok(index
        .node_instances
        .into_iter()
        .filter(|record| record.starmap_id == starmap_id)
        .map(|record| (record.instance_id, record.target_node_id))
        .collect())
}

pub fn list_library(
    app_data_root: &Path,
    scope: &StarMapAssetScope,
) -> Result<(Vec<StarMapLibraryEntry>, Vec<StarMapLibraryFolder>)> {
    let index = ensure_library_index(app_data_root)?;
    let entries = index
        .entries
        .into_iter()
        .filter(|entry| &entry.scope == scope)
        .collect();
    let folders = index
        .folders
        .into_iter()
        .filter(|folder| &folder.scope == scope)
        .collect();
    Ok((entries, folders))
}

pub fn create_folder(
    app_data_root: &Path,
    scope: &StarMapAssetScope,
    parent_folder_id: Option<String>,
    name: &str,
) -> Result<StarMapLibraryFolder> {
    let mut index = ensure_library_index(app_data_root)?;
    if let Some(parent_id) = parent_folder_id.as_deref() {
        let parent = index
            .folders
            .iter()
            .find(|folder| folder.folder_id == parent_id)
            .ok_or_else(|| Error::Other(format!("library folder not found: {parent_id}")))?;
        if &parent.scope != scope {
            return Err(Error::Other(
                "parent folder belongs to a different library scope".to_string(),
            ));
        }
    }
    let now = crate::starmap::now_epoch();
    let folder = StarMapLibraryFolder {
        folder_id: new_folder_id(),
        parent_folder_id,
        scope: scope.clone(),
        name: name.to_string(),
        created_at: now,
        updated_at: now,
    };
    index.folders.push(folder.clone());
    save_library_index(app_data_root, &index)?;
    Ok(folder)
}

pub fn move_entry(
    app_data_root: &Path,
    entry_id: &str,
    scope: &StarMapAssetScope,
    parent_folder_id: Option<String>,
) -> Result<StarMapLibraryEntry> {
    let mut index = ensure_library_index(app_data_root)?;
    if let Some(parent_id) = parent_folder_id.as_deref() {
        let parent = index
            .folders
            .iter()
            .find(|folder| folder.folder_id == parent_id)
            .ok_or_else(|| Error::Other(format!("library folder not found: {parent_id}")))?;
        if &parent.scope != scope {
            return Err(Error::Other(
                "target folder belongs to a different library scope".to_string(),
            ));
        }
    }
    let entry = index
        .entries
        .iter_mut()
        .find(|entry| entry.entry_id == entry_id)
        .ok_or_else(|| Error::Other(format!("library entry not found: {entry_id}")))?;
    entry.scope = scope.clone();
    entry.parent_folder_id = parent_folder_id;
    entry.updated_at = crate::starmap::now_epoch();
    let result = entry.clone();
    save_library_index(app_data_root, &index)?;
    Ok(result)
}

pub fn remove_entries_for_asset(
    app_data_root: &Path,
    target: &StarMapAssetRef,
) -> Result<()> {
    let mut index = load_library_index(app_data_root)?;
    index.entries.retain(|entry| &entry.target != target);
    save_library_index(app_data_root, &index)
}

pub fn asset_title(app_data_root: &Path, target: &StarMapAssetRef) -> Result<Option<String>> {
    match target.kind {
        StarMapAssetKind::Node => {
            Ok(load_node_definition(app_data_root, &target.asset_id)?
                .map(|definition| definition.title))
        }
        StarMapAssetKind::StarMap => {
            Ok(crate::starmap::get_starmap(app_data_root, &target.asset_id)
                .ok()
                .map(|meta| meta.title))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_entry_does_not_change_target() {
        let root = std::env::temp_dir().join(format!(
            "starmap-library-test-{}",
            uuid::Uuid::new_v4()
        ));
        let mut index = StarMapLibraryIndex::default();
        let entry = ensure_library_entry(
            &mut index,
            &StarMapAssetScope::Global,
            StarMapAssetRef::node("n1"),
            None,
            1,
        );
        save_library_index(&root, &mut index).unwrap();
        let moved = move_entry(
            &root,
            &entry.entry_id,
            &StarMapAssetScope::Project {
                project_id: "p1".to_string(),
            },
            None,
        )
        .unwrap();
        assert_eq!(moved.target.asset_id, "n1");
        let _ = std::fs::remove_dir_all(&root);
    }
}
