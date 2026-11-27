//! 全局/作品素材库目录、共享节点头与画布实例反向索引。
//!
//! - `starmaps/library/entries/<entry_id>.json` 是素材归属的唯一事实来源。
//! - `starmaps/library/folders/<folder_id>.json` 是库目录的唯一事实来源。
//! - `starmaps/library/index.json` 是由独立对象文件重建的派生缓存，不是事实来源。
//! - `starmaps/library/nodes/<bucket>/<asset_id>.json` 保存唯一可写内容本体。
//! - `starmaps/library/references.json` 是可重建的派生反向索引，不保存位置。
//! - 画布节点文件只保存 `StarMapNodeInstance`，位置属于各自画布。
//!
//! 删除以对象文件内的 `deletedAt` tombstone 表达；加载时过滤 tombstone，
//! 不把记录从独立文件中直接移除，避免同步无法区分删除与未同步的新增。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

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

pub(crate) fn library_entries_dir(app_data_root: &Path) -> PathBuf {
    library_dir(app_data_root).join("entries")
}

pub(crate) fn library_folders_dir(app_data_root: &Path) -> PathBuf {
    library_dir(app_data_root).join("folders")
}

pub(crate) fn library_entry_rel_path(entry_id: &str) -> PathBuf {
    PathBuf::from("starmaps")
        .join("library")
        .join("entries")
        .join(format!("{entry_id}.json"))
}

pub(crate) fn library_entry_abs_path(app_data_root: &Path, entry_id: &str) -> PathBuf {
    app_data_root.join(library_entry_rel_path(entry_id))
}

pub(crate) fn library_folder_rel_path(folder_id: &str) -> PathBuf {
    PathBuf::from("starmaps")
        .join("library")
        .join("folders")
        .join(format!("{folder_id}.json"))
}

pub(crate) fn library_folder_abs_path(app_data_root: &Path, folder_id: &str) -> PathBuf {
    app_data_root.join(library_folder_rel_path(folder_id))
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

/// 读取聚合库索引。
///
/// 存在独立对象目录时以 `entries/*.json`、`folders/*.json` 为唯一事实来源；
/// 只有旧 `index.json` 时执行一次性拆分（不覆盖已有对象文件），此后
/// `index.json` 仍作为派生缓存保留。
pub fn load_library_index(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let has_objects =
        library_entries_dir(app_data_root).exists() || library_folders_dir(app_data_root).exists();
    if has_objects {
        return load_library_objects(app_data_root);
    }
    if library_index_abs_path(app_data_root).exists() {
        migrate_legacy_library_index(app_data_root)?;
        return load_library_objects(app_data_root);
    }
    Ok(StarMapLibraryIndex::default())
}

/// 写入聚合缓存 `index.json`。
///
/// 这是由独立对象文件聚合出的派生缓存，不是事实来源；任何写路径都必须先写
/// `entries/`、`folders/` 对象文件。
pub fn save_library_index(app_data_root: &Path, index: &StarMapLibraryIndex) -> Result<()> {
    fs::create_dir_all(library_dir(app_data_root))?;
    let mut index = index.clone();
    index.schema_version = CURRENT_LIBRARY_SCHEMA_VERSION;
    index.updated_at = crate::starmap::now_epoch();
    let content = serde_json::to_string_pretty(&index)?;
    atomic_write_string(&library_index_abs_path(app_data_root), &content)
}

/// 从两个独立对象目录读取聚合索引。
fn load_library_objects(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let entries = read_library_records::<StarMapLibraryEntry>(&library_entries_dir(app_data_root))?;
    let folders =
        read_library_records::<StarMapLibraryFolder>(&library_folders_dir(app_data_root))?;
    Ok(aggregate_library_objects(entries, folders))
}

/// 读取目录下全部 `*.json` 对象；目录不存在视为空，单个坏文件返回错误。
fn read_library_records<T: DeserializeOwned>(dir: &Path) -> Result<Vec<T>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        let content = fs::read_to_string(&path)?;
        let record = serde_json::from_str::<T>(&content).map_err(|error| {
            Error::Other(format!(
                "invalid library object {}: {error}",
                path.display()
            ))
        })?;
        records.push(record);
    }
    Ok(records)
}

