//! # 项目/卷/章节 FFI 操作 — 作品目录结构的 C ABI 入口
//!
//! 提供项目列表、项目树、章节 CRUD、卷重命名等操作的 FFI 桥接。
//! 所有函数遵循统一的 JSON-in/JSON-out FFI 契约（见 `settings_ops` 模块文档）。
//!
//! 章节保存使用 `SaveTransaction` 确保正文和元数据的原子性写入。

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_projects() -> *mut c_char {
    match with_app_service(|svc| {
        let projects = svc.list_projects().map_err(|e| format!("{}", e))?;
        Ok(projects)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_project_tree(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let snapshot = svc
            .get_project_workspace_snapshot(pid)
            .map_err(|e| format!("{}", e))?;
        Ok(snapshot)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

/// # Safety
/// `name` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_create_project(name: *const c_char) -> *mut c_char {
    let title = match c_str_to_rust(name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid name: error {}", e)),
    };
    //   FFI 写操作改走 with_app_service，
    // 经 WriterAppService → WriterCoreApi → *_with_changes → record_workspace_change_set → ack，
    // 不再绕过 workspace history 协议。
    match with_app_service(|svc| {
        let project = svc.create_project(title).map_err(|e| format!("{}", e))?;
        Ok(project)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_ALREADY_EXISTS", &e),
    }
}

/// # Safety
/// `project_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_volumes(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let volumes = svc
            .list_volumes(pid.clone())
            .map_err(|e| format!("{}", e))?;
        Ok(volumes)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id` and `name` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_create_volume(
    project_id: *const c_char,
    name: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let title = match c_str_to_rust(name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid name: error {}", e)),
    };
    match with_app_service(|svc| {
        let vol = svc
            .create_volume(pid.clone(), title)
            .map_err(|e| format!("{}", e))?;
        Ok(vol)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_ALREADY_EXISTS", &e),
    }
}

/// # Safety
/// `project_id` and `volume_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_chapters(
    project_id: *const c_char,
    volume_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let chapters = svc
            .list_chapters(pid.clone(), vid.clone())
            .map_err(|e| format!("{}", e))?;
        Ok(chapters)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, and `name` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_create_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    name: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let title = match c_str_to_rust(name) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid name: error {}", e)),
    };
    match with_app_service(|svc| {
        let chapter = svc
            .create_chapter(pid.clone(), vid.clone(), title)
            .map_err(|e| format!("{}", e))?;
        Ok(chapter)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_ALREADY_EXISTS", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, and `chapter_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_open_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid chapter_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let result = svc
            .open_chapter(pid.clone(), vid.clone(), cid)
            .map_err(|e| format!("{}", e))?;
        Ok(result)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, `chapter_id`, and `content` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid chapter_id: error {}", e),
            )
        }
    };
    let text = match c_str_to_rust(content) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid content: error {}", e)),
    };
    match with_app_service(|svc| {
        let receipt = svc
            .save_chapter_content_with_options(pid, vid, cid, text, false)
            .map_err(|e| format!("{}", e))?;
        Ok(receipt)
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

/// # Safety
/// `project_id` and `new_name` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_project(
    project_id: *const c_char,
    new_name: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let title = match c_str_to_rust(new_name) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid new_name: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.rename_project(pid.clone(), title)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_project(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    //   FFI 写操作改走 with_app_service，
    // 经 WriterAppService → WriterCoreApi → delete_project_with_changes →
    // record_workspace_change_set → ack_project_delete_history，
    // 不再绕过 workspace history 协议。
    match with_app_service(|svc| {
        svc.delete_project(pid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_project_stats(project_id: *const c_char) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let stats = svc
            .get_project_stats(pid.clone())
            .map_err(|e| format!("{}", e))?;
        Ok(stats)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("PROJECT_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, and `new_name` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_volume(
    project_id: *const c_char,
    volume_id: *const c_char,
    new_name: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let title = match c_str_to_rust(new_name) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid new_name: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.rename_volume(pid, vid, title)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id` and `volume_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_volume(
    project_id: *const c_char,
    volume_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.delete_volume(pid, vid).map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id` and `ordered_ids_json` must be valid null-terminated UTF-8 C strings.
/// `ordered_ids_json` must contain a valid JSON array of volume ID strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_reorder_volumes(
    project_id: *const c_char,
    ordered_ids_json: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let json_str = match c_str_to_rust(ordered_ids_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid ordered_ids_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let ids: Vec<String> =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        svc.reorder_volumes(pid, ids)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("VOLUME_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, `chapter_id`, and `new_name` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_rename_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    new_name: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid chapter_id: error {}", e),
            )
        }
    };
    let title = match c_str_to_rust(new_name) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid new_name: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.rename_chapter(pid, vid, cid, title)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, and `chapter_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_delete_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid chapter_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        svc.delete_chapter(pid, vid, cid)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, and `ordered_ids_json` must be valid null-terminated UTF-8 C strings.
/// `ordered_ids_json` must contain a valid JSON array of chapter ID strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_reorder_chapters(
    project_id: *const c_char,
    volume_id: *const c_char,
    ordered_ids_json: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let json_str = match c_str_to_rust(ordered_ids_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid ordered_ids_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let ids: Vec<String> =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        svc.reorder_chapters(pid, vid, ids)
            .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}

/// # Safety
/// `project_id`, `volume_id`, and `chapter_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_clear_chapter(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
) -> *mut c_char {
    let pid = match c_str_to_rust(project_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid project_id: error {}", e),
            )
        }
    };
    let vid = match c_str_to_rust(volume_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid volume_id: error {}", e),
            )
        }
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid chapter_id: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let receipt = svc
            .clear_chapter_content(pid, vid, cid)
            .map_err(|e| format!("{}", e))?;
        Ok(receipt)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("CHAPTER_NOT_FOUND", &e),
    }
}
