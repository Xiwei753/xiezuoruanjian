// ── App State & Location NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
//
// 新 Core API 边界：平台自己提供 app_data_root 与 projects_root，
// 通过 nativeInit 注入；Core 不再创建/验证/打开 workspace。
// 此文件仅保留查询类入口（list/get state/resolve location）。

// ── App State 查询 ──

static napi_value NativeListAppSummaries(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_app_summaries());
}

static napi_value NativeGetAppState(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_app_state());
}

static napi_value NativeResolveChapterLocation(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char chapter_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], chapter_id, sizeof(chapter_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_resolve_chapter_location(chapter_id));
}

static napi_value NativeResolveVolumeLocation(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char volume_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], volume_id, sizeof(volume_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_resolve_volume_location(volume_id));
}

// ── App State property descriptors ──

napi_property_descriptor* getAppStateDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeListAppSummaries", nullptr, NativeListAppSummaries, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetAppState", nullptr, NativeGetAppState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveChapterLocation", nullptr, NativeResolveChapterLocation, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveVolumeLocation", nullptr, NativeResolveVolumeLocation, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