/// 过滤 tombstone、修复悬空父目录并按稳定 ID 排序。
fn aggregate_library_objects(
    mut entries: Vec<StarMapLibraryEntry>,
    mut folders: Vec<StarMapLibraryFolder>,
) -> StarMapLibraryIndex {
    folders.retain(|folder| folder.deleted_at.is_none());
    entries.retain(|entry| entry.deleted_at.is_none());
    let folder_ids: HashSet<&str> = folders
        .iter()
        .map(|folder| folder.folder_id.as_str())
        .collect();
    for entry in &mut entries {
        if entry
            .parent_folder_id
            .as_deref()
            .is_some_and(|parent| !folder_ids.contains(parent))
        {
            entry.parent_folder_id = None;
        }
    }
    entries.sort_by(|left, right| left.entry_id.cmp(&right.entry_id));
    folders.sort_by(|left, right| left.folder_id.cmp(&right.folder_id));
    let updated_at = entries
        .iter()
        .map(|entry| entry.updated_at)
        .chain(folders.iter().map(|folder| folder.updated_at))
        .max()
        .unwrap_or(0);
    StarMapLibraryIndex {
        schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
        entries,
        folders,
        updated_at,
    }
}

/// 把旧 `index.json` 中的归属记录逐条写成独立对象文件；已有文件不覆盖。
fn migrate_legacy_library_index(app_data_root: &Path) -> Result<()> {
    let path = library_index_abs_path(app_data_root);
    let content = fs::read_to_string(&path)?;
    let legacy: StarMapLibraryIndex = serde_json::from_str(&content)?;
    fs::create_dir_all(library_entries_dir(app_data_root))?;
    fs::create_dir_all(library_folders_dir(app_data_root))?;
    for folder in &legacy.folders {
        if !library_folder_abs_path(app_data_root, &folder.folder_id).exists() {
            save_library_folder(app_data_root, folder)?;
        }
    }
    for entry in &legacy.entries {
        if !library_entry_abs_path(app_data_root, &entry.entry_id).exists() {
            save_library_entry(app_data_root, entry)?;
        }
    }
    Ok(())
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

pub fn save_reference_index(app_data_root: &Path, index: &StarMapReferenceIndex) -> Result<()> {
    fs::create_dir_all(library_dir(app_data_root))?;
    let mut index = index.clone();
    index.schema_version = CURRENT_LIBRARY_SCHEMA_VERSION;
    index.updated_at = crate::starmap::now_epoch();
    let content = serde_json::to_string_pretty(&index)?;
    atomic_write_string(&reference_index_abs_path(app_data_root), &content)
}

/// 确保库目录已初始化，并把反向引用索引收敛为可从实例补全的派生视图。
///
/// 首次进入（既没有独立对象文件也没有旧索引）时执行旧数据拆分，保证旧画布/
/// 星图对象转换成独立归属文件。已初始化时由
/// [`reconcile_reference_index`] 一次性补回缺失引用与落后的 `projectId`；
/// 已有绑定与实例资产身份冲突时保留原绑定并返回错误，不静默覆盖身份冲突
/// 材料。批量收敛只写一次索引，不逐节点重复读写全量索引。
pub fn ensure_library_index(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    let initialized = library_entries_dir(app_data_root).exists()
        || library_folders_dir(app_data_root).exists()
        || library_index_abs_path(app_data_root).exists();
    if !initialized {
        return migrate_library(app_data_root);
    }
    let index = load_library_index(app_data_root)?;
    let reconciliation = reconcile_reference_index(app_data_root)?;
    if let Some(conflict) = reconciliation.conflicts.first() {
        return Err(Error::Other(format!(
            "starmap reference index conflict: {} binding(s) disagree with confirmed node \
             instances; first: {}; preserve existing references and node bytes",
            reconciliation.conflicts.len(),
            conflict.describe()
        )));
    }
    Ok(index)
}

/// 旧数据迁移：每个旧节点的 `id` 成为共享本体 ID，画布文件改写为实例文件。
///
/// 旧 `index.json` 中的 entries/folders 先由 [`load_library_index`] 拆成独立
/// 对象文件（保留原 ID/时间戳/scope/target/parent），再为旧图对象补齐缺失
/// 的归属文件。幂等：已有独立文件不覆盖。
pub fn migrate_library(app_data_root: &Path) -> Result<StarMapLibraryIndex> {
    // 先把旧图对象存储迁移到当前 schema，再拆分本体/实例；否则旧的完整
    // 节点文件会先被改写成实例文件，导致图迁移缺少 position 源数据。
    crate::starmap::migration::migrate_all_starmap_graphs(app_data_root)?;
    let starmaps_dir = app_data_root.join("starmaps");
    if !starmaps_dir.exists() {
        let index = StarMapLibraryIndex::default();
        fs::create_dir_all(library_entries_dir(app_data_root))?;
        fs::create_dir_all(library_folders_dir(app_data_root))?;
        save_library_index(app_data_root, &index)?;
        save_reference_index(app_data_root, &StarMapReferenceIndex::default())?;
        return Ok(index);
    }

    let now = crate::starmap::now_epoch();
    let mut index = load_library_index(app_data_root)?;
    index.updated_at = now;
    let mut refs = StarMapReferenceIndex::default();
    refs.updated_at = now;

    let mut legacy_instances: Vec<StarMapNodeInstance> = Vec::new();
    let mut seen_assets: HashSet<String> = index
        .entries
        .iter()
        .map(|entry| asset_key(&entry.target))
        .collect();

    let index_path = library_index_abs_path(app_data_root);
    if index_path.exists() {
        let content = fs::read_to_string(&index_path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;
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
                deleted_at: None,
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
                let definition = load_node_definition(app_data_root, &instance.asset_id)?;
                let Some(definition) = definition else {
                    continue;
                };
                (instance, definition)
            } else {
                let Ok(node) = serde_json::from_value::<StarMapNode>(value) else {
                    continue;
                };
                let asset_id = legacy_instance
                    .map(|record| record.asset_id.clone())
                    .unwrap_or_else(|| node.id.clone());
                let definition = StarMapNodeDefinition::from_node(asset_id.clone(), &node);
                let instance = StarMapNodeInstance {
                    instance_id: node.id.clone(),
                    asset_id: asset_id,
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

            let target = StarMapAssetRef::node(instance.asset_id.clone());
            if seen_assets.insert(asset_key(&target)) {
                index.entries.push(StarMapLibraryEntry {
                    entry_id: new_entry_id(),
                    parent_folder_id: None,
                    scope: scope.clone(),
                    target,
                    created_at: definition.created_at,
                    updated_at: now,
                    deleted_at: None,
                });
            }
            push_reference(&mut refs, &instance);
        }
    }

    persist_missing_library_objects(app_data_root, &index)?;
    save_library_index(app_data_root, &index)?;
    save_reference_index(app_data_root, &refs)?;
    Ok(index)
}

/// 把聚合索引里尚未落到独立文件的记录补齐；已有文件不覆盖。
fn persist_missing_library_objects(
    app_data_root: &Path,
    index: &StarMapLibraryIndex,
) -> Result<()> {
    fs::create_dir_all(library_entries_dir(app_data_root))?;
    fs::create_dir_all(library_folders_dir(app_data_root))?;
    for folder in &index.folders {
        if !library_folder_abs_path(app_data_root, &folder.folder_id).exists() {
            save_library_folder(app_data_root, folder)?;
        }
    }
    for entry in &index.entries {
        if !library_entry_abs_path(app_data_root, &entry.entry_id).exists() {
            save_library_entry(app_data_root, entry)?;
        }
    }
    Ok(())
}

fn asset_key(target: &StarMapAssetRef) -> String {
    match target.kind {
        StarMapAssetKind::Node => format!("node:{}", target.asset_id),
        StarMapAssetKind::StarMap => format!("starmap:{}", target.asset_id),
    }
}

fn push_reference(index: &mut StarMapReferenceIndex, instance: &StarMapNodeInstance) {
    if instance.asset_id.trim().is_empty() || instance.instance_id.is_empty() {
        return;
    }
    if index.nodes.iter().any(|record| {
        record.starmap_id == instance.starmap_id && record.instance_id == instance.instance_id
    }) {
        return;
    }
    index.nodes.push(StarMapNodeReference {
        asset_id: instance.asset_id.clone(),
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

/// 一次反向引用索引收敛的统计。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceIndexReconciliation {
    /// 从已确认画布实例补回的缺失引用数。
    pub added: usize,
    /// 按实例事实补齐 `projectId` 的记录数。
    pub updated: usize,
    /// 既有绑定与实例资产身份冲突、为保留材料而未覆盖的记录。
    pub conflicts: Vec<ReferenceIndexConflict>,
}

impl ReferenceIndexReconciliation {
    fn is_empty(&self) -> bool {
        self.added == 0 && self.updated == 0
    }
}

/// 反向引用索引与已确认实例的资产身份冲突。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReferenceIndexConflict {
    pub starmap_id: String,
    pub instance_id: String,
    /// 画布实例文件里的资产身份（事实）。
    pub instance_asset_id: String,
    /// `references.json` 既有记录里的资产身份（保留不覆盖）。
    pub reference_asset_id: String,
}

impl ReferenceIndexConflict {
    fn describe(&self) -> String {
        format!(
            "starmap_id={} instance_id={} instance_asset_id={} reference_asset_id={}",
            self.starmap_id, self.instance_id, self.instance_asset_id, self.reference_asset_id
        )
    }
}

/// 扫描全部画布实例文件，返回可确认的实例事实。
///
/// 已确认 = 文件名与 `instanceId` 一致、`starmapId` 与所在画布一致（旧端缺
/// `starmapId` 时按所在画布补全）、`assetId` 非空。旧格式完整节点、损坏文件
/// 都不是已确认实例，不参与派生视图，也不据此删除既有引用材料。
fn confirmed_node_instances(app_data_root: &Path) -> Result<Vec<StarMapNodeInstance>> {
    let starmaps_dir = app_data_root.join("starmaps");
    let mut instances = Vec::new();
    if !starmaps_dir.exists() {
        return Ok(instances);
    }
    let mut starmap_ids = Vec::new();
    for entry in fs::read_dir(&starmaps_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "library" {
            continue;
        }
        starmap_ids.push(name.to_string());
    }
    starmap_ids.sort();
    for starmap_id in starmap_ids {
        let nodes_dir = starmaps_dir.join(&starmap_id).join("nodes");
        let mut node_paths = Vec::new();
        collect_json_files(&nodes_dir, &mut node_paths);
        node_paths.sort();
        for node_path in node_paths {
            let Ok(content) = fs::read_to_string(&node_path) else {
                continue;
            };
            let Ok(mut instance) = serde_json::from_str::<StarMapNodeInstance>(&content) else {
                continue;
            };
            let Some(file_stem) = node_path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if instance.instance_id != file_stem {
                continue;
            }
            if instance.starmap_id.is_empty() {
                instance.starmap_id = starmap_id.clone();
            } else if instance.starmap_id != starmap_id {
                continue;
            }
            if instance.asset_id.trim().is_empty() {
                continue;
            }
            instances.push(instance);
        }
    }
    Ok(instances)
}

/// 按已确认实例事实生成一份完整的派生索引（不写盘）。
fn derive_reference_index_from_instances(app_data_root: &Path) -> Result<StarMapReferenceIndex> {
    let mut refs = StarMapReferenceIndex::default();
    for instance in confirmed_node_instances(app_data_root)? {
        push_reference(&mut refs, &instance);
    }
    sort_references(&mut refs.nodes);
    Ok(refs)
}

fn sort_references(nodes: &mut [StarMapNodeReference]) {
    nodes.sort_by(|left, right| {
        (&left.starmap_id, &left.instance_id).cmp(&(&right.starmap_id, &right.instance_id))
    });
}

/// 读取反向索引供收敛比较；文件缺失或损坏时返回 `None`（按实例事实重建），
/// 版本不支持仍然 fail-closed。
fn load_reference_index_for_reconcile(
    app_data_root: &Path,
) -> Result<Option<StarMapReferenceIndex>> {
    let path = reference_index_abs_path(app_data_root);
    if !path.exists() {
        return Ok(None);
    }
    match load_reference_index(app_data_root) {
        Ok(index) => Ok(Some(index)),
        Err(Error::UnsupportedVersion { version }) => Err(Error::UnsupportedVersion { version }),
        Err(error) => {
            log::warn!(
                "starmap reference index unreadable at {}, rebuilding from confirmed instances: {error}",
                path.display()
            );
            Ok(None)
        }
    }
}

/// 读取反向索引供写入合并；文件缺失视为空索引，损坏时从已确认实例重建
/// （不写盘，由调用方合并后一次写入）。
fn load_reference_index_for_update(app_data_root: &Path) -> Result<StarMapReferenceIndex> {
    match load_reference_index(app_data_root) {
        Ok(index) => Ok(index),
        Err(Error::UnsupportedVersion { version }) => Err(Error::UnsupportedVersion { version }),
        Err(error) => {
            log::warn!(
                "starmap reference index unreadable at {}, rebuilding from confirmed instances: {error}",
                reference_index_abs_path(app_data_root).display()
            );
            derive_reference_index_from_instances(app_data_root)
        }
    }
}

/// 加载反向引用索引；整体缺失或损坏时按已确认实例批量重建一次。
///
/// 正常加载只需要补自己缺的那一条；但整个派生索引不可用时必须批量恢复，
/// 否则逐节点补写会反复重写全量索引。
pub(crate) fn load_or_rebuild_reference_index(
    app_data_root: &Path,
) -> Result<StarMapReferenceIndex> {
    let path = reference_index_abs_path(app_data_root);
    if !path.exists() {
        return rebuild_reference_index(app_data_root);
    }
    match load_reference_index(app_data_root) {
        Ok(index) => Ok(index),
        Err(Error::UnsupportedVersion { version }) => Err(Error::UnsupportedVersion { version }),
        Err(error) => {
            log::warn!(
                "starmap reference index unreadable at {}, rebuilding from confirmed instances: {error}",
                path.display()
            );
            rebuild_reference_index(app_data_root)
        }
    }
}

/// 从已确认画布实例收敛反向引用索引（派生视图），一次写盘。
///
/// - 索引缺失/损坏：按实例事实重建后写回（即使为空也保持文件存在）；
/// - 索引存在但缺项：只补缺失记录；
/// - 记录与实例资产一致、`projectId` 落后：按实例事实补齐（实例是归属事实，
///   索引已有归属不会被实例的空值抹掉）；
/// - 记录与实例资产冲突：保留既有绑定不覆盖，冲突通过返回值交给调用方；
/// - 已确认实例覆盖不到的既有记录保留：`record_node_reference` 先于实例
///   文件落盘，记录可能存在而文件尚未写入，不能据此删除派生材料。
///
/// 有冲突时不写盘：既有绑定和实例字节都保持原样。
pub(crate) fn reconcile_reference_index(
    app_data_root: &Path,
) -> Result<ReferenceIndexReconciliation> {
    let mut facts: std::collections::HashMap<(String, String), StarMapNodeReference> =
        std::collections::HashMap::new();
    for instance in confirmed_node_instances(app_data_root)? {
        facts
            .entry((instance.starmap_id.clone(), instance.instance_id.clone()))
            .or_insert_with(|| StarMapNodeReference {
                asset_id: instance.asset_id.clone(),
                starmap_id: instance.starmap_id.clone(),
                instance_id: instance.instance_id.clone(),
                project_id: instance.project_id.clone(),
            });
    }

    let existing = load_reference_index_for_reconcile(app_data_root)?;
    let force_write = existing.is_none();
    let existing_records = existing.map(|index| index.nodes).unwrap_or_default();

    let mut reconciliation = ReferenceIndexReconciliation::default();
    let mut merged: Vec<StarMapNodeReference> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for record in &existing_records {
        let key = (record.starmap_id.clone(), record.instance_id.clone());
        if !seen.insert(key.clone()) {
            // 派生视图不保存重复绑定；既有文件里的重复项按坏数据收敛掉。
            reconciliation.updated += 1;
            continue;
        }
        let Some(fact) = facts.get(&key) else {
            merged.push(record.clone());
            continue;
        };
        if record.asset_id != fact.asset_id {
            reconciliation.conflicts.push(ReferenceIndexConflict {
                starmap_id: record.starmap_id.clone(),
                instance_id: record.instance_id.clone(),
                instance_asset_id: fact.asset_id.clone(),
                reference_asset_id: record.asset_id.clone(),
            });
            continue;
        }
        let mut record = record.clone();
        let next_project_id = fact
            .project_id
            .clone()
            .or_else(|| record.project_id.clone());
        if record.project_id != next_project_id {
            record.project_id = next_project_id;
            reconciliation.updated += 1;
        }
        merged.push(record);
    }
    for (key, fact) in &facts {
        if seen.contains(key) {
            continue;
        }
        merged.push(fact.clone());
        reconciliation.added += 1;
    }

    if reconciliation.conflicts.is_empty() && (force_write || !reconciliation.is_empty()) {
        sort_references(&mut merged);
        let refs = StarMapReferenceIndex {
            schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
            nodes: merged,
            updated_at: crate::starmap::now_epoch(),
        };
        save_reference_index(app_data_root, &refs)?;
    }
    Ok(reconciliation)
}

/// 从全部画布的实例文件重建反向引用索引。位置不进入索引。
///
/// 这是显式恢复入口：调用方已判定派生索引不可信（整体缺失、损坏、同步落盘
/// 后），以实例文件事实为准整体重写。正常加载的缺项补写与冲突保留走
/// [`reconcile_reference_index`]。
pub fn rebuild_reference_index(app_data_root: &Path) -> Result<StarMapReferenceIndex> {
    let mut refs = derive_reference_index_from_instances(app_data_root)?;
    refs.updated_at = crate::starmap::now_epoch();
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

/// 原子写入单条素材归属对象，返回相对路径。
pub(crate) fn save_library_entry(
    app_data_root: &Path,
    entry: &StarMapLibraryEntry,
) -> Result<PathBuf> {
    let rel_path = library_entry_rel_path(&entry.entry_id);
    let path = app_data_root.join(&rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(entry)?;
    atomic_write_string(&path, &content)?;
    Ok(rel_path)
}

fn read_library_entry(app_data_root: &Path, entry_id: &str) -> Result<Option<StarMapLibraryEntry>> {
    let path = library_entry_abs_path(app_data_root, entry_id);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&content)?))
}

/// 原子写入单个库目录对象，返回相对路径。
pub(crate) fn save_library_folder(
    app_data_root: &Path,
    folder: &StarMapLibraryFolder,
) -> Result<PathBuf> {
    let rel_path = library_folder_rel_path(&folder.folder_id);
    let path = app_data_root.join(&rel_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(folder)?;
    atomic_write_string(&path, &content)?;
    Ok(rel_path)
}

fn read_library_folder(
    app_data_root: &Path,
    folder_id: &str,
) -> Result<Option<StarMapLibraryFolder>> {
    let path = library_folder_abs_path(app_data_root, folder_id);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&content)?))
}

