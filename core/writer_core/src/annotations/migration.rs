//! # 旧标签/旧导航链接的一次性迁移
//!
//! Issue #1060：标签与导航链接的唯一持久化所有者是 annotations 仓库。
//! 旧格式（节点定义/旧星图图数据里的 `tags`、`starmaps/<id>/links/*.json`）
//! 只允许通过本模块读取一次，迁移完成后由调用方删除旧字段与旧文件，
//! 不再双写。
//!
//! 迁移原则：
//!
//! - 保留原标签名称、原 `link_id`、`label`、完整画布路由和时间戳；
//! - 旧标签按素材实际归属（作品域或全局域）落到对应命名空间，使用稳定键；
//! - 归属有多个候选、ID 冲突或字段无法映射时**保留源数据**并记录可处理的
//!   迁移冲突，不猜测归属、不覆盖现有绑定或删除事实；
//! - 迁移幂等：重复执行不产生重复记录，不复活已删除的绑定/链接。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::object_ref::{ObjectRef, ObjectScope};
use crate::starmap::types::{StarMapAssetKind, StarMapTargetDetail, StarMapTargetPath};

use super::model::{AnnotationMutation, LinkOrigin, LinkTarget, ObjectLink, Tag};
use super::store::AnnotationStore;

/// 统一 annotations 仓库的当前 schema 版本。
pub const CURRENT_ANNOTATION_SCHEMA_VERSION: u32 = 1;

/// 旧节点定义里需要迁移的部分：只读取扁平标签名。
///
/// 单独定义 legacy 结构，不复用新模型；未知字段由 serde 忽略。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyNodeTags {
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 旧 `starmaps/<id>/links/*.json` 记录。
///
/// 字段与旧 `StarMapLink` 对齐；`created_at`/`updated_at` 是秒。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyStarMapLink {
    pub link_id: String,
    pub source: StarMapTargetPath,
    pub target: StarMapTargetPath,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

/// 一条需要人工处理的迁移冲突。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationConflict {
    /// 冲突种类：`scopeAmbiguous`、`linkIdConflict`…
    pub kind: String,
    /// 冲突涉及的旧记录 ID（素材 ID、旧 link ID…）。
    pub source_id: String,
    pub detail: String,
}

/// 一次迁移的可审查结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnnotationMigrationReport {
    /// 本次真正写入的标签关系数（已存在的关系不重复计数）。
    pub migrated_tags: usize,
    /// 本次真正写入的导航链接数。
    pub migrated_links: usize,
    pub conflicts: Vec<MigrationConflict>,
    /// workspace 相对路径（`projects/<id>/annotations/...` 或 `annotations/...`）。
    pub changed_paths: Vec<String>,
    /// 可以删除的旧文件（workspace 相对路径）。
    pub removed_legacy_paths: Vec<String>,
}

