//! 全局/作品素材库目录、共享节点头与画布实例反向索引。
//!
//! - `starmaps/library/index.json` 只保存归属/分类（entries、folders）。
//! - `starmaps/library/nodes/<bucket>/<asset_id>.json` 保存唯一可写内容本体。
//! - `starmaps/library/references.json` 是可重建的派生反向索引，不保存位置。
//! - 画布节点文件只保存 `StarMapNodeInstance`，位置属于各自画布。

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

pub(crate) fn reference_index_rel_path() -> PathBuf {
    PathBuf::from("starmaps")
        .join("library")
        .join("references.json")
}

pub(crate) fn reference_index_abs_path(app_data_root: &Path) -> PathBuf {
    app_data_root.join(reference_index_rel_path())
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

pub fn load_reference_index(app_data_root: &Path) -> Result<StarMapReferenceIndex> {
    let path = reference_index_abs_path(app_data_root);
    if !path.exists() {
        return Ok(StarMapReferenceIndex::default());
    }
    let content = fs::read_to_string(&path)?;
    let index: StarMapReferenceIndex = serde_json::from_str(&content)?;
    if index.schema_version != CURRENT_LIBRARY_SCHEMA_VERSION {
        return Err(Error::UnsupportedVersion {
            version: index.schema_version.to_string(),
        });
    }
    Ok(index)
}

pub fn save_reference_index(
    app_data_root: &Path,
    index: &StarMapReferenceIndex,
) -> Result<()> {
    fs::create_dir_all(library_dir(app_data_root))?;
    let mut index = index.clone();
    index.schema_version = CURRENT_LIBRARY_SCHEMA_VERSION;
    index.updated_at = crate::starmap::now_epoch();
    let content = serde_json::to_string_pretty(&index)?;
    atomic_write_string(&reference_index_abs_path(app_data_root), &content)
}

/// 确保库索引存在并完成旧数据拆分；反向索引缺失时从画布实例文件重建。
pub fn ensure_library_index(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let path = library_index_abs_path(app_data_root);
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        let version = value
            .get("schemaVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        if version == CURRENT_LIBRARY_SCHEMA_VERSION {
            let index: StarMapLibraryIndex = serde_json::from_value(value)?;
            if !reference_index_abs_path(app_data_root).exists() {
                rebuild_reference_index(app_data_root)?;
            }
            return Ok(index);
        }
    }
    migrate_library(app_data_root)
}

/// 旧数据迁移：每个旧节点的 `id` 成为共享本体 ID，画布文件改写为实例文件。
///
/// 只执行一次；保留已有库目录归属/folder 和旧版 `nodeInstances` 映射。
pub fn migrate_library(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    // 先把旧图对象存储迁移到当前 schema，再拆分本体/实例；否则旧的完整
    // 节点文件会先被改写成实例文件，导致图迁移缺少 position 源数据。
    crate::starmap::migration::migrate_all_starmap_graphs(app_data_root)?;
    let starmaps_dir = app_data_root.join("starmaps");
    if !starmaps_dir.exists() {
        let index = StarMapLibraryIndex::default();
        save_library_index(app_data_root, &index)?;
        save_reference_index(app_data_root, &StarMapReferenceIndex::default())?;
        return Ok(index);
    }

    let now = crate::starmap::now_epoch();
    let mut index = StarMapLibraryIndex::default();
    index.updated_at = now;
    let mut refs = StarMapReferenceIndex::default();
    refs.updated_at = now;

    let mut legacy_instances: Vec<StarMapNodeInstance> = Vec::new();
    let mut seen_assets = HashSet::new();
    let mut seen_folders = HashSet::new();
    let mut seen_entries = HashSet::new();

    let index_path = library_index_abs_path(app_data_root);
    if index_path.exists() {
        let content = fs::read_to_string(&index_path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
        if let Ok(legacy) = serde_json::from_value::<StarMapLibraryIndex>(value.clone()) {
            for folder in legacy.folders {
                if seen_folders.insert(folder.folder_id.clone()) {
                    index.folders.push(folder);
                }
            }
            for entry in legacy.entries {
                if seen_entries.insert(entry.entry_id.clone()) {
                    seen_assets.insert(asset_key(&entry.target));
                    index.entries.push(entry);
                }
            }
        }
        if let Some(instances) = value.get("nodeInstances") {
            legacy_instances = serde_json::from_value(instances.clone()).unwrap_or_default();
        }
    }

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
        let target = StarMapAssetRef::starmap(starmap_id.clone());
        if seen_assets.insert(asset_key(&target)) {
            index.entries.push(StarMapLibraryEntry {
                entry_id: new_entry_id(),
                parent_folder_id: None,
                scope: scope.clone(),
                target,
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
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };
            let legacy_instance = legacy_instances.iter().find(|record| {
                record.starmap_id == *starmap_id
                    && node_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|id| id == record.instance_id)
                        .unwrap_or(false)
            });

            let (instance, definition) = if value.get("instanceId").is_some() {
                let Ok(instance) = serde_json::from_value::<StarMapNodeInstance>(value) else {
                    continue;
                };
                let definition = load_node_definition(app_data_root, &instance.target_node_id)?;
                let Some(definition) = definition else {
                    continue;
                };
                (instance, definition)
            } else {
                let Ok(node) = serde_json::from_value::<StarMapNode>(value) else {
                    continue;
                };
                let asset_id = legacy_instance
                    .map(|record| record.target_node_id.clone())
                    .unwrap_or_else(|| node.id.clone());
                let definition =
                    StarMapNodeDefinition::from_node(asset_id.clone(), &node);
                let instance = StarMapNodeInstance {
                    instance_id: node.id.clone(),
                    target_node_id: asset_id,
                    position: node.position.clone(),
                    starmap_id: starmap_id.clone(),
                    project_id: meta.project_id.clone(),
                    created_at: node.created_at,
                    updated_at: node.updated_at,
                };
                if !node_definition_abs_path(app_data_root, &definition.asset_id).exists() {
                    save_node_definition(app_data_root, &definition)?;
                }
                crate::starmap::package_storage::save_node_instance(
                    app_data_root,
                    starmap_id,
                    &instance,
                )?;
                (instance, definition)
            };

            let target = StarMapAssetRef::node(instance.target_node_id.clone());
            if seen_assets.insert(asset_key(&target)) {
                index.entries.push(StarMapLibraryEntry {
                    entry_id: new_entry_id(),
                    parent_folder_id: None,
                    scope: scope.clone(),
                    target,
                    created_at: definition.created_at,
                    updated_at: now,
                });
            }
            push_reference(&mut refs, &instance);
        }
    }

    save_library_index(app_data_root, &index)?;
    save_reference_index(app_data_root, &refs)?;
    Ok(index)
}

