//! 旧 `starmaps/<id>/links/**` 到 annotations 链接仓库的一次性迁移。
//!
//! 链接的唯一持久化所有者是 annotations 仓库；这里只在迁移阶段读取旧
//! `links/` 目录，确认仓库接管后把旧文件移到回收目录，并按仓库重建
//! graph.json 的 `linkIds` / `linkRelationIndex` 派生缓存。
//!
//! 幂等：已经迁移/删除的旧文件被视为完成，重复执行不产生新写入。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::annotations::migration::LegacyStarMapLink;
use crate::annotations::{LinkTarget, ObjectLink};
use crate::error::Result;
use crate::object_ref::ObjectRef;
use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::store::relation_index::LinkRelationIndex;
use crate::starmap::types::StarMapTargetPath;

/// 画布归属作品 ID；meta 不存在或未绑定作品时返回 `None`（全局素材域）。
fn link_canvas_project_id(app_data_root: &Path, starmap_id: &str) -> Option<String> {
    crate::starmap::get_starmap(app_data_root, starmap_id)
        .ok()
        .and_then(|meta| meta.project_id)
        .filter(|id| !id.is_empty())
}

/// 读取旧 `starmaps/<id>/links/<bucket>/*.json`。
///
/// 无法解析的旧文件只记录告警并保留原文件：迁移不能因为一条坏记录就阻断
/// 整张画布的加载；保留文件让后续版本仍有机会人工修复。
fn read_legacy_link_files(
    starmap_id: &str,
    links_dir: &Path,
) -> Result<(Vec<LegacyStarMapLink>, Vec<PathBuf>)> {
    let mut legacy_links = Vec::new();
    let mut legacy_paths = Vec::new();
    for bucket_entry in std::fs::read_dir(links_dir)? {
        let bucket_entry = bucket_entry?;
        let bucket_path = bucket_entry.path();
        if !bucket_path.is_dir() {
            continue;
        }
        for file_entry in std::fs::read_dir(&bucket_path)? {
            let file_entry = file_entry?;
            let path = file_entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            let link: LegacyStarMapLink = match serde_json::from_str(&content) {
                Ok(link) => link,
                Err(error) => {
                    log::warn!(
                        "legacy starmap link parse failed, keeping file {}: {}",
                        path.display(),
                        error
                    );
                    continue;
                }
            };
            // 旧布局固定为 `<bucket>/<link_id>.json`；用官方路径推导保证与
            // `finish_link_migration` 的存在性校验一致。
            legacy_paths.push(crate::starmap::package_storage::link_rel_path(
                starmap_id,
                &link.link_id,
            ));
            legacy_links.push(link);
        }
    }
    Ok((legacy_links, legacy_paths))
}

