use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use super::types::*;
use crate::annotations::{
    AnnotationStore, LinkTarget, NamedEntry, ObjectLink, ObjectRef, Tag, TagBinding,
};
use crate::error::{Error, Result};

/// 一次索引提取中单个来源的失败。
///
/// 索引是可重建的派生缓存：单个作品/卷/章节/星图/设置文件读取失败时，提取
/// 继续处理其它来源，并把这个来源的路径与原始错误带回来。调用方据此保留该
/// 来源上一次有效的索引条目，而不是用空结果覆盖整批索引。
#[derive(Debug, Clone)]
pub struct ExtractionFailure {
    /// 出错的文件或目录。
    pub path: PathBuf,
    /// 原始错误文本（包含 IO/JSON 的路径上下文）。
    pub error: String,
    /// 受影响的索引范围；`SearchScope::All` 表示该目标下的全部范围。
    pub scope: SearchScope,
    /// 受影响的持久化对象；只填写已经知道的部分。
    pub target: SearchTarget,
}

impl ExtractionFailure {
    /// 旧索引中的条目是否属于这个失败来源（需要原样保留）。
    pub(crate) fn protects(&self, entry: &IndexEntry) -> bool {
        (self.scope == SearchScope::All || self.scope == entry.scope)
            && target_covers(&self.target, &entry.target)
    }
}

/// `partial` 中已填写的字段必须与 `actual` 一致；未填写表示该字段不参与匹配。
fn target_covers(partial: &SearchTarget, actual: &SearchTarget) -> bool {
    fn same(left: Option<&str>, right: Option<&str>) -> bool {
        left.map_or(true, |value| right == Some(value))
    }
    same(partial.project_id.as_deref(), actual.project_id.as_deref())
        && same(partial.volume_id.as_deref(), actual.volume_id.as_deref())
        && same(partial.chapter_id.as_deref(), actual.chapter_id.as_deref())
        && same(partial.starmap_id.as_deref(), actual.starmap_id.as_deref())
        && same(partial.node_id.as_deref(), actual.node_id.as_deref())
        && same(partial.edge_id.as_deref(), actual.edge_id.as_deref())
        && same(partial.link_id.as_deref(), actual.link_id.as_deref())
        && same(partial.embed_id.as_deref(), actual.embed_id.as_deref())
        && same(
            partial.setting_key.as_deref(),
            actual.setting_key.as_deref(),
        )
        && same(
            partial.setting_section.as_deref(),
            actual.setting_section.as_deref(),
        )
}

/// 一次全量/按作品提取的结果。
///
/// `entries` 只包含**真正提取成功**的来源；`failures` 列出失败来源。两者一起
/// 交给索引更新入口，由它决定保留哪些旧条目。
#[derive(Debug, Default)]
pub struct ExtractionReport {
    pub entries: Vec<IndexEntry>,
    pub failures: Vec<ExtractionFailure>,
}

impl ExtractionReport {
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn extend(&mut self, other: ExtractionReport) {
        self.entries.extend(other.entries);
        self.failures.extend(other.failures);
    }

    fn push(&mut self, entry: IndexEntry) {
        self.entries.push(entry);
    }

    fn fail(
        &mut self,
        path: &Path,
        error: impl std::fmt::Display,
        scope: SearchScope,
        target: SearchTarget,
    ) {
        self.failures.push(ExtractionFailure {
            path: path.to_path_buf(),
            error: error.to_string(),
            scope,
            target,
        });
    }
}

fn project_target(project_id: &str) -> SearchTarget {
    SearchTarget {
        project_id: Some(project_id.to_string()),
        ..Default::default()
    }
}

fn volume_target(project_id: &str, volume_id: &str) -> SearchTarget {
    SearchTarget {
        project_id: Some(project_id.to_string()),
        volume_id: Some(volume_id.to_string()),
        ..Default::default()
    }
}

fn chapter_target(project_id: &str, volume_id: &str, chapter_id: &str) -> SearchTarget {
    SearchTarget {
        project_id: Some(project_id.to_string()),
        volume_id: Some(volume_id.to_string()),
        chapter_id: Some(chapter_id.to_string()),
        ..Default::default()
    }
}

fn starmap_target(starmap_id: &str) -> SearchTarget {
    SearchTarget {
        starmap_id: Some(starmap_id.to_string()),
        ..Default::default()
    }
}

/// 读取一个可选的 JSON 元数据文件。
///
/// - 文件真实不存在 → `Ok(None)`，由调用方按现有含义把它当作非业务目录略过；
/// - 权限/IO 失败或 JSON 损坏 → `Err`，消息带真实路径。
fn read_json_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read {}: {error}",
                path.display()
            )))
        }
    };
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| Error::Other(format!("{} is corrupt: {error}", path.display())))
}

// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn extract_chapter_entries(
    projects_root: &Path,
    project_id: Option<&str>,
) -> Result<ExtractionReport> {
    let mut report = ExtractionReport::default();
    let projects = scan_dirs(projects_root)?;
    for (pid, project_path) in projects {
        if let Some(filter_id) = project_id {
            if pid != filter_id {
                continue;
            }
        }

        let target = project_target(&pid);
        let project_meta = project_path.join("project.json");
        let project = match read_json_optional::<crate::project::Project>(&project_meta) {
            Ok(Some(project)) => Some(project),
            // 没有 project.json 的目录不是作品，按现有含义略过。
            Ok(None) => continue,
            Err(error) => {
                // 只有作品标题来源失败；卷/章节仍按目录继续扫描。
                report.fail(
                    &project_meta,
                    error,
                    SearchScope::ProjectTitle,
                    target.clone(),
                );
                None
            }
        };
        if let Some(project) = &project {
            report.push(extract_project_title_entry(&pid, &project.title));
        }

        // Annotation extraction needs the app data root for shared node
        // definitions and canvas graphs. `projects_root` is normally
        // `<app_data_root>/projects`, so its parent is the workspace root.
        match extract_annotation_entries_with_root(&project_path, &pid, projects_root.parent()) {
            Ok(entries) => report.entries.extend(entries),
            Err(error) => {
                let annotations_dir = project_path.join("annotations");
                for scope in [
                    SearchScope::AnnotationTag,
                    SearchScope::AnnotationEntry,
                    SearchScope::AnnotationLink,
                ] {
                    report.fail(&annotations_dir, &error, scope, target.clone());
                }
            }
        }

        let volumes_dir = project_path.join("volumes");
        let volumes = match scan_dirs(&volumes_dir) {
            Ok(volumes) => volumes,
            Err(error) => {
                report.fail(&volumes_dir, error, SearchScope::All, target.clone());
                continue;
            }
        };
        for (vid, volume_path) in volumes {
            let vt = volume_target(&pid, &vid);
            let volume_meta = volume_path.join("volume.json");
            let volume = match read_json_optional::<crate::volume::Volume>(&volume_meta) {
                Ok(Some(volume)) => volume,
                // 没有 volume.json 的目录不是卷，按现有含义略过它的章节。
                Ok(None) => continue,
                Err(error) => {
                    report.fail(&volume_meta, error, SearchScope::All, vt.clone());
                    continue;
                }
            };
            report.push(extract_volume_title_entry(&pid, &vid, &volume.title));

            let chapters_dir = volume_path.join("chapters");
            let chapters = match scan_dirs(&chapters_dir) {
                Ok(chapters) => chapters,
                Err(error) => {
                    report.fail(&chapters_dir, error, SearchScope::All, vt);
                    continue;
                }
            };
            for (cid, chapter_path) in chapters {
                extract_chapter_entries_for_chapter(&mut report, &pid, &vid, &cid, &chapter_path);
            }
        }
    }

    Ok(report)
}

