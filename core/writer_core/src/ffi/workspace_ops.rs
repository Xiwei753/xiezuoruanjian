use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core, CORE};

#[no_mangle]
pub unsafe extern "C" fn writer_core_validate_workspace() -> *mut c_char {
    match with_core(|core| {
        core.validate_workspace()
            .map_err(|e| format!("{}", e))
    }) {
        Ok(is_valid) => ok_json(is_valid),
        Err(e) => err_json("WORKSPACE_INVALID", &e),
    }
}

/// List all known workspaces. Currently returns the single active workspace.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_workspaces() -> *mut c_char {
    match with_core(|core| {
        let is_valid = core.validate_workspace().map_err(|e| format!("{}", e))?;
        let projects = core.list_projects().map_err(|e| format!("{}", e))?;
        let recent_edits = core.get_recent_edits().map_err(|e| format!("{}", e))?;
        let project_jsons: Vec<serde_json::Value> = projects.iter().map(|p| {
            let stats = core.get_project_stats(&p.id).ok();
            serde_json::json!({
                "id": p.id,
                "name": p.title,
                "volumeCount": stats.as_ref().map(|s| s.volume_count).unwrap_or(0),
                "chapterCount": stats.as_ref().map(|s| s.chapter_count).unwrap_or(0),
                "totalWordCount": stats.as_ref().map(|s| s.total_word_count).unwrap_or(0),
                "createdAt": p.created_at,
                "updatedAt": p.updated_at
            })
        }).collect();
        let recent_jsons: Vec<serde_json::Value> = recent_edits.iter().map(|e| {
            serde_json::json!({
                "projectId": e.project_id,
                "volumeId": e.volume_id,
                "chapterId": e.chapter_id,
                "editedAt": e.timestamp
            })
        }).collect();
        let summary = serde_json::json!({
            "path": core.workspace_path().to_string_lossy().to_string(),
            "isValid": is_valid,
            "projects": project_jsons,
            "recentEdits": recent_jsons
        });
        Ok(vec![summary])
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("WORKSPACE_ERROR", &e),
    }
}

/// Open (re-initialize) a workspace at the given path.
#[no_mangle]
pub unsafe extern "C" fn writer_core_open_workspace(path: *const c_char) -> *mut c_char {
    let path_str = match c_str_to_rust(path) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_INPUT", &format!("path is null or invalid UTF-8: {}", e)),
    };

    // Re-initialize the core with the new path
    let new_core = crate::facade::WriterCore::new(&path_str);
    if let Err(e) = new_core.create_workspace() {
        return err_json("WORKSPACE_ERROR", &format!("create_workspace failed: {}", e));
    }

    let is_valid = new_core.validate_workspace().unwrap_or(false);
    let projects = new_core.list_projects().unwrap_or_default();
    let recent_edits = new_core.get_recent_edits().unwrap_or_default();

    // Swap the global core
    if let Some(m) = CORE.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = Some(new_core);
        }
    }

    let project_jsons: Vec<serde_json::Value> = projects.iter().map(|p| {
        serde_json::json!({
            "id": p.id,
            "name": p.title,
            "createdAt": p.created_at,
            "updatedAt": p.updated_at
        })
    }).collect();
    let recent_jsons: Vec<serde_json::Value> = recent_edits.iter().map(|e| {
        serde_json::json!({
            "projectId": e.project_id,
            "volumeId": e.volume_id,
            "chapterId": e.chapter_id,
            "editedAt": e.timestamp
        })
    }).collect();

    let summary = serde_json::json!({
        "path": path_str,
        "isValid": is_valid,
        "projects": project_jsons,
        "recentEdits": recent_jsons
    });
    ok_json(summary)
}

/// Get the current workspace state (path, validity, projects, recent edits).
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_workspace_state() -> *mut c_char {
    match with_core(|core| {
        let is_valid = core.validate_workspace().map_err(|e| format!("{}", e))?;
        let projects = core.list_projects().map_err(|e| format!("{}", e))?;
        let recent_edits = core.get_recent_edits().map_err(|e| format!("{}", e))?;
        let project_jsons: Vec<serde_json::Value> = projects.iter().map(|p| {
            let stats = core.get_project_stats(&p.id).ok();
            serde_json::json!({
                "id": p.id,
                "name": p.title,
                "volumeCount": stats.as_ref().map(|s| s.volume_count).unwrap_or(0),
                "chapterCount": stats.as_ref().map(|s| s.chapter_count).unwrap_or(0),
                "totalWordCount": stats.as_ref().map(|s| s.total_word_count).unwrap_or(0),
                "createdAt": p.created_at,
                "updatedAt": p.updated_at
            })
        }).collect();
        let recent_jsons: Vec<serde_json::Value> = recent_edits.iter().map(|e| {
            serde_json::json!({
                "projectId": e.project_id,
                "volumeId": e.volume_id,
                "chapterId": e.chapter_id,
                "editedAt": e.timestamp
            })
        }).collect();
        Ok(serde_json::json!({
            "path": core.workspace_path().to_string_lossy().to_string(),
            "isValid": is_valid,
            "projects": project_jsons,
            "recentEdits": recent_jsons
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("WORKSPACE_ERROR", &e),
    }
}

/// Resolve the project and volume that contain a given chapter.
/// This replaces the ArkTS-side tree traversal in NativeWriterCoreBridge.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_chapter_location(chapter_id: *const c_char) -> *mut c_char {
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_INPUT", &format!("chapter_id is null or invalid UTF-8: {}", e)),
    };
    match with_core(|core| {
        let projects = core.list_projects().map_err(|e| format!("{}", e))?;
        for p in &projects {
            let volumes = core.list_volumes(&p.id).map_err(|e| format!("{}", e))?;
            for v in &volumes {
                let chapters = core.list_chapters(&p.id, &v.id).unwrap_or_default();
                for c in &chapters {
                    if c.id == cid {
                        return Ok(serde_json::json!({
                            "projectId": p.id,
                            "volumeId": v.id,
                            "chapterId": cid
                        }));
                    }
                }
            }
        }
        Err(format!("chapter {} not found in any project/volume", cid))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_recent_edits() -> *mut c_char {
    match with_core(|core| {
        let edits = core.get_recent_edits().map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = edits.iter().map(|e| {
            serde_json::json!({
                "projectId": e.project_id,
                "volumeId": e.volume_id,
                "chapterId": e.chapter_id,
                "editedAt": e.timestamp
            })
        }).collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("IO_READ_ERROR", &e),
    }
}