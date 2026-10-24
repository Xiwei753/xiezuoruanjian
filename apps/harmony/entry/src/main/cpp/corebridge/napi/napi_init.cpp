#include <cstring>
#include <cstdint>
#include <time.h>
#include <mutex>
#include <algorithm>
#include <deque>
#include <string>
#include <cstdlib>
#include <unistd.h>
#include <dlfcn.h>
#include <napi/native_api.h>
#include <hilog/log.h>
#include "writer_core_bridge.h"

// ── Harmony NAPI 绑定 ──
//
// 架构：NAPI handler → writer_core_bridge.h（C 声明）→ Rust core（实现）
//
// 内存所有权规则：
// - Core 通过 writer_core_* 函数返回的 char* 由 Core 分配，
//   调用方必须通过 writer_core_free_string 释放。
// - ReturnJsonString 在将 char* 复制到 NAPI string 后立即释放，
//   保证无内存泄漏。
// - dup_napi_string 将 NAPI string 复制到调用方提供的缓冲区，
//   缓冲区生命周期由调用方管理。

// ── HiLog 有界记录队列 + Callback ──
//
// 通过 OH_LOG_SetCallback 注册回调，将进程内所有 HiLog 日志按「整条记录」写入有界队列。
// NativeGetHilogSnapshot 返回队列内容，供诊断导出使用。
// 队列按记录字节总量上限 256KB，满时整条淘汰最旧记录（不切半条/半字符），防止内存无限增长。
//
// 每条记录携带采集时刻的墙钟毫秒（captureTimeMs，是回调收到该条日志的时间，
// 不是内核 HiLog 原始时间戳）、pid/tid、level、type、domain、tag 与 message，
// 以及是否被截断的标记。回调内只做内存写入：不做文件 IO、不调用 Rust、
// 不再调用 HiLog（否则会递归回到自身）。

static constexpr size_t HILOG_BUFFER_SIZE = 256 * 1024;
// 单条记录格式化上限：超出时按上限截断并标记 truncated。
static constexpr size_t HILOG_LINE_SIZE = 4096;
// 截断标记（含换行，替换行尾，保证记录仍以换行结束）。
static constexpr const char* HILOG_TRUNCATION_MARKER = " [truncated]\n";

static std::deque<std::string> hilog_records;  // 有界记录队列，按整条记录淘汰
static size_t hilog_retained_bytes = 0;        // 队列中记录的总字节数
static bool hilog_callback_registered = false;
static std::mutex hilog_mutex;

// HiLog 采集统计（全部在 hilog_mutex 保护下读写）。
static uint64_t hilog_captured_records = 0;     // 已追加的记录数
static uint64_t hilog_overwritten_records = 0;  // 因回绕被覆盖的记录数
static uint64_t hilog_truncated_records = 0;    // 被截断的记录数
static uint64_t hilog_capture_start_ms = 0;     // 首条采集记录的墙钟毫秒（0 = 未采集）
static uint64_t hilog_capture_end_ms = 0;       // 末条采集记录的墙钟毫秒（0 = 未采集）

// 当前墙钟毫秒（CLOCK_REALTIME）。回调内不抛异常，失败时返回 0。
static uint64_t HilogNowMs() {
    struct timespec ts;
    if (clock_gettime(CLOCK_REALTIME, &ts) != 0) {
        return 0;
    }
    return static_cast<uint64_t>(ts.tv_sec) * 1000ULL
        + static_cast<uint64_t>(ts.tv_nsec) / 1000000ULL;
}

// 把长度回退到最后一个完整 UTF-8 序列边界，避免截断留下半个字符。
// 只做内存扫描，不分配。
static size_t HilogUtf8SafeLen(const char* s, size_t len) {
    if (len == 0) {
        return 0;
    }
    size_t back = 0;
    while (back < 3 && back < len
           && (static_cast<unsigned char>(s[len - 1 - back]) & 0xC0) == 0x80) {
        ++back;
    }
    if (back >= len) {
        return len;  // 全是续字节，无法定位首字节，保守返回原长度
    }
    unsigned char lead = static_cast<unsigned char>(s[len - 1 - back]);
    size_t seq_len = 0;
    if (lead < 0x80) {
        seq_len = 1;
    } else if ((lead & 0xE0) == 0xC0) {
        seq_len = 2;
    } else if ((lead & 0xF0) == 0xE0) {
        seq_len = 3;
    } else if ((lead & 0xF8) == 0xF0) {
        seq_len = 4;
    }
    if (seq_len == 0) {
        return len;  // 非法首字节，交给上层处理
    }
    size_t seen = back + 1;  // 首字节 + 已观察到的续字节
    if (seen >= seq_len) {
        return len;  // 序列完整
    }
    return len - seen;  // 不完整，回退整段
}

