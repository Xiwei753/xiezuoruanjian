// ── Writing Stats NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

static napi_value NativeGetWritingStats(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_writing_stats());
}

static napi_value NativeProcessWritingEvent(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_process_writing_event(json));
    delete[] json;
    return result;
}

// ── Stats property descriptors ──

napi_property_descriptor* getStatsDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeGetWritingStats", nullptr, NativeGetWritingStats, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeProcessWritingEvent", nullptr, NativeProcessWritingEvent, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
