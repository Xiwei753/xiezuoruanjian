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

// 「今天」的写作汇总：无参数，「今天是哪一天」由 Core 的本地日历口径决定。
// Issue #829 评论10：端侧不再自己拼 YYYY-MM-DD，否则和 Core 时区口径错开时
// 凌晨会出现「今日进度提前清零」。
static napi_value NativeGetTodayWritingStatsSummary(napi_env env, napi_callback_info info) {
    (void)info;
    return ReturnJsonString(env, writer_core_get_today_writing_stats_summary());
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

// Issue #829 评论7：按编辑事务上报写作统计。
// ArkTS 传一个 JSON 字符串（EditorChangeStatsInputDto 线格式），C 层只透传，
// cause→EventSource 的映射和计数字段全在 Core 侧决定。
//
// **必须是 Node-API async work，不能同步调 Rust。**
// Core 这条链不轻：record_editor_change_stats → StatsApi::record_event →
// aggregate_single_event，每个编辑事务都要做一次 raw event append。
// 同步调就等于磁盘 I/O 落在 ArkUI 主线程上。
// ArkTS 的 async 函数不会自己开线程，所以光在 ArkTS 侧 await 没有意义 ——
// 真正的切线程点在这里：execute 回调跑在线程池，complete 回调只 resolve Promise。

// async work 的载荷。在 handler 里分配，随 work 一起活到 complete 回调。
struct StatsRecordWork {
    // 事件 JSON 的堆拷贝，execute 回调在线程池里读它。
    char* json = nullptr;
    // create_promise 给的 deferred。complete 回调用它 resolve/reject，
    // 放在载荷里而不是 instance data：instance data 是 env 全局的，
    // 两条调用重叠会互相覆盖对方的 deferred，第一个 Promise 永远不落地。
    napi_deferred deferred = nullptr;
    // create_async_work 之后回填。设置了 complete 回调时，官方要求在 complete 里
    // 调 napi_delete_async_work 释放 work 资源，否则每次编辑事务都漏一个。
    napi_async_work async_work = nullptr;
    // execute 的结果，供 complete 回调决定 resolve 还是 reject。
    bool ok = false;
};

// Execute 回调 — 运行在线程池线程，可以安全调用 Rust（含文件 I/O）。
// 这里不碰任何 napi_value：NAPI 的 JS 侧对象只能在主线程用。
static void StatsRecordExecute(napi_env env, void* data) {
    (void)env;
    auto* work = static_cast<StatsRecordWork*>(data);
    work->ok = writer_core_record_editor_change_stats(work->json);
}

// Complete 回调 — 回到主线程，把结果 resolve/reject 成 Promise，然后释放 work。
static void StatsRecordComplete(napi_env env, napi_status status, void* data) {
    auto* work = static_cast<StatsRecordWork*>(data);
    if (status == napi_ok) {
        if (work->ok) {
            napi_value result = nullptr;
            napi_get_boolean(env, true, &result);
            napi_resolve_deferred(env, work->deferred, result);
        } else {
            // 失败走 reject 而不是 resolve(false)：ArkTS 侧必须能区分
            // 「这条写成功了」和「这条没写进去」，不能把失败伪装成完成。
            napi_value message = nullptr;
            const char* text = "record_editor_change_stats failed";
            napi_create_string_utf8(env, text, NAPI_AUTO_LENGTH, &message);
            napi_reject_deferred(env, work->deferred, message);
        }
    } else {
        napi_value message = nullptr;
        const char* text = "record_editor_change_stats async work aborted";
        napi_create_string_utf8(env, text, NAPI_AUTO_LENGTH, &message);
        napi_reject_deferred(env, work->deferred, message);
    }
    // 设置了 complete 回调时 work 资源由 complete 负责释放（官方要求，
    // 否则每个编辑事务都漏一个 async work）。所有 napi 调用都已结束再删。
    napi_delete_async_work(env, work->async_work);
    delete[] work->json;
    delete work;
}

static napi_value NativeRecordEditorChangeStats(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1] = {nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    // JSON 先拷到堆上：execute 回调跑在别的线程，不能读 NAPI value。
    char* json = TakeStringArg(env, argc >= 1 ? args[0] : nullptr);

    auto* work = new StatsRecordWork();
    work->json = json;

    napi_value promise = nullptr;
    if (napi_create_promise(env, &work->deferred, &promise) != napi_ok) {
        delete[] json;
        delete work;
        napi_throw_error(env, nullptr, "napi_create_promise failed");
        return nullptr;
    }

    napi_value resource_name = nullptr;
    const char* name = "WriterCoreRecordEditorChangeStats";
    napi_create_string_utf8(env, name, NAPI_AUTO_LENGTH, &resource_name);

    napi_async_work async_work = nullptr;
    if (napi_create_async_work(env, nullptr, resource_name, StatsRecordExecute,
                               StatsRecordComplete, work, &async_work) != napi_ok) {
        delete[] json;
        delete work;
        napi_throw_error(env, nullptr, "napi_create_async_work failed");
        return nullptr;
    }
    work->async_work = async_work;

    if (napi_queue_async_work(env, async_work) != napi_ok) {
        napi_delete_async_work(env, async_work);
        delete[] json;
        delete work;
        napi_throw_error(env, nullptr, "napi_queue_async_work failed");
        return nullptr;
    }

    // 调用方（ArkTS）拿到 Promise，await 它就是等磁盘写入真正完成。
    return promise;
}

// ── Stats property descriptors ──

napi_property_descriptor* getStatsDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeGetWritingStats", nullptr, NativeGetWritingStats, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetTodayWritingStatsSummary", nullptr, NativeGetTodayWritingStatsSummary, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetWritingStatsSummary", nullptr, NativeGetWritingStatsSummary, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetWritingSpeedCurve", nullptr, NativeGetWritingSpeedCurve, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetCurrentWritingSpeed", nullptr, NativeGetCurrentWritingSpeed, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRecordEditorChangeStats", nullptr, NativeRecordEditorChangeStats, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
