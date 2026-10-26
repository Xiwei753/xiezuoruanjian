// ── Settings NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
//
// 设置分类：
// - Local Settings：设备级设置（如编辑器字号、主题），不同步到远端
// - Syncable Settings：跨设备同步设置（如写作目标、统计偏好），通过同步协议共享

// ── Local Settings ──

// 返回 ResultEnvelope<LocalSettingsDto> JSON
static napi_value NativeLoadLocalSettings(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_load_local_settings());
}

// 输入：LocalSettingsDto JSON；输出：ResultEnvelope JSON
static napi_value NativeSaveLocalSettings(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_local_settings(json));
    delete[] json;
    return result;
}

// ── Syncable Settings ──
// 与 Local Settings 的区别：Syncable Settings 通过同步协议在设备间共享，
// Local Settings 仅存储在本地设备上。

// 返回 ResultEnvelope<SyncableSettingsDto> JSON
static napi_value NativeLoadSyncableSettings(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_load_syncable_settings());
}

static napi_value NativeSaveSyncableSettings(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_syncable_settings(json));
    delete[] json;
    return result;
}

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