/// 迁移旧节点标签到 annotations 仓库。
///
/// `legacy` 每项是 `(素材 ID, 素材归属作品, 旧标签名列表)`。归属作品为空表示
/// 素材属于全局素材域。归属仍有多个候选（例如作品库与全局库都登记了同一
/// 素材）时记录 `scopeAmbiguous` 冲突并跳过，不猜测。
pub fn migrate_starmap_node_tags(
    app_data_root: &Path,
    legacy: &[(String, Option<String>, Vec<String>)],
) -> Result<AnnotationMigrationReport> {
    let mut report = AnnotationMigrationReport::default();
    let now_ms = chrono::Utc::now().timestamp_millis();
    for (asset_id, project_id, tag_names) in legacy {
        if asset_id.is_empty() {
            report.conflicts.push(MigrationConflict {
                kind: "missingAssetId".to_string(),
                source_id: String::new(),
                detail: "旧节点缺少素材 ID，标签无法绑定".to_string(),
            });
            continue;
        }
        let scopes = asset_scope_candidates(app_data_root, asset_id, project_id.as_deref())?;
        let scope = match scopes.len() {
            1 => scopes
                .into_iter()
                .next()
                .ok_or_else(|| Error::Other("asset scope vanished during migration".to_string()))?,
            0 => {
                report.conflicts.push(MigrationConflict {
                    kind: "scopeUnresolved".to_string(),
                    source_id: asset_id.clone(),
                    detail: format!(
                        "无法确定素材归属（作品 {:?} 未下载或素材不在库索引中）",
                        project_id
                    ),
                });
                continue;
            }
            _ => {
                report.conflicts.push(MigrationConflict {
                    kind: "scopeAmbiguous".to_string(),
                    source_id: asset_id.clone(),
                    detail: format!("素材存在多个归属候选：{scopes:?}"),
                });
                continue;
            }
        };
        let store = store_for_scope(app_data_root, &scope);
        let target = ObjectRef::NodeAsset {
            scope: scope.clone(),
            object_id: asset_id.clone(),
        };
        let mut seen_names = HashSet::new();
        for name in tag_names {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            if !seen_names.insert(name.to_string()) {
                continue;
            }
            if name.chars().any(char::is_control) {
                report.conflicts.push(MigrationConflict {
                    kind: "invalidTagName".to_string(),
                    source_id: asset_id.clone(),
                    detail: "旧标签名包含控制字符".to_string(),
                });
                continue;
            }
            let tag = match ensure_tag(&store, name, &scope, now_ms)? {
                EnsuredTag::Existing(tag) => tag,
                EnsuredTag::Created(tag, paths) => {
                    report.changed_paths.extend(prefix_paths(&scope, paths));
                    tag
                }
                EnsuredTag::Deleted => {
                    report.conflicts.push(MigrationConflict {
                        kind: "tagDeleted".to_string(),
                        source_id: asset_id.clone(),
                        detail: format!("标签 `{name}` 在该作用域已被删除，迁移不复活删除事实"),
                    });
                    continue;
                }
            };
            match store.find_binding_by_relation(&tag, &target) {
                Ok(Some(binding)) if binding.is_active() => continue,
                Ok(Some(tombstone)) => {
                    // 用户在迁移后删除过这条关系：重复迁移不得复活删除事实。
                    report.conflicts.push(MigrationConflict {
                        kind: "bindingDeleted".to_string(),
                        source_id: asset_id.clone(),
                        detail: format!(
                            "标签 `{name}` 的绑定 {} 已被删除，迁移保留删除事实",
                            tombstone.binding_id
                        ),
                    });
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    report.conflicts.push(MigrationConflict {
                        kind: "tagBindingFailed".to_string(),
                        source_id: asset_id.clone(),
                        detail: format!("读取标签 `{name}` 的绑定失败：{error}"),
                    });
                    continue;
                }
            }
            match store.attach_tag(&tag, &target, now_ms) {
                Ok((_, changed)) => {
                    if !changed.is_empty() {
                        report.migrated_tags += 1;
                        report.changed_paths.extend(prefix_paths(&scope, changed));
                    }
                }
                Err(error) => report.conflicts.push(MigrationConflict {
                    kind: "tagBindingFailed".to_string(),
                    source_id: asset_id.clone(),
                    detail: format!("标签 `{name}` 绑定失败：{error}"),
                }),
            }
        }
    }
    Ok(report)
}

