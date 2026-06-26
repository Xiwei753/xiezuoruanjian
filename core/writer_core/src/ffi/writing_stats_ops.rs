use std::os::raw::c_char;

use super::{c_str_to_rust, err_json, ok_json, with_core};

#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_stats() -> *mut c_char {
    match with_core(|core| {
        let now = chrono::Utc::now();
        let end = now.format("%Y-%m-%d").to_string();
        let start = (now - chrono::Duration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let summary = core
            .get_writing_stats_summary(&start, &end)
            .map_err(|e| format!("{}", e))?;
        Ok(summary)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

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
    match with_core(|core| {
        let val: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("JSON parse error: {}", e))?;
        let device_id = val
            .get("deviceId")
            .and_then(|v| v.as_str())
            .unwrap_or("harmony");
        let platform = val
            .get("platform")
            .and_then(|v| v.as_str())
            .unwrap_or("harmony");
        let project_id = val.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let volume_id = val.get("volumeId").and_then(|v| v.as_str()).unwrap_or("");
        let chapter_id = val.get("chapterId").and_then(|v| v.as_str()).unwrap_or("");
        let old_text = val.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
        let new_text = val.get("newText").and_then(|v| v.as_str()).unwrap_or("");
        let duration_seconds = val
            .get("durationSeconds")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as u32;
        let session_id = val.get("sessionId").and_then(|v| v.as_str()).unwrap_or("");

        core.process_writing_event(
            device_id,
            platform,
            project_id,
            volume_id,
            chapter_id,
            old_text,
            new_text,
            duration_seconds,
            session_id,
        )
        .map_err(|e| format!("{}", e))?;
        Ok(true)
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}