/// 把 entry 标记为 tombstone 并写回；不存在时返回 `Ok(None)`。
pub(crate) fn tombstone_library_entry(
    app_data_root: &Path,
    entry_id: &str,
    now: u64,
) -> Result<Option<PathBuf>> {
    let Some(mut entry) = read_library_entry(app_data_root, entry_id)? else {
        return Ok(None);
    };
    entry.deleted_at = Some(now);
    entry.updated_at = now.max(entry.updated_at.saturating_add(1));
    Ok(Some(save_library_entry(app_data_root, &entry)?))
}

/// 把 folder 标记为 tombstone 并写回；不存在时返回 `Ok(None)`。
pub fn tombstone_library_folder(
    app_data_root: &Path,
    folder_id: &str,
    now: u64,
) -> Result<Option<PathBuf>> {
    let Some(mut folder) = read_library_folder(app_data_root, folder_id)? else {
        return Ok(None);
    };
    folder.deleted_at = Some(now);
    folder.updated_at = now.max(folder.updated_at.saturating_add(1));
    Ok(Some(save_library_folder(app_data_root, &folder)?))
}

/// 从独立对象文件重建并写出 `index.json` 派生缓存。
///
/// 没有任何对象文件时保持无操作；旧布局存在 `index.json` 时先完成一次拆分，
/// 避免用空聚合覆盖旧缓存。
pub(crate) fn refresh_library_index_cache(app_data_root: &Path) -> Result<()> {
    let has_objects =
        library_entries_dir(app_data_root).exists() || library_folders_dir(app_data_root).exists();
    if !has_objects {
        if library_index_abs_path(app_data_root).exists() {
            migrate_legacy_library_index(app_data_root)?;
        } else {
            return Ok(());
        }
    }
    let index = load_library_objects(app_data_root)?;
    save_library_index(app_data_root, &index)
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
        deleted_at: None,
    };
    index.entries.push(entry.clone());
    entry
}

