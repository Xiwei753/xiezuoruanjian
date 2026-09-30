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

/// 用预先生成的 starmap_id 创建星图。
///
/// 与 `create_starmap()` 不同，此函数接受一个已经生成的 `starmap_id`，
/// 而不是内部生成 UUID。供 journal 事务在创建文件之前就知道 child ID 的场景使用。
///
/// 创建 meta 文件和 index 记录，返回 StarMapMeta。
pub fn create_starmap_with_id(
    app_data_root: &Path,
    starmap_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    let now = now_epoch();
    let meta = StarMapMeta {
        starmap_id: starmap_id.to_string(),
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

pub fn create_starmap(
    app_data_root: &Path,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    let starmap_id = format!("sm_{}", uuid::Uuid::new_v4());
    create_starmap_with_id(app_data_root, &starmap_id, title, description, accent_color)
}

///   create_starmap_with_id 的变更集版本。
///
/// 返回 `(StarMapMeta, WorkspaceChangeSet)`，变更集包含
/// `Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
pub fn create_starmap_with_id_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<(
    StarMapMeta,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let meta = create_starmap_with_id(app_data_root, starmap_id, title, description, accent_color)?;
    let change_set = change_set_for_meta_and_index(&meta.starmap_id);
    Ok((meta, change_set))
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
