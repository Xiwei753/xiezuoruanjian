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

/// 上报一段平台已测量的前台写作计时切片。
///
/// # Safety
/// `segment_json` 必须指向合法的、以 NUL 结尾的 UTF-8 C 字符串，内容为
/// [`crate::api::WritingActivitySegmentInputDto`] 的 JSON。返回 false 表示解析或落盘失败。
#[no_mangle]
pub unsafe extern "C" fn writer_core_record_writing_activity_segment(
    segment_json: *const c_char,
) -> bool {
    let json_str = match c_str_to_rust(segment_json) {
        Ok(s) => s,
        Err(_) => return false,
    };
    with_app_service(|svc| {
        let input: crate::api::WritingActivitySegmentInputDto = serde_json::from_str(&json_str)
            .map_err(|e| format!("invalid writing activity segment input: {}", e))?;
        svc.record_writing_activity_segment(input)
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
        _ => {
            return err_json(
                "INVALID_ARGUMENT",
                "volume_id must be a non-empty UTF-8 string",
            )
        }
    };
    let chapter_id = match c_str_to_rust(chapter_id) {
        Ok(id) if !id.is_empty() => id,
        _ => {
            return err_json(
                "INVALID_ARGUMENT",
                "chapter_id must be a non-empty UTF-8 string",
            )
        }
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
            Err(_) => return err_json("INVALID_ARGUMENT", "events_json must be a UTF-8 string"),
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

/// 读取可选的 C 字符串参数：NULL 或空串都表示「未提供」。
///
/// 无效 UTF-8 返回 `Err`，由调用方转成 `INVALID_ARGUMENT` 错误信封。
fn optional_c_str(value: *const c_char) -> Result<Option<String>, i32> {
    if value.is_null() {
        return Ok(None);
    }
    let text = c_str_to_rust(value)?;
    Ok(if text.is_empty() { None } else { Some(text) })
}

/// 统计页 Explorer 一次性快照（#944）：生涯摘要 + 四张时段卡 + 选中时段详情。
///
/// `preset` 固定为 `"last7Days"` / `"last30Days"` / `"last6Months"` /
/// `"lifetime"` / `"custom"`；`custom_start` / `custom_end` 是可选的自选闭区间
/// （两端包含），NULL 或空串表示未提供，`custom` 必须提供两端。
/// 参数非法（未知 preset / 只给一端 / 日期格式错 / 起止颠倒）返回
/// `INVALID_ARGUMENT`，其余失败返回 `UNKNOWN_ERROR`。
///
/// # Safety
/// 三个参数都必须是 NULL 或指向合法的、以 NUL 结尾的 UTF-8 C 字符串。
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_get_writing_stats_explorer(
    preset: *const c_char,
    custom_start: *const c_char,
    custom_end: *const c_char,
) -> *mut c_char {
    let preset = match c_str_to_rust(preset) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "preset must be a UTF-8 string"),
    };
    let custom_start = match optional_c_str(custom_start) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "custom_start must be a UTF-8 string"),
    };
    let custom_end = match optional_c_str(custom_end) {
        Ok(value) => value,
        Err(_) => return err_json("INVALID_ARGUMENT", "custom_end must be a UTF-8 string"),
    };
    // 参数校验与 Core 查询共用同一份规则（calendar::parse_explorer_query），
    // 非法输入在进入服务前就返回 INVALID_ARGUMENT。
    if let Err(e) = crate::writing_stats::calendar::parse_explorer_query(
        &preset,
        custom_start.as_deref(),
        custom_end.as_deref(),
    ) {
        return err_json("INVALID_ARGUMENT", &e.to_string());
    }
    match with_app_service(|svc| {
        svc.get_writing_stats_explorer(preset, custom_start, custom_end)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

/// 每日码字日历（#956）：指定公历年的逐日人工输入真值快照。
///
/// `year` 必须在 Core 允许的公历年份范围内（与 `get_heatmap_snapshot`
/// 共用同一份校验规则），越界返回 `INVALID_ARGUMENT`；其余失败返回
/// `UNKNOWN_ERROR`。
///
/// # Safety
/// 该函数没有需要解引用的指针参数；返回的 C 字符串由调用方所有，
/// 用 `writer_core_free_string` 释放。
#[no_mangle]
pub extern "C" fn writer_core_get_writing_stats_heatmap(year: i32) -> *mut c_char {
    if let Err(e) = crate::writing_stats::api::validate_heatmap_year(year) {
        return err_json("INVALID_ARGUMENT", &e.to_string());
    }
    match with_app_service(|svc| {
        svc.get_writing_stats_heatmap(year)
            .map_err(|e| format!("{}", e))
    }) {
        Ok(data) => ok_json(data),
        Err(e) => err_json("UNKNOWN_ERROR", &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::writer_core_init;
    use std::ffi::CStr;
    use std::sync::OnceLock;
    use tempfile::TempDir;

    /// 临时目录要活到进程结束：全局 Core 仍引用它，提前删会让后续 FFI 测试读到空目录。
    static TEST_TEMP_DIR: OnceLock<TempDir> = OnceLock::new();

    /// 与 `ffi::mod` 的并发测试共用同一个全局 Core 单例（只初始化一次）。
    fn ensure_core_init() {
        if crate::ffi::APP_SERVICE.get().is_some() {
            return;
        }
        let dir = tempfile::tempdir().expect("临时目录");
        let path = std::ffi::CString::new(dir.path().to_str().expect("UTF-8 临时路径"))
            .expect("CString 路径");
        // SAFETY: path 是合法的 NUL 结尾 UTF-8 C 字符串。
        let rc = unsafe { writer_core_init(path.as_ptr()) };
        assert_eq!(rc, 0, "writer_core_init 失败");
        let _ = TEST_TEMP_DIR.set(dir);
    }

    /// 调一次 C ABI，解析返回的 JSON 信封并释放 C 字符串。
    fn call_explorer(
        preset: Option<&str>,
        custom_start: Option<&str>,
        custom_end: Option<&str>,
    ) -> serde_json::Value {
        let preset = preset.map(|value| std::ffi::CString::new(value).expect("preset"));
        let custom_start =
            custom_start.map(|value| std::ffi::CString::new(value).expect("custom_start"));
        let custom_end = custom_end.map(|value| std::ffi::CString::new(value).expect("custom_end"));
        let as_ptr = |value: &Option<std::ffi::CString>| {
            value.as_ref().map_or(std::ptr::null(), |v| v.as_ptr())
        };
        // SAFETY: 三个参数都是 NULL 或本测试持有的合法 NUL 结尾 UTF-8 C 字符串。
        let response = unsafe {
            writer_core_get_writing_stats_explorer(
                as_ptr(&preset),
                as_ptr(&custom_start),
                as_ptr(&custom_end),
            )
        };
        // SAFETY: response 由上面的 FFI 返回，指向 NUL 结尾的 UTF-8 JSON 字符串。
        let text = unsafe { CStr::from_ptr(response) }
            .to_str()
            .expect("FFI 应返回 UTF-8 信封")
            .to_string();
        // SAFETY: response 只释放一次，之后不再使用。
        unsafe { crate::ffi::writer_core_free_string(response) };
        serde_json::from_str(&text).expect("FFI 应返回 JSON 信封")
    }

    #[test]
    fn explorer_ffi_maps_invalid_arguments_to_invalid_argument() {
        ensure_core_init();
        for (preset, start, end) in [
            (None, None, None),
            (Some("lastWeek"), None, None),
            (Some("custom"), None, None),
            (Some("custom"), Some("2026-05-01"), None),
            (Some("custom"), None, Some("2026-05-31")),
            (Some("custom"), Some("2026-05-31"), Some("2026-05-01")),
            (Some("custom"), Some("not-a-date"), Some("2026-05-31")),
        ] {
            let envelope = call_explorer(preset, start, end);
            assert_eq!(
                envelope["success"], false,
                "preset={preset:?} start={start:?} end={end:?} 应被拒绝"
            );
            assert_eq!(
                envelope["errorCode"], "INVALID_ARGUMENT",
                "preset={preset:?} start={start:?} end={end:?} 的错误码"
            );
        }
    }

    #[test]
    fn explorer_ffi_returns_snapshot_envelope() {
        ensure_core_init();
        let envelope = call_explorer(Some("last7Days"), Some("2026-05-01"), Some("2026-05-02"));
        assert_eq!(envelope["success"], true, "合法参数应成功：{envelope}");
        assert_eq!(envelope["data"]["historyScope"], "localEvents");
        assert_eq!(envelope["data"]["selected"]["preset"], "last7Days");
        assert_eq!(
            envelope["data"]["periodCards"]
                .as_array()
                .expect("四张时段卡")
                .len(),
            4
        );
    }
}
