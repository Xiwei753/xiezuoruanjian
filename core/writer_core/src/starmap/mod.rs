//! # 星图模块 (StarMap Module)
//!
//! 本模块实现了星图（StarMap）功能，用于可视化管理写作项目中的世界观元素、
//! 角色关系、情节线索等创作要素。星图是一种结构化的知识图谱工具，
//! 帮助作者组织和展示复杂的故事元素之间的关系。
//!
//! ## 主要功能
//! - **星图元数据管理**：创建、读取、更新、删除星图的基本信息
//! - **星图索引管理**：维护数据根中所有星图的索引，支持快速查询
//! - **项目关联**：将星图绑定到特定项目，支持设置项目主星图
//!
//! 显示/交互/渲染职责（布局算法、命中测试、运动策略、视口）已全部退出 Core，
//! 由平台端自行管理。Core 只保留节点/嵌入的 `position` 数据字段。

pub mod graph;
pub mod migration;
pub mod package_storage;
pub mod semantic;
pub mod store;
pub mod types;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 星图元数据。
///
/// `starmaps/{id}.meta.json` 是标题、描述、project_id、accent_color、
/// created_at、updated_at 的唯一事实源。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMeta {
    pub starmap_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    pub created_at: u64,
    pub updated_at: u64,
}

fn default_accent_color() -> String {
    "#7B8CDE".to_string()
}

/// 星图全局索引记录。
///
/// 存储于 `app-meta/starmaps/index.json`，只保存 starmap_ids 列表和
/// main_starmap_by_project 映射。各星图的详细元数据从独立的 meta 文件读取。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapIndexRecord {
    pub schema_version: u32,
    pub starmap_ids: Vec<String>,
    pub main_starmap_by_project: std::collections::HashMap<String, String>,
    pub updated_at: u64,
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn starmaps_dir(app_data_root: &Path) -> std::path::PathBuf {
    app_data_root.join("starmaps")
}

fn index_path(app_data_root: &Path) -> std::path::PathBuf {
    starmaps_dir(app_data_root).join("index.json")
}

fn starmap_meta_path(app_data_root: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(app_data_root).join(format!("{}.meta.json", starmap_id))
}

///   starmap meta 的 workspace-relative 路径。
fn starmap_meta_rel_path(starmap_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("starmaps").join(format!("{}.meta.json", starmap_id))
}

///   starmaps/index.json 的 workspace-relative 路径。
fn starmaps_index_rel_path() -> std::path::PathBuf {
    std::path::PathBuf::from("starmaps").join("index.json")
}

///   starmaps/{id}/ 目录的 workspace-relative 路径。
fn starmap_dir_rel_path(starmap_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("starmaps").join(starmap_id)
}

///   构造单个 starmap meta index 的变更集。
fn change_set_for_meta_and_index(
    starmap_id: &str,
) -> crate::storage::workspace_git::WorkspaceChangeSet {
    crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(starmap_meta_rel_path(starmap_id))
        .add_upsert(starmaps_index_rel_path())
}

fn load_index(app_data_root: &Path) -> Result<StarMapIndexRecord> {
    // 在读取 index 之前先做一次旧格式迁移（schema 1 -> 2）。
    // 迁移是幂等的：已经是新格式则跳过；未知版本 fail-closed 返回 Err。
    migration::migrate_index(app_data_root)?;

    let path = index_path(app_data_root);
    if !path.exists() {
        return Ok(StarMapIndexRecord {
            schema_version: migration::NEW_INDEX_SCHEMA_VERSION,
            starmap_ids: vec![],
            main_starmap_by_project: std::collections::HashMap::new(),
            updated_at: now_epoch(),
        });
    }
    let content = fs::read_to_string(&path)?;
    let idx: StarMapIndexRecord = serde_json::from_str(&content)?;
    // 双重保险：迁移入口可能被别的调用方式绕开，反序列化后再断言一次版本。
    if idx.schema_version != migration::NEW_INDEX_SCHEMA_VERSION {
        return Err(crate::error::Error::UnsupportedVersion {
            version: idx.schema_version.to_string(),
        });
    }
    Ok(idx)
}

fn save_index(app_data_root: &Path, idx: &StarMapIndexRecord) -> Result<()> {
    let dir = starmaps_dir(app_data_root);
    fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(idx)?;
    crate::storage::atomic_write_string(&index_path(app_data_root), &content)
}

