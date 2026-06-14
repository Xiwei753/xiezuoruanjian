#include <napi/native_api.h>
#include <hilog/log.h>
#include "writer_core_bridge.h"

#undef LOG_DOMAIN
#undef LOG_TAG
#define LOG_DOMAIN 0xFF00
#define LOG_TAG "WriterCoreNapi"

static napi_value NativeInit(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        napi_throw_error(env, nullptr, "Expected 1 argument: path");
        return nullptr;
    }

    char path[1024] = {0};
    size_t path_len = 0;
    napi_get_value_string_utf8(env, args[0], path, sizeof(path), &path_len);

    int32_t result = writer_core_init(path);

    napi_value ret;
    napi_create_int32(env, result, &ret);
    return ret;
}

static napi_value NativeGetLoadStatus(napi_env env, napi_callback_info info) {
    char* status = writer_core_get_load_status();
    if (status == nullptr) {
        napi_value null_val;
        napi_get_null(env, &null_val);
        return null_val;
    }

    napi_value result;
    napi_create_string_utf8(env, status, strlen(status), &result);
    writer_core_free_string(status);
    return result;
}

static napi_value NativeCalculateWordCount(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        napi_throw_error(env, nullptr, "Expected 1 argument: text");
        return nullptr;
    }

    char text[65536] = {0};
    size_t text_len = 0;
    napi_get_value_string_utf8(env, args[0], text, sizeof(text), &text_len);

    int32_t count = writer_core_calculate_word_count(text);

    napi_value result;
    napi_create_int32(env, count, &result);
    return result;
}

static napi_value Init(napi_env env, napi_value exports) {
    napi_property_descriptor desc[] = {
        {"nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLoadStatus", nullptr, NativeGetLoadStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCalculateWordCount", nullptr, NativeCalculateWordCount, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    napi_define_properties(env, exports, sizeof(desc) / sizeof(desc[0]), desc);
    return exports;
}

EXTERN_C_START
static napi_module g_module = {
    .nm_version = 1,
    .nm_flags = 0,
    .nm_filename = nullptr,
    .nm_register_func = Init,
    .nm_modname = "writer_core",
    .nm_priv = nullptr,
    .reserved = {0},
};

__attribute__((constructor)) void RegisterModule(void) {
    napi_module_register(&g_module);
}
EXTERN_C_END
