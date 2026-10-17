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
    // 带参操作（loadSyncConflictPreview / resolveSyncConflict）在 JS 线程复制出来的
    // 字符串参数；无参操作恒为 nullptr。所有权：SyncCompleteCb 或提交失败路径
    // 统一 delete[]（delete[] nullptr 是 no-op）。
    char* arg_target_kind;
    char* arg_project_id;
    char* arg_path;
    char* arg_decision;
};

// SyncAsyncArgsFree: 释放带参异步操作持有的字符串参数，避免创建/排队失败路径泄漏。
static void SyncAsyncArgsFree(SyncAsyncData* asyncData) {
    delete[] asyncData->arg_target_kind;
    delete[] asyncData->arg_project_id;
    delete[] asyncData->arg_path;
    delete[] asyncData->arg_decision;
    asyncData->arg_target_kind = nullptr;
    asyncData->arg_project_id = nullptr;
    asyncData->arg_path = nullptr;
    asyncData->arg_decision = nullptr;
}

// SyncCompleteCb: 通用 complete callback — 把 result_json 包装为 NAPI string，
//   通过 napi_resolve_deferred 返回；如果 result_json 为 null，resolve 一个标准
//   ResultEnvelope（success:false,errorCode:NULL_RESULT,userMessage），让 ArkTS 的
//   decodeEnvelope 正常解析错误码，而不是 reject 成 JSON 字符串后在 catch 里丢成 NATIVE_ERROR。
//   只有真正的工作线程/JS runtime 异常（status != napi_ok）才走 reject。
//   无论成功或失败，都会删除 async work、释放 SyncAsyncData。
//   注意：result_json 的释放统一在函数末尾，避免任何分支遗漏导致内存泄漏。
static void SyncCompleteCb(napi_env env, napi_status status, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    bool shouldResolve = true;

    if (status != napi_ok) {
        // 工作线程执行本身失败：这是真正的 runtime 异常，reject。
        shouldResolve = false;
        napi_value error;
        napi_create_string_utf8(env, "async sync work failed", NAPI_AUTO_LENGTH, &error);
        napi_reject_deferred(env, asyncData->deferred, error);
    } else if (asyncData->result_json != nullptr) {
        napi_value result;
        napi_create_string_utf8(env, asyncData->result_json, strlen(asyncData->result_json), &result);
        napi_resolve_deferred(env, asyncData->deferred, result);
        // result_json 释放统一在末尾
    } else {
        // Rust 返回 nullptr：作为标准 ResultEnvelope resolve（业务失败），
        // 保留 NULL_RESULT 错误码供 UI/log 追溯，不再 reject 后丢失。
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"NULL_RESULT\",\"userMessage\":\"Sync operation returned no result\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, asyncData->deferred, result);
    }

    // 统一释放：只要 result_json 不为 nullptr，就调用 writer_core_free_string
    if (asyncData->result_json != nullptr) {
        writer_core_free_string(asyncData->result_json);
    }
    // 释放带参操作复制出来的字符串参数（无参操作为 nullptr，delete[] 是 no-op）。
    SyncAsyncArgsFree(asyncData);
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
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        delete asyncData;
        return promise;
    }

    napi_status queueStatus = napi_queue_async_work(env, asyncData->work);
    if (queueStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        napi_delete_async_work(env, asyncData->work);
        delete asyncData;
        return promise;
    }

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
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        delete asyncData;
        return promise;
    }

    napi_status queueStatus = napi_queue_async_work(env, asyncData->work);
    if (queueStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        napi_delete_async_work(env, asyncData->work);
        delete asyncData;
        return promise;
    }

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
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        delete asyncData;
        return promise;
    }

    napi_status queueStatus = napi_queue_async_work(env, asyncData->work);
    if (queueStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        napi_delete_async_work(env, asyncData->work);
        delete asyncData;
        return promise;
    }

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

// ── 全量同步状态 / 冲突查询与解决（Issue #925 / #926） ──
// 这些操作会读写 full_state.local.json / conflicts.json 并可能触发磁盘 I/O，
// 与 NativeSyncDryRun / NativePerformSync 一样走 async work + deferred，
// 在工作线程执行，不阻塞 ArkTS 主线程。

// SyncAsyncCopyStringArg: 把第 index 个 JS 字符串参数复制为堆 C 字符串（调用方 delete[]）。
// 参数缺失、不是字符串或是 null 时返回 nullptr —— 必填参数由 Rust FFI 返回
// INVALID_ARGUMENT，可选参数（project_id）按 None 处理。
static char* SyncAsyncCopyStringArg(napi_env env, const napi_value* args, size_t argc, size_t index) {
    if (index >= argc) {
        return nullptr;
    }
    size_t len = 0;
    if (napi_get_value_string_utf8(env, args[index], nullptr, 0, &len) != napi_ok) {
        return nullptr;
    }
    char* buf = new char[len + 1];
    if (napi_get_value_string_utf8(env, args[index], buf, len + 1, &len) != napi_ok) {
        delete[] buf;
        return nullptr;
    }
    return buf;
}

