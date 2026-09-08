use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

fn parse_scope(s: &str) -> crate::search::SearchScope {
    match s {
        "chapterBody" => crate::search::SearchScope::ChapterBody,
        "chapterTitle" => crate::search::SearchScope::ChapterTitle,
        "chapterNote" => crate::search::SearchScope::ChapterNote,
        "projectTitle" => crate::search::SearchScope::ProjectTitle,
        "volumeTitle" => crate::search::SearchScope::VolumeTitle,
        "starmapTitle" => crate::search::SearchScope::StarmapTitle,
        "starmapNode" => crate::search::SearchScope::StarmapNode,
        "starmapEdgeLabel" => crate::search::SearchScope::StarmapEdgeLabel,
        "starmapHyperlink" => crate::search::SearchScope::StarmapHyperlink,
        "starmapLink" => crate::search::SearchScope::StarmapLink,
        "setting" => crate::search::SearchScope::Setting,
        _ => crate::search::SearchScope::All,
    }
}

/// # Safety
/// `query` and `scope` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
// TODO(#597): 既有代码可读性技术债，待后续重构拆分
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless,
    deprecated
)]
pub unsafe extern "C" fn writer_core_global_search(
    query: *const c_char,
    scope: *const c_char,
    limit: u32,
    cursor: *const c_char,
) -> *mut c_char {
    let query_str = match c_str_to_rust(query) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid query: error {}", e)),
    };
    let scope_str = match c_str_to_rust(scope) {
        Ok(s) => s,
        Err(e) => return err_json("INVALID_ARGUMENT", &format!("Invalid scope: error {}", e)),
    };
    let cursor_opt = if cursor.is_null() {
        None
    } else {
        c_str_to_rust(cursor).ok()
    };
    match with_core(|core| {
        let scope = parse_scope(&scope_str);
        let results = core.global_search(&query_str, scope, limit as usize, cursor_opt.as_deref());
        Ok(results)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SEARCH_ERROR", &e),
    }
}

/// # Safety
/// `project_id` may be null (rebuild all) or a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_rebuild_search_index(
    project_id: *const c_char,
) -> *mut c_char {
    let pid = if project_id.is_null() {
        None
    } else {
        match c_str_to_rust(project_id) {
            Ok(s) => Some(s),
            Err(e) => {
                return err_json(
                    "INVALID_ARGUMENT",
                    &format!("Invalid project_id: error {}", e),
                )
            }
        }
    };
    match with_core(|core| {
        let status = core
            .rebuild_search_index(pid.as_deref())
            .map_err(|e| format!("{}", e))?;
        Ok(status)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SEARCH_ERROR", &e),
    }
}

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_search_index_status() -> *mut c_char {
    match with_core(|core| {
        let status = core.get_search_index_status();
        Ok(status)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("SEARCH_ERROR", &e),
    }
}

/// # Safety
/// `action`, `object_id`, `scope`, `title`, `body` must be valid null-terminated UTF-8 C strings.
/// Returns 1 on success, 0 on error.
#[no_mangle]
pub unsafe extern "C" fn writer_core_enqueue_search_update(
    action: *const c_char,
    object_id: *const c_char,
    scope: *const c_char,
    title: *const c_char,
    body: *const c_char,
) -> i32 {
    let action_str = match c_str_to_rust(action) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let oid_str = match c_str_to_rust(object_id) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let scope_str = match c_str_to_rust(scope) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let title_str = match c_str_to_rust(title) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let body_str = match c_str_to_rust(body) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    match with_core(|core| {
        let action = match action_str.as_str() {
            "delete" => crate::search::SearchIndexAction::Delete,
            _ => crate::search::SearchIndexAction::Upsert,
        };
        let scope = parse_scope(&scope_str);
        core.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action,
            object_id: oid_str,
            scope,
            title: title_str,
            body: body_str,
            target: None,
        });
        Ok(true)
    }) {
        Ok(_) => 1,
        Err(_) => 0,
    }
}
