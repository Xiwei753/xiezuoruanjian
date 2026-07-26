use std::path::Path;

use crate::error::Result;
use super::types::*;

pub fn extract_chapter_entries(workspace: &Path, project_id: Option<&str>) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let projects_dir = workspace.join("projects");

    if !projects_dir.exists() {
        return Ok(entries);
    }

    let projects = scan_dirs(&projects_dir)?;
    for (pid, project_path) in projects {
        if let Some(filter_id) = project_id {
            if pid != filter_id {
                continue;
            }
        }

        let title = load_json_string_field(&project_path.join("project.meta.json"), "title");
        entries.push(IndexEntry {
            object_id: format!("project:{}", pid),
            scope: SearchScope::ProjectTitle,
            title: title.clone(),
            body: title,
            target: SearchTarget {
                project_id: Some(pid.clone()),
                volume_id: None,
                chapter_id: None,
                starmap_id: None,
                node_id: None,
                setting_key: None,
            },
        });

        let volumes_dir = project_path.join("volumes");
        if !volumes_dir.exists() {
            continue;
        }
        let volumes = scan_dirs(&volumes_dir)?;
        for (vid, volume_path) in volumes {
            let vol_title = load_json_string_field(&volume_path.join("volume.meta.json"), "title");
            entries.push(IndexEntry {
                object_id: format!("volume:{}:{}", pid, vid),
                scope: SearchScope::VolumeTitle,
                title: vol_title.clone(),
                body: vol_title,
                target: SearchTarget {
                    project_id: Some(pid.clone()),
                    volume_id: Some(vid.clone()),
                    chapter_id: None,
                    starmap_id: None,
                    node_id: None,
                    setting_key: None,
                },
            });

            let chapters_dir = volume_path.join("chapters");
            if !chapters_dir.exists() {
                continue;
            }
            let chapters = scan_dirs(&chapters_dir)?;
            for (cid, chapter_path) in chapters {
                let ch_title = load_json_string_field(&chapter_path.join("chapter.meta.json"), "title");
                entries.push(IndexEntry {
                    object_id: format!("chapter_title:{}:{}:{}", pid, vid, cid),
                    scope: SearchScope::ChapterTitle,
                    title: ch_title.clone(),
                    body: ch_title.clone(),
                    target: SearchTarget {
                        project_id: Some(pid.clone()),
                        volume_id: Some(vid.clone()),
                        chapter_id: Some(cid.clone()),
                        starmap_id: None,
                        node_id: None,
                        setting_key: None,
                    },
                });

                let md_path = chapter_path.join("chapter.md");
                if md_path.exists() {
                    if let Ok(body) = std::fs::read_to_string(&md_path) {
                        entries.push(IndexEntry {
                            object_id: format!("chapter_body:{}:{}:{}", pid, vid, cid),
                            scope: SearchScope::ChapterBody,
                            title: ch_title.clone(),
                            body,
                            target: SearchTarget {
                                project_id: Some(pid.clone()),
                                volume_id: Some(vid.clone()),
                                chapter_id: Some(cid.clone()),
                                starmap_id: None,
                                node_id: None,
                                setting_key: None,
                            },
                        });
                    }
                }

                let note_path = chapter_path.join("chapter.note.md");
                if note_path.exists() {
                    if let Ok(note) = std::fs::read_to_string(&note_path) {
                        if !note.is_empty() {
                            entries.push(IndexEntry {
                                object_id: format!("chapter_note:{}:{}:{}", pid, vid, cid),
                                scope: SearchScope::ChapterNote,
                                title: ch_title.clone(),
                                body: note,
                                target: SearchTarget {
                                    project_id: Some(pid.clone()),
                                    volume_id: Some(vid.clone()),
                                    chapter_id: Some(cid.clone()),
                                    starmap_id: None,
                                    node_id: None,
                                    setting_key: None,
                                },
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(entries)
}

pub fn extract_starmap_entries(workspace: &Path, project_id: Option<&str>) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let starmaps_dir = workspace.join("app-meta").join("starmaps");

    if !starmaps_dir.exists() {
        return Ok(entries);
    }

    let starmap_dirs = scan_dirs(&starmaps_dir)?;
    for (sid, starmap_path) in starmap_dirs {
        let meta_path = starmap_path.join(format!("{}.meta.json", sid));
        let bound_project = load_json_string_field(&meta_path, "project_id");

        if let Some(filter_id) = project_id {
            if bound_project != filter_id {
                continue;
            }
        }

        let graph_path = starmap_path.join("graph.json");
        let title = load_json_string_field(&graph_path, "title");
        entries.push(IndexEntry {
            object_id: format!("starmap:{}", sid),
            scope: SearchScope::StarmapTitle,
            title: title.clone(),
            body: title,
            target: SearchTarget {
                project_id: if bound_project.is_empty() { None } else { Some(bound_project.clone()) },
                volume_id: None,
                chapter_id: None,
                starmap_id: Some(sid.clone()),
                node_id: None,
                setting_key: None,
            },
        });

        let nodes_dir = starmap_path.join("nodes");
        if nodes_dir.exists() {
            if let Ok(node_files) = scan_json_files(&nodes_dir) {
                for (nid, node_file) in node_files {
                    let node_title = load_json_string_field(&node_file, "title");
                    let node_body = load_node_content_search_text(&node_file);
                    let node_tags = load_json_string_array_field(&node_file, "tags");
                    let mut search_parts = Vec::new();
                    if !node_body.is_empty() { search_parts.push(node_body); }
                    for tag in &node_tags {
                        if !tag.is_empty() { search_parts.push(tag.clone()); }
                    }
                    let search_body = search_parts.join(" ");
                    if !node_title.is_empty() || !search_body.is_empty() {
                        entries.push(IndexEntry {
                            object_id: format!("starmap_node:{}:{}", sid, nid),
                            scope: SearchScope::StarmapNode,
                            title: node_title.clone(),
                            body: if search_body.is_empty() { node_title } else { search_body },
                            target: SearchTarget {
                                project_id: if bound_project.is_empty() { None } else { Some(bound_project.clone()) },
                                volume_id: None,
                                chapter_id: None,
                                starmap_id: Some(sid.clone()),
                                node_id: Some(nid),
                                setting_key: None,
                            },
                        });
                    }
                }
            }
        }

        let edges_dir = starmap_path.join("edges");
        if edges_dir.exists() {
            if let Ok(edge_files) = scan_json_files(&edges_dir) {
                for (eid, edge_file) in edge_files {
                    let label = load_json_string_field(&edge_file, "label");
                    if !label.is_empty() {
                        entries.push(IndexEntry {
                            object_id: format!("starmap_edge:{}:{}", sid, eid),
                            scope: SearchScope::StarmapEdgeLabel,
                            title: label.clone(),
                            body: label,
                            target: SearchTarget {
                                project_id: if bound_project.is_empty() { None } else { Some(bound_project.clone()) },
                                volume_id: None,
                                chapter_id: None,
                                starmap_id: Some(sid.clone()),
                                node_id: None,
                                setting_key: None,
                            },
                        });
                    }
                }
            }
        }

        let hyperlinks_dir = starmap_path.join("hyperlinks");
        if hyperlinks_dir.exists() {
            if let Ok(hl_files) = scan_json_files(&hyperlinks_dir) {
                for (hid, hl_file) in hl_files {
                    let hl_title = load_json_string_field(&hl_file, "title");
                    let hl_url = load_json_string_field(&hl_file, "url");
                    if !hl_title.is_empty() || !hl_url.is_empty() {
                        entries.push(IndexEntry {
                            object_id: format!("starmap_hyperlink:{}:{}", sid, hid),
                            scope: SearchScope::StarmapHyperlink,
                            title: hl_title.clone(),
                            body: if hl_url.is_empty() { hl_title } else { format!("{} {}", hl_title, hl_url) },
                            target: SearchTarget {
                                project_id: if bound_project.is_empty() { None } else { Some(bound_project.clone()) },
                                volume_id: None,
                                chapter_id: None,
                                starmap_id: Some(sid.clone()),
                                node_id: None,
                                setting_key: None,
                            },
                        });
                    }
                }
            }
        }

        let links_dir = starmap_path.join("links");
        if links_dir.exists() {
            if let Ok(link_files) = scan_json_files(&links_dir) {
                for (lid, link_file) in link_files {
                    let link_label = load_json_string_field(&link_file, "label");
                    if !link_label.is_empty() {
                        entries.push(IndexEntry {
                            object_id: format!("starmap_link:{}:{}", sid, lid),
                            scope: SearchScope::StarmapLink,
                            title: link_label.clone(),
                            body: link_label,
                            target: SearchTarget {
                                project_id: if bound_project.is_empty() { None } else { Some(bound_project.clone()) },
                                volume_id: None,
                                chapter_id: None,
                                starmap_id: Some(sid.clone()),
                                node_id: None,
                                setting_key: None,
                            },
                        });
                    }
                }
            }
        }
    }

    Ok(entries)
}

pub fn extract_setting_entries(workspace: &Path) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let settings_dir = workspace.join("app-meta").join("settings");

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
    let body = if content.is_empty() { title.to_string() } else { content.to_string() };
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
    let body = if url.is_empty() { title.to_string() } else { format!("{} {}", title, url) };
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
            }
        }
    }
    Ok(result)
}

fn load_json_string_field(path: &Path, field: &str) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get(field).and_then(|f| f.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

fn load_json_string_array_field(path: &Path, field: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get(field).and_then(|f| f.as_array()).map(|arr| {
            arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
        }))
        .unwrap_or_default()
}

fn load_node_content_search_text(path: &Path) -> String {
    let content_value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("content").cloned());
    match content_value {
        Some(v) => {
            if let Ok(content) = serde_json::from_value::<crate::starmap::semantic::StarMapNodeContent>(v) {
                content.search_text()
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}