// SyncAsyncSubmit: 创建 promise + async work 并排队，完成后走 SyncCompleteCb。
// asyncData 及其字符串参数的所有权转交给本函数：创建/排队失败时 resolve 标准
// 错误信封并释放 asyncData（与现有 NativePerformSync 的失败语义一致）。
static napi_value SyncAsyncSubmit(napi_env env, const char* workName,
                                  napi_async_execute_callback execute, SyncAsyncData* asyncData) {
    napi_deferred deferred;
    napi_value promise;
    napi_create_promise(env, &deferred, &promise);

    asyncData->deferred = deferred;
    asyncData->result_json = nullptr;

    napi_value name;
    napi_create_string_utf8(env, workName, NAPI_AUTO_LENGTH, &name);

    napi_status asyncStatus = napi_create_async_work(env, nullptr, name, execute,
        SyncCompleteCb, asyncData, &asyncData->work);
    if (asyncStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        SyncAsyncArgsFree(asyncData);
        delete asyncData;
        return promise;
    }

    napi_status queueStatus = napi_queue_async_work(env, asyncData->work);
    if (queueStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue async work\"}", NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        napi_delete_async_work(env, asyncData->work);
        SyncAsyncArgsFree(asyncData);
        delete asyncData;
        return promise;
    }

    return promise;
}

// LoadFullSyncStateExecuteCb: 工作线程上执行 writer_core_load_full_sync_state()
static void LoadFullSyncStateExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_load_full_sync_state();
}

// ListAllSyncConflictsExecuteCb: 工作线程上执行 writer_core_list_all_sync_conflicts()
static void ListAllSyncConflictsExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_list_all_sync_conflicts();
}

// LoadSyncConflictPreviewExecuteCb: 工作线程上执行 writer_core_load_sync_conflict_preview()
static void LoadSyncConflictPreviewExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_load_sync_conflict_preview(
        asyncData->arg_target_kind, asyncData->arg_project_id, asyncData->arg_path);
}

// ResolveSyncConflictExecuteCb: 工作线程上执行 writer_core_resolve_sync_conflict()
static void ResolveSyncConflictExecuteCb(napi_env env, void* data) {
    SyncAsyncData* asyncData = static_cast<SyncAsyncData*>(data);
    asyncData->result_json = writer_core_resolve_sync_conflict(
        asyncData->arg_target_kind, asyncData->arg_project_id, asyncData->arg_path,
        asyncData->arg_decision);
}

static napi_value NativeLoadFullSyncState(napi_env env, napi_callback_info info) {
    (void)info;
    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->arg_target_kind = nullptr;
    asyncData->arg_project_id = nullptr;
    asyncData->arg_path = nullptr;
    asyncData->arg_decision = nullptr;
    return SyncAsyncSubmit(env, "NativeLoadFullSyncState", LoadFullSyncStateExecuteCb, asyncData);
}

static napi_value NativeListAllSyncConflicts(napi_env env, napi_callback_info info) {
    (void)info;
    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->arg_target_kind = nullptr;
    asyncData->arg_project_id = nullptr;
    asyncData->arg_path = nullptr;
    asyncData->arg_decision = nullptr;
    return SyncAsyncSubmit(env, "NativeListAllSyncConflicts", ListAllSyncConflictsExecuteCb, asyncData);
}

// NativeLoadSyncConflictPreview(targetKind, projectId, path)
//   projectId 允许传 null / 缺省（App target 的冲突不属于任何作品）。
static napi_value NativeLoadSyncConflictPreview(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->arg_target_kind = SyncAsyncCopyStringArg(env, args, argc, 0);
    asyncData->arg_project_id = SyncAsyncCopyStringArg(env, args, argc, 1);
    asyncData->arg_path = SyncAsyncCopyStringArg(env, args, argc, 2);
    asyncData->arg_decision = nullptr;
    return SyncAsyncSubmit(env, "NativeLoadSyncConflictPreview", LoadSyncConflictPreviewExecuteCb, asyncData);
}

// NativeResolveSyncConflict(targetKind, projectId, path, decision)
//   decision 为 "keep_local" / "take_remote" / "mark_merged"，由 Rust FFI 校验。
static napi_value NativeResolveSyncConflict(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    SyncAsyncData* asyncData = new SyncAsyncData();
    asyncData->arg_target_kind = SyncAsyncCopyStringArg(env, args, argc, 0);
    asyncData->arg_project_id = SyncAsyncCopyStringArg(env, args, argc, 1);
    asyncData->arg_path = SyncAsyncCopyStringArg(env, args, argc, 2);
    asyncData->arg_decision = SyncAsyncCopyStringArg(env, args, argc, 3);
    return SyncAsyncSubmit(env, "NativeResolveSyncConflict", ResolveSyncConflictExecuteCb, asyncData);
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
        {"nativeLoadFullSyncState", nullptr, NativeLoadFullSyncState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListAllSyncConflicts", nullptr, NativeListAllSyncConflicts, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadSyncConflictPreview", nullptr, NativeLoadSyncConflictPreview, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveSyncConflict", nullptr, NativeResolveSyncConflict, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