fn save_starmap_meta(app_data_root: &Path, meta: &StarMapMeta) -> Result<()> {
    let dir = starmaps_dir(app_data_root);
    fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(meta)?;
    crate::storage::atomic_write_string(
        &starmap_meta_path(app_data_root, &meta.starmap_id),
        &content,
    )
}

fn load_starmap_meta(app_data_root: &Path, starmap_id: &str) -> Result<StarMapMeta> {
    let path = starmap_meta_path(app_data_root, starmap_id);
    if !path.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("StarMap not found: {}", starmap_id),
        )));
    }
    let content = fs::read_to_string(&path)?;
    let meta: StarMapMeta = serde_json::from_str(&content)?;
    Ok(meta)
}

fn delete_starmap_meta(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    let path = starmap_meta_path(app_data_root, starmap_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn list_starmaps(app_data_root: &Path) -> Result<Vec<StarMapMeta>> {
    let idx = load_index(app_data_root)?;
    let mut metas = Vec::new();
    for id in &idx.starmap_ids {
        metas.push(load_starmap_meta(app_data_root, id)?);
    }
    Ok(metas)
}

pub fn list_starmaps_bound_to_project(
    app_data_root: &Path,
    project_id: &str,
) -> Result<Vec<StarMapMeta>> {
    let all = list_starmaps(app_data_root)?;
    Ok(all
        .into_iter()
        .filter(|m| m.project_id.as_deref() == Some(project_id))
        .collect())
}

pub fn get_starmap(app_data_root: &Path, starmap_id: &str) -> Result<StarMapMeta> {
    load_starmap_meta(app_data_root, starmap_id)
}

pub fn create_starmap(
    app_data_root: &Path,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    let now = now_epoch();
    let meta = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: title.to_string(),
        description: description.to_string(),
        project_id: None,
        accent_color: accent_color.unwrap_or(&default_accent_color()).to_string(),
        created_at: now,
        updated_at: now,
    };
    save_starmap_meta(app_data_root, &meta)?;
    let mut idx = load_index(app_data_root)?;
    idx.starmap_ids.push(meta.starmap_id.clone());
    idx.updated_at = now;
    save_index(app_data_root, &idx)?;
    Ok(meta)
}

///   create_starmap 的变更集版本。
///
/// 返回 `(StarMapMeta, WorkspaceChangeSet)`，变更集包含
/// `Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
pub fn create_starmap_with_changes(
    app_data_root: &Path,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<(
    StarMapMeta,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let meta = create_starmap(app_data_root, title, description, accent_color)?;
    let change_set = change_set_for_meta_and_index(&meta.starmap_id);
    Ok((meta, change_set))
}

pub fn rename_starmap(
    app_data_root: &Path,
    starmap_id: &str,
    new_title: &str,
) -> Result<StarMapMeta> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    meta.title = new_title.to_string();
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;
    // rename 不重写 index：title 只在 meta 文件里，index 不持久化 title，
    // 不为了更新 index.updated_at 形成无意义双写。
    Ok(meta)
}

///   rename_starmap 的变更集版本。
///
/// 变更集：`Upsert(starmaps/{id}.meta.json)`。rename 不再写 index。
pub fn rename_starmap_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    new_title: &str,
) -> Result<(
    StarMapMeta,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let meta = rename_starmap(app_data_root, starmap_id, new_title)?;
    Ok((
        meta,
        crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(starmap_meta_rel_path(starmap_id)),
    ))
}