// HiLog callback — 接收进程内所有 HiLog 日志，格式化后写入环形缓冲区。
// LogCallback 签名: void(const LogType, const LogLevel, const unsigned int, const char*, const char*)
static void HilogCallback(const LogType type, const LogLevel level, const unsigned int domain,
                          const char *tag, const char *msg) {
    std::lock_guard<std::mutex> lock(hilog_mutex);

    const uint64_t capture_time_ms = HilogNowMs();

    const char *level_str;
    switch (level) {
        case LOG_DEBUG: level_str = "D"; break;
        case LOG_INFO:  level_str = "I"; break;
        case LOG_WARN:  level_str = "W"; break;
        case LOG_ERROR: level_str = "E"; break;
        case LOG_FATAL: level_str = "F"; break;
        default:        level_str = "U"; break;
    }

    char line[HILOG_LINE_SIZE];
    // snprintf 截断时返回"本该写入的长度"，据此判断是否截断。
    int needed = snprintf(line, sizeof(line),
                          "[%llu %s type=%d 0x%04X %s pid=%d tid=%d] %s\n",
                          static_cast<unsigned long long>(capture_time_ms),
                          level_str,
                          static_cast<int>(type),
                          domain,
                          tag ? tag : "",
                          static_cast<int>(getpid()),
                          static_cast<int>(gettid()),
                          msg ? msg : "");
    if (needed < 0) {
        return;
    }

    const bool truncated = static_cast<size_t>(needed) >= sizeof(line);
    size_t write_len = truncated ? sizeof(line) - 1 : static_cast<size_t>(needed);

    if (truncated) {
        const size_t marker_len = strlen(HILOG_TRUNCATION_MARKER);
        // 为标记预留空间，并把正文回退到 UTF-8 边界
        write_len = std::min(write_len, sizeof(line) - marker_len);
        write_len = HilogUtf8SafeLen(line, write_len);
        memcpy(line + write_len, HILOG_TRUNCATION_MARKER, marker_len);
        write_len += marker_len;
    }
    if (write_len == 0) {
        return;
    }
    std::string record(line, write_len);
    // 缓冲满时按整条记录淘汰最旧记录，保证快照永远从完整日志行开始。
    while (hilog_retained_bytes + record.size() > HILOG_BUFFER_SIZE && !hilog_records.empty()) {
        hilog_retained_bytes -= hilog_records.front().size();
        hilog_records.pop_front();
        hilog_overwritten_records += 1;
    }
    hilog_retained_bytes += record.size();
    hilog_records.push_back(std::move(record));

    // 统计：时间窗以"采集时刻"为准
    if (hilog_capture_start_ms == 0) {
        hilog_capture_start_ms = capture_time_ms;
    }
    hilog_capture_end_ms = capture_time_ms;
    hilog_captured_records += 1;
    if (truncated) {
        hilog_truncated_records += 1;
    }
}

// 注册 HiLog callback — 幂等，多次调用不重复注册。
static void RegisterHilogCallback() {
    if (hilog_callback_registered) {
        return;
    }
    OH_LOG_SetCallback(HilogCallback);
    hilog_callback_registered = true;
}

#undef LOG_DOMAIN
#undef LOG_TAG
#define LOG_DOMAIN 0xFF00
#define LOG_TAG "WriterCoreNapi"

// ── Inline utility helpers (shared across domains) ──

// dup_napi_string: Copy a NAPI string value into a pre-allocated buffer.
//   Caller must ensure buf is large enough (typically 2048 for paths).
//   Returns pointer to buf; lifetime managed by caller.
static char* dup_napi_string(napi_env env, napi_value value, char* buf, size_t buf_size) {
    size_t len = 0;
    napi_get_value_string_utf8(env, value, buf, buf_size, &len);
    return buf;
}

