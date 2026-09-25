#include <cstring>
#include <mutex>
#include <algorithm>
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

// ── HiLog 环形缓冲 + Callback ──
//
// 通过 OH_LOG_SetCallback 注册回调，将进程内所有 HiLog 日志写入环形缓冲区。
// NativeGetHilogSnapshot 返回缓冲区内容，供诊断导出使用。
// 环形缓冲上限 256KB，防止内存无限增长。

static constexpr size_t HILOG_BUFFER_SIZE = 256 * 1024;
static char hilog_buffer[HILOG_BUFFER_SIZE];
static size_t hilog_buffer_pos = 0;
static size_t hilog_buffer_used = 0;  // 已写入的总字节数（用于区分是否发生过回绕）
static bool hilog_callback_registered = false;
static std::mutex hilog_mutex;

// HiLog callback — 接收进程内所有 HiLog 日志，格式化后写入环形缓冲区。
// LogCallback 签名: void(const LogType, const LogLevel, const unsigned int, const char*, const char*)
static void HilogCallback(const LogType type, const LogLevel level, const unsigned int domain,
                          const char *tag, const char *msg) {
    std::lock_guard<std::mutex> lock(hilog_mutex);
    // 格式化日志行：[level/domain/tag] message
    char line[1024];
    const char *level_str;
    switch (level) {
        case LOG_DEBUG: level_str = "D"; break;
        case LOG_INFO:  level_str = "I"; break;
        case LOG_WARN:  level_str = "W"; break;
        case LOG_ERROR: level_str = "E"; break;
        case LOG_FATAL: level_str = "F"; break;
        default:        level_str = "U"; break;
    }
    int len = snprintf(line, sizeof(line), "[%s/0x%04X/%s] %s\n",
                       level_str, domain, tag ? tag : "", msg ? msg : "");
    if (len <= 0) {
        return;
    }
    size_t write_len = std::min(static_cast<size_t>(len), sizeof(line) - 1);
    if (write_len > HILOG_BUFFER_SIZE) {
        write_len = HILOG_BUFFER_SIZE;  // 单行超长截断
    }
    // 环形写入：可能需要两段拷贝
    size_t first_copy = std::min(write_len, HILOG_BUFFER_SIZE - hilog_buffer_pos);
    memcpy(hilog_buffer + hilog_buffer_pos, line, first_copy);
    hilog_buffer_pos += first_copy;
    if (hilog_buffer_pos >= HILOG_BUFFER_SIZE) {
        hilog_buffer_pos = 0;  // 环形回绕
    }
    size_t remaining = write_len - first_copy;
    if (remaining > 0) {
        memcpy(hilog_buffer, line + first_copy, remaining);
        hilog_buffer_pos = remaining;
    }
    hilog_buffer_used += write_len;
    if (hilog_buffer_used > HILOG_BUFFER_SIZE) {
        hilog_buffer_used = HILOG_BUFFER_SIZE;  // 缓冲已满，后续覆盖旧数据
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

// ── Core lifecycle ──

// NativeInit: Initialize core with app data root path. Returns int32 status code:
//   0 = success
//   -1 = null/empty path
//   -2 = directory creation failed
//   -3 = core state initialization failed (sync state, settings, etc.)
static napi_value NativeInit(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        OH_LOG_ERROR(LOG_APP, "NativeInit: expected 1 argument (path), got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 1 argument: path");
        return nullptr;
    }

    char path[2048] = {0};
    size_t path_len = 0;
    napi_get_value_string_utf8(env, args[0], path, sizeof(path), &path_len);

    OH_LOG_INFO(LOG_APP, "NativeInit: calling writer_core_init with path='%{public}s'", path);
    int32_t result = writer_core_init(path);
    OH_LOG_INFO(LOG_APP, "NativeInit: writer_core_init returned %{public}d", result);

    // 注册 HiLog callback（幂等），尽早开始收集系统日志
    RegisterHilogCallback();

    if (result != 0) {
        OH_LOG_ERROR(LOG_APP, "NativeInit: FAILED with code %{public}d (path='%{public}s')", result, path);
    }

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
// Arguments: logDir, deviceId, appVersion, buildKey, locale, timezone (all strings).
// session_id is generated inside Rust; enabled/verbose default to true.
static napi_value NativeInitDiagnostics(napi_env env, napi_callback_info info) {
    size_t argc = 6;
    napi_value args[6];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 6) {
        OH_LOG_ERROR(LOG_APP, "NativeInitDiagnostics: expected 6 arguments, got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 6 arguments: logDir, deviceId, appVersion, buildKey, locale, timezone");
        return nullptr;
    }

    char log_dir[2048] = {0};
    char device_id[256] = {0};
    char app_version[128] = {0};
    char build_key[256] = {0};
    char locale[64] = {0};
    char timezone[64] = {0};
    size_t len = 0;
    napi_get_value_string_utf8(env, args[0], log_dir, sizeof(log_dir), &len);
    napi_get_value_string_utf8(env, args[1], device_id, sizeof(device_id), &len);
    napi_get_value_string_utf8(env, args[2], app_version, sizeof(app_version), &len);
    napi_get_value_string_utf8(env, args[3], build_key, sizeof(build_key), &len);
    napi_get_value_string_utf8(env, args[4], locale, sizeof(locale), &len);
    napi_get_value_string_utf8(env, args[5], timezone, sizeof(timezone), &len);

    // 注册 HiLog callback（幂等）— 必须在第一条 OH_LOG 和 writer_core_init_diagnostics() 之前注册，
    // 否则 EntryAbility.onCreate 最开始的日志、NativeInitDiagnostics 的 calling/returned、
    // 以及 Rust diagnostics 初始化过程中产生的 HiLog 都不会进环形缓冲。
    RegisterHilogCallback();

    OH_LOG_INFO(LOG_APP, "NativeInitDiagnostics: calling writer_core_init_diagnostics with logDir='%{public}s'", log_dir);
    int32_t result = writer_core_init_diagnostics(log_dir, device_id, app_version, build_key, locale, timezone);
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

// NativeGetHilogSnapshot: 返回当前环形缓冲中的 HiLog 内容字符串。
//   如果 callback 未注册或缓冲为空，返回空字符串。
static napi_value NativeGetHilogSnapshot(napi_env env, napi_callback_info info) {
    std::lock_guard<std::mutex> lock(hilog_mutex);

    napi_value result;
    if (hilog_buffer_used == 0) {
        napi_create_string_utf8(env, "", 0, &result);
        return result;
    }

    // 如果缓冲区未满（未回绕），直接返回 [0, pos) 范围的内容
    if (hilog_buffer_used < HILOG_BUFFER_SIZE) {
        napi_create_string_utf8(env, hilog_buffer, hilog_buffer_pos, &result);
        return result;
    }

    // 缓冲区已满（发生过回绕），需要拼接 [pos, end) + [0, pos) 两段
    // 分配临时缓冲区存放完整内容
    char *snapshot = new char[HILOG_BUFFER_SIZE + 1];
    size_t tail_len = HILOG_BUFFER_SIZE - hilog_buffer_pos;
    memcpy(snapshot, hilog_buffer + hilog_buffer_pos, tail_len);
    memcpy(snapshot + tail_len, hilog_buffer, hilog_buffer_pos);
    snapshot[HILOG_BUFFER_SIZE] = '\0';

    napi_create_string_utf8(env, snapshot, HILOG_BUFFER_SIZE, &result);
    delete[] snapshot;
    return result;
}

// NativeClearHilogSnapshot: 清空 HiLog 环形缓冲区。返回 int32 状态码：
//   0 = 成功
static napi_value NativeClearHilogSnapshot(napi_env env, napi_callback_info info) {
    std::lock_guard<std::mutex> lock(hilog_mutex);
    hilog_buffer_pos = 0;
    hilog_buffer_used = 0;

    napi_value ret;
    napi_create_int32(env, 0, &ret);
    return ret;
}

// ── Layout Policy ──
// NativeResolveLayout: Takes metrics JSON, returns ResultEnvelope<LayoutPolicyDto> JSON.
static napi_value NativeResolveLayout(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    size_t json_len = 0;
    char* json = nullptr;
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], nullptr, 0, &json_len);
        json = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[0], json, json_len + 1, &json_len);
    } else {
        json = new char[1];
        json[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_resolve_layout(json));
    delete[] json;
    return result;
}