/// 迁移旧星图导航链接到 annotations 仓库。
///
/// 链接按源所有权保存一次：源必须是本画布上的节点实例，目标保留完整
/// [`StarMapTargetPath`]（画布导航）。`project_id` 是画布归属作品；为空表示
/// 画布属于全局素材域。已存在相同 `link_id` 但内容不同的记录只记冲突，
/// 不覆盖。
pub fn migrate_legacy_links(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: Option<&str>,
    legacy: &[LegacyStarMapLink],
) -> Result<AnnotationMigrationReport> {
    let mut report = AnnotationMigrationReport::default();
    if starmap_id.is_empty() {
        return Err(Error::Other(
            "legacy link migration needs a starmap id".to_string(),
        ));
    }
    let scope = match project_id.filter(|id| !id.is_empty()) {
        Some(project_id) => {
            if !is_safe_segment(project_id) || !project_downloaded(app_data_root, project_id) {
                report.conflicts.push(MigrationConflict {
                    kind: "scopeUnresolved".to_string(),
                    source_id: starmap_id.to_string(),
                    detail: format!("画布归属作品 {project_id} 不存在或未下载"),
                });
                return Ok(report);
            }
            ObjectScope::Project {
                project_id: project_id.to_string(),
            }
        }
        None => ObjectScope::Global,
    };
    let store = store_for_scope(app_data_root, &scope);
    for legacy_link in legacy {
        if legacy_link.link_id.is_empty() {
            report.conflicts.push(MigrationConflict {
                kind: "missingLinkId".to_string(),
                source_id: starmap_id.to_string(),
                detail: "旧链接缺少 link_id".to_string(),
            });
            continue;
        }
        let Some(source) = legacy_node_instance_source(starmap_id, &legacy_link.source) else {
            report.conflicts.push(MigrationConflict {
                kind: "sourceNotNodeInstance".to_string(),
                source_id: legacy_link.link_id.clone(),
                detail: "旧链接源不是本画布上的节点实例，保留源数据不迁移".to_string(),
            });
            continue;
        };
        let link = ObjectLink {
            link_id: legacy_link.link_id.clone(),
            source,
            target: LinkTarget::CanvasNavigation(legacy_link.target.clone()),
            origin: LinkOrigin::Manual,
            label: legacy_link.label.clone(),
            created_at_ms: seconds_to_millis(legacy_link.created_at),
            updated_at_ms: seconds_to_millis(legacy_link.updated_at),
            deleted_at_ms: None,
        };
        match store.find_link_by_idempotency_key(&link.source, &link.target) {
            Ok(Some(existing)) if existing.deleted_at_ms.is_some() => {
                // 迁移后用户删除了这条链接：重复迁移不复活删除事实。
                report.conflicts.push(MigrationConflict {
                    kind: "linkDeleted".to_string(),
                    source_id: legacy_link.link_id.clone(),
                    detail: format!("链接 {} 已被删除，迁移保留删除事实", existing.link_id),
                });
                continue;
            }
            Ok(Some(existing)) => {
                if existing.link_id != legacy_link.link_id {
                    report.conflicts.push(MigrationConflict {
                        kind: "linkIdentityConflict".to_string(),
                        source_id: legacy_link.link_id.clone(),
                        detail: format!(
                            "同一源/目标已有链接 {}，保留现有记录与旧 ID 对应关系",
                            existing.link_id
                        ),
                    });
                }
                continue;
            }
            Ok(None) => {}
            Err(error) => {
                report.conflicts.push(MigrationConflict {
                    kind: "linkWriteConflict".to_string(),
                    source_id: legacy_link.link_id.clone(),
                    detail: format!("读取旧链接的现有记录失败：{error}"),
                });
                continue;
            }
        }
        match store.attach_link(link) {
            Ok((stored, changed)) => {
                if stored.link_id != legacy_link.link_id {
                    report.conflicts.push(MigrationConflict {
                        kind: "linkIdentityConflict".to_string(),
                        source_id: legacy_link.link_id.clone(),
                        detail: format!(
                            "同一源/目标已有链接 {}，保留现有记录与旧 ID 对应关系",
                            stored.link_id
                        ),
                    });
                    continue;
                }
                if !changed.is_empty() {
                    report.migrated_links += 1;
                    report.changed_paths.extend(prefix_paths(&scope, changed));
                }
            }
            Err(error) => report.conflicts.push(MigrationConflict {
                kind: "linkWriteConflict".to_string(),
                source_id: legacy_link.link_id.clone(),
                detail: error.to_string(),
            }),
        }
    }
    Ok(report)
}

/// 迁移提交后确认统一仓库已接管这些链接，再返回可以删除的旧文件。
///
/// 本函数只做校验与返回，不删除任何星图文件：删除由 starmap 侧迁移在确认
/// 后执行，并把删除事实记录进旧/新路径对应关系。已经删除的旧文件不再返回，
/// 因此重复调用是幂等的。
pub fn finish_link_migration(
    app_data_root: &Path,
    store: &AnnotationStore,
    migrated_link_ids: &[String],
    legacy_rel_paths: &[PathBuf],
) -> Result<Vec<String>> {
    for link_id in migrated_link_ids {
        match store.get_link(link_id)? {
            Some(link) if link.deleted_at_ms.is_none() => {}
            Some(_) => {
                return Err(Error::Other(format!(
                    "迁移后的链接 {link_id} 已被删除，不能删除旧链接文件"
                )))
            }
            None => {
                return Err(Error::Other(format!(
                    "统一仓库缺少迁移后的链接 {link_id}，保留旧链接文件"
                )))
            }
        }
    }
    let mut removable = Vec::new();
    for path in legacy_rel_paths {
        let normalized = path.to_string_lossy().replace('\\', "/");
        if !normalized.starts_with("starmaps/") || normalized.split('/').any(|part| part == "..") {
            return Err(Error::Other(format!(
                "旧链接路径不在星图目录内：{normalized}"
            )));
        }
        if app_data_root.join(&normalized).is_file() {
            removable.push(normalized);
        }
    }
    Ok(removable)
}