// ReturnJsonString: Bridge helper — wraps a core-allocated JSON char* into a NAPI string.
//   Takes ownership of `json`: calls writer_core_free_string after copying to NAPI value.
//   Returns a minimal error envelope if json is null.
//
//   SAFETY: json 的所有权在调用时转移给本函数。无论 napi_create_string_utf8
//   是否成功，json 都会被 writer_core_free_string 释放。如果 napi 调用失败，
//   result 为未定义值但 json 已释放，不会泄漏。
static napi_value ReturnJsonString(napi_env env, char* json) {
    if (json == nullptr) {
        napi_value empty;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"NULL_RESULT\"}", 47, &empty);
        return empty;
    }
    napi_value result;
    napi_create_string_utf8(env, json, strlen(json), &result);
    writer_core_free_string(json);
    return result;
}

// ── Include domain implementations ──
// Each included file defines static NAPI handler functions and a get*Descriptors() function.

#include "napi/napi_app_state.cpp"
#include "napi/napi_project.cpp"
#include "napi/napi_chapter.cpp"
#include "napi/napi_settings.cpp"
#include "napi/napi_sync.cpp"
#include "napi/napi_stats.cpp"
#include "napi/napi_starmap.cpp"
#include "napi/napi_editor_session.cpp"
#include "napi/napi_correction.cpp"
#include "napi/napi_search.cpp"

// ── Core lifecycle ──