/// 删除星图。
///
/// 先检查是否有外部引用（embed/link/edge 指向此星图），有则拒绝删除。
/// 自引用（星图内部的边/嵌入指向自身）不阻止删除。
///
/// 写盘顺序：先断 index 引用 → 删对象目录 → 删 meta 真相。
/// 任何中途失败最多留下"index 已不引用的孤儿文件"，
/// 不会留下"有效 index 指向不存在 meta"的 dangling 状态。
/// 再次调用 delete 能继续清理孤儿文件：index 已不引用该 id 时 retain 是 no-op，
/// 后续删除对象目录和 meta 仍会执行。
pub fn delete_starmap(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    // Before deleting, check if it's referenced by any EXTERNAL StarMap.
    let refs = find_starmap_references(app_data_root, starmap_id)?;
    let external_refs: Vec<_> = refs
        .into_iter()
        .filter(|r| r.host_starmap_id != starmap_id)
        .collect();
    if !external_refs.is_empty() {
        return Err(crate::error::Error::Io(std::io::Error::other(format!(
            "Cannot delete StarMap because it is referenced by {} external places.",
            external_refs.len()
        ))));
    }

    // 写盘顺序：先断 index 引用，再删对象目录，最后删 meta 真相。
    // 任何中途失败最多留下 index 已不引用的孤儿文件，
    // 不会留下"有效 index 指向不存在 meta"的 dangling 状态。
    // 再次调用 delete 能继续清理孤儿文件：index 已不引用该 id 时 retain 是 no-op，
    // 后续删除对象目录和 meta 仍会执行。
    let mut idx = load_index(app_data_root)?;
    idx.starmap_ids.retain(|id| id != starmap_id);
    idx.main_starmap_by_project.retain(|_, v| v != starmap_id);
    idx.updated_at = now_epoch();
    save_index(app_data_root, &idx)?;

    // 删除对象目录。失败时最多留下 index 已不引用的孤儿目录。
    let graph_dir = starmaps_dir(app_data_root).join(starmap_id);
    if graph_dir.exists() {
        fs::remove_dir_all(&graph_dir)?;
    }

    // 最后删除 meta 真相。失败时最多留下 index 已不引用的孤儿 meta。
    delete_starmap_meta(app_data_root, starmap_id)?;

    Ok(())
}

///   delete_starmap 的变更集版本。
///
/// 变更集：`Delete(starmaps/{id}.meta.json) + DeleteTree(starmaps/{id}) +
/// Upsert(starmaps/index.json)`。
pub fn delete_starmap_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    delete_starmap(app_data_root, starmap_id)?;
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete(starmap_meta_rel_path(starmap_id))
        .add_delete_tree(starmap_dir_rel_path(starmap_id))
        .add_upsert(starmaps_index_rel_path());
    Ok(change_set)
}

/// 把星图绑定到项目。
///
/// 先清旧 main index，再写 meta：如果 meta 写失败，最多变成"这个 project
/// 暂时没有 main"，不会留下"main 指向不属于这个 project 的星图"。
///
/// 返回 `index_changed`：index 是否真的被改过。`_with_changes` 版本据此
/// 组装真实写入路径的 change set。
pub fn bind_starmap_to_project(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<bool> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    let old_project_id = meta.project_id.clone();

    // 维护不变量：main_starmap_by_project[pid] 指向的星图其 meta.project_id 必须等于 pid。
    // 先清旧 main 映射（如果存在且不同），确保 save_index 成功落盘后再写 meta。
    let mut index_changed = false;
    if let Some(old_pid) = &old_project_id {
        if old_pid != project_id {
            let mut idx = load_index(app_data_root)?;
            if idx.main_starmap_by_project.get(old_pid) == Some(&starmap_id.to_string()) {
                idx.main_starmap_by_project.remove(old_pid);
                idx.updated_at = now_epoch();
                save_index(app_data_root, &idx)?;
                index_changed = true;
            }
        }
    }

    meta.project_id = Some(project_id.to_string());
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    Ok(index_changed)
}

///   bind_starmap_to_project 的变更集版本。
///
/// 根据 `bind_starmap_to_project` 返回的 `index_changed` 组装真实写入路径的
/// change set：index 没有变化时只含 meta，否则含 meta + index。
pub fn bind_starmap_to_project_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    let index_changed = bind_starmap_to_project(app_data_root, starmap_id, project_id)?;
    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(starmap_meta_rel_path(starmap_id));
    if index_changed {
        change_set = change_set.add_upsert(starmaps_index_rel_path());
    }
    Ok(change_set)
}