fn asset_key(target: &StarMapAssetRef) -> String {
    match target.kind {
        StarMapAssetKind::Node => format!("node:{}", target.asset_id),
        StarMapAssetKind::StarMap => format!("starmap:{}", target.asset_id),
    }
}

fn push_reference(index: &mut StarMapReferenceIndex, instance: &StarMapNodeInstance) {
    if index.nodes.iter().any(|record| {
        record.starmap_id == instance.starmap_id && record.instance_id == instance.instance_id
    }) {
        return;
    }
    index.nodes.push(StarMapNodeReference {
        target_node_id: instance.target_node_id.clone(),
        starmap_id: instance.starmap_id.clone(),
        instance_id: instance.instance_id.clone(),
        project_id: instance.project_id.clone(),
    });
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

/// 从全部画布的实例文件重建反向引用索引。位置不进入索引。
pub fn rebuild_reference_index(app_data_root: &Path) -> Result<StarMapReferenceIndex> {
    let starmaps_dir = app_data_root.join("starmaps");
    let mut refs = StarMapReferenceIndex::default();
    if !starmaps_dir.exists() {
        save_reference_index(app_data_root, &refs)?;
        return Ok(refs);
    }
    let mut starmaps = Vec::new();
    for entry in fs::read_dir(&starmaps_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "library" {
            continue;
        }
        starmaps.push(name.to_string());
    }
    for starmap_id in starmaps {
        let nodes_dir = starmaps_dir.join(&starmap_id).join("nodes");
        let mut node_paths = Vec::new();
        collect_json_files(&nodes_dir, &mut node_paths);
        for node_path in node_paths {
            let Ok(content) = fs::read_to_string(&node_path) else {
                continue;
            };
            let Ok(instance) = serde_json::from_str::<StarMapNodeInstance>(&content) else {
                continue;
            };
            if instance.starmap_id == starmap_id {
                push_reference(&mut refs, &instance);
            }
        }
    }
    refs.nodes.sort_by(|a, b| {
        (&a.starmap_id, &a.instance_id).cmp(&(&b.starmap_id, &b.instance_id))
    });
    save_reference_index(app_data_root, &refs)?;
    Ok(refs)
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

/// 新增或复用共享节点头，并登记库归属与画布反向引用。
///
/// 位置不写入任何中央索引；共享内容只在定义文件不存在时初始化。
pub fn ensure_node_asset(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    asset_id: &str,
    instance_id: &str,
    node: &StarMapNode,
    _position: &StarMapPoint,
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
    let before = index.entries.len();
    ensure_library_entry(
        &mut index,
        &scope,
        StarMapAssetRef::node(asset_id),
        None,
        crate::starmap::now_epoch(),
    );
    if index.entries.len() != before {
        save_library_index(app_data_root, &index)?;
    }
    record_node_reference(
        app_data_root,
        starmap_id,
        project_id,
        instance_id,
        asset_id,
    )?;
    Ok(asset_id.to_string())
}

pub fn ensure_starmap_asset(
    app_data_root: &Path,
    meta: &crate::starmap::StarMapMeta,
) -> Result<StarMapLibraryEntry> {
    let mut index = ensure_library_index(app_data_root)?;
    let scope = StarMapAssetScope::from_project_id(meta.project_id.as_deref());
    let before = index.entries.len();
    let entry = ensure_library_entry(
        &mut index,
        &scope,
        StarMapAssetRef::starmap(meta.starmap_id.clone()),
        None,
        crate::starmap::now_epoch(),
    );
    if index.entries.len() != before {
        save_library_index(app_data_root, &index)?;
    }
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

pub fn record_node_reference(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    instance_id: &str,
    target_node_id: &str,
) -> Result<()> {
    let mut refs = load_reference_index(app_data_root)?;
    let project_id = project_id.map(ToString::to_string);
    let mut changed = false;
    if let Some(record) = refs
        .nodes
        .iter_mut()
        .find(|record| record.starmap_id == starmap_id && record.instance_id == instance_id)
    {
        if record.target_node_id != target_node_id || record.project_id != project_id {
            record.target_node_id = target_node_id.to_string();
            record.project_id = project_id;
            changed = true;
        }
    } else {
        refs.nodes.push(StarMapNodeReference {
            target_node_id: target_node_id.to_string(),
            starmap_id: starmap_id.to_string(),
            instance_id: instance_id.to_string(),
            project_id,
        });
        changed = true;
    }
    if changed {
        save_reference_index(app_data_root, &refs)?;
    }
    Ok(())
}

/// 兼容旧调用：实例位置已只存在于画布文件中，这里只登记反向引用。
pub fn ensure_instance_record(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    instance_id: &str,
    target_node_id: &str,
    _position: &StarMapPoint,
    _created_at: u64,
    _updated_at: u64,
) -> Result<()> {
    record_node_reference(
        app_data_root,
        starmap_id,
        project_id,
        instance_id,
        target_node_id,
    )
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
    let mut refs = load_reference_index(app_data_root)?;
    let before = refs.nodes.len();
    refs.nodes.retain(|record| {
        !(record.starmap_id == starmap_id
            && instance_ids.iter().any(|id| id == &record.instance_id))
    });
    if refs.nodes.len() != before {
        save_reference_index(app_data_root, &refs)?;
    }
    Ok(())
}

/// 整张画布删除后清理该画布的反向引用；共享本体与库归属保留。
pub fn remove_starmap_instance_records(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    let mut refs = load_reference_index(app_data_root)?;
    let before = refs.nodes.len();
    refs.nodes.retain(|record| record.starmap_id != starmap_id);
    if refs.nodes.len() != before {
        save_reference_index(app_data_root, &refs)?;
    }
    Ok(())
}

pub fn asset_id_for_instance(
    app_data_root: &Path,
    starmap_id: &str,
    instance_id: &str,
) -> Result<Option<String>> {
    let refs = load_reference_index(app_data_root)?;
    Ok(refs
        .nodes
        .iter()
        .find(|record| record.starmap_id == starmap_id && record.instance_id == instance_id)
        .map(|record| record.target_node_id.clone()))
}

/// 反向引用；位置字段仅为旧调用兼容，不再从中央索引提供。
pub fn instances_for_asset(
    app_data_root: &Path,
    asset_id: &str,
) -> Result<Vec<StarMapNodeInstance>> {
    let refs = load_reference_index(app_data_root)?;
    Ok(refs
        .nodes
        .into_iter()
        .filter(|record| record.target_node_id == asset_id)
        .map(|record| StarMapNodeInstance {
            instance_id: record.instance_id,
            target_node_id: record.target_node_id,
            position: StarMapPoint::default(),
            starmap_id: record.starmap_id,
            project_id: record.project_id,
            created_at: 0,
            updated_at: 0,
        })
        .collect())
}

/// 返回某画布中全部节点实例到共享本体的映射，供 Graph DTO 一次带到平台端。
pub fn instance_asset_bindings_for_starmap(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<Vec<(String, String)>> {
    let refs = load_reference_index(app_data_root)?;
    Ok(refs
        .nodes
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
    let mut index = ensure_library_index(app_data_root)?;
    let before = index.entries.len();
    index.entries.retain(|entry| &entry.target != target);
    if index.entries.len() != before {
        save_library_index(app_data_root, &index)?;
    }
    Ok(())
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
        save_library_index(&root, &index).unwrap();
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