/// 纯内存 [`ensure_library_entry`] 的持久化版本：新 entry 写独立对象文件并
/// 刷新聚合缓存，已有 entry 不重写。
pub(crate) fn ensure_library_entry_persisted(
    app_data_root: &Path,
    scope: &StarMapAssetScope,
    target: StarMapAssetRef,
    parent_folder_id: Option<String>,
    timestamp: u64,
) -> Result<StarMapLibraryEntry> {
    let mut index = ensure_library_index(app_data_root)?;
    if let Some(existing) = index
        .entries
        .iter()
        .find(|entry| entry.target == target)
        .cloned()
    {
        return Ok(existing);
    }
    let entry = ensure_library_entry(&mut index, scope, target, parent_folder_id, timestamp);
    save_library_entry(app_data_root, &entry)?;
    refresh_library_index_cache(app_data_root)?;
    Ok(entry)
}

/// 创建共享素材本体与库归属；不登记画布引用，也不写实例记录。
///
/// Issue #1060：普通编辑不写库索引。本函数只在**首次创建素材**（新建节点、
/// 复制素材）时调用；已有素材直接返回，不写独立归属文件、不刷新缓存。
pub fn create_node_asset(
    app_data_root: &Path,
    project_id: Option<&str>,
    asset_id: &str,
    definition: &StarMapNodeDefinition,
) -> Result<()> {
    let definition_path = node_definition_abs_path(app_data_root, asset_id);
    if !definition_path.exists() {
        save_node_definition(app_data_root, definition)?;
    }
    let scope = StarMapAssetScope::from_project_id(project_id);
    ensure_library_entry_persisted(
        app_data_root,
        &scope,
        StarMapAssetRef::node(asset_id),
        None,
        crate::starmap::now_epoch(),
    )?;
    Ok(())
}