/// 提取一个章节的标题/正文/备注。标题读取失败时跳过整个章节，避免用空标题
/// 覆盖该章节上一次有效的标题、正文和备注索引。
fn extract_chapter_entries_for_chapter(
    report: &mut ExtractionReport,
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    chapter_path: &Path,
) {
    let target = chapter_target(project_id, volume_id, chapter_id);
    let meta_path = chapter_path.join("chapter.meta.json");
    let title = match load_chapter_title(&meta_path) {
        Ok(Some(title)) => title,
        // 没有 chapter.meta.json 的目录不是章节。
        Ok(None) => return,
        Err(error) => {
            report.fail(&meta_path, error, SearchScope::All, target);
            return;
        }
    };
    report.push(extract_chapter_title_entry(
        project_id,
        volume_id,
        chapter_id,
        &title,
    ));

    let body_path = chapter_path.join("chapter.md");
    match std::fs::read_to_string(&body_path) {
        Ok(body) => report.push(extract_chapter_body_entry(
            project_id,
            volume_id,
            chapter_id,
            &title,
            &body,
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => report.fail(
            &body_path,
            error,
            SearchScope::ChapterBody,
            chapter_target(project_id, volume_id, chapter_id),
        ),
    }

    let note_path = chapter_path.join("chapter.note.md");
    match std::fs::read_to_string(&note_path) {
        Ok(note) if !note.is_empty() => report.push(extract_chapter_note_entry(
            project_id,
            volume_id,
            chapter_id,
            &title,
            &note,
        )),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => report.fail(
            &note_path,
            error,
            SearchScope::ChapterNote,
            chapter_target(project_id, volume_id, chapter_id),
        ),
    }
}

/// Build the searchable projection of one project's active annotation records.
/// Tags, bindings, entries, and links each receive stable IDs derived from their
/// persisted record IDs so incremental updates can replace them independently.
pub fn extract_annotation_entries(
    project_root: &Path,
    project_id: &str,
) -> Result<Vec<IndexEntry>> {
    extract_annotation_entries_with_root(
        project_root,
        project_id,
        project_root.parent().and_then(Path::parent),
    )
}

/// Same as [`extract_annotation_entries`] with an explicit app data root so
/// shared node definitions and canvas graphs can be resolved from outside the
/// conventional `projects/<id>` layout.
pub fn extract_annotation_entries_with_root(
    project_root: &Path,
    project_id: &str,
    app_data_root: Option<&Path>,
) -> Result<Vec<IndexEntry>> {
    let store = AnnotationStore::new(project_root, project_id);
    let tags = store.list_tags()?;
    let tags_by_id: std::collections::HashMap<_, _> =
        tags.iter().map(|tag| (tag.tag_id.as_str(), tag)).collect();
    let mut entries = Vec::new();

    for tag in &tags {
        entries.push(extract_annotation_tag_entry(tag));
    }
    for binding in store.list_bindings()? {
        if let Some(tag) = tags_by_id.get(binding.tag_id.as_str()) {
            entries.push(extract_annotation_binding_entry(
                &store,
                tag,
                &binding,
                app_data_root,
            )?);
        }
    }
    for named_entry in store.list_named_entries()? {
        entries.push(extract_annotation_named_entry(&named_entry));
    }
    for link in store.list_links()? {
        entries.push(extract_annotation_link_entry(&store, &link, app_data_root)?);
    }

    Ok(entries)
}

/// Produce incremental search updates for the active projection and remove
/// tombstoned or no-longer-derived objects from the existing project index.
pub fn annotation_search_updates_for_project(
    app_data_root: &Path,
    project_root: &Path,
    project_id: &str,
    existing_ids: &[String],
) -> Result<Vec<SearchIndexUpdate>> {
    let store = AnnotationStore::new(project_root, project_id);
    let mut updates = Vec::new();
    let mut active_ids = std::collections::HashSet::new();

    for entry in
        extract_annotation_entries_with_root(project_root, project_id, Some(app_data_root))?
    {
        active_ids.insert(entry.object_id.clone());
        updates.push(SearchIndexUpdate {
            action: SearchIndexAction::Upsert,
            object_id: entry.object_id,
            scope: entry.scope,
            title: entry.title,
            body: entry.body,
            target: Some(entry.target),
        });
    }

    let mut tombstones = Vec::new();
    tombstones.extend(
        store
            .list_tag_records()?
            .into_iter()
            .filter(|record| record.project_id == project_id && record.deleted_at_ms.is_some())
            .map(|record| {
                (
                    format!("annotation:{project_id}:tag:{}", record.tag_id),
                    SearchScope::AnnotationTag,
                )
            }),
    );
    tombstones.extend(
        store
            .list_binding_records()?
            .into_iter()
            .filter(|record| record.project_id == project_id && record.deleted_at_ms.is_some())
            .map(|record| {
                (
                    format!("annotation:{project_id}:binding:{}", record.binding_id),
                    SearchScope::AnnotationTag,
                )
            }),
    );
    tombstones.extend(
        store
            .list_named_entry_records()?
            .into_iter()
            .filter(|record| record.project_id == project_id && record.deleted_at_ms.is_some())
            .map(|record| {
                (
                    format!("annotation:{project_id}:entry:{}", record.entry_id),
                    SearchScope::AnnotationEntry,
                )
            }),
    );
    tombstones.extend(
        store
            .list_link_records()?
            .into_iter()
            .filter(|record| {
                record.source.project_id() == Some(project_id) && record.deleted_at_ms.is_some()
            })
            .map(|record| {
                (
                    format!("annotation:{project_id}:link:{}", record.link_id),
                    SearchScope::AnnotationLink,
                )
            }),
    );

    let mut delete_ids = std::collections::HashSet::new();
    for (object_id, scope) in tombstones {
        if delete_ids.insert(object_id.clone()) {
            updates.push(annotation_delete_update(&object_id, scope, project_id));
        }
    }
    for object_id in existing_ids {
        if !object_id.starts_with(&format!("annotation:{project_id}:"))
            || active_ids.contains(object_id)
            || !delete_ids.insert(object_id.clone())
        {
            continue;
        }
        updates.push(annotation_delete_update(
            object_id,
            annotation_scope_for_id(object_id),
            project_id,
        ));
    }

    Ok(updates)
}

fn annotation_delete_update(
    object_id: &str,
    scope: SearchScope,
    project_id: &str,
) -> SearchIndexUpdate {
    SearchIndexUpdate {
        action: SearchIndexAction::Delete,
        object_id: object_id.to_string(),
        scope,
        title: String::new(),
        body: String::new(),
        target: Some(SearchTarget {
            project_id: Some(project_id.to_string()),
            ..Default::default()
        }),
    }
}

fn annotation_scope_for_id(object_id: &str) -> SearchScope {
    if object_id.contains(":entry:") {
        SearchScope::AnnotationEntry
    } else if object_id.contains(":link:") {
        SearchScope::AnnotationLink
    } else {
        SearchScope::AnnotationTag
    }
}

fn extract_annotation_tag_entry(tag: &Tag) -> IndexEntry {
    IndexEntry {
        object_id: format!("annotation:{}:tag:{}", tag.project_id, tag.tag_id),
        scope: SearchScope::AnnotationTag,
        title: tag.name.clone(),
        body: tag.name.clone(),
        target: annotation_project_target(&tag.project_id),
    }
}

fn extract_annotation_binding_entry(
    store: &AnnotationStore,
    tag: &Tag,
    binding: &TagBinding,
    app_data_root: Option<&Path>,
) -> Result<IndexEntry> {
    let target = search_target_for_object_ref(store, &binding.target)?;
    let mut body = vec![object_ref_kind_label(&binding.target).to_string()];
    let detail = object_ref_search_text(store, &binding.target, app_data_root)?;
    if !detail.is_empty() {
        body.push(detail);
    }
    Ok(IndexEntry {
        object_id: format!(
            "annotation:{}:binding:{}",
            binding.project_id, binding.binding_id
        ),
        scope: SearchScope::AnnotationTag,
        title: tag.name.clone(),
        body: body.join(" "),
        target,
    })
}

fn extract_annotation_named_entry(entry: &NamedEntry) -> IndexEntry {
    let mut body = Vec::new();
    body.extend(entry.aliases.iter().map(String::as_str));
    body.extend(entry.confirmed_misspellings.iter().map(String::as_str));
    IndexEntry {
        object_id: format!("annotation:{}:entry:{}", entry.project_id, entry.entry_id),
        scope: SearchScope::AnnotationEntry,
        title: entry.canonical_name.clone(),
        body: body.join(" "),
        target: annotation_project_target(&entry.project_id),
    }
}

fn extract_annotation_link_entry(
    store: &AnnotationStore,
    link: &ObjectLink,
    app_data_root: Option<&Path>,
) -> Result<IndexEntry> {
    let source = object_ref_search_text(store, &link.source, app_data_root)?;
    let target_text = match &link.target {
        LinkTarget::Object(target) => object_ref_search_text(store, target, app_data_root)?,
        LinkTarget::CanvasNavigation(route) => canvas_route_search_text(route),
    };
    let title = if target_text.is_empty() {
        format!("链接 {}", link.link_id)
    } else {
        target_text.clone()
    };
    let mut body = format!("{source} {target_text}");
    if let Some(label) = link.label.as_deref().filter(|label| !label.is_empty()) {
        body = format!("{label} {body}");
    }
    Ok(IndexEntry {
        object_id: format!(
            "annotation:{}:link:{}",
            link.source.project_id().unwrap_or_default(),
            link.link_id
        ),
        scope: SearchScope::AnnotationLink,
        title,
        body: body.trim().to_string(),
        target: search_target_for_object_ref(store, &link.source)?,
    })
}

fn annotation_project_target(project_id: &str) -> SearchTarget {
    SearchTarget {
        project_id: Some(project_id.to_string()),
        ..Default::default()
    }
}

fn search_target_for_object_ref(
    store: &AnnotationStore,
    object_ref: &ObjectRef,
) -> Result<SearchTarget> {
    let mut target = match object_ref.project_id() {
        Some(project_id) => annotation_project_target(project_id),
        None => SearchTarget::default(),
    };
    match object_ref {
        ObjectRef::Project { .. } | ObjectRef::NamedEntry { .. } => {}
        ObjectRef::Volume { object_id, .. } => target.volume_id = Some(object_id.clone()),
        ObjectRef::Chapter { object_id, .. } => target.chapter_id = Some(object_id.clone()),
        ObjectRef::TextAnchor { object_id, .. } => {
            if let Some(anchor) = store.get_text_anchor(object_id)? {
                target.chapter_id = Some(anchor.chapter_id);
            }
        }
        ObjectRef::StarMap { object_id, .. } => target.starmap_id = Some(object_id.clone()),
        ObjectRef::NodeAsset { object_id, .. } => target.node_id = Some(object_id.clone()),
        ObjectRef::NodeInstance {
            starmap_id,
            object_id,
        } => {
            target.starmap_id = Some(starmap_id.clone());
            target.node_id = Some(object_id.clone());
        }
        ObjectRef::EmbedInstance {
            starmap_id,
            object_id,
        } => {
            target.starmap_id = Some(starmap_id.clone());
            target.embed_id = Some(object_id.clone());
        }
        ObjectRef::CanvasAnchor {
            starmap_id,
            object_id,
            ..
        } => {
            target.starmap_id = Some(starmap_id.clone());
            target.node_id = Some(object_id.clone());
        }
    }
    Ok(target)
}

fn object_ref_search_text(
    store: &AnnotationStore,
    object_ref: &ObjectRef,
    app_data_root: Option<&Path>,
) -> Result<String> {
    let kind = object_ref_kind_label(object_ref);
    let fallback = || format!("{} {}", kind, object_ref.object_id());
    match object_ref {
        ObjectRef::NamedEntry { object_id, .. } => Ok(store
            .get_named_entry(object_id)?
            .filter(|entry| entry.deleted_at_ms.is_none())
            .map(|entry| {
                let mut names = vec![entry.canonical_name];
                names.extend(entry.aliases);
                format!("{} {}", kind, names.join(" "))
            })
            .unwrap_or_else(fallback)),
        ObjectRef::TextAnchor { object_id, .. } => Ok(store
            .get_text_anchor(object_id)?
            .map(|anchor| {
                format!(
                    "{} {} {}-{}",
                    kind, anchor.chapter_id, anchor.start_byte, anchor.end_byte
                )
            })
            .unwrap_or_else(fallback)),
        ObjectRef::NodeAsset { object_id, .. } => Ok(
            node_asset_search_text(app_data_root, object_id)?.unwrap_or_else(fallback),
        ),
        ObjectRef::NodeInstance {
            starmap_id,
            object_id,
        } => Ok(
            canvas_node_search_text(app_data_root, starmap_id, object_id)?
                .unwrap_or_else(fallback),
        ),
        ObjectRef::EmbedInstance {
            starmap_id,
            object_id,
        } => Ok(
            canvas_embed_search_text(app_data_root, starmap_id, object_id)?
                .unwrap_or_else(fallback),
        ),
        ObjectRef::CanvasAnchor {
            object_id,
            anchor_id,
            ..
        } => Ok(
            canvas_anchor_search_text(app_data_root, object_id, anchor_id)?
                .unwrap_or_else(fallback),
        ),
        _ => Ok(fallback()),
    }
}

fn canvas_route_search_text(route: &crate::object_ref::CanvasRoute) -> String {
    match &route.target {
        crate::starmap::types::StarMapTargetDetail::Starmap => "星图".to_string(),
        crate::starmap::types::StarMapTargetDetail::Node { node_id } => {
            format!("星图节点 {node_id}")
        }
        crate::starmap::types::StarMapTargetDetail::Anchor { node_id, anchor_id } => {
            format!("星图锚点 {node_id}/{anchor_id}")
        }
        crate::starmap::types::StarMapTargetDetail::ChapterRange {
            chapter_id,
            range_start,
            range_end,
            ..
        } => match (range_start, range_end) {
            (Some(start), Some(end)) => format!("章节 {chapter_id} {start}-{end}"),
            _ => format!("章节 {chapter_id}"),
        },
        crate::starmap::types::StarMapTargetDetail::Entity {
            entity_type,
            entity_id,
        } => format!("{entity_type} {entity_id}"),
    }
}

/// Shared node asset text. The definition is the single content owner; every
/// canvas instance projects the same text.
///
/// 只有素材真实不存在时返回 `Ok(None)`；素材库读取/解析失败向上传播，不能把
/// 失败当成「该对象没有可搜索文本」。
fn node_asset_search_text(
    app_data_root: Option<&Path>,
    asset_id: &str,
) -> Result<Option<String>> {
    let Some(app_data_root) = app_data_root else {
        return Ok(None);
    };
    Ok(
        crate::starmap::library::load_node_definition(app_data_root, asset_id)?
            .map(|definition| node_definition_text(&definition.title, &definition.content)),
    )
}

fn canvas_node_search_text(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    instance_id: &str,
) -> Result<Option<String>> {
    let Some(store) = load_canvas_store(app_data_root, starmap_id)? else {
        return Ok(None);
    };
    Ok(store
        .get_node(instance_id)
        .map(|node| node_definition_text(&node.title, &node.content)))
}

fn canvas_embed_search_text(
    app_data_root: Option<&Path>,
    starmap_id: &str,
    instance_id: &str,
) -> Result<Option<String>> {
    let Some(store) = load_canvas_store(app_data_root, starmap_id)? else {
        return Ok(None);
    };
    Ok(store.get_embed(instance_id).map(|embed| {
        embed
            .label
            .clone()
            .unwrap_or_else(|| embed.target_starmap_id.clone())
    }))
}

fn canvas_anchor_search_text(
    app_data_root: Option<&Path>,
    asset_id: &str,
    anchor_id: &str,
) -> Result<Option<String>> {
    let Some(app_data_root) = app_data_root else {
        return Ok(None);
    };
    let Some(definition) =
        crate::starmap::library::load_node_definition(app_data_root, asset_id)?
    else {
        return Ok(None);
    };
    Ok(definition
        .anchors
        .iter()
        .find(|anchor| anchor.anchor_id == anchor_id)
        .map(|anchor| {
            anchor
                .label
                .clone()
                .unwrap_or_else(|| format!("锚点 {}", anchor.anchor_id))
        }))
}

/// 加载画布。画布目录/`graph.json` 真实不存在时保持空图（返回空 store），
/// 只有 IO/JSON 损坏才向上传播。
fn load_canvas_store(
    app_data_root: Option<&Path>,
    starmap_id: &str,
) -> Result<Option<crate::starmap::store::StarMapStore>> {
    let Some(app_data_root) = app_data_root else {
        return Ok(None);
    };
    let mut store = crate::starmap::store::StarMapStore::new(app_data_root, starmap_id);
    store.load_full()?;
    Ok(Some(store))
}

fn node_definition_text(
    title: &str,
    content: &crate::starmap::semantic::StarMapNodeContent,
) -> String {
    let mut parts = Vec::new();
    if !title.is_empty() {
        parts.push(title.to_string());
    }
    let text = content.search_text();
    if !text.is_empty() {
        parts.push(text);
    }
    parts.join(" ")
}

fn object_ref_kind_label(object_ref: &ObjectRef) -> &'static str {
    match object_ref {
        ObjectRef::Project { .. } => "作品",
        ObjectRef::Volume { .. } => "卷",
        ObjectRef::Chapter { .. } => "章节",
        ObjectRef::TextAnchor { .. } => "正文选区",
        ObjectRef::StarMap { .. } => "星图",
        ObjectRef::NodeAsset { .. } => "节点素材",
        ObjectRef::NodeInstance { .. } => "画布节点",
        ObjectRef::EmbedInstance { .. } => "画布嵌入",
        ObjectRef::CanvasAnchor { .. } => "画布锚点",
        ObjectRef::NamedEntry { .. } => "专有名词",
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn extract_starmap_entries(
    app_data_root: &Path,
    project_id: Option<&str>,
) -> Result<ExtractionReport> {
    let mut report = ExtractionReport::default();
    let starmaps_dir = app_data_root.join("starmaps");
    let starmap_dirs = scan_dirs(&starmaps_dir)?;

    for (sid, starmap_path) in starmap_dirs {
        let meta_path = starmaps_dir.join(format!("{}.meta.json", sid));
        let meta = match read_json_optional::<crate::starmap::StarMapMeta>(&meta_path) {
            Ok(Some(meta)) => meta,
            // 没有 meta 的目录不是可索引的星图。
            Ok(None) => continue,
            Err(error) => {
                report.fail(&meta_path, error, SearchScope::All, starmap_target(&sid));
                continue;
            }
        };

        let bound_project: Option<&str> = meta.project_id.as_deref();

        if let Some(filter_id) = project_id {
            if bound_project != Some(filter_id) {
                continue;
            }
        }

        report.push(extract_starmap_title_entry(
            &sid,
            bound_project,
            &meta.title,
        ));

        let nodes_dir = starmap_path.join("nodes");
        match scan_json_files(&nodes_dir) {
            Ok(node_files) => {
                for (nid, node_file) in node_files {
                    extract_starmap_node(
                        &mut report,
                        app_data_root,
                        &sid,
                        bound_project,
                        &nid,
                        &node_file,
                    );
                }
            }
            Err(error) => report.fail(
                &nodes_dir,
                error,
                SearchScope::StarmapNode,
                starmap_target(&sid),
            ),
        }

        let edges_dir = starmap_path.join("edges");
        match scan_json_files(&edges_dir) {
            Ok(edge_files) => {
                for (eid, edge_file) in edge_files {
                    match read_json_optional::<crate::starmap::types::StarMapEdge>(&edge_file) {
                        Ok(Some(edge)) => {
                            if let Some(label) = edge.label.filter(|label| !label.is_empty()) {
                                report.push(extract_starmap_edge_entry(
                                    &sid,
                                    &eid,
                                    bound_project,
                                    &label,
                                ));
                            }
                        }
                        Ok(None) => {}
                        Err(error) => report.fail(
                            &edge_file,
                            error,
                            SearchScope::StarmapEdgeLabel,
                            SearchTarget {
                                project_id: bound_project.map(str::to_string),
                                starmap_id: Some(sid.clone()),
                                edge_id: Some(eid),
                                ..Default::default()
                            },
                        ),
                    }
                }
            }
            Err(error) => report.fail(
                &edges_dir,
                error,
                SearchScope::StarmapEdgeLabel,
                starmap_target(&sid),
            ),
        }

        let links_dir = starmap_path.join("links");
        match scan_json_files(&links_dir) {
            Ok(link_files) => {
                for (lid, link_file) in link_files {
                    match read_json_optional::<crate::starmap::types::StarMapLink>(&link_file) {
                        Ok(Some(link)) => {
                            if let Some(label) = link.label.filter(|label| !label.is_empty()) {
                                report.push(extract_starmap_link_entry(
                                    &sid,
                                    &lid,
                                    bound_project,
                                    &label,
                                ));
                            }
                        }
                        Ok(None) => {}
                        Err(error) => report.fail(
                            &link_file,
                            error,
                            SearchScope::StarmapLink,
                            SearchTarget {
                                project_id: bound_project.map(str::to_string),
                                starmap_id: Some(sid.clone()),
                                link_id: Some(lid),
                                ..Default::default()
                            },
                        ),
                    }
                }
            }
            Err(error) => report.fail(
                &links_dir,
                error,
                SearchScope::StarmapLink,
                starmap_target(&sid),
            ),
        }

        let embeds_dir = starmap_path.join("embeds");
        match scan_json_files(&embeds_dir) {
            Ok(embed_files) => {
                for (eid, embed_file) in embed_files {
                    match read_json_optional::<crate::starmap::types::StarMapEmbed>(&embed_file) {
                        Ok(Some(embed)) => {
                            let label = embed.label.as_deref().unwrap_or("");
                            report.push(extract_starmap_embed_entry(
                                &sid,
                                &eid,
                                bound_project,
                                label,
                            ));
                        }
                        Ok(None) => {}
                        Err(error) => report.fail(
                            &embed_file,
                            error,
                            SearchScope::StarmapEmbed,
                            SearchTarget {
                                project_id: bound_project.map(str::to_string),
                                starmap_id: Some(sid.clone()),
                                embed_id: Some(eid),
                                ..Default::default()
                            },
                        ),
                    }
                }
            }
            Err(error) => report.fail(
                &embeds_dir,
                error,
                SearchScope::StarmapEmbed,
                starmap_target(&sid),
            ),
        }
    }

    Ok(report)
}

/// 提取一个画布节点。节点文件/共享定义读取失败时记录该节点来源，不生成空标题
/// 条目去覆盖旧索引。
fn extract_starmap_node(
    report: &mut ExtractionReport,
    app_data_root: &Path,
    starmap_id: &str,
    bound_project: Option<&str>,
    node_id: &str,
    node_file: &Path,
) {
    let target = SearchTarget {
        project_id: bound_project.map(str::to_string),
        starmap_id: Some(starmap_id.to_string()),
        node_id: Some(node_id.to_string()),
        ..Default::default()
    };
    // 新契约（Issue #1060/#1062）：节点文件保存 StarMapNodeInstance，
    // 共享标题/正文在素材库定义里；必须走统一解码入口，不能把节点文件
    // 当完整 StarMapNode 反序列化。
    let bytes = match std::fs::read(node_file) {
        Ok(bytes) => bytes,
        Err(error) => {
            report.fail(
                node_file,
                format!("failed to read {}: {error}", node_file.display()),
                SearchScope::StarmapNode,
                target,
            );
            return;
        }
    };
    let plan = match crate::starmap::migration::nodes::decode_node_object(
        app_data_root,
        starmap_id,
        node_id,
        &bytes,
        None,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            report.fail(node_file, error, SearchScope::StarmapNode, target);
            return;
        }
    };
    let definition = match plan.definition {
        Some(definition) => Some(definition),
        None => match crate::starmap::library::load_node_definition(
            app_data_root,
            &plan.instance.asset_id,
        ) {
            Ok(definition) => definition,
            Err(error) => {
                report.fail(node_file, error, SearchScope::StarmapNode, target);
                return;
            }
        },
    };
    let Some(definition) = definition else {
        report.fail(
            node_file,
            format!(
                "node definition {} is missing for canvas node {}",
                plan.instance.asset_id, node_id
            ),
            SearchScope::StarmapNode,
            target,
        );
        return;
    };
    // Issue #1060：节点上的旧 `tags` 字段已迁到 annotations
    // 仓库，星图实体条目不再携带第二份标签文本。
    let content = extract_node_search_text(&definition.content);
    if !definition.title.is_empty() || !content.is_empty() {
        report.push(extract_starmap_node_entry(
            starmap_id,
            node_id,
            bound_project,
            &definition.title,
            &content,
        ));
    }
}

struct SettingCatalogEntry {
    group: &'static str,
    key: &'static str,
    title: &'static str,
    aliases: &'static str,
    description: &'static str,
    section: &'static str,
}

/// 设置项的人类可读名称/别名/说明。索引展示这些文本，而不是原始 JSON。
const SETTINGS_CATALOG: &[SettingCatalogEntry] = &[
    SettingCatalogEntry {
        group: "local",
        key: "appearanceMode",
        title: "主题模式",
        aliases: "外观 深浅 深色 浅色 跟随系统 appearance theme dark light system",
        description: "选择跟随系统、浅色或深色外观",
        section: "appearance",
    },
    SettingCatalogEntry {
        group: "local",
        key: "colorSource",
        title: "颜色来源",
        aliases: "配色 动态取色 内置主题 color source",
        description: "选择动态取色或内置主题配色",
        section: "appearance",
    },
    SettingCatalogEntry {
        group: "local",
        key: "dynamicColorEnabled",
        title: "动态取色",
        aliases: "壁纸 配色 monet dynamic color",
        description: "根据系统壁纸自动生成界面配色",
        section: "appearance",
    },
    SettingCatalogEntry {
        group: "local",
        key: "selectedBuiltinThemeId",
        title: "内置主题",
        aliases: "配色 主题 builtin theme",
        description: "选择内置主题",
        section: "appearance",
    },
    SettingCatalogEntry {
        group: "local",
        key: "selectedPaletteId",
        title: "已保存配色",
        aliases: "调色板 主题 palette",
        description: "选择已保存的配色方案",
        section: "appearance",
    },
    SettingCatalogEntry {
        group: "local",
        key: "locale",
        title: "界面语言",
        aliases: "语言 地区 language locale",
        description: "选择界面显示语言",
        section: "appearance",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorFontSize",
        title: "编辑器字号",
        aliases: "字体大小 正文字号 font size",
        description: "正文编辑器的字号大小",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorLineSpacingMultiplier",
        title: "行距",
        aliases: "行间距 间距 line spacing",
        description: "正文行间距倍数",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "autoIndentEnabled",
        title: "自动首行缩进",
        aliases: "缩进 首行 indent",
        description: "换行时自动缩进首行",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "autoIndentWidth",
        title: "缩进宽度",
        aliases: "缩进 字符 indent width",
        description: "自动缩进的宽度",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorTypingAnimationEnabled",
        title: "打字动画",
        aliases: "输入动画 typing animation",
        description: "输入文字时播放打字动画",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorSmoothCursorEnabled",
        title: "平滑光标",
        aliases: "光标动画 cursor smooth",
        description: "光标移动时使用平滑动画",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorTypingAnimationDurationMs",
        title: "打字动画时长",
        aliases: "输入动画 时间 typing duration",
        description: "打字动画的持续时间",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorSmoothCursorDurationMs",
        title: "平滑光标时长",
        aliases: "光标动画 时间 cursor duration",
        description: "平滑光标动画的持续时间",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "editorCoordinatedTextCursorAnimationEnabled",
        title: "协同动画",
        aliases: "吞字 吐字 文本动画 coordinated animation",
        description: "开启吞字与吐字的协同文本动画",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "local",
        key: "autoSaveEnabled",
        title: "自动保存",
        aliases: "保存 auto save",
        description: "编辑时自动保存正文",
        section: "save_sync",
    },
    SettingCatalogEntry {
        group: "local",
        key: "autoSaveDelayMs",
        title: "自动保存延迟",
        aliases: "保存 等待 时间 auto save delay",
        description: "停止输入后等待多久自动保存",
        section: "save_sync",
    },
    SettingCatalogEntry {
        group: "local",
        key: "aiEnabled",
        title: "启用 AI 功能",
        aliases: "人工智能 AI artificial intelligence",
        description: "开启 AI 辅助功能",
        section: "ai",
    },
    SettingCatalogEntry {
        group: "local",
        key: "diagnosticsEnabled",
        title: "启用诊断日志",
        aliases: "日志 诊断 diagnostics log",
        description: "记录应用诊断日志",
        section: "diagnostics",
    },
    SettingCatalogEntry {
        group: "local",
        key: "diagnosticsVerbose",
        title: "详细日志",
        aliases: "日志 诊断 verbose diagnostics",
        description: "记录更详细的诊断日志",
        section: "diagnostics",
    },
    SettingCatalogEntry {
        group: "local",
        key: "statsDeviceId",
        title: "统计设备标识",
        aliases: "设备 统计 device stats",
        description: "本机写作统计使用的设备标识",
        section: "diagnostics",
    },
    SettingCatalogEntry {
        group: "sync",
        key: "fontSize",
        title: "同步字号",
        aliases: "字体大小 正文字号 float font size",
        description: "跨设备同步的编辑器字号",
        section: "editor",
    },
    SettingCatalogEntry {
        group: "sync",
        key: "themeMode",
        title: "主题模式（旧版）",
        aliases: "外观 深浅 深色 浅色 theme dark light",
        description: "旧版可同步主题模式",
        section: "appearance",
    },
];

fn setting_value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Bool(true) => "已开启".to_string(),
        serde_json::Value::Bool(false) => "已关闭".to_string(),
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => "未设置".to_string(),
        other => other.to_string(),
    }
}

