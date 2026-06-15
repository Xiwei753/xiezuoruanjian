use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_projects() -> *mut c_char {
    match with_core(|core| {
        let projects = core.list_projects().map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = projects.iter().map(|p| {
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
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_project_tree(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    match with_core(|core| {
        let project = core.list_projects()
            .map_err(|e| format!("{}", e))?
            .into_iter()
            .find(|p| p.id == pid)
            .ok_or_else(|| "project not found".to_string())?;

        let stats = core.get_project_stats(&pid).ok();
        let project_json = serde_json::json!({
            "id": project.id,
            "name": project.title,
            "volumeCount": stats.as_ref().map(|s| s.volume_count).unwrap_or(0),
            "chapterCount": stats.as_ref().map(|s| s.chapter_count).unwrap_or(0),
            "totalWordCount": stats.as_ref().map(|s| s.total_word_count).unwrap_or(0),
            "createdAt": project.created_at,
            "updatedAt": project.updated_at
        });

        let volumes = core.list_volumes(&pid).map_err(|e| format!("{}", e))?;
        let mut volume_trees = Vec::new();
        for vol in volumes {
            let chapters = core.list_chapters(&pid, &vol.id).unwrap_or_default();
            let vol_json = serde_json::json!({
                "id": vol.id,
                "projectId": pid,
                "name": vol.title,
                "order": vol.order,
                "chapterCount": chapters.len(),
                "createdAt": vol.created_at,
                "updatedAt": vol.updated_at
            });
            let chapters_json: Vec<serde_json::Value> = chapters.iter().map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "volumeId": vol.id,
                    "name": c.title,
                    "wordCount": c.word_count,
                    "order": c.order,
                    "updatedAt": c.updated_at,
                    "createdAt": c.created_at
                })
            }).collect();
            volume_trees.push(serde_json::json!({
                "volume": vol_json,
                "chapters": chapters_json
            }));
        }

        Ok(serde_json::json!({
            "project": project_json,
            "volumes": volume_trees
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_create_project(name: *const c_char) -> *mut c_char {
    let title = match c_str_to_rust(name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid name: error {}", e)),
    };
    match with_core(|core| {
        let project = core.create_project(&title).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": project.id,
            "name": project.title,
            "volumeCount": 0,
            "chapterCount": 0,
            "totalWordCount": 0,
            "createdAt": project.created_at,
            "updatedAt": project.updated_at
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_volumes(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    match with_core(|core| {
        let volumes = core.list_volumes(&pid).map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = volumes.iter().map(|v| {
            let chapters = core.list_chapters(&pid, &v.id).unwrap_or_default();
            serde_json::json!({
                "id": v.id,
                "projectId": pid,
                "name": v.title,
                "order": v.order,
                "chapterCount": chapters.len(),
                "createdAt": v.created_at,
                "updatedAt": v.updated_at
            })
        }).collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_create_volume(project_id: *const c_char, name: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let title = match c_str_to_rust(name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid name: error {}", e)),
    };
    match with_core(|core| {
        let vol = core.create_volume(&pid, &title).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": vol.id,
            "projectId": pid,
            "name": vol.title,
            "order": vol.order,
            "chapterCount": 0,
            "createdAt": vol.created_at,
            "updatedAt": vol.updated_at
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_chapters(project_id: *const c_char, volume_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    match with_core(|core| {
        let chapters = core.list_chapters(&pid, &vid).map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = chapters.iter().map(|c| {
            serde_json::json!({
                "id": c.id,
                "volumeId": vid,
                "name": c.title,
                "wordCount": c.word_count,
                "order": c.order,
                "updatedAt": c.updated_at,
                "createdAt": c.created_at
            })
        }).collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_create_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    name: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let title = match c_str_to_rust(name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid name: error {}", e)),
    };
    match with_core(|core| {
        let chapter = core.create_chapter(&pid, &vid, &title).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": chapter.id,
            "volumeId": vid,
            "name": chapter.title,
            "wordCount": chapter.word_count,
            "order": chapter.order,
            "updatedAt": chapter.updated_at,
            "createdAt": chapter.created_at
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_open_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter_id: error {}", e)),
    };
    match with_core(|core| {
        let result = core.open_chapter(&pid, &vid, &cid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": result.meta.id,
            "title": result.meta.title,
            "content": result.content,
            "wordCount": result.meta.word_count,
            "volumeId": vid,
            "projectId": pid,
            "updatedAt": result.meta.updated_at,
            "createdAt": result.meta.created_at
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_save_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter_id: error {}", e)),
    };
    let text = match c_str_to_rust(content) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid content: error {}", e)),
    };
    match with_core(|core| {
        let receipt = core.write_chapter_verified_with_allow_empty_overwrite(
            &pid, &vid, &cid, &text, false,
        ).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "success": true,
            "wordCount": receipt.word_count,
            "savedAt": receipt.updated_at,
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => {
            if e.contains("empty") || e.contains("Empty") {
                err_json("EMPTY_OVERWRITE_BLOCKED", &e)
            } else {
                err_json("IO_WRITE_ERROR", &e)
            }
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_project(project_id: *const c_char, new_name: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let title = match c_str_to_rust(new_name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid new_name: error {}", e)),
    };
    match with_core(|core| {
        core.rename_project(&pid, &title).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_project(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    match with_core(|core| {
        core.delete_project(&pid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_project_stats(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    match with_core(|core| {
        let stats = core.get_project_stats(&pid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "totalWordCount": stats.total_word_count,
            "volumeCount": stats.volume_count,
            "chapterCount": stats.chapter_count
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_volume(project_id: *const c_char, volume_id: *const c_char, new_name: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let title = match c_str_to_rust(new_name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid new_name: error {}", e)),
    };
    match with_core(|core| {
        core.rename_volume(&pid, &vid, &title).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_volume(project_id: *const c_char, volume_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    match with_core(|core| {
        core.delete_volume(&pid, &vid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_reorder_volumes(project_id: *const c_char, ordered_ids_json: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let json_str = match c_str_to_rust(ordered_ids_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid ordered_ids_json: error {}", e)),
    };
    match with_core(|core| {
        let ids: Vec<String> = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        core.reorder_volumes(&pid, &ids).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_chapter(project_id: *const c_char, volume_id: *const c_char, chapter_id: *const c_char, new_name: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter_id: error {}", e)),
    };
    let title = match c_str_to_rust(new_name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid new_name: error {}", e)),
    };
    match with_core(|core| {
        core.rename_chapter(&pid, &vid, &cid, &title).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_chapter(project_id: *const c_char, volume_id: *const c_char, chapter_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter_id: error {}", e)),
    };
    match with_core(|core| {
        core.delete_chapter(&pid, &vid, &cid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_reorder_chapters(project_id: *const c_char, volume_id: *const c_char, ordered_ids_json: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let json_str = match c_str_to_rust(ordered_ids_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid ordered_ids_json: error {}", e)),
    };
    match with_core(|core| {
        let ids: Vec<String> = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        core.reorder_chapters(&pid, &vid, &ids).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_clear_chapter(project_id: *const c_char, volume_id: *const c_char, chapter_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: error {}", e)),
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter_id: error {}", e)),
    };
    match with_core(|core| {
        let receipt = core.clear_chapter_content_verified(&pid, &vid, &cid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "success": true,
            "wordCount": receipt.word_count,
            "savedAt": receipt.updated_at
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}