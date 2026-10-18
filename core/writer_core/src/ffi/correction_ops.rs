//! Proofreading operations for HarmonyOS' asynchronous NAPI bridge.
//!
//! All diagnostics and edit validation are delegated to `WriterAppService`.

use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};
use crate::api::CorrectionIgnoreScopeDto;

fn parse_ignore_scope(scope: &str) -> Result<CorrectionIgnoreScopeDto, String> {
    match scope {
        "once" => Ok(CorrectionIgnoreScopeDto::Once),
        "chapter" => Ok(CorrectionIgnoreScopeDto::Chapter),
        "permanent" => Ok(CorrectionIgnoreScopeDto::Permanent),
        "rule" => Ok(CorrectionIgnoreScopeDto::Rule),
        _ => Err(format!("unknown correction ignore scope: {scope}")),
    }
}

/// Returns the current Core-owned proofreading resource status as JSON.
/// The returned string is caller-owned and must be freed with `writer_core_free_string`.
#[no_mangle]
pub extern "C" fn writer_core_get_correction_resource_status() -> *mut c_char {
    match with_app_service(|service| {
        service
            .get_correction_resource_status()
            .map_err(|error| error.to_string())
    }) {
        Ok(status) => ok_json(status),
        Err(error) => err_json("CORRECTION_RESOURCE_STATUS_ERROR", &error),
    }
}

/// # Safety
/// `path` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned JSON C string; free it with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_correction_model_path_status(
    path: *const c_char,
) -> *mut c_char {
    let path = match c_str_to_rust(path) {
        Ok(path) => path,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid model path: {error}")),
    };
    match with_app_service(|service| {
        service
            .get_correction_model_path_status(path)
            .map_err(|error| error.to_string())
    }) {
        Ok(status) => ok_json(status),
        Err(error) => err_json("CORRECTION_MODEL_STATUS_ERROR", &error),
    }
}

/// # Safety
/// `path` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned JSON C string; free it with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_set_correction_model_path(path: *const c_char) -> *mut c_char {
    let path = match c_str_to_rust(path) {
        Ok(path) => path,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid model path: {error}")),
    };
    match with_app_service(|service| {
        service
            .set_correction_model_path(path)
            .map_err(|error| error.to_string())
    }) {
        Ok(()) => ok_json(true),
        Err(error) => err_json("CORRECTION_MODEL_ERROR", &error),
    }
}

/// # Safety
/// `project_id` and `chapter_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned JSON C string; free it with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_check_correction_segment(
    session_id: u64,
    project_id: *const c_char,
    chapter_id: *const c_char,
    text_revision: u64,
    start_byte: u32,
    end_byte: u32,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid project id: {error}")),
    };
    let chapter_id = match c_str_to_rust(chapter_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter id: {error}")),
    };
    match with_app_service(|service| {
        service
            .check_correction_segment(
                session_id,
                project_id,
                chapter_id,
                text_revision,
                start_byte,
                end_byte,
            )
            .map_err(|error| error.to_string())
    }) {
        Ok(data) => ok_json(data),
        Err(error) => err_json("CORRECTION_ERROR", &error),
    }
}

/// # Safety
/// `project_id` and `chapter_id` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned JSON C string; free it with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_check_correction_chapter(
    session_id: u64,
    project_id: *const c_char,
    chapter_id: *const c_char,
    text_revision: u64,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid project id: {error}")),
    };
    let chapter_id = match c_str_to_rust(chapter_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter id: {error}")),
    };
    match with_app_service(|service| {
        service
            .check_correction_chapter(session_id, project_id, chapter_id, text_revision)
            .map_err(|error| error.to_string())
    }) {
        Ok(data) => ok_json(data),
        Err(error) => err_json("CORRECTION_ERROR", &error),
    }
}

/// # Safety
/// All string arguments must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned JSON C string; free it with `writer_core_free_string`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn writer_core_ignore_correction(
    project_id: *const c_char,
    chapter_id: *const c_char,
    rule_id: *const c_char,
    original: *const c_char,
    scope: *const c_char,
    text_revision: u64,
    start_byte: u32,
    end_byte: u32,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid project id: {error}")),
    };
    let chapter_id = match c_str_to_rust(chapter_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter id: {error}")),
    };
    let rule_id = match c_str_to_rust(rule_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid rule id: {error}")),
    };
    let original = match c_str_to_rust(original) {
        Ok(value) => value,
        Err(error) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid original text: {error}"),
            )
        }
    };
    let scope = match c_str_to_rust(scope) {
        Ok(value) => match parse_ignore_scope(&value) {
            Ok(scope) => scope,
            Err(error) => return err_json("INVALID_ARGUMENT", &error),
        },
        Err(error) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid correction ignore scope: {error}"),
            )
        }
    };
    match with_app_service(|service| {
        service
            .ignore_correction(
                project_id,
                chapter_id,
                rule_id,
                original,
                scope,
                text_revision,
                start_byte,
                end_byte,
            )
            .map_err(|error| error.to_string())
    }) {
        Ok(data) => ok_json(data),
        Err(error) => err_json("CORRECTION_ERROR", &error),
    }
}

/// # Safety
/// `project_id` and `entry` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned JSON C string; free it with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_add_correction_dictionary_entry(
    project_id: *const c_char,
    entry: *const c_char,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid project id: {error}")),
    };
    let entry = match c_str_to_rust(entry) {
        Ok(value) => value,
        Err(error) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid dictionary entry: {error}"),
            )
        }
    };
    match with_app_service(|service| {
        service
            .add_correction_dictionary_entry(project_id, entry)
            .map_err(|error| error.to_string())
    }) {
        Ok(data) => ok_json(data),
        Err(error) => err_json("CORRECTION_ERROR", &error),
    }
}

/// # Safety
/// `project_id`, `chapter_id`, `original`, and `replacement` must be valid
/// null-terminated UTF-8 C strings. Returns a caller-owned JSON C string; free
/// it with `writer_core_free_string`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn writer_core_apply_correction(
    session_id: u64,
    project_id: *const c_char,
    chapter_id: *const c_char,
    text_revision: u64,
    start_byte: u32,
    end_byte: u32,
    original: *const c_char,
    replacement: *const c_char,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid project id: {error}")),
    };
    let chapter_id = match c_str_to_rust(chapter_id) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid chapter id: {error}")),
    };
    let original = match c_str_to_rust(original) {
        Ok(value) => value,
        Err(error) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid original text: {error}"),
            )
        }
    };
    let replacement = match c_str_to_rust(replacement) {
        Ok(value) => value,
        Err(error) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid replacement text: {error}"),
            )
        }
    };
    match with_app_service(|service| {
        Ok(service.apply_correction(
            session_id,
            project_id,
            chapter_id,
            text_revision,
            start_byte,
            end_byte,
            original,
            replacement,
        ))
    }) {
        Ok(data) => ok_json(data),
        Err(error) => err_json("CORRECTION_ERROR", &error),
    }
}