/// 把一份已解析的设置 JSON 展开成逐项可搜索的索引条目。
fn settings_entries_from_value(group: &str, value: &serde_json::Value) -> Vec<IndexEntry> {
    SETTINGS_CATALOG
        .iter()
        .filter(|item| item.group == group)
        .filter_map(|item| {
            let raw = value.get(item.key)?;
            Some(IndexEntry {
                object_id: format!("setting:{}:{}", group, item.key),
                scope: SearchScope::Setting,
                title: item.title.to_string(),
                body: format!(
                    "{} {} {} {}",
                    item.key,
                    item.aliases,
                    item.description,
                    setting_value_text(raw)
                ),
                target: SearchTarget {
                    setting_key: Some(item.key.to_string()),
                    setting_section: Some(item.section.to_string()),
                    ..Default::default()
                },
            })
        })
        .collect()
}

/// 把一份设置 JSON 展开成逐项可搜索的索引条目。
///
/// 调用方传入的是刚从强类型设置 DTO 序列化出的内存 JSON（设置保存后的增量
/// 投影）；磁盘读取走 [`extract_setting_entries`]，后者区分真实不存在与
/// IO/JSON 损坏。
pub fn extract_settings_from_json(group: &str, content: &str) -> Vec<IndexEntry> {
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(value) => settings_entries_from_value(group, &value),
        Err(_) => Vec::new(),
    }
}

