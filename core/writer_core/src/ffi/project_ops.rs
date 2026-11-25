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
    save_chapter_json(project_id, volume_id, chapter_id, content, false)
}

/// Issue #1054：带显式空覆盖意图的章节保存。
///
/// 编辑器只有在“非空 Core 会话被用户的删除事务变为空”并记录精确
/// sessionId/revision 意图后，才会传 `allow_empty_overwrite = true`。
/// 普通 `writer_core_save_chapter` 仍固定传 false，保留 Core 对意外空覆盖的拒绝。
///
/// # Safety
/// `project_id`, `volume_id`, `chapter_id`, and `content` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_chapter_with_options(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
    allow_empty_overwrite: bool,
) -> *mut c_char {
    save_chapter_json(
        project_id,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
    )
}

unsafe fn save_chapter_json(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
    allow_empty_overwrite: bool,
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
            .save_chapter_content_with_options(pid, vid, cid, text, allow_empty_overwrite)
            .map_err(|e| format!("{}", e))?;
        Ok(receipt)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => {
            if e.contains("version conflict") || e.contains("ChapterVersionConflict") {
                err_json("CHAPTER_VERSION_CONFLICT", &e)
            } else if e.contains("empty") || e.contains("Empty") {
                err_json("EMPTY_OVERWRITE_BLOCKED", &e)
            } else {
                err_json("IO_WRITE_ERROR", &e)
            }
        }
    }
}

/// JSON 入口：把多个字符串参数打包成一次异步 NAPI 调用，避免 ArkTS 主线程
/// 直接执行 Rust I/O。字段：projectId/volumeId/chapterId/content/generation/
/// revision/sha256/baseContent/allowEmptyOverwrite/clear。
///
/// # Safety
/// `payload` must be a valid null-terminated UTF-8 JSON string.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_chapter_versioned_json(
    payload: *const c_char,
) -> *mut c_char {
    let raw = match c_str_to_rust(payload) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid payload: {error}")),
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid JSON: {error}")),
    };
    let string_field = |name: &str| -> Option<String> {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let (Some(project_id), Some(volume_id), Some(chapter_id)) = (
        string_field("projectId"),
        string_field("volumeId"),
        string_field("chapterId"),
    ) else {
        return err_json("INVALID_ARGUMENT", "Missing chapter identity");
    };
    let clear = value
        .get("clear")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let content = string_field("content").unwrap_or_default();
    let Some(generation) = string_field("generation") else {
        return err_json("INVALID_ARGUMENT", "Missing generation");
    };
    let Some(sha256) = string_field("sha256") else {
        return err_json("INVALID_ARGUMENT", "Missing sha256");
    };
    let Some(revision) = value.get("revision").and_then(serde_json::Value::as_i64) else {
        return err_json("INVALID_ARGUMENT", "Missing revision");
    };
    let base_content = string_field("baseContent").unwrap_or_default();
    let allow_empty = value
        .get("allowEmptyOverwrite")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let proof_snapshot_revision = value
        .get("proofSnapshotRevision")
        .and_then(serde_json::Value::as_u64);
    let proof_event_ids = value
        .get("proofEventIds")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !clear {
        if let Some(snapshot_revision) = proof_snapshot_revision {
            let expected = crate::api::ChapterVersionDto {
                object_generation: generation.clone(),
                revision: u64::try_from(revision).unwrap_or(0),
                content_sha256: sha256.clone(),
            };
            let base = (!base_content.is_empty()).then_some(base_content.clone());
            let project = project_id.clone();
            let volume = volume_id.clone();
            let chapter = chapter_id.clone();
            let content_value = content.clone();
            let event_ids = proof_event_ids;
            return match with_app_service(move |svc| {
                let receipt = svc
                    .save_chapter_content_versioned_with_batch(
                        project,
                        volume,
                        chapter,
                        content_value,
                        Some(expected),
                        base,
                        snapshot_revision,
                        event_ids,
                        allow_empty,
                    )
                    .map_err(|error| format!("{}", error))?;
                Ok(receipt)
            }) {
                Ok(receipt) => ok_json(&receipt),
                Err(error) => err_json("CHAPTER_SAVE_FAILED", &error),
            };
        }
    }
    let c_project = match std::ffi::CString::new(project_id) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "projectId contains NUL"),
    };
    let c_volume = match std::ffi::CString::new(volume_id) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "volumeId contains NUL"),
    };
    let c_chapter = match std::ffi::CString::new(chapter_id) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "chapterId contains NUL"),
    };
    let c_content = match std::ffi::CString::new(content) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "content contains NUL"),
    };
    let c_generation = match std::ffi::CString::new(generation) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "generation contains NUL"),
    };
    let c_sha = match std::ffi::CString::new(sha256) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "sha256 contains NUL"),
    };
    let c_base = match std::ffi::CString::new(base_content) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "baseContent contains NUL"),
    };
    if clear {
        writer_core_clear_chapter_versioned(
            c_project.as_ptr(),
            c_volume.as_ptr(),
            c_chapter.as_ptr(),
            c_generation.as_ptr(),
            revision,
            c_sha.as_ptr(),
            c_base.as_ptr(),
        )
    } else {
        writer_core_save_chapter_versioned(
            c_project.as_ptr(),
            c_volume.as_ptr(),
            c_chapter.as_ptr(),
            c_content.as_ptr(),
            c_generation.as_ptr(),
            revision,
            c_sha.as_ptr(),
            c_base.as_ptr(),
            allow_empty,
        )
    }
}

