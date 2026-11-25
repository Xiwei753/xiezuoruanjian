//! 星图导航链接的仓库读取层（Issue #1060 第 3 节）。
//!
//! 链接的唯一持久化所有者是 annotations 仓库：
//! - 归属作品的画布：`projects/<project_id>/annotations/links/<link_id>.json`
//! - 全局素材域的画布：`annotations/links/<link_id>.json`
//!
//! `StarMapStore.links` 只是按画布过滤的读投影，不再有第二份对象文件、
//! 写队列或索引所有权。这里集中三件事：
//!
//! 1. 由画布 meta 的 `project_id` 选择注解仓库域；
//! 2. `StarMapLink`（画布导航路径）与 `ObjectLink`（仓库记录）的双向转换；
//! 3. 从仓库重建当前画布的内存投影与 `GraphMeta.link_ids` 派生缓存。

use std::path::{Path, PathBuf};

use crate::annotations::{AnnotationStore, LinkOrigin, LinkTarget, ObjectLink};
use crate::error::{Error, Result};
use crate::object_ref::ObjectRef;
use crate::starmap::semantic::StarMapTargetDetail;
use crate::starmap::types::{StarMapLink, StarMapTargetPath};

use super::meta::GraphMeta;
use super::relation_index::LinkRelationIndex;
use super::StarMapStore;

/// 画布 link 源不属于本画布时的统一错误信息。
const SOURCE_NOT_OWNED: &str = "link source is not owned by this canvas";

fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 旧 `StarMapLink` 的秒级时间戳转毫秒；0 保留「未设置」语义，用当前时间。
fn seconds_to_millis(seconds: u64, now_ms: i64) -> i64 {
    if seconds == 0 {
        return now_ms;
    }
    i64::try_from(seconds)
        .unwrap_or(i64::MAX / 1_000)
        .saturating_mul(1_000)
}

/// 仓库毫秒时间戳转旧投影的秒级时间戳。
fn millis_to_seconds(millis: i64) -> u64 {
    u64::try_from(millis.max(0) / 1_000).unwrap_or(0)
}