fn extract_settings_file<T: serde::Serialize>(
    report: &mut ExtractionReport,
    app_data_root: &Path,
    file_name: &str,
    group: &str,
    default: T,
) {
    let path = app_data_root.join(file_name);
    let value = match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(value) => value,
            Err(error) => {
                report.fail(
                    &path,
                    format!("settings {} is corrupt: {error}", path.display()),
                    SearchScope::Setting,
                    SearchTarget::default(),
                );
                return;
            }
        },
        // 只有设置文件真实不存在才使用默认配置；权限/IO 失败必须保留旧索引。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match serde_json::to_value(default) {
                Ok(value) => value,
                Err(error) => {
                    report.fail(
                        &path,
                        format!(
                            "failed to serialize default settings for {}: {error}",
                            path.display()
                        ),
                        SearchScope::Setting,
                        SearchTarget::default(),
                    );
                    return;
                }
            }
        }
        Err(error) => {
            report.fail(
                &path,
                format!("failed to read settings {}: {error}", path.display()),
                SearchScope::Setting,
                SearchTarget::default(),
            );
            return;
        }
    };
    report
        .entries
        .extend(settings_entries_from_value(group, &value));
}

pub fn extract_setting_entries(app_data_root: &Path) -> Result<ExtractionReport> {
    let mut report = ExtractionReport::default();
    extract_settings_file(
        &mut report,
        app_data_root,
        "settings.local.json",
        "local",
        crate::settings::LocalSettings::default(),
    );
    extract_settings_file(
        &mut report,
        app_data_root,
        "settings.sync.json",
        "sync",
        crate::settings::SyncableSettings::default(),
    );
    Ok(report)
}