/// 设置项目的主星图。
///
/// 先校验目标星图已绑定到该 project（`meta.project_id == Some(project_id)`），
/// 校验通过后只修改 `main_starmap_by_project` 映射，不再顺带改写目标 meta。
/// "设为主星图"与"绑定到作品"是两个独立的关系，不能混在一起。
///
/// 维护不变量：`main_starmap_by_project[project_id]` 指向的星图，
/// 其 `meta.project_id` 必须等于这个 `project_id`。
///
/// 返回 `index_changed`：index 是否真的被改过。映射本来就相同时返回 `Ok(false)`。
pub fn set_main_starmap_for_project(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<bool> {
    // 先读目标 meta，确认星图存在。
    let meta = load_starmap_meta(app_data_root, starmap_id)?;
    // 要设成某个 project 的 main，目标必须已经 meta.project_id == Some(project_id)，
    // 否则直接 Err。
    if meta.project_id.as_deref() != Some(project_id) {
        return Err(crate::error::Error::Other(format!(
            "starmap '{}' is not bound to project '{}' (current project_id: {:?}); \
             bind it first before setting as main",
            starmap_id, project_id, meta.project_id
        )));
    }

    // 校验完成以后只修改 main_starmap_by_project。
    // 只有在 main 映射真的变化时才写 index，不为了 index.updated_at 形成无意义双写。
    let mut idx = load_index(app_data_root)?;
    if idx.main_starmap_by_project.get(project_id) == Some(&starmap_id.to_string()) {
        return Ok(false);
    }
    idx.main_starmap_by_project
        .insert(project_id.to_string(), starmap_id.to_string());
    idx.updated_at = now_epoch();
    save_index(app_data_root, &idx)?;
    Ok(true)
}

///   set_main_starmap_for_project 的变更集版本。
///
/// 根据 `set_main_starmap_for_project` 返回的 `index_changed` 组装真实写入路径的
/// change set：index 没有变化时返回空 change set，否则含 `Upsert(starmaps/index.json)`。
pub fn set_main_starmap_for_project_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    let index_changed = set_main_starmap_for_project(app_data_root, starmap_id, project_id)?;

    let change_set = if index_changed {
        crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(starmaps_index_rel_path())
    } else {
        crate::storage::workspace_git::WorkspaceChangeSet::new()
    };
    Ok(change_set)
}

pub fn get_main_starmap_for_project(
    app_data_root: &Path,
    project_id: &str,
) -> Result<Option<StarMapMeta>> {
    let idx = load_index(app_data_root)?;
    if let Some(starmap_id) = idx.main_starmap_by_project.get(project_id) {
        return Ok(Some(load_starmap_meta(app_data_root, starmap_id)?));
    }
    Ok(None)
}

/// 把星图从项目解绑。
///
/// 先清 main index，再写 meta：如果 meta 写失败，最多变成"这个 project
/// 暂时没有 main"，不会留下"main 指向不属于这个 project 的星图"。
///
/// 返回 `index_changed`：index 是否真的被改过。
pub fn unbind_starmap_from_project(app_data_root: &Path, starmap_id: &str) -> Result<bool> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    let old_project_id = meta.project_id.clone();

    // 先清 main 映射（如果它是 main），确保 save_index 成功落盘后再写 meta。
    let mut index_changed = false;
    if let Some(pid) = &old_project_id {
        let mut idx = load_index(app_data_root)?;
        if idx.main_starmap_by_project.get(pid) == Some(&starmap_id.to_string()) {
            idx.main_starmap_by_project.remove(pid);
            idx.updated_at = now_epoch();
            save_index(app_data_root, &idx)?;
            index_changed = true;
        }
    }

    meta.project_id = None;
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    Ok(index_changed)
}

///   unbind_starmap_from_project 的变更集版本。
///
/// 根据 `unbind_starmap_from_project` 返回的 `index_changed` 组装真实写入路径的
/// change set：index 没有变化时只含 meta，否则含 meta + index。
pub fn unbind_starmap_from_project_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    let index_changed = unbind_starmap_from_project(app_data_root, starmap_id)?;
    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(starmap_meta_rel_path(starmap_id));
    if index_changed {
        change_set = change_set.add_upsert(starmaps_index_rel_path());
    }
    Ok(change_set)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarMapReference {
    pub host_starmap_id: String,
    pub host_title: String,
    pub ref_type: String, // "embed", "link", "portal", "edge", "hyperlink"
    pub ref_id: String,
    pub target_starmap_id: String,
}