// NativeInit: Initialize core with Harmony-specific PlatformServices injection.
//   Accepts 9 arguments: app_data_root, files_dir, cache_dir, device_id, app_version, locale, timezone, is_connected, is_metered.
//   For backward compatibility, if only 1 argument is provided (app_data_root), the rest are passed as null/0
//   and Rust-side defaults apply.
//   Returns int32 status code:
//     0 = success
//     -1 = app_data_root is null
//     -2 = invalid UTF-8
//     -4 = bootstrap failed
static napi_value NativeInit(napi_env env, napi_callback_info info) {
    size_t argc = 9;
    napi_value args[9];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        OH_LOG_ERROR(LOG_APP, "NativeInit: expected at least 1 argument (app_data_root), got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected at least 1 argument: app_data_root");
        return nullptr;
    }

    char app_data_root[2048] = {0};
    char files_dir[2048] = {0};
    char cache_dir[2048] = {0};
    char device_id[256] = {0};
    char app_version[128] = {0};
    char locale[64] = {0};
    char timezone[64] = {0};
    size_t len = 0;

    napi_get_value_string_utf8(env, args[0], app_data_root, sizeof(app_data_root), &len);

    // Optional arguments: only read if provided, otherwise pass null/0 to Rust
    const char* p_files_dir = nullptr;
    const char* p_cache_dir = nullptr;
    const char* p_device_id = nullptr;
    const char* p_app_version = nullptr;
    const char* p_locale = nullptr;
    const char* p_timezone = nullptr;
    int32_t is_connected = -1;  // 未知（未提供时不得伪造成在线）
    int32_t is_metered = -1;    // 未知

    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], files_dir, sizeof(files_dir), &len);
        p_files_dir = files_dir;
    }
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], cache_dir, sizeof(cache_dir), &len);
        p_cache_dir = cache_dir;
    }
    if (argc >= 4) {
        napi_get_value_string_utf8(env, args[3], device_id, sizeof(device_id), &len);
        p_device_id = device_id;
    }
    if (argc >= 5) {
        napi_get_value_string_utf8(env, args[4], app_version, sizeof(app_version), &len);
        p_app_version = app_version;
    }
    if (argc >= 6) {
        napi_get_value_string_utf8(env, args[5], locale, sizeof(locale), &len);
        p_locale = locale;
    }
    if (argc >= 7) {
        napi_get_value_string_utf8(env, args[6], timezone, sizeof(timezone), &len);
        p_timezone = timezone;
    }
    if (argc >= 8) {
        napi_get_value_int32(env, args[7], &is_connected);
    }
    if (argc >= 9) {
        napi_get_value_int32(env, args[8], &is_metered);
    }

    OH_LOG_INFO(LOG_APP, "NativeInit: calling writer_core_init_harmony with app_data_root='%{public}s' (argc=%{public}zu, is_connected=%{public}d, is_metered=%{public}d)",
                app_data_root, argc, is_connected, is_metered);
    int32_t result = writer_core_init_harmony(app_data_root, p_files_dir, p_cache_dir,
                                               p_device_id, p_app_version, p_locale, p_timezone,
                                               is_connected, is_metered);
    OH_LOG_INFO(LOG_APP, "NativeInit: writer_core_init_harmony returned %{public}d", result);

    if (result != 0) {
        OH_LOG_ERROR(LOG_APP, "NativeInit: FAILED with code %{public}d (app_data_root='%{public}s')", result, app_data_root);
    }

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeUpdateNetworkState: Update the Core's cached network state from the platform's
//   current query result. Three-state ints: 1 = yes, 0 = no, -1 = unknown (clears the
//   cached state, never fabricates online). Returns the Core status code (0 on success).
static napi_value NativeUpdateNetworkState(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    int32_t is_connected = -1;
    int32_t is_metered = -1;
    if (argc >= 1) {
        napi_get_value_int32(env, args[0], &is_connected);
    }
    if (argc >= 2) {
        napi_get_value_int32(env, args[1], &is_metered);
    }

    int32_t result = writer_core_update_network_state(is_connected, is_metered);
    OH_LOG_INFO(LOG_APP,
                "NativeUpdateNetworkState: is_connected=%{public}d is_metered=%{public}d returned %{public}d",
                is_connected, is_metered, result);

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeGetLoadStatus: Returns current core load status as JSON string, or null on failure.
static napi_value NativeGetLoadStatus(napi_env env, napi_callback_info info) {
    char* status = writer_core_get_load_status();
    if (status == nullptr) {
        napi_value null_val;
        napi_get_null(env, &null_val);
        return null_val;
    }

    napi_value result;
    napi_create_string_utf8(env, status, strlen(status), &result);
    writer_core_free_string(status);
    return result;
}

// NativeInitDiagnostics: Initialize diagnostics backend. Returns int32 status code:
//   0 = success
//   -1 = any argument is null
//   -2 = any argument contains invalid UTF-8
//   -3 = local settings could not be read/parsed
// Arguments: logDir, appDataRoot, deviceId, appVersion, buildKey, locale, timezone (all strings).
// session_id is generated inside Rust; enabled/verbose are read from the Core LocalSettings
// under appDataRoot (settings.local.json), not hardcoded.
static napi_value NativeInitDiagnostics(napi_env env, napi_callback_info info) {
    size_t argc = 7;
    napi_value args[7];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 7) {
        OH_LOG_ERROR(LOG_APP, "NativeInitDiagnostics: expected 7 arguments, got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 7 arguments: logDir, appDataRoot, deviceId, appVersion, buildKey, locale, timezone");
        return nullptr;
    }

    char log_dir[2048] = {0};
    char app_data_root[2048] = {0};
    char device_id[256] = {0};
    char app_version[128] = {0};
    char build_key[256] = {0};
    char locale[64] = {0};
    char timezone[64] = {0};
    size_t len = 0;

    // app_data_root 是设置文件所在的数据根路径，绝不能把半截路径传给 Rust 去
    // 读 settings.local.json。Node-API 契约：buf 非空时 result 返回“实际复制
    // 的字节数”（不含终止符），缓冲不足只会截断补 NUL，因此复制长度无法检测
    // 截断。这里先用 buf=nullptr 只查询原字符串字节长度，确认能完整装入缓冲后
    // 再复制，并校验复制字节数与查询长度一致。
    size_t root_len = 0;
    napi_status root_status = napi_get_value_string_utf8(env, args[1], nullptr, 0, &root_len);
    if (root_status != napi_ok || root_len == 0 || root_len >= sizeof(app_data_root)) {
        OH_LOG_ERROR(LOG_APP, "NativeInitDiagnostics: invalid appDataRoot length/status");
        napi_throw_error(env, nullptr, "Invalid appDataRoot");
        return nullptr;
    }
    size_t root_copied = 0;
    root_status = napi_get_value_string_utf8(env, args[1], app_data_root, sizeof(app_data_root), &root_copied);
    if (root_status != napi_ok || root_copied != root_len) {
        OH_LOG_ERROR(LOG_APP, "NativeInitDiagnostics: appDataRoot copy failed");
        napi_throw_error(env, nullptr, "Failed to read appDataRoot");
        return nullptr;
    }

    napi_get_value_string_utf8(env, args[0], log_dir, sizeof(log_dir), &len);
    napi_get_value_string_utf8(env, args[2], device_id, sizeof(device_id), &len);
    napi_get_value_string_utf8(env, args[3], app_version, sizeof(app_version), &len);
    napi_get_value_string_utf8(env, args[4], build_key, sizeof(build_key), &len);
    napi_get_value_string_utf8(env, args[5], locale, sizeof(locale), &len);
    napi_get_value_string_utf8(env, args[6], timezone, sizeof(timezone), &len);

    // Issue #760 评论 5824787641：RegisterHilogCallback() 已移至 NAPI Init() 开头，
    // 在模块加载时（比 EntryAbility.onCreate 更早）完成注册，此处不再重复调用。

    OH_LOG_INFO(LOG_APP, "NativeInitDiagnostics: calling writer_core_init_diagnostics with logDir='%{public}s' appDataRoot='%{public}s'", log_dir, app_data_root);
    int32_t result = writer_core_init_diagnostics(log_dir, app_data_root, device_id, app_version, build_key, locale, timezone);
    OH_LOG_INFO(LOG_APP, "NativeInitDiagnostics: writer_core_init_diagnostics returned %{public}d", result);

    if (result != 0) {
        OH_LOG_ERROR(LOG_APP, "NativeInitDiagnostics: FAILED with code %{public}d", result);
    }

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeSetDiagnosticsConfig: Update diagnostics config at runtime. Returns int32 status code:
//   0 = success
// Arguments: enabled (int32, non-zero = true), verbose (int32, non-zero = true).
static napi_value NativeSetDiagnosticsConfig(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 2) {
        OH_LOG_ERROR(LOG_APP, "NativeSetDiagnosticsConfig: expected 2 arguments, got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 2 arguments: enabled, verbose");
        return nullptr;
    }

    int32_t enabled = 0;
    int32_t verbose = 0;
    napi_get_value_int32(env, args[0], &enabled);
    napi_get_value_int32(env, args[1], &verbose);

    int32_t result = writer_core_set_diagnostics_config(enabled, verbose);

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeFlushDiagnostics: Flush diagnostics logs to disk. Returns int32 status code:
//   0 = success, -1 = flush failed.
static napi_value NativeFlushDiagnostics(napi_env env, napi_callback_info info) {
    int32_t result = writer_core_flush_diagnostics();

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeClearDiagnostics: Clear diagnostics log files. Returns int32 status code:
//   0 = success, -1 = clear failed.
static napi_value NativeClearDiagnostics(napi_env env, napi_callback_info info) {
    int32_t result = writer_core_clear_diagnostics();

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeRecordDiagnosticEvent: Record one structured diagnostic event.
//   Returns int32 status code:
//     0 = success, -1 = null pointer, -2 = invalid UTF-8, -3 = JSON parse failure,
//     -4 = core record failure.
// Arguments: payload_json (string, JSON object
//   { level, origin, event, target, message?, fields:[{key,value}] }).
//
// 注意：这里刻意不调用 OH_LOG_* 记录事件内容，避免诊断事件被 HiLog callback
// 再次捕获形成回环，也避免每条事件都产生一条 HiLog 垃圾。
static napi_value NativeRecordDiagnosticEvent(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        OH_LOG_ERROR(LOG_APP, "NativeRecordDiagnosticEvent: expected 1 argument, got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 1 argument: payload_json");
        return nullptr;
    }

    size_t payload_len = 0;
    napi_get_value_string_utf8(env, args[0], nullptr, 0, &payload_len);
    char* payload = new char[payload_len + 1];
    napi_get_value_string_utf8(env, args[0], payload, payload_len + 1, &payload_len);

    int32_t result = writer_core_record_diagnostic_event_json(payload);
    delete[] payload;

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

// NativeExportDiagnostics: Export diagnostics to a zip file. Returns string:
//   - On success: the zip file path
//   - On failure: a JSON error envelope string
// Arguments: output_dir (string), attachments_json (string, JSON array of
//   { relative_path: string, content: string (base64) }).
static napi_value NativeExportDiagnostics(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 2) {
        OH_LOG_ERROR(LOG_APP, "NativeExportDiagnostics: expected 2 arguments, got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 2 arguments: output_dir, attachments_json");
        return nullptr;
    }

    // Extract output_dir
    size_t output_dir_len = 0;
    napi_get_value_string_utf8(env, args[0], nullptr, 0, &output_dir_len);
    char* output_dir = new char[output_dir_len + 1];
    napi_get_value_string_utf8(env, args[0], output_dir, output_dir_len + 1, &output_dir_len);

    // Extract attachments_json
    size_t attachments_len = 0;
    napi_get_value_string_utf8(env, args[1], nullptr, 0, &attachments_len);
    char* attachments_json = new char[attachments_len + 1];
    napi_get_value_string_utf8(env, args[1], attachments_json, attachments_len + 1, &attachments_len);

    char* zip_path = writer_core_export_diagnostics(output_dir, attachments_json);

    delete[] output_dir;
    delete[] attachments_json;

    if (zip_path == nullptr) {
        // Return error JSON envelope
        napi_value err;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"EXPORT_FAILED\"}", 44, &err);
        return err;
    }

    // Return zip path string, then free the core-allocated char*
    napi_value result;
    napi_create_string_utf8(env, zip_path, strlen(zip_path), &result);
    writer_core_free_string(zip_path);
    return result;
}

// NativeGetLastError: Returns last error message, or empty string if none.
static napi_value NativeGetLastError(napi_env env, napi_callback_info info) {
    char* err = writer_core_get_last_error();
    if (err == nullptr) {
        napi_value empty;
        napi_create_string_utf8(env, "", 0, &empty);
        return empty;
    }

    napi_value result;
    napi_create_string_utf8(env, err, strlen(err), &result);
    writer_core_free_string(err);
    return result;
}

// NativeCalculateWordCount: Returns word count for the given text (int32).
static napi_value NativeCalculateWordCount(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        napi_throw_error(env, nullptr, "Expected 1 argument: text");
        return nullptr;
    }

    size_t text_len = 0;
    napi_get_value_string_utf8(env, args[0], nullptr, 0, &text_len);
    char* text = new char[text_len + 1];
    napi_get_value_string_utf8(env, args[0], text, text_len + 1, &text_len);

    int32_t count = writer_core_calculate_word_count(text);
    delete[] text;

    napi_value result;
    napi_create_int32(env, count, &result);
    return result;
}

