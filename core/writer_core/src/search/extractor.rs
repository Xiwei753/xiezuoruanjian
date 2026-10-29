use std::path::Path;

use super::types::*;
use crate::annotations::{
    AnnotationStore, LinkTarget, NamedEntry, ObjectLink, ObjectRef, Tag, TagBinding,
};
use crate::error::Result;

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
) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let projects_dir = projects_root;

    if !projects_dir.exists() {
        return Ok(entries);
    }

    let projects = scan_dirs(projects_dir)?;
    for (pid, project_path) in projects {
        if let Some(filter_id) = project_id {
            if pid != filter_id {
                continue;
            }
        }

        let proj: Option<crate::project::Project> =
            std::fs::read_to_string(project_path.join("project.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
        let proj_title = proj.as_ref().map(|p| p.title.as_str()).unwrap_or("");
        entries.push(extract_project_title_entry(&pid, proj_title));
        entries.extend(extract_annotation_entries(&project_path, &pid)?);

        let volumes_dir = project_path.join("volumes");
        if !volumes_dir.exists() {
            continue;
        }
        let volumes = scan_dirs(&volumes_dir)?;
        for (vid, volume_path) in volumes {
            let vol: Option<crate::volume::Volume> =
                std::fs::read_to_string(volume_path.join("volume.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());
            let vol_title = vol.as_ref().map(|v| v.title.as_str()).unwrap_or("");
            entries.push(extract_volume_title_entry(&pid, &vid, vol_title));

            let chapters_dir = volume_path.join("chapters");
            if !chapters_dir.exists() {
                continue;
            }
            let chapters = scan_dirs(&chapters_dir)?;
            for (cid, chapter_path) in chapters {
                let ch_title = load_chapter_title(&chapter_path);
                entries.push(extract_chapter_title_entry(&pid, &vid, &cid, &ch_title));

                let md_path = chapter_path.join("chapter.md");
                if md_path.exists() {
                    if let Ok(body) = std::fs::read_to_string(&md_path) {
                        entries.push(extract_chapter_body_entry(
                            &pid, &vid, &cid, &ch_title, &body,
                        ));
                    }
                }

                let note_path = chapter_path.join("chapter.note.md");
                if note_path.exists() {
                    if let Ok(note) = std::fs::read_to_string(&note_path) {
                        if !note.is_empty() {
                            entries.push(extract_chapter_note_entry(
                                &pid, &vid, &cid, &ch_title, &note,
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(entries)
}

/// Build the searchable projection of one project's active annotation records.
/// Tags, bindings, entries, and links each receive stable IDs derived from their
/// persisted record IDs so incremental updates can replace them independently.
pub fn extract_annotation_entries(
    project_root: &Path,
    project_id: &str,
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
            entries.push(extract_annotation_binding_entry(&store, tag, &binding)?);
        }
    }
    for named_entry in store.list_named_entries()? {
        entries.push(extract_annotation_named_entry(&named_entry));
    }
    for link in store.list_links()? {
        entries.push(extract_annotation_link_entry(&store, &link)?);
    }

    Ok(entries)
}

/// Produce incremental search updates for the active projection and remove
/// tombstoned or no-longer-derived objects from the existing project index.
pub fn annotation_search_updates_for_project(
    project_root: &Path,
    project_id: &str,
    existing_ids: &[String],
) -> Result<Vec<SearchIndexUpdate>> {
    let store = AnnotationStore::new(project_root, project_id);
    let mut updates = Vec::new();
    let mut active_ids = std::collections::HashSet::new();

    for entry in extract_annotation_entries(project_root, project_id)? {
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
                record.source.project_id() == project_id && record.deleted_at_ms.is_some()
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
) -> Result<IndexEntry> {
    let target = search_target_for_object_ref(store, &binding.target)?;
    let body = format!(
        "{} {}",
        object_ref_kind_label(&binding.target),
        binding.target.object_id()
    );
    Ok(IndexEntry {
        object_id: format!(
            "annotation:{}:binding:{}",
            binding.project_id, binding.binding_id
        ),
        scope: SearchScope::AnnotationTag,
        title: tag.name.clone(),
        body,
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

fn extract_annotation_link_entry(store: &AnnotationStore, link: &ObjectLink) -> Result<IndexEntry> {
    let source = object_ref_search_text(store, &link.source)?;
    let LinkTarget::Object(target) = &link.target;
    let target_text = object_ref_search_text(store, target)?;
    let title = if target_text.is_empty() {
        format!("链接 {}", link.link_id)
    } else {
        target_text.clone()
    };
    Ok(IndexEntry {
        object_id: format!(
            "annotation:{}:link:{}",
            link.source.project_id(),
            link.link_id
        ),
        scope: SearchScope::AnnotationLink,
        title,
        body: format!("{} {}", source, target_text).trim().to_string(),
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
    let mut target = annotation_project_target(object_ref.project_id());
    match object_ref {
        ObjectRef::Project { .. } => {}
        ObjectRef::Volume { object_id, .. } => target.volume_id = Some(object_id.clone()),
        ObjectRef::Chapter { object_id, .. } => target.chapter_id = Some(object_id.clone()),
        ObjectRef::TextAnchor { object_id, .. } => {
            if let Some(anchor) = store.get_text_anchor(object_id)? {
                target.chapter_id = Some(anchor.chapter_id);
            }
        }
        ObjectRef::StarMap { object_id, .. } => target.starmap_id = Some(object_id.clone()),
        ObjectRef::StarMapNode { object_id, .. } => target.node_id = Some(object_id.clone()),
        ObjectRef::StarMapEmbed { object_id, .. } => target.embed_id = Some(object_id.clone()),
        ObjectRef::NamedEntry { .. } => {}
    }
    Ok(target)
}

fn object_ref_search_text(store: &AnnotationStore, object_ref: &ObjectRef) -> Result<String> {
    let kind = object_ref_kind_label(object_ref);
    match object_ref {
        ObjectRef::NamedEntry { object_id, .. } => Ok(store
            .get_named_entry(object_id)?
            .filter(|entry| entry.deleted_at_ms.is_none())
            .map(|entry| {
                let mut names = vec![entry.canonical_name];
                names.extend(entry.aliases);
                format!("{} {}", kind, names.join(" "))
            })
            .unwrap_or_else(|| format!("{} {}", kind, object_ref.object_id()))),
        ObjectRef::TextAnchor { object_id, .. } => Ok(store
            .get_text_anchor(object_id)?
            .map(|anchor| {
                format!(
                    "{} {} {}-{}",
                    kind, anchor.chapter_id, anchor.start_byte, anchor.end_byte
                )
            })
            .unwrap_or_else(|| format!("{} {}", kind, object_ref.object_id()))),
        _ => Ok(format!("{} {}", kind, object_ref.object_id())),
    }
}

fn object_ref_kind_label(object_ref: &ObjectRef) -> &'static str {
    match object_ref {
        ObjectRef::Project { .. } => "作品",
        ObjectRef::Volume { .. } => "卷",
        ObjectRef::Chapter { .. } => "章节",
        ObjectRef::TextAnchor { .. } => "正文选区",
        ObjectRef::StarMap { .. } => "星图",
        ObjectRef::StarMapNode { .. } => "星图节点",
        ObjectRef::StarMapEmbed { .. } => "星图嵌入",
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
) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let starmaps_dir = app_data_root.join("starmaps");

    if !starmaps_dir.exists() {
        return Ok(entries);
    }

    let starmap_dirs = scan_dirs(&starmaps_dir)?;
    for (sid, starmap_path) in starmap_dirs {
        let meta_path = starmaps_dir.join(format!("{}.meta.json", sid));
        let meta: Option<crate::starmap::StarMapMeta> = std::fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok());

        let bound_project: Option<&str> = meta.as_ref().and_then(|m| m.project_id.as_deref());

        if let Some(filter_id) = project_id {
            if bound_project != Some(filter_id) {
                continue;
            }
        }

        let title = meta.as_ref().map(|m| m.title.as_str()).unwrap_or("");
        entries.push(extract_starmap_title_entry(&sid, bound_project, title));

        let nodes_dir = starmap_path.join("nodes");
        if nodes_dir.exists() {
            if let Ok(node_files) = scan_json_files(&nodes_dir) {
                for (nid, node_file) in node_files {
                    let node: Option<crate::starmap::types::StarMapNode> =
                        std::fs::read_to_string(&node_file)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                    if let Some(n) = node {
                        let content = extract_node_search_text(&n.content, &n.tags);
                        if !n.title.is_empty() || !content.is_empty() {
                            entries.push(extract_starmap_node_entry(
                                &sid,
                                &nid,
                                bound_project,
                                &n.title,
                                &content,
                            ));
                        }
                    }
                }
            }
        }

        let edges_dir = starmap_path.join("edges");
        if edges_dir.exists() {
            if let Ok(edge_files) = scan_json_files(&edges_dir) {
                for (eid, edge_file) in edge_files {
                    let edge: Option<crate::starmap::types::StarMapEdge> =
                        std::fs::read_to_string(&edge_file)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                    if let Some(e) = edge {
                        let label = e.label.as_deref().unwrap_or("");
                        if !label.is_empty() {
                            entries.push(extract_starmap_edge_entry(
                                &sid,
                                &eid,
                                bound_project,
                                label,
                            ));
                        }
                    }
                }
            }
        }

        let links_dir = starmap_path.join("links");
        if links_dir.exists() {
            if let Ok(link_files) = scan_json_files(&links_dir) {
                for (lid, link_file) in link_files {
                    let link: Option<crate::starmap::types::StarMapLink> =
                        std::fs::read_to_string(&link_file)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                    if let Some(l) = link {
                        let label = l.label.as_deref().unwrap_or("");
                        if !label.is_empty() {
                            entries.push(extract_starmap_link_entry(
                                &sid,
                                &lid,
                                bound_project,
                                label,
                            ));
                        }
                    }
                }
            }
        }

        let embeds_dir = starmap_path.join("embeds");
        if embeds_dir.exists() {
            if let Ok(embed_files) = scan_json_files(&embeds_dir) {
                for (eid, embed_file) in embed_files {
                    let embed: Option<crate::starmap::types::StarMapEmbed> =
                        std::fs::read_to_string(&embed_file)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                    if let Some(e) = embed {
                        let label = e.label.as_deref().unwrap_or("");
                        entries.push(extract_starmap_embed_entry(
                            &sid,
                            &eid,
                            bound_project,
                            label,
                        ));
                    }
                }
            }
        }
    }

    Ok(entries)
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

/// 把一份设置 JSON 展开成逐项可搜索的索引条目。
pub fn extract_settings_from_json(group: &str, content: &str) -> Vec<IndexEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
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

pub fn extract_setting_entries(app_data_root: &Path) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();

    let local_content = std::fs::read_to_string(app_data_root.join("settings.local.json"))
        .ok()
        .or_else(|| serde_json::to_string(&crate::settings::LocalSettings::default()).ok());
    if let Some(content) = local_content {
        entries.extend(extract_settings_from_json("local", &content));
    }

    let sync_content = std::fs::read_to_string(app_data_root.join("settings.sync.json"))
        .ok()
        .or_else(|| serde_json::to_string(&crate::settings::SyncableSettings::default()).ok());
    if let Some(content) = sync_content {
        entries.extend(extract_settings_from_json("sync", &content));
    }

    Ok(entries)
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

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn scan_dirs(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    result.push((name.to_string(), path));
                }
            }
        }
    }
    Ok(result)
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn scan_json_files(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".json") {
                        let id = name.trim_end_matches(".json").to_string();
                        result.push((id, path));
                    }
                }
            } else if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if sub_path.is_file() {
                            if let Some(name) = sub_path.file_name().and_then(|n| n.to_str()) {
                                if name.ends_with(".json") {
                                    let id = name.trim_end_matches(".json").to_string();
                                    result.push((id, sub_path));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

fn load_chapter_title(chapter_path: &Path) -> String {
    std::fs::read_to_string(chapter_path.join("chapter.meta.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v.get("title")
                .and_then(|f| f.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default()
}

fn extract_node_search_text(
    content: &crate::starmap::semantic::StarMapNodeContent,
    tags: &[String],
) -> String {
    let mut parts = Vec::new();
    let text = content.search_text();
    if !text.is_empty() {
        parts.push(text);
    }
    for tag in tags {
        if !tag.is_empty() {
            parts.push(tag.clone());
        }
    }
    parts.join(" ")
}