/// 把旧 `starmaps/<id>/links/**` 迁移到 annotations 仓库（幂等）。
///
/// 1. 解析旧链接（`LegacyStarMapLink`，秒级时间戳）；
/// 2. `migrate_legacy_links` 幂等写入仓库（已存在的 source/target 关系不重复写）；
/// 3. 只对仓库确认已接管的 `link_id` 调 `finish_link_migration` 校验；
/// 4. 校验通过后把旧文件移到 `sync/trash/<token>/...`；
/// 5. 按仓库重建 graph.json 的 `linkIds` / `linkRelationIndex` 派生缓存。
///
/// 无法迁移的旧记录（来源不是本画布节点、作品未下载等）保留原文件并记录冲突，
/// 不猜测、不删除；已经删除的旧文件不会再次出现，因此重复执行不产生新写入。
pub(super) fn migrate_legacy_link_files(
    app_data_root: &Path,
    starmap_id: &str,
    graph_dir: &Path,
) -> Result<()> {
    let links_dir = graph_dir.join("links");
    if !links_dir.exists() {
        return Ok(());
    }
    let (legacy_links, legacy_paths) = read_legacy_link_files(starmap_id, &links_dir)?;
    let project_id = link_canvas_project_id(app_data_root, starmap_id);
    let store = crate::starmap::store::link_repository::annotation_store_for_project(
        app_data_root,
        project_id.as_deref(),
    );

    // 已经在仓库里有 tombstone 的链接不能被旧文件复活：`attach_link` 会
    // 把 tombstoned 关系复活，所以先按删除事实过滤，旧文件按残留清理。
    let mut pending_links: Vec<LegacyStarMapLink> = Vec::new();
    let mut pending_paths: Vec<PathBuf> = Vec::new();
    let mut tombstoned_paths: Vec<String> = Vec::new();
    for (link, rel_path) in legacy_links.iter().zip(legacy_paths.iter()) {
        match store.get_link(&link.link_id)? {
            Some(record) if record.deleted_at_ms.is_some() => {
                tombstoned_paths.push(rel_path.to_string_lossy().replace('\\', "/"));
            }
            _ => {
                pending_links.push(link.clone());
                pending_paths.push(rel_path.clone());
            }
        }
    }

    let mut migrated_ids: Vec<String> = Vec::new();
    let mut migrated_paths: Vec<PathBuf> = Vec::new();
    if !pending_links.is_empty() {
        let report = crate::annotations::migration::migrate_legacy_links(
            app_data_root,
            starmap_id,
            project_id.as_deref(),
            &pending_links,
        )?;
        for conflict in &report.conflicts {
            log::warn!(
                "legacy starmap link migration conflict ({}): {} — {}",
                conflict.kind,
                conflict.source_id,
                conflict.detail
            );
        }
        for (link, rel_path) in pending_links.iter().zip(pending_paths.iter()) {
            let Some(record) = store.get_link(&link.link_id)? else {
                // 同一 (source,target) 已由另一个 link_id 接管时旧记录不再迁移；
                // 保守保留旧文件，等待人工确认，不猜测删除。
                continue;
            };
            if record.deleted_at_ms.is_none() {
                migrated_ids.push(link.link_id.clone());
                migrated_paths.push(rel_path.clone());
            }
        }
    }

    move_legacy_link_files_to_trash(app_data_root, &tombstoned_paths);
    if !migrated_ids.is_empty() {
        match crate::annotations::migration::finish_link_migration(
            app_data_root,
            &store,
            &migrated_ids,
            &migrated_paths,
        ) {
            Ok(removable) => move_legacy_link_files_to_trash(app_data_root, &removable),
            Err(error) => {
                // 校验失败说明仓库还没真正接管这些记录：保留旧文件，下次启动重试。
                // 迁移问题不能阻断整张画布的加载。
                log::warn!(
                    "legacy starmap link migration validation failed for {}: {}",
                    starmap_id,
                    error
                );
            }
        }
    }
    cleanup_empty_legacy_link_dir(&links_dir);
    rebuild_graph_meta_link_cache(
        app_data_root,
        starmap_id,
        graph_dir,
        project_id.as_deref(),
        &store,
    )?;
    Ok(())
}

/// 把已确认迁移完成的旧链接文件移到回收目录。
///
/// 回收目录不可写时退化为删除：链接事实已由 annotations 记录承担，旧文件不再
/// 被读取。这里刻意不向上返回错误：迁移清理失败不应阻断整张画布加载，失败时
/// 保留文件、下次启动重试（仓库 tombstone 会阻止旧文件复活已删除的链接）。
fn move_legacy_link_files_to_trash(app_data_root: &Path, rel_paths: &[String]) {
    if rel_paths.is_empty() {
        return;
    }
    let trash_rel_path = format!(
        "sync/trash/{}_legacy_links",
        chrono::Utc::now().timestamp_millis()
    );
    let trash_root = app_data_root.join(&trash_rel_path);
    for rel in rel_paths {
        let source = app_data_root.join(rel);
        if !source.exists() {
            continue;
        }
        let destination = trash_root.join(rel);
        if let Some(parent) = destination.parent() {
            if let Err(error) = std::fs::create_dir_all(parent) {
                log::warn!(
                    "create trash dir failed for legacy starmap link ({}): {}",
                    parent.display(),
                    error
                );
                continue;
            }
        }
        match std::fs::rename(&source, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                log::warn!(
                    "move legacy starmap link file to trash failed ({}): {} — deleting instead",
                    source.display(),
                    error
                );
                if let Err(error) = std::fs::remove_file(&source) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        log::warn!(
                            "delete legacy starmap link file failed ({}): {}",
                            source.display(),
                            error
                        );
                    }
                }
            }
        }
    }
}

