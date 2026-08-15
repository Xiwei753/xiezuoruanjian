// ── Settings NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
//
// 设置分类：
// - Local Settings：设备级设置（如编辑器字号、主题），不同步到远端
// - Syncable Settings：跨设备同步设置（如写作目标、统计偏好），通过同步协议共享
// - Palette Records：主题配色记录，按 device_id + fingerprint 标识

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

// ── Palette Records ──
// device_id 标识创建此配色的设备，fingerprint 标识主题指纹（唯一标识一个配色方案）。
// 删除不存在的记录时 Core 静默成功（幂等语义）。

static napi_value NativeListPaletteRecords(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_palette_records());
}

static napi_value NativeLoadPaletteRecord(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    size_t len1 = 0;
    char* device_id = nullptr;
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], nullptr, 0, &len1);
        device_id = new char[len1 + 1];
        napi_get_value_string_utf8(env, args[0], device_id, len1 + 1, &len1);
    } else {
        device_id = new char[1];
        device_id[0] = '\0';
    }

    size_t len2 = 0;
    char* fingerprint = nullptr;
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], nullptr, 0, &len2);
        fingerprint = new char[len2 + 1];
        napi_get_value_string_utf8(env, args[1], fingerprint, len2 + 1, &len2);
    } else {
        fingerprint = new char[1];
        fingerprint[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_load_palette_record(device_id, fingerprint));
    delete[] device_id;
    delete[] fingerprint;
    return result;
}

static napi_value NativeDeletePaletteRecord(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    size_t len1 = 0;
    char* device_id = nullptr;
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], nullptr, 0, &len1);
        device_id = new char[len1 + 1];
        napi_get_value_string_utf8(env, args[0], device_id, len1 + 1, &len1);
    } else {
        device_id = new char[1];
        device_id[0] = '\0';
    }

    size_t len2 = 0;
    char* fingerprint = nullptr;
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], nullptr, 0, &len2);
        fingerprint = new char[len2 + 1];
        napi_get_value_string_utf8(env, args[1], fingerprint, len2 + 1, &len2);
    } else {
        fingerprint = new char[1];
        fingerprint[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_delete_palette_record(device_id, fingerprint));
    delete[] device_id;
    delete[] fingerprint;
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
        {"nativeListPaletteRecords", nullptr, NativeListPaletteRecords, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadPaletteRecord", nullptr, NativeLoadPaletteRecord, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeletePaletteRecord", nullptr, NativeDeletePaletteRecord, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListBuiltinThemes", nullptr, NativeListBuiltinThemes, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