pub fn extract_starmap_node_entry(
    starmap_id: &str,
    node_id: &str,
    project_id: Option<&str>,
    title: &str,
    content: &str,
) -> IndexEntry {
    let body = if content.is_empty() {
        title.to_string()
    } else {
        content.to_string()
    };
    IndexEntry {
        object_id: format!("starmap_node:{}:{}", starmap_id, node_id),
        scope: SearchScope::StarmapNode,
        title: title.to_string(),
        body,
        target: SearchTarget {
            project_id: project_id.map(|s| s.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: Some(starmap_id.to_string()),
            node_id: Some(node_id.to_string()),
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_starmap_edge_entry(
    starmap_id: &str,
    edge_id: &str,
    project_id: Option<&str>,
    label: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("starmap_edge:{}:{}", starmap_id, edge_id),
        scope: SearchScope::StarmapEdgeLabel,
        title: label.to_string(),
        body: label.to_string(),
        target: SearchTarget {
            project_id: project_id.map(|s| s.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: Some(starmap_id.to_string()),
            node_id: None,
            edge_id: Some(edge_id.to_string()),
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_starmap_link_entry(
    starmap_id: &str,
    link_id: &str,
    project_id: Option<&str>,
    label: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("starmap_link:{}:{}", starmap_id, link_id),
        scope: SearchScope::StarmapLink,
        title: label.to_string(),
        body: label.to_string(),
        target: SearchTarget {
            project_id: project_id.map(|s| s.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: Some(starmap_id.to_string()),
            node_id: None,
            link_id: Some(link_id.to_string()),
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_starmap_embed_entry(
    starmap_id: &str,
    instance_id: &str,
    project_id: Option<&str>,
    label: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("starmap_embed:{}:{}", starmap_id, instance_id),
        scope: SearchScope::StarmapEmbed,
        title: label.to_string(),
        body: label.to_string(),
        target: SearchTarget {
            project_id: project_id.map(|s| s.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: Some(starmap_id.to_string()),
            node_id: None,
            embed_id: Some(instance_id.to_string()),
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_starmap_title_entry(
    starmap_id: &str,
    project_id: Option<&str>,
    title: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("starmap:{}", starmap_id),
        scope: SearchScope::StarmapTitle,
        title: title.to_string(),
        body: title.to_string(),
        target: SearchTarget {
            project_id: project_id.map(|s| s.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: Some(starmap_id.to_string()),
            node_id: None,
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_project_title_entry(project_id: &str, title: &str) -> IndexEntry {
    IndexEntry {
        object_id: format!("project:{}", project_id),
        scope: SearchScope::ProjectTitle,
        title: title.to_string(),
        body: title.to_string(),
        target: SearchTarget {
            project_id: Some(project_id.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: None,
            node_id: None,
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_volume_title_entry(project_id: &str, volume_id: &str, title: &str) -> IndexEntry {
    IndexEntry {
        object_id: format!("volume:{}:{}", project_id, volume_id),
        scope: SearchScope::VolumeTitle,
        title: title.to_string(),
        body: title.to_string(),
        target: SearchTarget {
            project_id: Some(project_id.to_string()),
            volume_id: Some(volume_id.to_string()),
            chapter_id: None,
            starmap_id: None,
            node_id: None,
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_chapter_title_entry(
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    title: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("chapter_title:{}:{}:{}", project_id, volume_id, chapter_id),
        scope: SearchScope::ChapterTitle,
        title: title.to_string(),
        body: title.to_string(),
        target: SearchTarget {
            project_id: Some(project_id.to_string()),
            volume_id: Some(volume_id.to_string()),
            chapter_id: Some(chapter_id.to_string()),
            starmap_id: None,
            node_id: None,
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_chapter_body_entry(
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    title: &str,
    body: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("chapter_body:{}:{}:{}", project_id, volume_id, chapter_id),
        scope: SearchScope::ChapterBody,
        title: title.to_string(),
        body: body.to_string(),
        target: SearchTarget {
            project_id: Some(project_id.to_string()),
            volume_id: Some(volume_id.to_string()),
            chapter_id: Some(chapter_id.to_string()),
            starmap_id: None,
            node_id: None,
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn extract_chapter_note_entry(
    project_id: &str,
    volume_id: &str,
    chapter_id: &str,
    title: &str,
    note: &str,
) -> IndexEntry {
    IndexEntry {
        object_id: format!("chapter_note:{}:{}:{}", project_id, volume_id, chapter_id),
        scope: SearchScope::ChapterNote,
        title: title.to_string(),
        body: note.to_string(),
        target: SearchTarget {
            project_id: Some(project_id.to_string()),
            volume_id: Some(volume_id.to_string()),
            chapter_id: Some(chapter_id.to_string()),
            starmap_id: None,
            node_id: None,
            setting_key: None,
            ..Default::default()
        },
    }
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_dir_entries(dir: &Path) -> Result<Option<Vec<std::fs::DirEntry>>> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.collect::<std::io::Result<Vec<_>>>().map(Some).map_err(
            |error| Error::Other(format!("failed to read directory {}: {error}", dir.display())),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Other(format!(
            "failed to read directory {}: {error}",
            dir.display()
        ))),
    }
}

fn entry_is_dir(entry: &std::fs::DirEntry) -> Result<bool> {
    let path = entry.path();
    let file_type = entry
        .file_type()
        .map_err(|error| Error::Other(format!("failed to inspect {}: {error}", path.display())))?;
    Ok(file_type.is_dir() || (file_type.is_symlink() && path.is_dir()))
}

fn entry_is_file(entry: &std::fs::DirEntry) -> Result<bool> {
    let path = entry.path();
    let file_type = entry
        .file_type()
        .map_err(|error| Error::Other(format!("failed to inspect {}: {error}", path.display())))?;
    Ok(file_type.is_file() || (file_type.is_symlink() && path.is_file()))
}

fn json_file_id(path: &Path) -> Option<(String, PathBuf)> {
    let name = path.file_name().and_then(|name| name.to_str())?;
    let id = name.strip_suffix(".json")?;
    Some((id.to_string(), path.to_path_buf()))
}

fn scan_dirs(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut result = Vec::new();
    let Some(entries) = read_dir_entries(dir)? else {
        return Ok(result);
    };
    for entry in entries {
        if !entry_is_dir(&entry)? {
            continue;
        }
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            result.push((name.to_string(), path));
        }
    }
    Ok(result)
}

fn scan_json_files(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut result = Vec::new();
    let Some(entries) = read_dir_entries(dir)? else {
        return Ok(result);
    };
    for entry in entries {
        let path = entry.path();
        if entry_is_file(&entry)? {
            // 包存储把对象放在一级 bucket 子目录里；这里保持原有的一层扫描。
            if let Some(file) = json_file_id(&path) {
                result.push(file);
            }
        } else if entry_is_dir(&entry)? {
            let Some(sub_entries) = read_dir_entries(&path)? else {
                continue;
            };
            for sub_entry in sub_entries {
                if !entry_is_file(&sub_entry)? {
                    continue;
                }
                if let Some(file) = json_file_id(&sub_entry.path()) {
                    result.push(file);
                }
            }
        }
    }
    Ok(result)
}

/// 读取章节标题。meta 文件真实不存在返回 `Ok(None)`（不是章节目录）；读取/
/// 解析失败或缺少标题字段返回带路径错误，由调用方保留该章节旧索引。
fn load_chapter_title(meta_path: &Path) -> Result<Option<String>> {
    let content = match std::fs::read_to_string(meta_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::Other(format!(
                "failed to read chapter meta {}: {error}",
                meta_path.display()
            )))
        }
    };
    let value: serde_json::Value = serde_json::from_str(&content).map_err(|error| {
        Error::Other(format!(
            "chapter meta {} is corrupt: {error}",
            meta_path.display()
        ))
    })?;
    let title = value
        .get("title")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::Other(format!(
                "chapter meta {} has no title field",
                meta_path.display()
            ))
        })?;
    Ok(Some(title.to_string()))
}

fn extract_node_search_text(content: &crate::starmap::semantic::StarMapNodeContent) -> String {
    content.search_text()
}