// NativeGetHilogSnapshot: 返回当前有界记录队列中的 HiLog 内容字符串。
//   如果 callback 未注册或队列为空，返回空字符串。
static napi_value NativeGetHilogSnapshot(napi_env env, napi_callback_info info) {
    std::lock_guard<std::mutex> lock(hilog_mutex);

    napi_value result;
    if (hilog_records.empty()) {
        napi_create_string_utf8(env, "", 0, &result);
        return result;
    }

    std::string snapshot;
    snapshot.reserve(hilog_retained_bytes);
    for (const std::string& record : hilog_records) {
        snapshot += record;
    }
    napi_create_string_utf8(env, snapshot.c_str(), snapshot.size(), &result);
    return result;
}

// NativeClearHilogSnapshot: 清空 HiLog 有界记录队列。返回 int32 状态码：
//   0 = 成功
// 同时重置采集统计与时间窗（registered 保持不变，代表 callback 注册状态而非内容）。
static napi_value NativeClearHilogSnapshot(napi_env env, napi_callback_info info) {
    std::lock_guard<std::mutex> lock(hilog_mutex);
    hilog_records.clear();
    hilog_retained_bytes = 0;
    hilog_captured_records = 0;
    hilog_overwritten_records = 0;
    hilog_truncated_records = 0;
    hilog_capture_start_ms = 0;
    hilog_capture_end_ms = 0;

    napi_value ret;
    napi_create_int32(env, 0, &ret);
    return ret;
}