/// 画布归属作品 ID；画布 meta 不存在（例如尚未落盘的新建包）视为全局素材域。
fn canvas_project_id(app_data_root: &Path, starmap_id: &str) -> Result<Option<String>> {
    match crate::starmap::get_starmap(app_data_root, starmap_id) {
        Ok(meta) => Ok(meta.project_id.filter(|id| !id.is_empty())),
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// 由作品 ID 选择注解仓库：作品域用 `app_data_root/projects/<pid>` 作为
/// project_root，与 `api/service/annotation_ops.rs::annotation_store` 一致。
pub(in crate::starmap) fn annotation_store_for_project(
    app_data_root: &Path,
    project_id: Option<&str>,
) -> AnnotationStore {
    match project_id.filter(|id| !id.is_empty()) {
        Some(project_id) => {
            AnnotationStore::new(&app_data_root.join("projects").join(project_id), project_id)
        }
        None => AnnotationStore::global(app_data_root),
    }
}

/// 某张画布的注解仓库（按 meta 的 `project_id` 选择作品域/全局域）。
pub(in crate::starmap) fn annotation_store_for_canvas_at(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<AnnotationStore> {
    Ok(annotation_store_for_project(
        app_data_root,
        canvas_project_id(app_data_root, starmap_id)?.as_deref(),
    ))
}

/// 注解链接记录的 workspace 相对路径。
///
/// 项目域：`projects/<project_id>/annotations/links/<link_id>.json`；
/// 全局域：`annotations/links/<link_id>.json`。
/// 供本地 history / change set 登记链接写入，不再有 `starmaps/<id>/links/...`。
pub(crate) fn annotation_link_rel_path(
    app_data_root: &Path,
    starmap_id: &str,
    link_id: &str,
) -> Result<PathBuf> {
    let project_id = canvas_project_id(app_data_root, starmap_id)?;
    let mut path = PathBuf::new();
    if let Some(project_id) = project_id.as_deref() {
        path.push("projects");
        path.push(project_id);
    }
    path.push("annotations");
    path.push("links");
    path.push(format!("{link_id}.json"));
    Ok(path)
}

/// 从画布投影重算 `GraphMeta.link_ids` 与 `GraphMeta.link_relation_index`。
///
/// 两者都只是派生缓存：链接的持久事实在 annotations 仓库，加载时按仓库重建。
/// 返回是否发生了重建（内容不同）。
pub(in crate::starmap::store) fn derive_link_indexes(
    meta: &mut GraphMeta,
    links: impl Iterator<Item = StarMapLink>,
) -> bool {
    let mut links: Vec<StarMapLink> = links.collect();
    links.sort_by(|left, right| left.link_id.cmp(&right.link_id));
    let link_ids: Vec<String> = links.iter().map(|link| link.link_id.clone()).collect();
    let link_relation_index: Vec<LinkRelationIndex> = links
        .iter()
        .map(|link| LinkRelationIndex {
            link_id: link.link_id.clone(),
            source: link.source.clone(),
            target: link.target.clone(),
        })
        .collect();
    let index_matches = meta.link_relation_index.len() == link_relation_index.len()
        && meta
            .link_relation_index
            .iter()
            .zip(link_relation_index.iter())
            .all(|(left, right)| {
                left.link_id == right.link_id
                    && left.source == right.source
                    && left.target == right.target
            });
    if meta.link_ids == link_ids && index_matches {
        return false;
    }
    meta.link_ids = link_ids;
    meta.link_relation_index = link_relation_index;
    true
}

/// 该链接的 source 是否属于指定画布（用于按画布过滤仓库记录）。
pub(in crate::starmap::store) fn link_source_is_owned_by(
    link: &ObjectLink,
    starmap_id: &str,
) -> bool {
    match &link.source {
        ObjectRef::NodeInstance {
            starmap_id: source_starmap_id,
            ..
        }
        | ObjectRef::CanvasAnchor {
            starmap_id: source_starmap_id,
            ..
        } => source_starmap_id == starmap_id,
        _ => false,
    }
}

/// 把一条仓库链接投影成指定画布的 `StarMapLink`。
///
/// 只有 source 是该画布的节点实例/画布锚点、且 target 是画布导航时才属于该
/// 画布的链接投影；作品域仓库里其它画布的链接返回 `None` 而不是错误。
pub(in crate::starmap) fn project_object_link(
    starmap_id: &str,
    link: &ObjectLink,
) -> Option<StarMapLink> {
    if link.deleted_at_ms.is_some() {
        return None;
    }
    let source = match &link.source {
        ObjectRef::NodeInstance {
            starmap_id: source_starmap_id,
            object_id,
        } if source_starmap_id == starmap_id && !object_id.is_empty() => StarMapTargetPath {
            starmap_id: source_starmap_id.clone(),
            segments: Vec::new(),
            target: StarMapTargetDetail::Node {
                node_id: object_id.clone(),
            },
        },
        ObjectRef::CanvasAnchor {
            starmap_id: source_starmap_id,
            object_id,
            anchor_id,
        } if source_starmap_id == starmap_id && !object_id.is_empty() && !anchor_id.is_empty() => {
            StarMapTargetPath {
                starmap_id: source_starmap_id.clone(),
                segments: Vec::new(),
                target: StarMapTargetDetail::Anchor {
                    node_id: object_id.clone(),
                    anchor_id: anchor_id.clone(),
                },
            }
        }
        _ => return None,
    };
    let LinkTarget::CanvasNavigation(target) = &link.target else {
        return None;
    };
    Some(StarMapLink {
        link_id: link.link_id.clone(),
        source,
        target: target.clone(),
        label: link.label.clone(),
        created_at: millis_to_seconds(link.created_at_ms),
        updated_at: millis_to_seconds(link.updated_at_ms),
    })
}

impl StarMapStore {
    /// 当前画布所属注解仓库（作品域或全局域）。
    pub(in crate::starmap::store) fn annotation_store_for_canvas(&self) -> Result<AnnotationStore> {
        annotation_store_for_canvas_at(&self.app_data_root, &self.starmap_id)
    }

    /// 画布导航路径转对象身份：只有本画布本地节点/锚点才归本画布所有。
    ///
    /// 跨层路径（非空 segments）、其它星图路径、以及 Starmap / ChapterRange /
    /// Entity 终点都不是本画布的链接源，返回显式错误而不是猜测归属。
    pub(in crate::starmap::store) fn source_path_to_object_ref(
        &self,
        path: &StarMapTargetPath,
    ) -> Result<ObjectRef> {
        if path.starmap_id != self.starmap_id || !path.segments.is_empty() {
            return Err(Error::Other(SOURCE_NOT_OWNED.to_string()));
        }
        match &path.target {
            StarMapTargetDetail::Node { node_id } if !node_id.is_empty() => {
                Ok(ObjectRef::NodeInstance {
                    starmap_id: self.starmap_id.clone(),
                    object_id: node_id.clone(),
                })
            }
            StarMapTargetDetail::Anchor { node_id, anchor_id }
                if !node_id.is_empty() && !anchor_id.is_empty() =>
            {
                Ok(ObjectRef::CanvasAnchor {
                    starmap_id: self.starmap_id.clone(),
                    object_id: node_id.clone(),
                    anchor_id: anchor_id.clone(),
                })
            }
            _ => Err(Error::Other(SOURCE_NOT_OWNED.to_string())),
        }
    }

    /// 旧投影 `StarMapLink` 转 annotations 仓库 `ObjectLink`。
    ///
    /// 目标保留完整画布导航路径；时间戳从秒换算到毫秒（0 用当前时间）。
    pub(in crate::starmap::store) fn star_map_link_to_object_link(
        &self,
        link: &StarMapLink,
    ) -> Result<ObjectLink> {
        let source = self.source_path_to_object_ref(&link.source)?;
        let now_ms = now_millis();
        Ok(ObjectLink {
            link_id: link.link_id.clone(),
            source,
            target: LinkTarget::CanvasNavigation(link.target.clone()),
            origin: LinkOrigin::Manual,
            label: link.label.clone(),
            created_at_ms: seconds_to_millis(link.created_at, now_ms),
            updated_at_ms: seconds_to_millis(link.updated_at, now_ms),
            deleted_at_ms: None,
        })
    }

    /// annotations 仓库 `ObjectLink` 转当前画布投影。
    ///
    /// 只有 source 是本画布的节点实例/画布锚点、且 target 是画布导航时才属于
    /// 本画布的链接投影；作品域仓库里其它画布的链接返回 `None` 而不是错误。
    pub(in crate::starmap::store) fn object_link_to_star_map_link(
        &self,
        link: &ObjectLink,
    ) -> Option<StarMapLink> {
        project_object_link(&self.starmap_id, link)
    }

    /// 从注解仓库全量重建当前画布的链接投影，返回投影条数。
    ///
    /// 仓库是唯一事实来源：源属于本画布的链接全部重填，其它终点的记录被过滤。
    /// 清空重建保证仓库里已删除的链接不会残留在投影里。
    pub(in crate::starmap::store) fn load_canvas_links(&mut self) -> Result<usize> {
        let store = self.annotation_store_for_canvas()?;
        let links = store.list_links_for_starmap(&self.starmap_id)?;
        self.links.clear();
        for link in links {
            if let Some(projected) = self.object_link_to_star_map_link(&link) {
                self.links.insert(projected.link_id.clone(), projected);
            }
        }
        Ok(self.links.len())
    }

    /// 用投影重建 `GraphMeta` 的链接派生缓存；不一致时标记 GraphMeta dirty。
    ///
    /// 返回是否发生了重建。`GraphMeta.link_ids` 不是业务事实，重建不算用户
    /// 改动，但仍需写回 graph.json 才能让派生缓存与仓库保持一致。
    pub(in crate::starmap::store) fn reconcile_graph_meta_link_indexes(&mut self) -> bool {
        let links: Vec<StarMapLink> = self.links.values().cloned().collect();
        let Some(meta) = self.graph_meta.as_mut() else {
            return false;
        };
        if derive_link_indexes(meta, links.into_iter()) {
            self.dirty_graph_meta = true;
            true
        } else {
            false
        }
    }
}
