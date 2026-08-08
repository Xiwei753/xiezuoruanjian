// ── Sync NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
// All handlers return ResultEnvelope JSON via ReturnJsonString (which frees the core-allocated char*).
// All sync handlers are per-project: project_id is the first argument (Issue #600 评论 #3).
// NativeSaveSyncConfig takes project_id + SyncConfigDto JSON as arguments.

static napi_value NativeLoadSyncConfig(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_load_sync_config(project_id));
}

static napi_value NativeSaveSyncConfig(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    size_t json_len = 0;
    char* json = nullptr;
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], nullptr, 0, &json_len);
        json = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[1], json, json_len + 1, &json_len);
    } else {
        json = new char[1];
        json[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_save_sync_config(project_id, json));
    delete[] json;
    return result;
}

static napi_value NativeSyncDryRun(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_sync_dry_run(project_id));
}

static napi_value NativeSyncDiagnostics(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_sync_diagnostics(project_id));
}

static napi_value NativePerformSync(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_perform_sync(project_id));
}

// ── Sync property descriptors ──

napi_property_descriptor* getSyncDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeLoadSyncConfig", nullptr, NativeLoadSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncConfig", nullptr, NativeSaveSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDryRun", nullptr, NativeSyncDryRun, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDiagnostics", nullptr, NativeSyncDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativePerformSync", nullptr, NativePerformSync, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