/// 判断路径是否穿越或落在 `target_starmap_id`。
///
/// 用 `resolve_target` 返回的 `traversed_starmap_ids` 判断目标星图是否在
/// 路径的穿越链中（含起点和终点图）。这是删除保护的唯一真实依据——
/// 任何经过目标星图的路径都构成引用，删除目标星图会破坏该路径的可达性。
///
/// **Fail-safe**：resolver 出错时返回 `Err`，而不是静默返回 `false`。
/// 删除保护宁可拒绝删除也不能因为 resolver 故障而漏掉真实引用。
/// 引用扫描场景已 flush，传空 overlays 的 context（从磁盘读取）。
fn target_path_references_starmap(
    context: &crate::starmap::graph::resolve::GraphResolverContext,
    path: &crate::starmap::types::reference::StarMapTargetPath,
    target_starmap_id: &str,
) -> Result<bool> {
    match crate::starmap::graph::resolve::resolve_target(context, path) {
        Ok(resolved) => Ok(resolved
            .traversed_starmap_ids
            .iter()
            .any(|id| id == target_starmap_id)),
        Err(status) => Err(crate::error::Error::Io(std::io::Error::other(format!(
            "StarMap resolver failed during reference scan: {status:?}"
        )))),
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn find_starmap_references(
    app_data_root: &Path,
    target_starmap_id: &str,
) -> Result<Vec<StarMapReference>> {
    let mut refs = Vec::new();
    let idx = load_index(app_data_root)?;

    // 引用扫描场景已 flush，传空 overlays 的 context（从磁盘读取）。
    let context =
        crate::starmap::graph::resolve::GraphResolverContext::new_disk_only(app_data_root);

    for id in &idx.starmap_ids {
        let host_meta = load_starmap_meta(app_data_root, id)?;
        let mut store = crate::starmap::store::StarMapStore::new(app_data_root, id);
        // 引用扫描必须基于完整加载的图。任一 host 星图加载失败就返回 Err，
        // 不允许在引用扫描不完整时继续删除（否则会漏掉真实引用导致误删）。
        store.load_full()?;

        let graph = store.to_starmap_graph();
        // 1. Check embeds — target_starmap_id 和 host_path 都可能引用目标星图
        for embed in &graph.embeds {
            if embed.target_starmap_id == target_starmap_id {
                refs.push(StarMapReference {
                    host_starmap_id: id.clone(),
                    host_title: host_meta.title.clone(),
                    ref_type: "embed".to_string(),
                    ref_id: embed.instance_id.clone(),
                    target_starmap_id: target_starmap_id.to_string(),
                });
            }
            // Embed 的 host_path 也可能穿越或落在目标星图
            if target_path_references_starmap(&context, &embed.host_path, target_starmap_id)? {
                refs.push(StarMapReference {
                    host_starmap_id: id.clone(),
                    host_title: host_meta.title.clone(),
                    ref_type: "embed".to_string(),
                    ref_id: embed.instance_id.clone(),
                    target_starmap_id: target_starmap_id.to_string(),
                });
            }
        }

        // 2. Check links — source 和 target 都可能引用目标星图
        for link in &graph.links {
            if target_path_references_starmap(&context, &link.source, target_starmap_id)? {
                refs.push(StarMapReference {
                    host_starmap_id: id.clone(),
                    host_title: host_meta.title.clone(),
                    ref_type: "link".to_string(),
                    ref_id: link.link_id.clone(),
                    target_starmap_id: target_starmap_id.to_string(),
                });
            }
            if target_path_references_starmap(&context, &link.target, target_starmap_id)? {
                refs.push(StarMapReference {
                    host_starmap_id: id.clone(),
                    host_title: host_meta.title.clone(),
                    ref_type: "link".to_string(),
                    ref_id: link.link_id.clone(),
                    target_starmap_id: target_starmap_id.to_string(),
                });
            }
        }

        // 3. Check edges
        for edge in &graph.edges {
            let matches = target_path_references_starmap(&context, &edge.from, target_starmap_id)?
                || target_path_references_starmap(&context, &edge.to, target_starmap_id)?;

            if matches {
                refs.push(StarMapReference {
                    host_starmap_id: id.clone(),
                    host_title: host_meta.title.clone(),
                    ref_type: "edge".to_string(),
                    ref_id: edge.id.clone(),
                    target_starmap_id: target_starmap_id.to_string(),
                });
            }
        }

        // 4. Check portals
        for node in &graph.nodes {
            if let Some(portal) = &node.portal {
                if portal.destination_starmap_id == target_starmap_id {
                    refs.push(StarMapReference {
                        host_starmap_id: id.clone(),
                        host_title: host_meta.title.clone(),
                        ref_type: "portal".to_string(),
                        ref_id: node.id.clone(),
                        target_starmap_id: target_starmap_id.to_string(),
                    });
                }
            }
        }

        // 5. Check hyperlinks — hyperlink 的 source 路径也可能穿越目标星图。
        for hl in &graph.hyperlinks {
            if target_path_references_starmap(&context, &hl.source, target_starmap_id)? {
                refs.push(StarMapReference {
                    host_starmap_id: id.clone(),
                    host_title: host_meta.title.clone(),
                    ref_type: "hyperlink".to_string(),
                    ref_id: hl.hyperlink_id.clone(),
                    target_starmap_id: target_starmap_id.to_string(),
                });
            }
        }
    }

    Ok(refs)
}

/// 根星图过滤纯函数：给定全部星图 meta 和对应的图数据，返回未被嵌入且非
/// legacy child 的根星图列表。
///
/// 此函数不自己加载任何 `StarMapStore`，调用方负责提供与 meta 对应的 graph。
/// 这样 facade 可以传入内存中尚未 flush 的图数据，避免从磁盘读到旧状态。
///
/// 判断依据：
/// 1. 扫描所有星图的 `graph.embeds[].target_starmap_id`，这些目标不进入一级列表。
/// 2. 兼容旧版"伪子星图"：旧实现用 Note 节点 + portal（destination_target=null）
///    来模拟子星图嵌入。旧生成签名里 `portal.destination_starmap_id` 直接指向被嵌入
///    的子星图 id，因此按 `portal.destination_starmap_id` 判断该子星图应被排除，
///    不再按节点标题匹配（标题同名星图可能不止一个，按标题匹配会误伤）。
pub(crate) fn filter_root_starmaps(
    all_starmaps: Vec<StarMapMeta>,
    graphs: &[crate::starmap::types::StarMapGraph],
) -> Vec<StarMapMeta> {
    // starmap_id → title：用于 legacy child 判断时校验 portal 目标确实是
    // 旧实现的子星图（destination_target 为 null 且目标 id 在已知星图集合中）。
    let titles_by_id: std::collections::HashMap<String, String> = all_starmaps
        .iter()
        .map(|sm| (sm.starmap_id.clone(), sm.title.clone()))
        .collect();

    // 收集所有应从一级列表排除的 starmap_id（embed 目标 + legacy child）。
    let mut excluded: std::collections::HashSet<String> = std::collections::HashSet::new();

    for graph in graphs {
        // 正式 Embed：target_starmap_id 是子星图，排除。
        for embed in &graph.embeds {
            excluded.insert(embed.target_starmap_id.clone());
        }

        // Legacy child：Note + portal 非空 + destination_target 为 null
        // + portal.destination_starmap_id 指向已知星图且其标题与节点标题一致
        // → 该目标星图是旧实现的伪子星图，排除。
        for node in &graph.nodes {
            if node.kind != crate::starmap::types::StarMapNodeKind::Note {
                continue;
            }
            let Some(portal) = &node.portal else {
                continue;
            };
            let target_id = &portal.destination_starmap_id;
            let is_legacy_child = portal.destination_target.is_none()
                && titles_by_id
                    .get(target_id)
                    .map(|title| title == &node.title)
                    .unwrap_or(false);
            if is_legacy_child {
                excluded.insert(target_id.clone());
            }
        }
    }

    all_starmaps
        .into_iter()
        .filter(|sm| !excluded.contains(&sm.starmap_id))
        .collect()
}

/// 孤儿子图识别的时间窗口（毫秒）。
///
/// 旧版本两段式创建失败时，子星图的创建时间与宿主星图中 Note 节点的创建时间
/// 应该非常接近。60 秒窗口足够覆盖两段式创建的正常延迟，同时不会把
/// 用户隔很久手动创建的同名星图误判为孤儿。
const ORPHAN_TIME_WINDOW_MS: u64 = 60_000;

/// 识别可能是孤儿子图的星图 — 即没有任何 Embed 指向它，
/// 但其创建时间非常近（在一定窗口内），且标题匹配某个已知星图中的 Note 节点 portal 标题。
/// 这些星图可能是旧版本两段式创建失败留下的孤儿。
/// 返回的列表只包含能明确判断为未完成子图的数据，不会误删正常用户创建的根星图。
///
/// 识别逻辑（保守策略，宁可漏识别也不误判）：
/// 1. 收集所有 Embed target ID（这些是正常的子星图，不是孤儿）
/// 2. 收集所有 Note 节点的 portal destination_starmap_id（这些也是正常的子星图引用）
/// 3. 对于不在以上两个集合中的星图，检查：
///    - 是否有任何其他星图的 Note 节点标题与该星图标题完全一致
///    - 且该星图的 `created_at` 与那个 Note 节点的宿主星图的 `created_at` 非常接近（在 60 秒窗口内）
///    - 且该星图没有任何节点、边、embed、link、hyperlink（即完全空白的星图）
/// 4. 只有同时满足以上所有条件的星图才被识别为可能的孤儿
pub(crate) fn identify_orphan_starmaps(
    all_starmaps: &[StarMapMeta],
    graphs: &[crate::starmap::types::StarMapGraph],
) -> Vec<StarMapMeta> {
    // 1. 收集所有 Embed target ID — 这些是正常的子星图，不是孤儿。
    let mut embed_targets: std::collections::HashSet<String> = std::collections::HashSet::new();
    for graph in graphs {
        for embed in &graph.embeds {
            embed_targets.insert(embed.target_starmap_id.clone());
        }
    }

    // 2. 收集所有 Note 节点的 portal destination_starmap_id — 这些也是正常的子星图引用。
    let mut portal_targets: std::collections::HashSet<String> = std::collections::HashSet::new();
    for graph in graphs {
        for node in &graph.nodes {
            if node.kind == crate::starmap::types::StarMapNodeKind::Note {
                if let Some(portal) = &node.portal {
                    portal_targets.insert(portal.destination_starmap_id.clone());
                }
            }
        }
    }

    // 构建 starmap_id → meta 的查找表。
    let meta_by_id: std::collections::HashMap<String, &StarMapMeta> = all_starmaps
        .iter()
        .map(|sm| (sm.starmap_id.clone(), sm))
        .collect();

    // 构建 starmap_id → graph 的查找表。
    let graph_by_id: std::collections::HashMap<String, &crate::starmap::types::StarMapGraph> =
        graphs.iter().map(|g| (g.starmap_id.clone(), g)).collect();

    // 3. 对于不在 embed_targets 和 portal_targets 中的星图，检查是否为孤儿。
    let mut orphans = Vec::new();

    for sm in all_starmaps {
        // 如果该星图被 Embed 或 portal 引用，则不是孤儿。
        if embed_targets.contains(&sm.starmap_id) || portal_targets.contains(&sm.starmap_id) {
            continue;
        }

        // 该星图必须完全空白（没有任何节点、边、embed、link、hyperlink）。
        let graph_empty = graph_by_id
            .get(&sm.starmap_id)
            .map(|g| {
                g.nodes.is_empty()
                    && g.edges.is_empty()
                    && g.embeds.is_empty()
                    && g.links.is_empty()
                    && g.hyperlinks.is_empty()
            })
            .unwrap_or(true); // 没有 graph 数据视为空（新创建的星图可能还没有 graph 目录）

        if !graph_empty {
            continue;
        }

        // 检查是否有其他星图的 Note 节点标题与该星图标题完全一致，
        // 且创建时间在窗口内。
        let mut found_host = false;
        for graph in graphs {
            if graph.starmap_id == sm.starmap_id {
                continue; // 不检查自身
            }

            // 获取宿主星图的 meta。
            let Some(host_meta) = meta_by_id.get(&graph.starmap_id) else {
                continue;
            };

            // 检查宿主星图的 created_at 与候选孤儿的 created_at 是否在窗口内。
            let time_diff = sm.created_at.abs_diff(host_meta.created_at);
            if time_diff > ORPHAN_TIME_WINDOW_MS {
                continue;
            }

            // 检查宿主星图中是否有 Note 节点标题与候选孤儿标题完全一致。
            for node in &graph.nodes {
                if node.kind == crate::starmap::types::StarMapNodeKind::Note
                    && node.title == sm.title
                {
                    found_host = true;
                    break;
                }
            }

            if found_host {
                break;
            }
        }

        if found_host {
            orphans.push(sm.clone());
        }
    }

    orphans
}

/// 安全迁移孤儿子图：对于能明确判断为未完成子图的星图，
/// 创建从宿主星图到孤儿星图的 Embed 关系，使其不再出现在根列表中。
/// 不能明确判断的星图保留不动，不做任何猜测性操作。
///
/// 迁移逻辑：
/// 1. 加载所有星图 meta 和 graph
/// 2. 调用 `identify_orphan_starmaps` 识别孤儿
/// 3. 对于每个识别出的孤儿：
///    - 找到宿主星图（标题匹配的 Note 节点所在星图）
///    - 在宿主星图中创建一个 Embed 指向孤儿星图
///    - 这样孤儿星图就不再是"根"星图，不会出现在 `listRootStarMaps()` 结果中
/// 4. 返回成功迁移的星图 ID 列表
pub fn safe_migrate_orphan_starmaps(base_dir: &Path) -> Result<Vec<String>> {
    let all_starmaps = list_starmaps(base_dir)?;

    let mut graphs = Vec::with_capacity(all_starmaps.len());
    let mut stores: Vec<crate::starmap::store::StarMapStore> =
        Vec::with_capacity(all_starmaps.len());

    for sm in &all_starmaps {
        let mut store = crate::starmap::store::StarMapStore::new(base_dir, &sm.starmap_id);
        store.load_full()?;
        graphs.push(store.to_starmap_graph());
        stores.push(store);
    }

    let orphans = identify_orphan_starmaps(&all_starmaps, &graphs);

    if orphans.is_empty() {
        return Ok(Vec::new());
    }

    // 为每个孤儿找到宿主星图并创建 Embed。
    let mut migrated = Vec::new();

    // 构建 starmap_id → meta 的查找表。
    let meta_by_id: std::collections::HashMap<String, &StarMapMeta> = all_starmaps
        .iter()
        .map(|sm| (sm.starmap_id.clone(), sm))
        .collect();

    for orphan in &orphans {
        // 找到宿主星图：标题匹配的 Note 节点所在星图，且创建时间在窗口内。
        let mut host_id: Option<String> = None;

        for graph in &graphs {
            if graph.starmap_id == orphan.starmap_id {
                continue;
            }

            let Some(host_meta) = meta_by_id.get(&graph.starmap_id) else {
                continue;
            };

            let time_diff = orphan.created_at.abs_diff(host_meta.created_at);
            if time_diff > ORPHAN_TIME_WINDOW_MS {
                continue;
            }

            for node in &graph.nodes {
                if node.kind == crate::starmap::types::StarMapNodeKind::Note
                    && node.title == orphan.title
                {
                    host_id = Some(graph.starmap_id.clone());
                    break;
                }
            }

            if host_id.is_some() {
                break;
            }
        }

        let Some(host_starmap_id) = host_id else {
            // 找不到宿主星图，跳过这个孤儿（保守策略，不做猜测）。
            continue;
        };

        // 在宿主星图中创建 Embed 指向孤儿星图。
        let now = now_epoch();
        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: format!("em_{}", uuid::Uuid::new_v4()),
            target_starmap_id: orphan.starmap_id.clone(),
            label: Some(orphan.title.clone()),
            position: crate::starmap::types::StarMapPoint::default(),
            host_path: crate::starmap::types::StarMapTargetPath {
                starmap_id: host_starmap_id.clone(),
                segments: Vec::new(),
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            provenance: crate::starmap::semantic::StarMapProvenance::default(),
            created_at: now,
            updated_at: now,
        };

        // 找到宿主星图的 store 索引并添加 embed。
        let host_idx = stores
            .iter()
            .position(|s| s.starmap_id() == host_starmap_id);
        if let Some(idx) = host_idx {
            stores[idx].upsert_embed(embed);
            stores[idx].flush()?;
            migrated.push(orphan.starmap_id.clone());
        }
    }

    Ok(migrated)
}

/// 列出根星图（未被任何星图嵌入且非 legacy child 的星图）。
///
/// 此函数从磁盘加载星图数据。facade 层（`WriterCore::list_root_starmaps`）
/// 使用内存中的 `starmap_stores` 以看到尚未 flush 的变更，应优先调用 facade 版本。
///
/// 加载失败时返回 Err（不静默跳过），避免漏扫某个父图而把它的子星图错误暴露到一级列表。
pub fn list_root_starmaps(app_data_root: &Path) -> Result<Vec<StarMapMeta>> {
    let all_starmaps = list_starmaps(app_data_root)?;

    let mut graphs = Vec::with_capacity(all_starmaps.len());
    for sm in &all_starmaps {
        let mut store = crate::starmap::store::StarMapStore::new(app_data_root, &sm.starmap_id);
        // 加载失败时返回 Err，不静默跳过：漏扫某个父图可能把它的子星图错误暴露到一级列表。
        store.load_full()?;
        graphs.push(store.to_starmap_graph());
    }

    Ok(filter_root_starmaps(all_starmaps, &graphs))
}

#[cfg(test)]
mod tests;
