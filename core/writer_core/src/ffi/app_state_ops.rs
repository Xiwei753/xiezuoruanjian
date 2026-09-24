use std::os::raw::c_char;

use serde::Serialize;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};
use crate::api::types::{ProjectSummaryDto, RecentEditDto};

/// 首页/应用态聚合 DTO。
///
/// 只组合已有的 canonical DTO（`ProjectSummaryDto` / `RecentEditDto`），
/// 不在 FFI 手写字段映射。Issue #753：凡是 Core 已有 Serialize DTO 的地方，
/// 直接序列化 DTO，避免手写 `serde_json::json!` 字段漂移。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStateSummaryDto {
    projects: Vec<ProjectSummaryDto>,
    recent_edit: Option<RecentEditDto>,
}

/// List all known projects with stats and recent edits.
///
/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_list_app_summaries() -> *mut c_char {
    match with_app_service(|svc| {
        let projects = svc
            .list_project_summaries()
            .map_err(|e| format!("{}", e))?;
        let recent_edits = svc.get_recent_edits().map_err(|e| format!("{}", e))?;
        // #732 评论第5节：首页契约 singular — 只输出最近一次编辑（nullable）。
        // Core 的 recent_edits.json 仍保留历史/去重能力；get_recent_edits() 通用 API 不变。
        let recent_edit: Option<RecentEditDto> =
            recent_edits.into_iter().next().map(RecentEditDto::from);
        let summary = AppStateSummaryDto {
            projects,
            recent_edit,
        };
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
        "recentEdit": null
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
        let projects = svc
            .list_project_summaries()
            .map_err(|e| format!("{}", e))?;
        let recent_edits = svc.get_recent_edits().map_err(|e| format!("{}", e))?;
        // #732 评论第5节：首页契约 singular — 只输出最近一次编辑（nullable）。
        let recent_edit: Option<RecentEditDto> =
            recent_edits.into_iter().next().map(RecentEditDto::from);
        let summary = AppStateSummaryDto {
            projects,
            recent_edit,
        };
        Ok(summary)
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
        let dtos: Vec<RecentEditDto> = edits.into_iter().map(RecentEditDto::from).collect();
        Ok(dtos)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("IO_READ_ERROR", &e),
    }
}