// NativeGetHilogCaptureStats: 返回 HiLog 采集统计的 JSON 字符串。
//   {"registered":bool,"capturedRecords":N,"overwrittenRecords":N,
//    "truncatedRecords":N,"capturedStartMs":N,"capturedEndMs":N}
//   用于解释 hilog.txt 为空的原因（callback 未注册 / 确实无日志 / 已被 clear）。
static napi_value NativeGetHilogCaptureStats(napi_env env, napi_callback_info info) {
    std::lock_guard<std::mutex> lock(hilog_mutex);

    char json[256];
    int len = snprintf(json, sizeof(json),
        "{\"registered\":%s,\"capturedRecords\":%llu,\"overwrittenRecords\":%llu,"
        "\"truncatedRecords\":%llu,\"capturedStartMs\":%llu,\"capturedEndMs\":%llu}",
        hilog_callback_registered ? "true" : "false",
        static_cast<unsigned long long>(hilog_captured_records),
        static_cast<unsigned long long>(hilog_overwritten_records),
        static_cast<unsigned long long>(hilog_truncated_records),
        static_cast<unsigned long long>(hilog_capture_start_ms),
        static_cast<unsigned long long>(hilog_capture_end_ms));
    if (len < 0) {
        len = 0;
    }
    size_t json_len = std::min(static_cast<size_t>(len), sizeof(json) - 1);

    napi_value result;
    napi_create_string_utf8(env, json, json_len, &result);
    return result;
}