enum EnsuredTag {
    Existing(Tag),
    Created(Tag, Vec<String>),
    /// 同名标签已被删除：不复活删除事实。
    Deleted,
}

fn ensure_tag(
    store: &AnnotationStore,
    name: &str,
    scope: &ObjectScope,
    now_ms: i64,
) -> Result<EnsuredTag> {
    if let Some(tag) = store.list_tags()?.into_iter().find(|tag| tag.name == name) {
        return Ok(EnsuredTag::Existing(tag));
    }
    if store
        .list_tag_records()?
        .iter()
        .any(|tag| tag.name == name && tag.deleted_at_ms.is_some())
    {
        return Ok(EnsuredTag::Deleted);
    }
    let tag = Tag {
        tag_id: uuid::Uuid::new_v4().to_string(),
        project_id: scope.project_id().unwrap_or_default().to_string(),
        name: name.to_string(),
        color: None,
        updated_at_ms: now_ms,
        deleted_at_ms: None,
    };
    let changed = store.apply_mutation(AnnotationMutation::UpsertTag(tag.clone()))?;
    Ok(EnsuredTag::Created(tag, changed))
}

/// 素材归属候选：作品库或全局库；同时存在多个候选时不猜。
fn asset_scope_candidates(
    app_data_root: &Path,
    asset_id: &str,
    project_id: Option<&str>,
) -> Result<Vec<ObjectScope>> {
    let mut candidates = Vec::new();
    match project_id.filter(|id| !id.is_empty()) {
        Some(project_id) => {
            if is_safe_segment(project_id) && project_downloaded(app_data_root, project_id) {
                candidates.push(ObjectScope::Project {
                    project_id: project_id.to_string(),
                });
            }
        }
        None => candidates.push(ObjectScope::Global),
    }
    // 库索引中的另一条归属（例如素材被移动到作品库、或全局库仍留着旧条目）
    // 也是候选；索引损坏时不据此猜测，交给上面的结果决定。
    if let Ok(index) = crate::starmap::library::load_library_index(app_data_root) {
        for entry in &index.entries {
            if entry.target.kind != StarMapAssetKind::Node || entry.target.asset_id != asset_id {
                continue;
            }
            let candidate = match &entry.scope {
                crate::starmap::types::StarMapAssetScope::Global => ObjectScope::Global,
                crate::starmap::types::StarMapAssetScope::Project { project_id } => {
                    if !is_safe_segment(project_id)
                        || !project_downloaded(app_data_root, project_id)
                    {
                        continue;
                    }
                    ObjectScope::Project {
                        project_id: project_id.clone(),
                    }
                }
            };
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    Ok(candidates)
}

fn store_for_scope(app_data_root: &Path, scope: &ObjectScope) -> AnnotationStore {
    match scope {
        ObjectScope::Global => AnnotationStore::global(app_data_root),
        ObjectScope::Project { project_id } => {
            AnnotationStore::new(&app_data_root.join("projects").join(project_id), project_id)
        }
    }
}

fn prefix_paths(scope: &ObjectScope, paths: Vec<String>) -> Vec<String> {
    match scope.project_id() {
        Some(project_id) => paths
            .into_iter()
            .map(|path| format!("projects/{project_id}/{path}"))
            .collect(),
        None => paths,
    }
}

fn legacy_node_instance_source(starmap_id: &str, source: &StarMapTargetPath) -> Option<ObjectRef> {
    if source.starmap_id != starmap_id || !source.segments.is_empty() {
        return None;
    }
    match &source.target {
        StarMapTargetDetail::Node { node_id } if !node_id.is_empty() => {
            Some(ObjectRef::NodeInstance {
                starmap_id: starmap_id.to_string(),
                object_id: node_id.clone(),
            })
        }
        _ => None,
    }
}

fn project_downloaded(app_data_root: &Path, project_id: &str) -> bool {
    app_data_root
        .join("projects")
        .join(project_id)
        .join("project.json")
        .is_file()
}

fn is_safe_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn seconds_to_millis(seconds: u64) -> i64 {
    i64::try_from(seconds)
        .unwrap_or(i64::MAX / 1_000)
        .saturating_mul(1_000)
}

#[cfg(test)]
mod tests;