/// 把素材放置到画布：只登记派生反向引用，不写素材本体与库索引。
pub fn place_node_instance(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    instance_id: &str,
    asset_id: &str,
) -> Result<()> {
    record_node_reference(app_data_root, starmap_id, project_id, instance_id, asset_id)
}

/// 兼容入口：创建素材 + 登记实例引用。
///
/// 新代码应显式调用 [`create_node_asset`] 与 [`place_node_instance`]，
/// 避免每次编辑都走"创建素材"路径。
pub fn ensure_node_asset(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    asset_id: &str,
    instance_id: &str,
    node: &StarMapNode,
    _position: &StarMapPoint,
) -> Result<String> {
    create_node_asset(
        app_data_root,
        project_id,
        asset_id,
        &StarMapNodeDefinition::from_node(asset_id.to_string(), node),
    )?;
    place_node_instance(app_data_root, starmap_id, project_id, instance_id, asset_id)?;
    Ok(asset_id.to_string())
}

pub fn ensure_starmap_asset(
    app_data_root: &Path,
    meta: &crate::starmap::StarMapMeta,
) -> Result<StarMapLibraryEntry> {
    let scope = StarMapAssetScope::from_project_id(meta.project_id.as_deref());
    ensure_library_entry_persisted(
        app_data_root,
        &scope,
        StarMapAssetRef::starmap(meta.starmap_id.clone()),
        None,
        crate::starmap::now_epoch(),
    )
}

