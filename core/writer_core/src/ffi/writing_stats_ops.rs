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

/// 「今天」的写作统计汇总。
///
/// 「今天是哪一天」由 Core 的本地日历口径决定，平台端不传日期。
///
/// # Safety
/// 无参数。Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_today_writing_stats_summary() -> *mut c_char {
    match with_app_service(|svc| {
        svc.get_today_writing_stats_summary()
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 按调用方给定的日期区间取写作统计汇总。
///
/// # Safety
/// `start_date` / `end_date` 必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_stats_summary(
    start_date: *const c_char,
    end_date: *const c_char,
) -> *mut c_char {
    // SAFETY: 上面的 # Safety 前置条件由 C 调用方保证（合法 NUL 结尾 UTF-8 字符串）。
    let (start, end) = match unsafe { read_date_pair(start_date, end_date) } {
        Some(pair) => pair,
        None => {
            return err_json(
                "INVALID_ARGUMENT",
                "start_date and end_date must be non-empty UTF-8 strings",
            )
        }
    };
    match with_app_service(|svc| {
        svc.get_writing_stats_summary(start, end)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 按调用方给定的日期区间和分桶粒度取写作速度曲线。
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
    // SAFETY: 上面的 # Safety 前置条件由 C 调用方保证（合法 NUL 结尾 UTF-8 字符串）。
    let (start, end) = match unsafe { read_date_pair(start_date, end_date) } {
        Some(pair) => pair,
        None => {
            return err_json(
                "INVALID_ARGUMENT",
                "start_date and end_date must be non-empty UTF-8 strings",
            )
        }
    };
    match with_app_service(|svc| {
        svc.get_writing_speed_curve(start, end, bucket_minutes)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 「当前写作速度」：以调用时刻为终点的实时纯输入速度。
///
/// # Safety
/// `window_seconds` 会被 Core 内部钳到至少 1 秒（0 秒窗口无意义且无法折算速度）。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_current_writing_speed(window_seconds: u32) -> *mut c_char {
    match with_app_service(|svc| {
        svc.get_current_writing_speed(window_seconds)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 按编辑事务上报写作统计。
///
/// 平台端在编辑事务发生时调本函数，把编辑事实（cause + contentDelta）原样送进来，
/// `cause → EventSource` 和各计数字段的映射由 Core 决定，端侧不猜 source。
///
/// # Safety
/// `event_json` 必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串，内容为
/// [`crate::api::EditorChangeStatsInputDto`] 的 JSON。返回 false 表示解析或落盘失败。
#[no_mangle]
pub unsafe extern "C" fn writer_core_record_editor_change_stats(event_json: *const c_char) -> bool {
    let json_str = match c_str_to_rust(event_json) {
        Ok(s) => s,
        Err(_) => return false,
    };
    with_app_service(|svc| {
        let input: crate::api::EditorChangeStatsInputDto =
            serde_json::from_str(&json_str).map_err(|e| format!("invalid stats input: {}", e))?;
        svc.record_editor_change_stats(input)
            .map_err(|e| format!("{}", e))
    })
    .is_ok()
}
