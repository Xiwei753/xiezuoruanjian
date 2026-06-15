//! # C-ABI FFI 层（HarmonyOS / OHOS）
//!
//! 通过 C ABI 暴露 WriterCore 操作，供 NAPI 桥接层调用。
//! 所有复杂数据通过 JSON 字符串传递：Rust 序列化 → C string → NAPI → ArkTS JSON.parse。
//!
//! ## 设计原则
//!
//! - 简单标量直接返回 i32
//! - 复杂数据（struct/vec）返回 JSON C string，调用方须用 `writer_core_free_string` 释放
//! - 错误通过负数返回码或 JSON ResultEnvelope 传递
//! - 所有函数要求先调用 `writer_core_init` 初始化全局单例

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

use once_cell::sync::OnceCell;

use crate::facade::WriterCore;

static CORE: OnceCell<Mutex<Option<WriterCore>>> = OnceCell::new();

fn with_core<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&WriterCore) -> Result<R, String>,
{
    let guard = CORE
        .get()
        .and_then(|m| m.lock().ok())
        .ok_or("core not initialized")?;
    let core = guard.as_ref().ok_or("core not initialized")?;
    f(core)
}

fn ok_json<T: serde::Serialize>(data: T) -> *mut c_char {
    let envelope = serde_json::json!({
        "success": true,
        "data": data
    });
    let s = serde_json::to_string(&envelope).unwrap_or_else(|_| r#"{"success":false,"errorCode":"SERDE_ERROR"}"#.to_string());
    CString::new(s).unwrap_or_default().into_raw()
}

fn err_json(code: &str, msg: &str) -> *mut c_char {
    let envelope = serde_json::json!({
        "success": false,
        "errorCode": code,
        "userMessage": msg
    });
    let s = serde_json::to_string(&envelope).unwrap_or_else(|_| format!(r#"{{"success":false,"errorCode":"{}","userMessage":"{}"}}"#, code, msg));
    CString::new(s).unwrap_or_default().into_raw()
}

fn c_str_to_rust(s: *const c_char) -> Result<String, i32> {
    if s.is_null() {
        return Err(-1);
    }
    match unsafe { CStr::from_ptr(s) }.to_str() {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(-2),
    }
}

/// # Safety
/// `path` must be a valid null-terminated UTF-8 C string.
///
/// Return codes:
///   0  = success
///  -1  = null pointer
///  -2  = invalid UTF-8
///  -3  = mutex poisoned
///  -4  = create_workspace failed
#[no_mangle]
pub unsafe extern "C" fn writer_core_init(path: *const c_char) -> i32 {
    let c_str = match c_str_to_rust(path) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let core = WriterCore::new(&c_str);
    if core.create_workspace().is_err() {
        return -4;
    }
    let m = CORE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = m.lock() {
        *guard = Some(core);
        0
    } else {
        -3
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_load_status() -> *mut c_char {
    let status = match with_core(|_| Ok::<_, String>("native_loaded".to_string())) {
        Ok(s) => s,
        Err(e) => e,
    };
    CString::new(status).unwrap_or_default().into_raw()
}

/// # Safety
/// `text` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn writer_core_calculate_word_count(text: *const c_char) -> i32 {
    let text_str = match c_str_to_rust(text) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match with_core(|core| Ok(core.calculate_word_count(&text_str) as i32)) {
        Ok(count) => count,
        Err(_) => -3,
    }
}

/// # Safety
/// `ptr` must have been returned by a `writer_core_*` function that returns `*mut c_char`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)) };
    }
}

/// Validate workspace. Returns JSON ResultEnvelope<boolean>.
/// # Safety
/// No additional requirements beyond normal C ABI calling convention.
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

/// List projects. Returns JSON ResultEnvelope with project list.
/// # Safety
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

/// Get project tree. Returns JSON ResultEnvelope with ProjectTree.
/// # Safety
/// `project_id` must be a valid null-terminated UTF-8 C string.
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

/// Create project. Returns JSON ResultEnvelope with Project.
/// # Safety
/// `name` must be a valid null-terminated UTF-8 C string.
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

/// List volumes. Returns JSON ResultEnvelope with Volume list.
/// # Safety
/// `project_id` must be a valid null-terminated UTF-8 C string.
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

/// Create volume. Returns JSON ResultEnvelope with Volume.
/// # Safety
/// Both strings must be valid null-terminated UTF-8 C strings.
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

/// List chapters. Returns JSON ResultEnvelope with Chapter list.
/// # Safety
/// Both strings must be valid null-terminated UTF-8 C strings.
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

/// Create chapter. Returns JSON ResultEnvelope with Chapter.
/// # Safety
/// All three strings must be valid null-terminated UTF-8 C strings.
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

/// Open chapter (load metadata + content). Returns JSON ResultEnvelope with ChapterData.
/// # Safety
/// All three strings must be valid null-terminated UTF-8 C strings.
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

/// Save chapter. Returns JSON ResultEnvelope with SaveReceipt.
/// # Safety
/// All four strings must be valid null-terminated UTF-8 C strings.
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

/// Get recent edits. Returns JSON ResultEnvelope with RecentEdit list.
/// # Safety
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

/// Load local settings. Returns JSON ResultEnvelope with LocalSettings.
/// # Safety
#[no_mangle]
pub unsafe extern "C" fn writer_core_load_local_settings() -> *mut c_char {
    match with_core(|core| {
        let settings = core.load_local_settings().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "fontSize": settings.editor_font_size,
            "lineHeight": settings.editor_line_spacing_multiplier,
            "fontFamily": "HarmonyOS Sans",
            "theme": settings.theme_mode.as_deref().unwrap_or("system"),
            "autoSave": settings.auto_save_enabled,
            "autoSaveInterval": settings.auto_save_delay_ms as f64 / 1000.0,
            "autoIndent": settings.auto_indent_enabled,
            "showWordCount": true,
            "showLineNumbers": false,
            "wordWrap": true,
            "spellCheck": false
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

/// Save local settings. Takes JSON string of settings. Returns JSON ResultEnvelope<boolean>.
/// # Safety
/// `settings_json` must be a valid null-terminated UTF-8 C string containing JSON.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_local_settings(settings_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid settings_json: error {}", e)),
    };
    match with_core(|core| {
        let mut settings = core.load_local_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("fontSize").and_then(|v| v.as_f64()) {
            settings.editor_font_size = v as f32;
        }
        if let Some(v) = val.get("lineHeight").and_then(|v| v.as_f64()) {
            settings.editor_line_spacing_multiplier = v as f32;
        }
        if let Some(v) = val.get("autoSave").and_then(|v| v.as_bool()) {
            settings.auto_save_enabled = v;
        }
        if let Some(v) = val.get("autoSaveInterval").and_then(|v| v.as_f64()) {
            settings.auto_save_delay_ms = (v * 1000.0) as u64;
        }
        if let Some(v) = val.get("autoIndent").and_then(|v| v.as_bool()) {
            settings.auto_indent_enabled = v;
        }
        if let Some(v) = val.get("theme").and_then(|v| v.as_str()) {
            settings.theme_mode = Some(v.to_string());
        }
        core.save_local_settings(&settings).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

// ── Project mutations ──

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

// ── Volume mutations ──

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

// ── Chapter mutations ──

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

// ── StarMap ──

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_starmaps() -> *mut c_char {
    match with_core(|core| {
        let starmaps = core.list_starmaps().map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = starmaps.iter().map(|sm| {
            serde_json::json!({
                "id": sm.starmap_id,
                "title": sm.title,
                "description": sm.description,
                "nodeCount": sm.node_count,
                "edgeCount": sm.edge_count,
                "projectId": sm.project_id,
                "createdAt": sm.created_at,
                "updatedAt": sm.updated_at,
                "accentColor": sm.accent_color
            })
        }).collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_list_starmaps_for_project(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid project_id: error {}", e)),
    };
    match with_core(|core| {
        let starmaps = core.list_starmaps_for_project(&pid).map_err(|e| format!("{}", e))?;
        let json_arr: Vec<serde_json::Value> = starmaps.iter().map(|sm| {
            serde_json::json!({
                "id": sm.starmap_id,
                "title": sm.title,
                "description": sm.description,
                "nodeCount": sm.node_count,
                "edgeCount": sm.edge_count,
                "projectId": sm.project_id,
                "createdAt": sm.created_at,
                "updatedAt": sm.updated_at,
                "accentColor": sm.accent_color
            })
        }).collect();
        Ok(json_arr)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_starmap(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
    };
    match with_core(|core| {
        let sm = core.get_starmap(&sid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": sm.starmap_id,
            "title": sm.title,
            "description": sm.description,
            "nodeCount": sm.node_count,
            "edgeCount": sm.edge_count,
            "projectId": sm.project_id,
            "createdAt": sm.created_at,
            "updatedAt": sm.updated_at,
            "accentColor": sm.accent_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_starmap_graph(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
    };
    match with_core(|core| {
        let graph = core.get_starmap_graph(&sid).map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&graph).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_create_starmap(title: *const c_char, description: *const c_char) -> *mut c_char {
    let t = match c_str_to_rust(title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid title: error {}", e)),
    };
    let d = match c_str_to_rust(description) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid description: error {}", e)),
    };
    match with_core(|core| {
        let sm = core.create_starmap(&t, &d, None).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": sm.starmap_id,
            "title": sm.title,
            "description": sm.description,
            "nodeCount": sm.node_count,
            "edgeCount": sm.edge_count,
            "projectId": sm.project_id,
            "createdAt": sm.created_at,
            "updatedAt": sm.updated_at,
            "accentColor": sm.accent_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_ALREADY_EXISTS", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_starmap(starmap_id: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
    };
    match with_core(|core| {
        core.delete_starmap(&sid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_starmap(starmap_id: *const c_char, new_title: *const c_char) -> *mut c_char {
    let sid = match c_str_to_rust(starmap_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid starmap_id: error {}", e)),
    };
    let t = match c_str_to_rust(new_title) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid new_title: error {}", e)),
    };
    match with_core(|core| {
        let sm = core.rename_starmap(&sid, &t).map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "id": sm.starmap_id,
            "title": sm.title,
            "description": sm.description,
            "nodeCount": sm.node_count,
            "edgeCount": sm.edge_count,
            "projectId": sm.project_id,
            "createdAt": sm.created_at,
            "updatedAt": sm.updated_at,
            "accentColor": sm.accent_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("STARMAP_NOT_FOUND", &e),
    }
}

// ── Syncable Settings ──

#[no_mangle]
pub unsafe extern "C" fn writer_core_load_syncable_settings() -> *mut c_char {
    match with_core(|core| {
        let settings = core.load_syncable_settings().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "fontSize": settings.font_size,
            "theme": settings.theme_mode,
            "monetColor": settings.monet_color
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_save_syncable_settings(settings_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(settings_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid settings_json: error {}", e)),
    };
    match with_core(|core| {
        let mut settings = core.load_syncable_settings().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("fontSize").and_then(|v| v.as_f64()) {
            settings.font_size = v;
        }
        if let Some(v) = val.get("theme").and_then(|v| v.as_str()) {
            settings.theme_mode = v.to_string();
        }
        if let Some(v) = val.get("monetColor").and_then(|v| v.as_str()) {
            settings.monet_color = v.to_string();
        }
        core.save_syncable_settings(&settings).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

// ── Sync ──

#[no_mangle]
pub unsafe extern "C" fn writer_core_load_sync_config() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        Ok(serde_json::json!({
            "enabled": config.enabled,
            "provider": format!("{:?}", config.backend_type).to_lowercase(),
            "remoteUrl": config.remote_url,
            "branch": config.branch,
            "autoSync": config.auto_sync,
            "conflictStrategy": "manual"
        }))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_NOT_FOUND", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_save_sync_config(config_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(config_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid config_json: error {}", e)),
    };
    match with_core(|core| {
        let mut config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let val: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        if let Some(v) = val.get("enabled").and_then(|v| v.as_bool()) {
            config.enabled = v;
        }
        if let Some(v) = val.get("remoteUrl").and_then(|v| v.as_str()) {
            config.remote_url = v.to_string();
        }
        if let Some(v) = val.get("branch").and_then(|v| v.as_str()) {
            config.branch = v.to_string();
        }
        if let Some(v) = val.get("autoSync").and_then(|v| v.as_bool()) {
            config.auto_sync = v;
        }
        core.save_sync_config(&config).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SETTINGS_INVALID", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_sync_dry_run() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let plan = core.perform_sync_dry_run(&config).map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&plan).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_sync_diagnostics() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let diag = core.perform_sync_diagnostics(&config).map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&diag).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_perform_sync() -> *mut c_char {
    match with_core(|core| {
        let config = core.load_sync_config().map_err(|e| format!("{}", e))?;
        let result = core.perform_sync(&config).map_err(|e| format!("{}", e))?;
        Ok(serde_json::to_value(&result).unwrap_or_default())
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SYNC_NETWORK_ERROR", &e),
    }
}

// ── Writing Stats ──

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_stats() -> *mut c_char {
    match with_core(|core| {
        let now = chrono::Utc::now();
        let end = now.format("%Y-%m-%d").to_string();
        let start = (now - chrono::Duration::days(30)).format("%Y-%m-%d").to_string();
        let summary = core.get_writing_stats_summary(&start, &end).map_err(|e| format!("{}", e))?;
        Ok(summary)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_process_writing_event(event_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(event_json) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid event_json: error {}", e)),
    };
    match with_core(|core| {
        let val: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("JSON parse error: {}", e))?;
        let device_id = val.get("deviceId").and_then(|v| v.as_str()).unwrap_or("harmony");
        let platform = val.get("platform").and_then(|v| v.as_str()).unwrap_or("harmony");
        let project_id = val.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let volume_id = val.get("volumeId").and_then(|v| v.as_str()).unwrap_or("");
        let chapter_id = val.get("chapterId").and_then(|v| v.as_str()).unwrap_or("");
        let chars_added = val.get("charsAdded").and_then(|v| v.as_i64()).unwrap_or(0) as u32;
        let duration_seconds = val.get("durationSeconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let session_id = val.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");

        let old_text = "";
        let new_text_len = chars_added as usize;
        let new_text = if new_text_len > 0 { "x".repeat(new_text_len) } else { String::new() };

        core.process_writing_event(
            device_id, platform, project_id, volume_id, chapter_id,
            &old_text, &new_text, session_id,
        ).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

#[no_mangle]
pub unsafe extern "C" fn writer_core_is_ai_available() -> i32 {
    match with_core(|core| Ok(core.ai_available() as i32)) {
        Ok(v) => v,
        Err(_) => 0,
    }
}