// ── Screen Contract（#610：动作区域/顺序是产品语义，不随壳层变化） ──
// NativeResolveScreenPolicy: Takes screen_role JSON, returns ResultEnvelope<ScreenPolicyDto> JSON.
static napi_value NativeResolveScreenPolicy(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    size_t json_len = 0;
    char* json1 = nullptr;
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], nullptr, 0, &json_len);
        json1 = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[0], json1, json_len + 1, &json_len);
    } else {
        json1 = new char[1];
        json1[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_resolve_screen_policy(json1));
    delete[] json1;
    return result;
}

// ── Misc ──

static napi_value NativeIsAiAvailable(napi_env env, napi_callback_info info) {
    int32_t available = writer_core_is_ai_available();
    napi_value result;
    napi_create_int32(env, available, &result);
    return result;
}

// ── Module registration ──
// Init: Merges descriptors from all domains (app_state, project, chapter, settings,
//   sync, stats, starmap) plus core lifecycle into a single NAPI module.
//   Domain descriptor arrays are allocated by each get*Descriptors() function
//   and must remain valid for the lifetime of the module.
static napi_value Init(napi_env env, napi_value exports) {
    // Collect descriptors from all domains
    size_t app_state_count = 0, proj_count = 0, chap_count = 0, set_count = 0;
    size_t sync_count = 0, stats_count = 0, sm_count = 0, editor_count = 0;

    napi_property_descriptor* app_state_desc = getAppStateDescriptors(&app_state_count);
    napi_property_descriptor* proj_desc = getProjectDescriptors(&proj_count);
    napi_property_descriptor* chap_desc = getChapterDescriptors(&chap_count);
    napi_property_descriptor* set_desc = getSettingsDescriptors(&set_count);
    napi_property_descriptor* sync_desc = getSyncDescriptors(&sync_count);
    napi_property_descriptor* stats_desc = getStatsDescriptors(&stats_count);
    napi_property_descriptor* sm_desc = getStarMapDescriptors(&sm_count);
    napi_property_descriptor* editor_desc = getEditorSessionDescriptors(&editor_count);

    // Core lifecycle + layout + misc descriptors
    napi_property_descriptor core_desc[] = {
        {"nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeInitDiagnostics", nullptr, NativeInitDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSetDiagnosticsConfig", nullptr, NativeSetDiagnosticsConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeFlushDiagnostics", nullptr, NativeFlushDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearDiagnostics", nullptr, NativeClearDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeExportDiagnostics", nullptr, NativeExportDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLoadStatus", nullptr, NativeGetLoadStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLastError", nullptr, NativeGetLastError, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCalculateWordCount", nullptr, NativeCalculateWordCount, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetHilogSnapshot", nullptr, NativeGetHilogSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearHilogSnapshot", nullptr, NativeClearHilogSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveLayout", nullptr, NativeResolveLayout, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveScreenPolicy", nullptr, NativeResolveScreenPolicy, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeIsAiAvailable", nullptr, NativeIsAiAvailable, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    size_t core_count = sizeof(core_desc) / sizeof(core_desc[0]);

    // Merge all descriptors
    size_t total = core_count + app_state_count + proj_count + chap_count + set_count + sync_count + stats_count + sm_count + editor_count;
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

    napi_define_properties(env, exports, total, all_desc);
    delete[] all_desc;
    delete[] editor_desc;
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