/// 完整版本 CAS 的章节保存（Harmony/Windows 共用）。
///
/// `expected_generation`、`expected_revision`、`expected_sha256` 必须来自
/// 最近一次 open/save receipt；任一为空/负数都会被 Core 拒绝。
///
/// # Safety
/// All string arguments must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_chapter_versioned(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
    expected_generation: *const c_char,
    expected_revision: i64,
    expected_sha256: *const c_char,
    base_content: *const c_char,
    allow_empty_overwrite: bool,
) -> *mut c_char {
    save_chapter_versioned_json(
        project_id,
        volume_id,
        chapter_id,
        content,
        expected_generation,
        expected_revision,
        expected_sha256,
        base_content,
        allow_empty_overwrite,
        false,
    )
}

/// 完整版本 CAS 的章节清空。
///
/// # Safety
/// All string arguments must be valid null-terminated UTF-8 C strings.
#[no_mangle]
pub unsafe extern "C" fn writer_core_clear_chapter_versioned(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    expected_generation: *const c_char,
    expected_revision: i64,
    expected_sha256: *const c_char,
    base_content: *const c_char,
) -> *mut c_char {
    save_chapter_versioned_json(
        project_id,
        volume_id,
        chapter_id,
        std::ptr::null(),
        expected_generation,
        expected_revision,
        expected_sha256,
        base_content,
        true,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
unsafe fn save_chapter_versioned_json(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
    expected_generation: *const c_char,
    expected_revision: i64,
    expected_sha256: *const c_char,
    base_content: *const c_char,
    allow_empty_overwrite: bool,
    clear_content: bool,
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
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid volume_id: {e}")),
    };
    let cid = match c_str_to_rust(chapter_id) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter_id: {e}")),
    };
    let text = if clear_content {
        String::new()
    } else {
        match c_str_to_rust(content) {
            Ok(s) => s,
            Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid content: {e}")),
        }
    };
    let generation = match c_str_to_rust(expected_generation) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return err_json("INVALID_ARGUMENT", "expected_generation is empty"),
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid generation: {e}")),
    };
    let sha256 = match c_str_to_rust(expected_sha256) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => return err_json("INVALID_ARGUMENT", "expected_sha256 is empty"),
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid sha256: {e}")),
    };
    let revision = match u64::try_from(expected_revision) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "expected_revision is negative"),
    };
    let base = if base_content.is_null() {
        None
    } else {
        match c_str_to_rust(base_content) {
            Ok(value) => Some(value),
            Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid base_content: {e}")),
        }
    };
    let expected = crate::api::ChapterVersionDto {
        object_generation: generation,
        revision,
        content_sha256: sha256,
    };
    match with_app_service(|svc| {
        let receipt = if clear_content {
            svc.clear_chapter_content_versioned(pid, vid, cid, Some(expected), base)
        } else if allow_empty_overwrite {
            svc.save_chapter_content_versioned_allow_empty(
                pid,
                vid,
                cid,
                text,
                Some(expected),
                base,
            )
        } else {
            svc.save_chapter_content_versioned(pid, vid, cid, text, Some(expected), base)
        }
        .map_err(|e| format!("{}", e))?;
        Ok(receipt)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => {
            if e.contains("version conflict") || e.contains("ChapterVersionConflict") {
                err_json("CHAPTER_VERSION_CONFLICT", &e)
            } else if e.contains("empty") || e.contains("Empty") {
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
