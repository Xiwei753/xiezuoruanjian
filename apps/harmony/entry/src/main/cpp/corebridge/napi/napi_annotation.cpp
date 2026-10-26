// ── Shared annotation NAPI handler ──
// Included by napi_init.cpp; arguments and DTOs remain camelCase JSON at this boundary.

static char* NapiAnnotationDupString(napi_env env, napi_value value) {
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

static napi_value NativeAnnotationCall(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2] = {nullptr};
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    char* operation = argc >= 1 ? NapiAnnotationDupString(env, args[0]) : nullptr;
    char* args_json = argc >= 2 ? NapiAnnotationDupString(env, args[1]) : nullptr;
    if (operation == nullptr || args_json == nullptr) {
        delete[] operation;
        delete[] args_json;
        napi_value error;
        napi_create_string_utf8(env,
            "{\"success\":false,\"errorCode\":\"INVALID_ARGUMENT\",\"userMessage\":\"Annotation operation and arguments are required\"}",
            NAPI_AUTO_LENGTH, &error);
        return error;
    }
    char* result_json = writer_core_annotation_call(operation, args_json);
    delete[] operation;
    delete[] args_json;
    return ReturnJsonString(env, result_json);
}

napi_property_descriptor* getAnnotationDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeAnnotationCall", nullptr, NativeAnnotationCall, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
