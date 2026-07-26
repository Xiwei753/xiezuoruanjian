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

    let meta_files = scan_meta_files(&starmaps_dir)?;
    for (sid, meta_path) in meta_files {
        let title = load_json_string_field(&meta_path, "title");
        let bound_project = load_json_string_field(&meta_path, "project_id");

        if let Some(filter_id) = project_id {
            if bound_project != filter_id {
                continue;
            }
        }

        entries.push(IndexEntry {
            object_id: format!("starmap:{}", sid),
            scope: SearchScope::StarmapTitle,
            title: title.clone(),
            body: title,
            target: SearchTarget {
                project_id: if bound_project.is_empty() { None } else { Some(bound_project) },
                volume_id: None,
                chapter_id: None,
                starmap_id: Some(sid.clone()),
                node_id: None,
                setting_key: None,
            },
        });
    }

    Ok(entries)
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

fn scan_meta_files(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".meta.json") {
                    let sid = name.trim_end_matches(".meta.json").to_string();
                    result.push((sid, path));
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
