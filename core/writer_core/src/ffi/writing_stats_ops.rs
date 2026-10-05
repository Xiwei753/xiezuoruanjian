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

/// 读取两个 C 字符串日期参数，任一为空/非 UTF-8 就返回 None（由调用方转错误 JSON）。
///
/// # Safety
/// `start_date` / `end_date` 都必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串。
unsafe fn read_date_pair(
    start_date: *const c_char,
    end_date: *const c_char,
) -> Option<(String, String)> {
    let start = c_str_to_rust(start_date).ok()?;
    let end = c_str_to_rust(end_date).ok()?;
    if start.is_empty() || end.is_empty() {
        return None;
    }
    Some((start.to_string(), end.to_string()))
}

/// 按调用方给定的日期区间取写作统计汇总。
///
/// `writer_core_get_writing_stats` 把区间写死成最近 30 天，写作页的「今日进度」拿不到
/// 当日口径；这里让平台端自己决定区间，两端读的是同一份 Core 汇总语义。
///
/// # Safety
/// `start_date` / `end_date` 必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_stats_summary(
    start_date: *const c_char,
    end_date: *const c_char,
) -> *mut c_char {
    let (start, end) = match read_date_pair(start_date, end_date) {
        Some(pair) => pair,
        None => {
            return err_json(
                "INVALID_ARGUMENT",
                "start_date and end_date must be non-empty UTF-8 strings",
            )
        }
    };
    match with_app_service(|svc| svc.get_writing_stats_summary(start, end)) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 按调用方给定的日期区间和分桶粒度取写作速度曲线。
///
/// 写作页状态栏左段的「字/分」就是最后一个桶的 `chars_per_minute`，端侧不自己算速度。
///
/// # Safety
/// `start_date` / `end_date` 必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_speed_curve(
    start_date: *const c_char,
    end_date: *const c_char,
    bucket_minutes: u32,
) -> *mut c_char {
    let (start, end) = match read_date_pair(start_date, end_date) {
        Some(pair) => pair,
        None => {
            return err_json(
                "INVALID_ARGUMENT",
                "start_date and end_date must be non-empty UTF-8 strings",
            )
        }
    };
    match with_app_service(|svc| svc.get_writing_speed_curve(start, end, bucket_minutes)) {
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
