// ── Sync NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
// All handlers return ResultEnvelope JSON via ReturnJsonString (which frees the core-allocated char*).
// 全量同步统一入口（Issue #630）：一个全局 SyncConfig + 一份全局凭据，
// App target + 所有 Project target 一次同步。旧的 per-project sync /
// app-level sync 双套 handler 已删除，只保留全量同步三个入口
// （dry-run / diagnostics / perform）+ 全局 config 读写 + App target 状态查询。

static napi_value NativeLoadSyncConfig(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_load_sync_config());
}

static napi_value NativeSaveSyncConfig(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_sync_config(json));
    delete[] json;
    return result;
}

static napi_value NativeSyncDryRun(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_full_sync_dry_run());
}

static napi_value NativeSyncDiagnostics(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_full_sync_diagnostics());
}

static napi_value NativePerformSync(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_perform_full_sync());
}

static napi_value NativeLoadAppSyncState(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_load_app_sync_state());
}

static napi_value NativeSaveAppSyncState(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_app_sync_state(json));
    delete[] json;
    return result;
}

// ── Sync property descriptors ──

napi_property_descriptor* getSyncDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeLoadSyncConfig", nullptr, NativeLoadSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncConfig", nullptr, NativeSaveSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDryRun", nullptr, NativeSyncDryRun, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDiagnostics", nullptr, NativeSyncDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativePerformSync", nullptr, NativePerformSync, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadAppSyncState", nullptr, NativeLoadAppSyncState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveAppSyncState", nullptr, NativeSaveAppSyncState, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
