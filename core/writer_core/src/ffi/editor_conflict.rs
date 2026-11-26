//! 编辑会话冲突材料的 JSON/C-ABI 入口。
//!
//! 与正文保存分离：只写 `app-meta/chapter-commits/conflicts/<transaction_id>` 下的
//! base/core/native 三方材料与身份元数据，不发布正文、不做空覆盖检查、不参与版本 CAS。

use std::os::raw::c_char;

use serde_json::Value;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};
use crate::api::EditorConflictMaterialRequestDto;

fn string_field(value: &Value, name: &str) -> Result<Option<String>, String> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{name} must be a string")),
    }
}

fn required_string(value: &Value, name: &str) -> Result<String, String> {
    string_field(value, name)?.ok_or_else(|| format!("missing {name}"))
}

fn optional_u64(value: &Value, name: &str) -> Result<Option<u64>, String> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{name} must be an unsigned integer")),
        Some(_) => Err(format!("{name} must be a number")),
    }
}

fn optional_version(
    value: &Value,
    name: &str,
) -> Result<Option<crate::api::ChapterVersionDto>, String> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(object)) => {
            let object_generation = object
                .get("objectGeneration")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let revision = object
                .get("revision")
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("{name}.revision must be an unsigned integer"))?;
            let content_sha256 = object
                .get("contentSha256")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(Some(crate::api::ChapterVersionDto {
                object_generation,
                revision,
                content_sha256,
            }))
        }
        Some(_) => Err(format!("{name} must be an object")),
    }
}

fn build_request(value: &Value) -> Result<EditorConflictMaterialRequestDto, String> {
    let project_id = required_string(value, "projectId")?;
    let volume_id = required_string(value, "volumeId")?;
    let chapter_id = required_string(value, "chapterId")?;
    if project_id.is_empty() || volume_id.is_empty() || chapter_id.is_empty() {
        return Err("chapter identity must not be empty".to_string());
    }
    Ok(EditorConflictMaterialRequestDto {
        project_id,
        volume_id,
        chapter_id,
        base_content: string_field(value, "baseContent")?,
        core_content: required_string(value, "coreContent")?,
        native_content: required_string(value, "nativeContent")?,
        core_version: optional_version(value, "coreVersion")?,
        local_version: optional_version(value, "localVersion")?,
        core_session_id: optional_u64(value, "coreSessionId")?,
        local_session_id: optional_u64(value, "localSessionId")?,
        core_revision: optional_u64(value, "coreRevision")?,
        base_revision: optional_u64(value, "baseRevision")?,
        change_sequence: optional_u64(value, "changeSequence")?,
        detail: string_field(value, "detail")?,
    })
}

/// JSON 入口：字段 projectId/volumeId/chapterId/baseContent/coreContent/nativeContent/
/// coreVersion/localVersion/coreSessionId/localSessionId/detail。
///
/// # Safety
/// `payload` must be a valid null-terminated UTF-8 JSON string.
#[no_mangle]
pub unsafe extern "C" fn writer_core_write_editor_conflict_material_json(
    payload: *const c_char,
) -> *mut c_char {
    let raw = match c_str_to_rust(payload) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid payload: {error}")),
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => return err_json("INVALID_ARGUMENT", &format!("Invalid JSON: {error}")),
    };
    let request = match build_request(&value) {
        Ok(request) => request,
        Err(error) => return err_json("INVALID_ARGUMENT", &error),
    };
    match with_app_service(move |svc| {
        svc.write_editor_conflict_material(request)
            .map_err(|error| format!("{error}"))
    }) {
        Ok(data) => ok_json(data),
        Err(error) => err_json("IO_WRITE_ERROR", &error),
    }
}
