// ── Writing Stats NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

static napi_value NativeGetWritingStats(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_writing_stats());
}

// 读第 index 个 string 参数；ArkTS 少传时返回空串，让 Rust 侧按 INVALID_ARGUMENT 报，
// 不用在 C++ 这层自己判空再拼错误 JSON——错误形状由 Core 单侧定义。
// 调用方负责 delete[]。
static char* TakeStringArg(napi_env env, napi_value value) {
    if (value == nullptr) {
        char* empty = new char[1];
        empty[0] = '\0';
        return empty;
    }
    size_t len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) {
        char* empty = new char[1];
        empty[0] = '\0';
        return empty;
    }
    char* buffer = new char[len + 1];
    buffer[0] = '\0';
    napi_get_value_string_utf8(env, value, buffer, len + 1, &len);
    return buffer;
}

static napi_value NativeGetWritingStatsSummary(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2] = {nullptr, nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* start = TakeStringArg(env, argc >= 1 ? args[0] : nullptr);
    char* end = TakeStringArg(env, argc >= 2 ? args[1] : nullptr);

    napi_value result = ReturnJsonString(env, writer_core_get_writing_stats_summary(start, end));
    delete[] start;
    delete[] end;
    return result;
}

static napi_value NativeGetWritingSpeedCurve(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3] = {nullptr, nullptr, nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* start = TakeStringArg(env, argc >= 1 ? args[0] : nullptr);
    char* end = TakeStringArg(env, argc >= 2 ? args[1] : nullptr);
    uint32_t bucket_minutes = 0;
    if (argc >= 3 && args[2] != nullptr) {
        napi_get_value_uint32(env, args[2], &bucket_minutes);
    }

    napi_value result =
        ReturnJsonString(env, writer_core_get_writing_speed_curve(start, end, bucket_minutes));
    delete[] start;
    delete[] end;
    return result;
}

// Issue #829：实时写作速度。windowSeconds 少传或非数值时留 0，Rust 侧会钳到
// 至少 1 秒（0 秒窗口无意义且无法折算速度），不在 C++ 这层重复校验。
static napi_value NativeGetCurrentWritingSpeed(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1] = {nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint32_t window_seconds = 0;
    if (argc >= 1 && args[0] != nullptr) {
        napi_get_value_uint32(env, args[0], &window_seconds);
    }

    return ReturnJsonString(env, writer_core_get_current_writing_speed(window_seconds));
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
        {"nativeGetWritingStatsSummary", nullptr, NativeGetWritingStatsSummary, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetWritingSpeedCurve", nullptr, NativeGetWritingSpeedCurve, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetCurrentWritingSpeed", nullptr, NativeGetCurrentWritingSpeed, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeProcessWritingEvent", nullptr, NativeProcessWritingEvent, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
