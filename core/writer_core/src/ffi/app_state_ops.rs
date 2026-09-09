use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};

/// List all known projects with stats and recent edits.
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_app_summaries() -> *mut c_char {
    match with_app_service(|svc| {
        let projects = svc.list_projects().map_err(|e| format!("{}", e))?;
        let recent_edits = svc.get_recent_edits().map_err(|e| format!("{}", e))?;
        let project_jsons: Vec<serde_json::Value> = projects
            .iter()
            .map(|p| {
                let stats = svc.get_project_stats(p.id.clone()).ok();
                serde_json::json!({
                    "id": p.id,
                    "title": p.title,
                    "volumeCount": stats.as_ref().map(|s| s.volume_count).unwrap_or(0),
                    "chapterCount": stats.as_ref().map(|s| s.chapter_count).unwrap_or(0),
                    "totalWordCount": stats.as_ref().map(|s| s.total_word_count).unwrap_or(0),
                    "createdAt": p.created_at,
                    "updatedAt": p.updated_at
                })
            })
            .collect();
        let recent_jsons: Vec<serde_json::Value> = recent_edits
            .iter()
            .map(|e| {
                serde_json::json!({
                    "projectId": e.project_id,
                    "volumeId": e.volume_id,
                    "chapterId": e.chapter_id,
                    "timestamp": e.timestamp
                })
            })
            .collect();
        let summary = serde_json::json!({
            "projects": project_jsons,
            "recentEdits": recent_jsons
        });
        Ok(vec![summary])
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("APP_STATE_ERROR", &e),
    }
}

/// Open (re-initialize) the core at the given path.
///
/// ## 全局状态替换
///
/// TODO: This function previously swapped the global `CORE` singleton directly.
/// With the migration to `APP_SERVICE` (OnceLock-based, init-once), full
/// re-initialization is not yet supported. For now it returns a success
/// response with the path but does not re-bootstrap the app service.
///
/// # Safety
///
/// The caller must ensure `path` points to a valid, null-terminated C string.
/// Passing a null pointer or an invalid pointer is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn writer_core_open_data_root(path: *const c_char) -> *mut c_char {
    let path_str = match c_str_to_rust(path) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("path is null or invalid UTF-8: {}", e),
            )
        }
    };

    // TODO: re-bootstrap APP_SERVICE with the new path once OnceLock supports
    // replacement, or move to a mutable static for the global service handle.
    // For now, acknowledge the path change and return a minimal response.
    let summary = serde_json::json!({
        "path": path_str,
        "projects": [],
        "recentEdits": []
    });
    ok_json(summary)
}

/// Get the current app state (projects, recent edits).
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_app_state() -> *mut c_char {
    match with_app_service(|svc| {
        let projects = svc.list_projects().map_err(|e| format!("{}", e))?;
        let recent_edits = svc.get_recent_edits().map_err(|e| format!("{}", e))?;
        let project_jsons: Vec<serde_json::Value> = projects
            .iter()
            .map(|p| {
                let stats = svc.get_project_stats(p.id.clone()).ok();
                serde_json::json!({
                    "id": p.id,
                    "title": p.title,
                    "volumeCount": stats.as_ref().map(|s| s.volume_count).unwrap_or(0),
                    "chapterCount": stats.as_ref().map(|s| s.chapter_count).unwrap_or(0),
                    "totalWordCount": stats.as_ref().map(|s| s.total_word_count).unwrap_or(0),
                    "createdAt": p.created_at,
                    "updatedAt": p.updated_at
                })
            })
            .collect();
        let recent_jsons: Vec<serde_json::Value> = recent_edits
            .iter()
            .map(|e| {
                serde_json::json!({
                    "projectId": e.project_id,
                    "volumeId": e.volume_id,
                    "chapterId": e.chapter_id,
                    "timestamp": e.timestamp
                })
            })
            .collect();
        Ok(serde_json::json!({
            "projects": project_jsons,
            "recentEdits": recent_jsons
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("APP_STATE_ERROR", &e),
    }
}

/// Resolve the project and volume that contain a given chapter.
/// This replaces the ArkTS-side tree traversal in NativeWriterCoreBridge.
///
/// # Safety
/// `chapter_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub unsafe extern "C" fn writer_core_resolve_chapter_location(
    chapter_id: *const c_char,
) -> *mut c_char {
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("chapter_id is null or invalid UTF-8: {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let projects = svc.list_projects().map_err(|e| format!("{}", e))?;
        for p in &projects {
            let volumes = svc
                .list_volumes(p.id.clone())
                .map_err(|e| format!("{}", e))?;
            for v in &volumes {
                let target_chap_dir = svc
                    .project_root(&p.id)
                    .join("volumes")
                    .join(&v.id)
                    .join("chapters")
                    .join(&cid);
                if target_chap_dir.exists() {
                    return Ok(serde_json::json!({
                        "projectId": p.id,
                        "volumeId": v.id,
                        "chapterId": cid
                    }));
                }
            }
        }
        Err(format!("chapter {} not found in any project/volume", cid))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

/// Resolve the project that contains a given volume.
/// This replaces the ArkTS-side tree traversal for volumeId -> projectId.
///
/// # Safety
/// `volume_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_volume_location(
    volume_id: *const c_char,
) -> *mut c_char {
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("volume_id is null or invalid UTF-8: {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let projects = svc.list_projects().map_err(|e| format!("{}", e))?;
        for p in &projects {
            let target_vol_dir = svc.project_root(&p.id).join("volumes").join(&vid);
            if target_vol_dir.exists() {
                return Ok(serde_json::json!({
                    "projectId": p.id,
                    "volumeId": vid
                }));
            }
        }
        Err(format!("volume {} not found in any project", vid))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_recent_edits() -> *mut c_char {
    match with_app_service(|svc| {
        let edits = svc.get_recent_edits().map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = edits
            .iter()
            .map(|e| {
                serde_json::json!({
                    "projectId": e.project_id,
                    "volumeId": e.volume_id,
                    "chapterId": e.chapter_id,
                    "timestamp": e.timestamp
                })
            })
            .collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("IO_READ_ERROR", &e),
    }
}
