// ── Sync NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
// 全量同步统一入口（Issue #630）：一个全局 SyncConfig + 一份全局凭据，
// App target + 所有 Project target 一次同步。旧的 per-project sync /
// app-level sync 双套 handler 已删除，只保留全量同步三个入口
// （dry-run / diagnostics / perform）+ 全局 config 读写 + App target 状态查询。
//
// NativePerformSync / NativeSyncDiagnostics / NativeSyncDryRun 使用
// napi_create_async_work + napi_create_promise 在工作线程上执行 Rust 同步调用，
// 避免阻塞 ArkTS 主线程。轻量级本地操作（config/secrets/state 读写）
// 仍使用同步 ReturnJsonString。

// ── Async work infrastructure for sync operations ──

struct SyncAsyncData {
    napi_async_work work;
    napi_deferred deferred;
    char* result_json;  // writer_core_* 返回的 JSON C string，在工作线程中赋值
};

// SyncCompleteCb: 通用 complete callback — 把 result_json 包装为 NAPI string，
//   通过 napi_resolve_deferred 返回；如果 result_json 为 null，reject 一个错误 JSON。
//   无论成功或失败，都会释放 result_json（writer_core_free_string）、删除 async work、
//   释放 SyncAsyncData。
static void SyncCompleteCb(napi_env env, napi_status status, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);

    if (asyncData->result_json != nullptr) {
        napi_value result;
        napi_create_string_utf8(env, asyncData->result_json, strlen(asyncData->result_json), &result);
        writer_core_free_string(asyncData->result_json);
        napi_resolve_deferred(env, asyncData->deferred, result);
    } else {
        napi_value error;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"NULL_RESULT\"}", 47, &error);
        napi_reject_deferred(env, asyncData->deferred, error);
    }

    napi_delete_async_work(env, asyncData->work);
    delete asyncData;
}

// PerformSyncExecuteCb: 工作线程上执行 writer_core_perform_full_sync()
static void PerformSyncExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_perform_full_sync();
}

// SyncDiagnosticsExecuteCb: 工作线程上执行 writer_core_full_sync_diagnostics()
static void SyncDiagnosticsExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_full_sync_diagnostics();
}

// SyncDryRunExecuteCb: 工作线程上执行 writer_core_full_sync_dry_run()
static void SyncDryRunExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_full_sync_dry_run();
}

// ── 轻量级本地操作（同步调用，不阻塞主线程） ──

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

// ── 异步同步操作（使用 napi_create_async_work + napi_create_promise） ──

static napi_value NativeSyncDryRun(napi_env env, napi_callback_info info) {
    (void)info;

    napi_deferred deferred;
    napi_value promise;
    napi_create_promise(env, &deferred, &promise);

    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->deferred = deferred;
    asyncData->result_json = nullptr;

    napi_value name;
    napi_create_string_utf8(env, "NativeSyncDryRun", NAPI_AUTO_LENGTH, &name);

    napi_status asyncStatus = napi_create_async_work(env, nullptr, name,
        SyncDryRunExecuteCb, SyncCompleteCb, asyncData, &asyncData->work);
    if (asyncStatus != napi_ok) {
        napi_value error;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_WORK_FAILED\"}", 50, &error);
        napi_reject_deferred(env, deferred, error);
        delete asyncData;
        return promise;
    }

    napi_queue_async_work(env, asyncData->work);
    return promise;
}

static napi_value NativeSyncDiagnostics(napi_env env, napi_callback_info info) {
    (void)info;

    napi_deferred deferred;
    napi_value promise;
    napi_create_promise(env, &deferred, &promise);

    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->deferred = deferred;
    asyncData->result_json = nullptr;

    napi_value name;
    napi_create_string_utf8(env, "NativeSyncDiagnostics", NAPI_AUTO_LENGTH, &name);

    napi_status asyncStatus = napi_create_async_work(env, nullptr, name,
        SyncDiagnosticsExecuteCb, SyncCompleteCb, asyncData, &asyncData->work);
    if (asyncStatus != napi_ok) {
        napi_value error;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_WORK_FAILED\"}", 50, &error);
        napi_reject_deferred(env, deferred, error);
        delete asyncData;
        return promise;
    }

    napi_queue_async_work(env, asyncData->work);
    return promise;
}

static napi_value NativePerformSync(napi_env env, napi_callback_info info) {
    (void)info;

    napi_deferred deferred;
    napi_value promise;
    napi_create_promise(env, &deferred, &promise);

    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->deferred = deferred;
    asyncData->result_json = nullptr;

    napi_value name;
    napi_create_string_utf8(env, "NativePerformSync", NAPI_AUTO_LENGTH, &name);

    napi_status asyncStatus = napi_create_async_work(env, nullptr, name,
        PerformSyncExecuteCb, SyncCompleteCb, asyncData, &asyncData->work);
    if (asyncStatus != napi_ok) {
        napi_value error;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_WORK_FAILED\"}", 50, &error);
        napi_reject_deferred(env, deferred, error);
        delete asyncData;
        return promise;
    }

    napi_queue_async_work(env, asyncData->work);
    return promise;
}

// ── 轻量级本地操作（同步调用，不阻塞主线程） ──

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

static napi_value NativeLoadSyncSecrets(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_load_sync_secrets());
}

static napi_value NativeSaveSyncSecrets(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_sync_secrets(json));
    delete[] json;
    return result;
}

// ── Sync property descriptors ──

napi_property_descriptor* getSyncDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeLoadSyncConfig", nullptr, NativeLoadSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncConfig", nullptr, NativeSaveSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadSyncSecrets", nullptr, NativeLoadSyncSecrets, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncSecrets", nullptr, NativeSaveSyncSecrets, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDryRun", nullptr, NativeSyncDryRun, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDiagnostics", nullptr, NativeSyncDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativePerformSync", nullptr, NativePerformSync, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadAppSyncState", nullptr, NativeLoadAppSyncState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveAppSyncState", nullptr, NativeSaveAppSyncState, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
