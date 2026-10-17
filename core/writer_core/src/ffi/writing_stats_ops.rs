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

/// 单作品写作档案（#934）：开写日期、累计人工码字、单日最高纪录、写作天数。
///
/// # Safety
/// `project_id` 必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_project_writing_lifetime_stats(
    project_id: *const c_char,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(id) if !id.is_empty() => id,
        _ => {
            return err_json(
                "INVALID_ARGUMENT",
                "project_id must be a non-empty UTF-8 string",
            )
        }
    };
    match with_app_service(|svc| {
        svc.get_project_writing_lifetime_stats(project_id)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 正文保存 + 未确认统计事件一次提交。
///
/// 平台端把「这次保存的正文快照」和「该正文版本之前未确认的统计事件」一起送来。
/// Core 先走既有正文保存事务，成功后按稳定 `event_id` 幂等记录事件：
/// 正文保存失败一条事件都不记；重复提交同一事件只落一次、只计一次字数。
///
/// # Safety
/// 前四个参数必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串；`content` 可为空串。
/// `events_json` 可为 NULL 或空串（没有要附带提交的事件），否则必须是
/// `[EditorChangeStatsInputDto]` 的 JSON 数组。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_save_chapter_with_stats(
    project_id: *const c_char,
    volume_id: *const c_char,
    chapter_id: *const c_char,
    content: *const c_char,
    events_json: *const c_char,
) -> *mut c_char {
    let project_id = match c_str_to_rust(project_id) {
        Ok(id) if !id.is_empty() => id,
        _ => {
            return err_json(
                "INVALID_ARGUMENT",
                "project_id must be a non-empty UTF-8 string",
            )
        }
    };
    let volume_id = match c_str_to_rust(volume_id) {
        Ok(id) if !id.is_empty() => id,
        _ => return err_json("INVALID_ARGUMENT", "volume_id must be a non-empty UTF-8 string"),
    };
    let chapter_id = match c_str_to_rust(chapter_id) {
        Ok(id) if !id.is_empty() => id,
        _ => return err_json("INVALID_ARGUMENT", "chapter_id must be a non-empty UTF-8 string"),
    };
    let content = match c_str_to_rust(content) {
        Ok(text) => text,
        Err(_) => return err_json("INVALID_ARGUMENT", "content must be a UTF-8 string"),
    };
    // NULL 与空串都表示「没有附带事件」，无效 UTF-8 才报错。
    let events_json = if events_json.is_null() {
        String::new()
    } else {
        match c_str_to_rust(events_json) {
            Ok(json) => json,
            Err(_) => {
                return err_json("INVALID_ARGUMENT", "events_json must be a UTF-8 string")
            }
        }
    };
    let events: Vec<crate::api::EditorChangeStatsInputDto> = if events_json.trim().is_empty() {
        Vec::new()
    } else {
        match serde_json::from_str(&events_json) {
            Ok(events) => events,
            Err(e) => return err_json("INVALID_ARGUMENT", &format!("invalid stats events: {}", e)),
        }
    };
    match with_app_service(|svc| {
        svc.save_chapter_with_editor_stats(project_id, volume_id, chapter_id, content, events)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}
