// ── Generic JSON async-work helper ──
// Included by napi_settings.cpp / napi_sync.cpp (themselves included by napi_init.cpp).
//
// Local settings/sync config I/O used to call writer_core_* directly on the ArkTS
// main thread. This helper moves those C ABI calls to Node-API async work:
//   JS handler: only read/copy arguments, create promise + async work, queue it;
//   execute callback: only call the Rust C ABI, never touch napi_env/napi_value;
//   complete callback: back on the ArkTS thread, copy the JSON result into a NAPI
//   string, resolve the deferred, then free the Rust string exactly once.
#ifndef CORE_BRIDGE_NAPI_JSON_ASYNC_H
#define CORE_BRIDGE_NAPI_JSON_ASYNC_H

#include <cstring>
#include <new>
#include <string>

using NapiJsonAsyncExecuteFn = char* (*)(const char* input);

struct NapiJsonAsyncData {
    napi_async_work work = nullptr;
    napi_deferred deferred = nullptr;
    std::string input;
    NapiJsonAsyncExecuteFn execute = nullptr;
    char* result_json = nullptr;
};

static void NapiJsonAsyncResolveEnvelope(napi_env env, napi_deferred deferred,
                                         const char* code, const char* message) {
    std::string envelope = "{\"success\":false,\"errorCode\":\"";
    envelope += code;
    envelope += "\",\"userMessage\":\"";
    envelope += message;
    envelope += "\"}";
    napi_value result = nullptr;
    if (napi_create_string_utf8(env, envelope.c_str(), envelope.size(), &result) != napi_ok ||
        result == nullptr) {
        napi_get_undefined(env, &result);
    }
    napi_resolve_deferred(env, deferred, result);
}

// Execute callback runs on a Node-API worker thread: only touch owned C++ data and
// the Rust C ABI. Do not read/write any napi_env/napi_value here.
static void NapiJsonAsyncExecuteCb(napi_env env, void* data) {
    (void)env;
    auto* asyncData = static_cast<NapiJsonAsyncData*>(data);
    asyncData->result_json = asyncData->execute(asyncData->input.c_str());
}

static void NapiJsonAsyncCompleteCb(napi_env env, napi_status status, void* data) {
    auto* asyncData = static_cast<NapiJsonAsyncData*>(data);

    if (status != napi_ok) {
        napi_value error = nullptr;
        if (napi_create_string_utf8(env, "async JSON work failed", NAPI_AUTO_LENGTH,
                                    &error) != napi_ok ||
            error == nullptr) {
            napi_get_undefined(env, &error);
        }
        napi_reject_deferred(env, asyncData->deferred, error);
    } else if (asyncData->result_json != nullptr) {
        napi_value result = nullptr;
        if (napi_create_string_utf8(env, asyncData->result_json,
                                    strlen(asyncData->result_json), &result) != napi_ok ||
            result == nullptr) {
            NapiJsonAsyncResolveEnvelope(env, asyncData->deferred, "NATIVE_STRING_FAILED",
                                         "Failed to create native result string");
        } else {
            napi_resolve_deferred(env, asyncData->deferred, result);
        }
    } else {
        NapiJsonAsyncResolveEnvelope(env, asyncData->deferred, "NULL_RESULT",
                                     "Native operation returned no result");
    }

    // The Rust-allocated result is owned by this callback and freed exactly once.
    if (asyncData->result_json != nullptr) {
        writer_core_free_string(asyncData->result_json);
        asyncData->result_json = nullptr;
    }
    if (asyncData->work != nullptr) {
        napi_delete_async_work(env, asyncData->work);
    }
    delete asyncData;
}

// Copy an optional string argument into owned storage on the JS thread. A missing
// or non-string argument leaves `out` empty; the Rust FFI reports INVALID_ARGUMENT.
static void NapiJsonAsyncReadArg(napi_env env, napi_callback_info info, std::string* out) {
    if (out == nullptr) {
        return;
    }
    size_t argc = 1;
    napi_value args[1];
    if (napi_get_cb_info(env, info, &argc, args, nullptr, nullptr) != napi_ok || argc < 1) {
        return;
    }
    size_t len = 0;
    if (napi_get_value_string_utf8(env, args[0], nullptr, 0, &len) != napi_ok) {
        return;
    }
    out->resize(len + 1);
    if (napi_get_value_string_utf8(env, args[0], out->data(), out->size(), &len) == napi_ok) {
        out->resize(len);
    } else {
        out->clear();
    }
}

// Create and queue an async work item, returning its promise. All allocation,
// create/queue failure and completion paths release the copied input and work.
static napi_value NapiJsonAsyncSubmit(napi_env env, const char* workName,
                                      const char* input, NapiJsonAsyncExecuteFn execute) {
    napi_deferred deferred;
    napi_value promise;
    if (napi_create_promise(env, &deferred, &promise) != napi_ok) {
        return nullptr;
    }

    auto* asyncData = new (std::nothrow) NapiJsonAsyncData();
    if (asyncData == nullptr) {
        NapiJsonAsyncResolveEnvelope(env, deferred, "ASYNC_CREATE_FAILED",
                                     "Failed to allocate async work");
        return promise;
    }
    asyncData->deferred = deferred;
    asyncData->execute = execute;
    if (input != nullptr) {
        asyncData->input = input;
    }

    napi_value name;
    if (napi_create_string_utf8(env, workName, NAPI_AUTO_LENGTH, &name) != napi_ok) {
        NapiJsonAsyncResolveEnvelope(env, deferred, "ASYNC_CREATE_FAILED",
                                     "Failed to create async work name");
        delete asyncData;
        return promise;
    }

    if (napi_create_async_work(env, nullptr, name, NapiJsonAsyncExecuteCb,
                               NapiJsonAsyncCompleteCb, asyncData,
                               &asyncData->work) != napi_ok) {
        NapiJsonAsyncResolveEnvelope(env, deferred, "ASYNC_CREATE_FAILED",
                                     "Failed to create async work");
        delete asyncData;
        return promise;
    }

    if (napi_queue_async_work(env, asyncData->work) != napi_ok) {
        NapiJsonAsyncResolveEnvelope(env, deferred, "ASYNC_QUEUE_FAILED",
                                     "Failed to queue async work");
        napi_delete_async_work(env, asyncData->work);
        delete asyncData;
        return promise;
    }

    return promise;
}

#endif  // CORE_BRIDGE_NAPI_JSON_ASYNC_H
