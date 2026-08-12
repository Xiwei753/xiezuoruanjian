use std::path::Path;

use super::types::*;
use crate::error::Result;

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

        let hyperlinks_dir = starmap_path.join("hyperlinks");
        if hyperlinks_dir.exists() {
            if let Ok(hl_files) = scan_json_files(&hyperlinks_dir) {
                for (hid, hl_file) in hl_files {
                    let hl: Option<crate::starmap::types::StarMapHyperlink> =
                        std::fs::read_to_string(&hl_file)
                            .ok()
                            .and_then(|s| serde_json::from_str(&s).ok());
                    if let Some(h) = hl {
                        let hl_title = h.label.as_deref().unwrap_or("");
                        if !hl_title.is_empty() || !h.target_uri.is_empty() {
                            entries.push(extract_starmap_hyperlink_entry(
                                &sid,
                                &hid,
                                bound_project,
                                hl_title,
                                &h.target_uri,
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

        let embeds_dir = starmap_path.join("child_starmaps");
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

pub fn extract_setting_entries(app_data_root: &Path) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let settings_dir = app_data_root.join("settings");

    if !settings_dir.exists() {
        return Ok(entries);
    }

    if let Ok(files) = scan_json_files(&settings_dir) {
        for (key, file_path) in files {
            if let Ok(content) = std::fs::read_to_string(&file_path) {
                entries.push(IndexEntry {
                    object_id: format!("setting:{}", key),
                    scope: SearchScope::Setting,
                    title: key.clone(),
                    body: content,
                    target: SearchTarget {
                        project_id: None,
                        volume_id: None,
                        chapter_id: None,
                        starmap_id: None,
                        node_id: None,
                        setting_key: Some(key),
                    },
                });
            }
        }
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
            setting_key: None,
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
            setting_key: None,
        },
    }
}

pub fn extract_starmap_hyperlink_entry(
    starmap_id: &str,
    hyperlink_id: &str,
    project_id: Option<&str>,
    title: &str,
    url: &str,
) -> IndexEntry {
    let body = if url.is_empty() {
        title.to_string()
    } else {
        format!("{} {}", title, url)
    };
    IndexEntry {
        object_id: format!("starmap_hyperlink:{}:{}", starmap_id, hyperlink_id),
        scope: SearchScope::StarmapHyperlink,
        title: title.to_string(),
        body,
        target: SearchTarget {
            project_id: project_id.map(|s| s.to_string()),
            volume_id: None,
            chapter_id: None,
            starmap_id: Some(starmap_id.to_string()),
            node_id: None,
            setting_key: None,
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
            setting_key: None,
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
        },
    }
}

pub fn extract_setting_entry(key: &str, content: &str) -> IndexEntry {
    IndexEntry {
        object_id: format!("setting:{}", key),
        scope: SearchScope::Setting,
        title: key.to_string(),
        body: content.to_string(),
        target: SearchTarget {
            project_id: None,
            volume_id: None,
            chapter_id: None,
            starmap_id: None,
            node_id: None,
            setting_key: Some(key.to_string()),
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
