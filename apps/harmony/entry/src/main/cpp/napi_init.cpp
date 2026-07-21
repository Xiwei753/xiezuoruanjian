#include <cstring>
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

#include "napi_workspace.cpp"
#include "napi_project.cpp"
#include "napi_chapter.cpp"
#include "napi_settings.cpp"
#include "napi_sync.cpp"
#include "napi_stats.cpp"
#include "napi_starmap.cpp"

// ── Core lifecycle ──

// NativeInit: Initialize core with workspace path. Returns int32 status code:
//   0 = success
//   -1 = null/empty path
//   -2 = directory creation failed
//   -3 = manifest file initialization failed (sync state, settings, etc.)
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

// ── Screen Policy ──
// NativeResolveScreenPolicy: Takes screen_role + shell_mode JSON, returns ResultEnvelope<ScreenPolicyDto> JSON.
static napi_value NativeResolveScreenPolicy(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
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

    char* json2 = nullptr;
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], nullptr, 0, &json_len);
        json2 = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[1], json2, json_len + 1, &json_len);
    } else {
        json2 = new char[1];
        json2[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_resolve_screen_policy(json1, json2));
    delete[] json1;
    delete[] json2;
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
// Init: Merges descriptors from all domains (workspace, project, chapter, settings,
//   sync, stats, starmap) plus core lifecycle into a single NAPI module.
//   Domain descriptor arrays are allocated by each get*Descriptors() function
//   and must remain valid for the lifetime of the module.
static napi_value Init(napi_env env, napi_value exports) {
    // Collect descriptors from all domains
    size_t ws_count = 0, proj_count = 0, chap_count = 0, set_count = 0;
    size_t sync_count = 0, stats_count = 0, sm_count = 0;

    napi_property_descriptor* ws_desc = getWorkspaceDescriptors(&ws_count);
    napi_property_descriptor* proj_desc = getProjectDescriptors(&proj_count);
    napi_property_descriptor* chap_desc = getChapterDescriptors(&chap_count);
    napi_property_descriptor* set_desc = getSettingsDescriptors(&set_count);
    napi_property_descriptor* sync_desc = getSyncDescriptors(&sync_count);
    napi_property_descriptor* stats_desc = getStatsDescriptors(&stats_count);
    napi_property_descriptor* sm_desc = getStarMapDescriptors(&sm_count);

    // Core lifecycle + layout + misc descriptors
    napi_property_descriptor core_desc[] = {
        {"nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLoadStatus", nullptr, NativeGetLoadStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLastError", nullptr, NativeGetLastError, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCalculateWordCount", nullptr, NativeCalculateWordCount, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveLayout", nullptr, NativeResolveLayout, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveScreenPolicy", nullptr, NativeResolveScreenPolicy, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeIsAiAvailable", nullptr, NativeIsAiAvailable, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    size_t core_count = sizeof(core_desc) / sizeof(core_desc[0]);

    // Merge all descriptors
    size_t total = core_count + ws_count + proj_count + chap_count + set_count + sync_count + stats_count + sm_count;
    napi_property_descriptor* all_desc = new napi_property_descriptor[total];
    size_t offset = 0;

    memcpy(all_desc + offset, core_desc, core_count * sizeof(napi_property_descriptor)); offset += core_count;
    memcpy(all_desc + offset, ws_desc, ws_count * sizeof(napi_property_descriptor)); offset += ws_count;
    memcpy(all_desc + offset, proj_desc, proj_count * sizeof(napi_property_descriptor)); offset += proj_count;
    memcpy(all_desc + offset, chap_desc, chap_count * sizeof(napi_property_descriptor)); offset += chap_count;
    memcpy(all_desc + offset, set_desc, set_count * sizeof(napi_property_descriptor)); offset += set_count;
    memcpy(all_desc + offset, sync_desc, sync_count * sizeof(napi_property_descriptor)); offset += sync_count;
    memcpy(all_desc + offset, stats_desc, stats_count * sizeof(napi_property_descriptor)); offset += stats_count;
    memcpy(all_desc + offset, sm_desc, sm_count * sizeof(napi_property_descriptor)); offset += sm_count;

    napi_define_properties(env, exports, total, all_desc);
    delete[] all_desc;
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