/// 把已有或新建的素材 entry 放到指定 scope/文件夹；只改归属，不改本体 ID。
///
/// 已 tombstone 的 entry 不在聚合索引里，不能被移动或复用。
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
            deleted_at: None,
        };
        index.entries.push(entry.clone());
        entry
    };
    save_library_entry(app_data_root, &entry)?;
    refresh_library_index_cache(app_data_root)?;
    Ok(entry)
}

/// 登记一条画布实例到共享本体的反向引用。
///
/// - 缺失 → 追加；
/// - 已存在且 `assetId` 相同 → 只补齐 `projectId`（实例是归属事实；索引里
///   已有的项目归属不会被空值抹掉）；
/// - 已存在但指向另一个 `assetId` → 返回错误，保留既有绑定材料，绝不静默
///   改写身份。
pub fn record_node_reference(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    instance_id: &str,
    asset_id: &str,
) -> Result<()> {
    let mut refs = load_reference_index_for_update(app_data_root)?;
    let project_id = project_id.map(ToString::to_string);
    let mut changed = false;
    if let Some(record) = refs
        .nodes
        .iter_mut()
        .find(|record| record.starmap_id == starmap_id && record.instance_id == instance_id)
    {
        if record.asset_id != asset_id {
            return Err(Error::Other(format!(
                "starmap node reference conflict: starmap_id={starmap_id} \
                 instance_id={instance_id} existing_asset_id={} incoming_asset_id={asset_id}; \
                 preserve existing reference",
                record.asset_id
            )));
        }
        let next_project_id = match project_id {
            Some(project_id) => Some(project_id),
            None => record.project_id.clone(),
        };
        if record.project_id != next_project_id {
            record.project_id = next_project_id;
            changed = true;
        }
    } else {
        refs.nodes.push(StarMapNodeReference {
            asset_id: asset_id.to_string(),
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
    asset_id: &str,
    _position: &StarMapPoint,
    _created_at: u64,
    _updated_at: u64,
) -> Result<()> {
    record_node_reference(app_data_root, starmap_id, project_id, instance_id, asset_id)
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
        .map(|record| record.asset_id.clone()))
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
        .filter(|record| record.asset_id == asset_id)
        .map(|record| StarMapNodeInstance {
            instance_id: record.instance_id,
            asset_id: record.asset_id,
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
        .map(|record| (record.instance_id, record.asset_id))
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

/// 按素材查找库归属条目（跨 scope）。
///
/// 写入方在创建素材后用它把独立 entry 文件登记进同一份变更事实；
/// tombstone 条目已被 `load_library_index` 过滤，不会返回。
pub fn find_library_entry(
    app_data_root: &Path,
    target: &StarMapAssetRef,
) -> Result<Option<StarMapLibraryEntry>> {
    Ok(load_library_index(app_data_root)?
        .entries
        .into_iter()
        .find(|entry| &entry.target == target))
}

pub fn create_folder(
    app_data_root: &Path,
    scope: &StarMapAssetScope,
    parent_folder_id: Option<String>,
    name: &str,
) -> Result<StarMapLibraryFolder> {
    let index = ensure_library_index(app_data_root)?;
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
        deleted_at: None,
    };
    save_library_folder(app_data_root, &folder)?;
    refresh_library_index_cache(app_data_root)?;
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
    save_library_entry(app_data_root, &result)?;
    refresh_library_index_cache(app_data_root)?;
    Ok(result)
}

/// 对匹配素材的全部 active entry 写 tombstone，再刷新派生缓存。
pub fn remove_entries_for_asset(app_data_root: &Path, target: &StarMapAssetRef) -> Result<()> {
    let index = ensure_library_index(app_data_root)?;
    let now = crate::starmap::now_epoch();
    let mut changed = false;
    for entry in index.entries.iter().filter(|entry| &entry.target == target) {
        if tombstone_library_entry(app_data_root, &entry.entry_id, now)?.is_some() {
            changed = true;
        }
    }
    if changed {
        refresh_library_index_cache(app_data_root)?;
    }
    Ok(())
}

pub fn asset_title(app_data_root: &Path, target: &StarMapAssetRef) -> Result<Option<String>> {
    match target.kind {
        StarMapAssetKind::Node => Ok(load_node_definition(app_data_root, &target.asset_id)?
            .map(|definition| definition.title)),
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

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("starmap-library-test-{}", uuid::Uuid::new_v4()))
    }

    fn entry(entry_id: &str, asset_id: &str) -> StarMapLibraryEntry {
        StarMapLibraryEntry {
            entry_id: entry_id.into(),
            parent_folder_id: None,
            scope: StarMapAssetScope::Global,
            target: StarMapAssetRef::node(asset_id),
            created_at: 1,
            updated_at: 2,
            deleted_at: None,
        }
    }

    fn folder(folder_id: &str) -> StarMapLibraryFolder {
        StarMapLibraryFolder {
            folder_id: folder_id.into(),
            parent_folder_id: None,
            scope: StarMapAssetScope::Global,
            name: "folder".into(),
            created_at: 1,
            updated_at: 2,
            deleted_at: None,
        }
    }

    #[test]
    fn entry_files_are_fact_source_and_tombstones_are_hidden() {
        let root = temp_root();
        let mut dead = folder("lf_dead");
        dead.deleted_at = Some(5);
        save_library_folder(&root, &dead).unwrap();
        let mut first = entry("le_1", "n1");
        first.parent_folder_id = Some("lf_dead".into());
        save_library_entry(&root, &first).unwrap();
        save_library_entry(&root, &entry("le_2", "n2")).unwrap();
        move_entry(
            &root,
            "le_1",
            &StarMapAssetScope::Project {
                project_id: "p1".into(),
            },
            None,
        )
        .unwrap();
        let loaded = load_library_index(&root).unwrap();
        assert!(loaded.folders.is_empty());
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].parent_folder_id, None);
        assert_eq!(loaded.entries[0].scope.project_id(), Some("p1"));

        remove_entries_for_asset(&root, &StarMapAssetRef::node("n1")).unwrap();
        assert_eq!(load_library_index(&root).unwrap().entries.len(), 1);
        let raw: StarMapLibraryEntry = serde_json::from_str(
            &std::fs::read_to_string(library_entry_abs_path(&root, "le_1")).unwrap(),
        )
        .unwrap();
        assert_eq!(raw.deleted_at, Some(raw.updated_at));
        let cached: StarMapLibraryIndex =
            serde_json::from_str(&std::fs::read_to_string(library_index_abs_path(&root)).unwrap())
                .unwrap();
        assert_eq!(cached.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_index_migrates_to_independent_files_once() {
        let root = temp_root();
        let legacy_dir = root.join("starmaps/library");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        let legacy = StarMapLibraryIndex {
            schema_version: CURRENT_LIBRARY_SCHEMA_VERSION,
            entries: vec![entry("le_legacy", "n_legacy")],
            folders: vec![folder("lf_legacy")],
            updated_at: 3,
        };
        std::fs::write(
            legacy_dir.join("index.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();

        assert_eq!(load_library_index(&root).unwrap().entries[0].created_at, 1);
        assert!(library_entry_abs_path(&root, "le_legacy").is_file());
        assert!(library_folder_abs_path(&root, "lf_legacy").is_file());
        assert_eq!(load_library_index(&root).unwrap().entries.len(), 1);
        assert_eq!(
            std::fs::read_dir(library_entries_dir(&root))
                .unwrap()
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
