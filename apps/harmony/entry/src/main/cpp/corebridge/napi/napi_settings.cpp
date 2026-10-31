// ── Settings NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
//
// 设置分类：
// - Local Settings：设备级设置（如编辑器字号、主题），不同步到远端
// - Syncable Settings：跨设备同步设置（如写作目标、统计偏好），通过同步协议共享
//
// 本地设置读写会访问磁盘并可能进入 Core 状态，必须走 Node-API async work，
// 不能在 ArkTS 主线程上直接调用 writer_core_*。

#include "napi_json_async.h"

// ── Local Settings ──

static char* LoadLocalSettingsExecute(const char* input) {
    (void)input;
    return writer_core_load_local_settings();
}

static char* SaveLocalSettingsExecute(const char* input) {
    return writer_core_save_local_settings(input);
}

// 返回 ResultEnvelope<LocalSettingsDto> JSON（Promise）
static napi_value NativeLoadLocalSettings(napi_env env, napi_callback_info info) {
    (void)info;
    return NapiJsonAsyncSubmit(env, "NativeLoadLocalSettings", "",
                               LoadLocalSettingsExecute);
}

// 输入：LocalSettingsDto JSON；输出：ResultEnvelope JSON（Promise）
static napi_value NativeSaveLocalSettings(napi_env env, napi_callback_info info) {
    std::string input;
    NapiJsonAsyncReadArg(env, info, &input);
    return NapiJsonAsyncSubmit(env, "NativeSaveLocalSettings", input.c_str(),
                               SaveLocalSettingsExecute);
}

// ── Syncable Settings ──
// 与 Local Settings 的区别：Syncable Settings 通过同步协议在设备间共享，
// Local Settings 仅存储在本地设备上。

static char* LoadSyncableSettingsExecute(const char* input) {
    (void)input;
    return writer_core_load_syncable_settings();
}

static char* SaveSyncableSettingsExecute(const char* input) {
    return writer_core_save_syncable_settings(input);
}

// 返回 ResultEnvelope<SyncableSettingsDto> JSON（Promise）
static napi_value NativeLoadSyncableSettings(napi_env env, napi_callback_info info) {
    (void)info;
    return NapiJsonAsyncSubmit(env, "NativeLoadSyncableSettings", "",
                               LoadSyncableSettingsExecute);
}

static napi_value NativeSaveSyncableSettings(napi_env env, napi_callback_info info) {
    std::string input;
    NapiJsonAsyncReadArg(env, info, &input);
    return NapiJsonAsyncSubmit(env, "NativeSaveSyncableSettings", input.c_str(),
                               SaveSyncableSettingsExecute);
}

// 内置主题是纯内存常量，不进入 Core 状态锁，可保持同步返回。
static napi_value NativeListBuiltinThemes(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_builtin_themes());
}

// ── Settings property descriptors ──
// 使用 static 数组保证描述符在模块生命周期内有效（NAPI 要求描述符指针在注册后仍可访问）

napi_property_descriptor* getSettingsDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeLoadLocalSettings", nullptr, NativeLoadLocalSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveLocalSettings", nullptr, NativeSaveLocalSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadSyncableSettings", nullptr, NativeLoadSyncableSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncableSettings", nullptr, NativeSaveSyncableSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListBuiltinThemes", nullptr, NativeListBuiltinThemes, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