// ── Misc ──

static napi_value NativeIsAiAvailable(napi_env env, napi_callback_info info) {
    int32_t available = writer_core_is_ai_available();
    napi_value result;
    napi_create_int32(env, available, &result);
    return result;
}

// 将随 HAP 交付的 libonnxruntime.so 绝对路径交给 Rust 的 load-dynamic 初始化。
// libwriter_core.so 与 libonnxruntime.so 位于同一 native 库目录；用 dladdr
// 取得本模块所在目录，避免依赖进程当前目录或设备上的系统安装路径。
static void ConfigureOnnxRuntimePath() {
    static bool configured = false;
    if (configured) {
        return;
    }
    configured = true;
    Dl_info info{};
    if (dladdr(reinterpret_cast<void*>(&NativeIsAiAvailable), &info) == 0 ||
        info.dli_fname == nullptr) {
        OH_LOG_WARN(LOG_APP, "ConfigureOnnxRuntimePath: dladdr failed, cannot locate module path");
        return;
    }
    std::string library(info.dli_fname);
    const size_t slash = library.rfind('/');
    if (slash == std::string::npos) {
        OH_LOG_WARN(LOG_APP, "ConfigureOnnxRuntimePath: no slash in module path '%{public}s'", library.c_str());
        return;
    }
    library.resize(slash + 1);
    library += "libonnxruntime.so";
    if (access(library.c_str(), R_OK) == 0) {
        setenv("SUJIAN_ONNXRUNTIME_LIB", library.c_str(), 0);
        OH_LOG_INFO(LOG_APP, "ConfigureOnnxRuntimePath: set SUJIAN_ONNXRUNTIME_LIB=%{public}s", library.c_str());
    } else {
        OH_LOG_ERROR(LOG_APP, "ConfigureOnnxRuntimePath: libonnxruntime.so not found at %{public}s", library.c_str());
    }
}

