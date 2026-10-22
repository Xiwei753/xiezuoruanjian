// ── Global Search NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.
//
// ArkTS 只传查询参数，搜索、排序、分页游标与命中定位都由 Core 统一计算。
// 返回 ResultEnvelope JSON；正文 UTF-8 byte 偏移原样透传，由 ArkTS 在真实原文上转 UTF-16。

// 读取 NAPI string 到调用方持有的堆内存；调用方负责 delete[]。
// 返回 nullptr 表示参数缺失或不是字符串。
static char* NapiSearchDupString(napi_env env, napi_value value) {
    size_t len = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &len) != napi_ok) {
        return nullptr;
    }
    char* buffer = new char[len + 1];
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, buffer, len + 1, &copied) != napi_ok) {
        delete[] buffer;
        return nullptr;
    }
    buffer[copied] = '\0';
    return buffer;
}

// ── Async search work ──
// 查询是只读的索引扫描，但索引可能很大；与同步操作一样使用 NAPI async work，
// 避免在 ArkTS UI 线程上执行 Core 查询。

struct SearchAsyncData {
    napi_async_work work;
    napi_deferred deferred;
    char* result_json;
    char* query;
    char* scope;
    char* cursor;
    char* context_json;
    int32_t limit;
};

static void SearchAsyncArgsFree(SearchAsyncData* asyncData) {
    delete[] asyncData->query;
    delete[] asyncData->scope;
    delete[] asyncData->cursor;
    delete[] asyncData->context_json;
    asyncData->query = nullptr;
    asyncData->scope = nullptr;
    asyncData->cursor = nullptr;
    asyncData->context_json = nullptr;
}

static void SearchExecuteCb(napi_env env, void* data) {
    (void)env;
    SearchAsyncData* asyncData = static_cast<SearchAsyncData*>(data);
    const char* query_ptr = asyncData->query != nullptr ? asyncData->query : "";
    const char* scope_ptr = asyncData->scope != nullptr && asyncData->scope[0] != '\0'
        ? asyncData->scope
        : "all";
    // 空游标/空上下文按“没有”传，避免把空串当作有效游标。
    const char* cursor_ptr = asyncData->cursor != nullptr && asyncData->cursor[0] != '\0'
        ? asyncData->cursor
        : nullptr;
    const char* context_ptr = asyncData->context_json != nullptr && asyncData->context_json[0] != '\0'
        ? asyncData->context_json
        : nullptr;
    asyncData->result_json = writer_core_global_search_page(
        query_ptr, scope_ptr, static_cast<uint32_t>(asyncData->limit), cursor_ptr, context_ptr);
}

static void SearchCompleteCb(napi_env env, napi_status status, void* data) {
    SearchAsyncData* asyncData = static_cast<SearchAsyncData*>(data);

    if (status != napi_ok) {
        napi_value error;
        napi_create_string_utf8(env, "async search work failed", NAPI_AUTO_LENGTH, &error);
        napi_reject_deferred(env, asyncData->deferred, error);
    } else if (asyncData->result_json != nullptr) {
        napi_value result;
        napi_create_string_utf8(
            env, asyncData->result_json, NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, asyncData->deferred, result);
    } else {
        napi_value result;
        napi_create_string_utf8(env,
            "{\"success\":false,\"errorCode\":\"NULL_RESULT\",\"userMessage\":\"Search returned no result\"}",
            NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, asyncData->deferred, result);
    }

    if (asyncData->result_json != nullptr) {
        writer_core_free_string(asyncData->result_json);
    }
    SearchAsyncArgsFree(asyncData);
    napi_delete_async_work(env, asyncData->work);
    delete asyncData;
}

static napi_value NativeGlobalSearchPage(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5] = {nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* query = argc >= 1 ? NapiSearchDupString(env, args[0]) : nullptr;
    char* scope = argc >= 2 ? NapiSearchDupString(env, args[1]) : nullptr;
    char* cursor = argc >= 4 ? NapiSearchDupString(env, args[3]) : nullptr;
    char* context_json = argc >= 5 ? NapiSearchDupString(env, args[4]) : nullptr;

    int32_t limit = 50;
    if (argc >= 3) {
        napi_get_value_int32(env, args[2], &limit);
    }
    if (limit <= 0) {
        limit = 50;
    }

    // 搜索可能扫描整份索引；放到 NAPI async work 的工作线程执行，ArkTS 主线程保持可交互。
    // 参数在 JS 线程复制，完成后统一释放，避免工作线程访问 napi_value。
    SearchAsyncData* asyncData = new SearchAsyncData();
    asyncData->deferred = nullptr;
    asyncData->work = nullptr;
    asyncData->result_json = nullptr;
    asyncData->query = query;
    asyncData->scope = scope;
    asyncData->cursor = cursor;
    asyncData->context_json = context_json;
    asyncData->limit = limit;

    napi_deferred deferred;
    napi_value promise;
    napi_create_promise(env, &deferred, &promise);
    asyncData->deferred = deferred;

    napi_value name;
    napi_create_string_utf8(env, "NativeGlobalSearchPage", NAPI_AUTO_LENGTH, &name);

    napi_status asyncStatus = napi_create_async_work(env, nullptr, name,
        SearchExecuteCb, SearchCompleteCb, asyncData, &asyncData->work);
    if (asyncStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env,
            "{\"success\":false,\"errorCode\":\"ASYNC_CREATE_FAILED\",\"userMessage\":\"Failed to create async work\"}",
            NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        SearchAsyncArgsFree(asyncData);
        delete asyncData;
        return promise;
    }

    napi_status queueStatus = napi_queue_async_work(env, asyncData->work);
    if (queueStatus != napi_ok) {
        napi_value result;
        napi_create_string_utf8(env,
            "{\"success\":false,\"errorCode\":\"ASYNC_QUEUE_FAILED\",\"userMessage\":\"Failed to queue async work\"}",
            NAPI_AUTO_LENGTH, &result);
        napi_resolve_deferred(env, deferred, result);
        SearchAsyncArgsFree(asyncData);
        napi_delete_async_work(env, asyncData->work);
        delete asyncData;
    }

    return promise;
}

// ── Global search property descriptors ──

napi_property_descriptor* getSearchDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeGlobalSearchPage", nullptr, NativeGlobalSearchPage, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
