use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_app_service};

/// # Safety
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_stats() -> *mut c_char {
    match with_app_service(|svc| {
        let now = chrono::Utc::now();
        let end = now.format("%Y-%m-%d").to_string();
        let start = (now - chrono::Duration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let summary = svc
            .get_writing_stats_summary(start, end)
            .map_err(|e| format!("{}", e))?;
        Ok(summary)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// # Safety
/// `event_json` must be a valid null-terminated UTF-8 C string containing valid JSON.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_process_writing_event(
    event_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(event_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_ARGUMENT",
                &format!("Invalid event_json: error {}", e),
            )
        }
    };
    match with_app_service(|svc| {
        let event: crate::api::WritingEventInputDto =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        svc.process_writing_event(
            event.device_id,
            event.platform,
            event.project_id,
            event.volume_id,
            event.chapter_id,
            event.old_text,
            event.new_text,
            event.duration_seconds,
            event.session_id,
        )
        .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}