// ── Module registration ──
// Init: Merges descriptors from all domains (app_state, project, chapter, settings,
//   sync, stats, starmap) plus core lifecycle into a single NAPI module.
//   Domain descriptor arrays are allocated by each get*Descriptors() function
//   and must remain valid for the lifetime of the module.
static napi_value Init(napi_env env, napi_value exports) {
    OH_LOG_INFO(LOG_APP, "NAPI writer_core Init: starting module registration");
    RegisterHilogCallback();
    ConfigureOnnxRuntimePath();

    // Collect descriptors from all domains
    size_t app_state_count = 0, proj_count = 0, chap_count = 0, set_count = 0;
    size_t sync_count = 0, stats_count = 0, sm_count = 0, editor_count = 0, correction_count = 0;
    size_t search_count = 0;

    napi_property_descriptor* app_state_desc = getAppStateDescriptors(&app_state_count);
    napi_property_descriptor* proj_desc = getProjectDescriptors(&proj_count);
    napi_property_descriptor* chap_desc = getChapterDescriptors(&chap_count);
    napi_property_descriptor* set_desc = getSettingsDescriptors(&set_count);
    napi_property_descriptor* sync_desc = getSyncDescriptors(&sync_count);
    napi_property_descriptor* stats_desc = getStatsDescriptors(&stats_count);
    napi_property_descriptor* sm_desc = getStarMapDescriptors(&sm_count);
    napi_property_descriptor* editor_desc = getEditorSessionDescriptors(&editor_count);
    napi_property_descriptor* correction_desc = getCorrectionDescriptors(&correction_count);
    napi_property_descriptor* search_desc = getSearchDescriptors(&search_count);

    // Core lifecycle + misc descriptors
    napi_property_descriptor core_desc[] = {
        {"nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeUpdateNetworkState", nullptr, NativeUpdateNetworkState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeInitDiagnostics", nullptr, NativeInitDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSetDiagnosticsConfig", nullptr, NativeSetDiagnosticsConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeFlushDiagnostics", nullptr, NativeFlushDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearDiagnostics", nullptr, NativeClearDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeExportDiagnostics", nullptr, NativeExportDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRecordDiagnosticEvent", nullptr, NativeRecordDiagnosticEvent, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLoadStatus", nullptr, NativeGetLoadStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLastError", nullptr, NativeGetLastError, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCalculateWordCount", nullptr, NativeCalculateWordCount, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetHilogSnapshot", nullptr, NativeGetHilogSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearHilogSnapshot", nullptr, NativeClearHilogSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetHilogCaptureStats", nullptr, NativeGetHilogCaptureStats, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeIsAiAvailable", nullptr, NativeIsAiAvailable, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    size_t core_count = sizeof(core_desc) / sizeof(core_desc[0]);

    // Merge all descriptors
    size_t total = core_count + app_state_count + proj_count + chap_count + set_count + sync_count + stats_count + sm_count + editor_count + correction_count + search_count;
    napi_property_descriptor* all_desc = new napi_property_descriptor[total];
    size_t offset = 0;

    memcpy(all_desc + offset, core_desc, core_count * sizeof(napi_property_descriptor)); offset += core_count;
    memcpy(all_desc + offset, app_state_desc, app_state_count * sizeof(napi_property_descriptor)); offset += app_state_count;
    memcpy(all_desc + offset, proj_desc, proj_count * sizeof(napi_property_descriptor)); offset += proj_count;
    memcpy(all_desc + offset, chap_desc, chap_count * sizeof(napi_property_descriptor)); offset += chap_count;
    memcpy(all_desc + offset, set_desc, set_count * sizeof(napi_property_descriptor)); offset += set_count;
    memcpy(all_desc + offset, sync_desc, sync_count * sizeof(napi_property_descriptor)); offset += sync_count;
    memcpy(all_desc + offset, stats_desc, stats_count * sizeof(napi_property_descriptor)); offset += stats_count;
    memcpy(all_desc + offset, sm_desc, sm_count * sizeof(napi_property_descriptor)); offset += sm_count;
    memcpy(all_desc + offset, editor_desc, editor_count * sizeof(napi_property_descriptor)); offset += editor_count;
    memcpy(all_desc + offset, correction_desc, correction_count * sizeof(napi_property_descriptor)); offset += correction_count;
    memcpy(all_desc + offset, search_desc, search_count * sizeof(napi_property_descriptor)); offset += search_count;

    napi_define_properties(env, exports, total, all_desc);
    delete[] all_desc;
    delete[] editor_desc;
    delete[] correction_desc;
    return exports;
}

EXTERN_C_START
static napi_module g_module = {
    .nm_version = 1,
    .nm_flags = 0,
    .nm_filename = nullptr,
    .nm_register_func = Init,
    .nm_modname = "writer_core",
    .nm_priv = nullptr,
    .reserved = {0},
};

__attribute__((constructor)) void RegisterModule(void) {
    napi_module_register(&g_module);
}
EXTERN_C_END