/// 删除迁移后留下的空 `links/<bucket>` 目录；非空目录原样保留。
fn cleanup_empty_legacy_link_dir(links_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(links_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir(&path);
            }
        }
    }
    let _ = std::fs::remove_dir(links_dir);
}

/// 用 annotations 仓库重建 graph.json 的链接派生缓存（`linkIds` /
/// `linkRelationIndex`）。
///
/// 链接的唯一持久事实是注解记录；graph.json 里的这两个字段只是可审查的派生
/// 缓存。只有内容真的变了才重写并推进 package revision，因此重复执行无副作用。
fn rebuild_graph_meta_link_cache(
    app_data_root: &Path,
    starmap_id: &str,
    graph_dir: &Path,
    project_id: Option<&str>,
    store: &crate::annotations::AnnotationStore,
) -> Result<()> {
    if let Some(project_id) = project_id {
        if !app_data_root
            .join("projects")
            .join(project_id)
            .join("project.json")
            .is_file()
        {
            // 作品未下载：注解仓库不可用，保留旧派生缓存等待下次迁移。
            return Ok(());
        }
    }
    let mut links: Vec<ObjectLink> = store.list_links_for_starmap(starmap_id)?;
    links.sort_by(|left, right| left.link_id.cmp(&right.link_id));
    let mut link_ids: Vec<String> = Vec::new();
    let mut relation_index: Vec<LinkRelationIndex> = Vec::new();
    for link in &links {
        let Some(source) = owned_canvas_source_path(starmap_id, &link.source) else {
            continue;
        };
        let LinkTarget::CanvasNavigation(target) = &link.target else {
            continue;
        };
        link_ids.push(link.link_id.clone());
        relation_index.push(LinkRelationIndex {
            link_id: link.link_id.clone(),
            source,
            target: target.clone(),
        });
    }
    let graph_json_path = graph_dir.join("graph.json");
    let content = std::fs::read_to_string(&graph_json_path)?;
    let mut value: Value = serde_json::from_str(&content)?;
    let new_index = serde_json::to_value(&relation_index)?;
    let current_ids: Vec<String> = value
        .get("linkIds")
        .and_then(|ids| serde_json::from_value(ids.clone()).ok())
        .unwrap_or_default();
    if current_ids == link_ids && value.get("linkRelationIndex") == Some(&new_index) {
        return Ok(());
    }
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    object.insert("linkIds".to_string(), serde_json::to_value(&link_ids)?);
    object.insert("linkRelationIndex".to_string(), new_index);
    let old_revision = object
        .get("packageRevision")
        .and_then(|revision| revision.as_u64())
        .unwrap_or(0);
    object.insert(
        "packageRevision".to_string(),
        Value::from(old_revision.saturating_add(1)),
    );
    object.insert(
        "updatedAt".to_string(),
        Value::from(crate::starmap::now_epoch()),
    );
    let new_content = serde_json::to_string_pretty(&value)?;
    crate::storage::atomic_write_string(&graph_json_path, &new_content)?;
    Ok(())
}

/// 仓库链接源转本画布导航路径；不属于本画布时返回 `None`。
fn owned_canvas_source_path(starmap_id: &str, source: &ObjectRef) -> Option<StarMapTargetPath> {
    match source {
        ObjectRef::NodeInstance {
            starmap_id: source_starmap_id,
            object_id,
        } if source_starmap_id == starmap_id && !object_id.is_empty() => Some(StarMapTargetPath {
            starmap_id: source_starmap_id.clone(),
            segments: Vec::new(),
            target: StarMapTargetDetail::Node {
                node_id: object_id.clone(),
            },
        }),
        ObjectRef::CanvasAnchor {
            starmap_id: source_starmap_id,
            object_id,
            anchor_id,
        } if source_starmap_id == starmap_id && !object_id.is_empty() && !anchor_id.is_empty() => {
            Some(StarMapTargetPath {
                starmap_id: source_starmap_id.clone(),
                segments: Vec::new(),
                target: StarMapTargetDetail::Anchor {
                    node_id: object_id.clone(),
                    anchor_id: anchor_id.clone(),
                },
            })
        }
        _ => None,
    }
}
