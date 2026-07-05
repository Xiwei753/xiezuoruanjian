// ── Workspace & Location NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

// ── Workspace ──

static napi_value NativeValidateWorkspace(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_validate_workspace());
}

static napi_value NativeListWorkspaces(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_workspaces());
}

static napi_value NativeOpenWorkspace(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char path[2048] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], path, sizeof(path), nullptr);
    }

    return ReturnJsonString(env, writer_core_open_workspace(path));
}

static napi_value NativeGetWorkspaceState(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_workspace_state());
}

static napi_value NativeResolveChapterLocation(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char chapter_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], chapter_id, sizeof(chapter_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_resolve_chapter_location(chapter_id));
}

static napi_value NativeResolveVolumeLocation(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char volume_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], volume_id, sizeof(volume_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_resolve_volume_location(volume_id));
}

// ── Workspace property descriptors ──

napi_property_descriptor* getWorkspaceDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeValidateWorkspace", nullptr, NativeValidateWorkspace, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListWorkspaces", nullptr, NativeListWorkspaces, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeOpenWorkspace", nullptr, NativeOpenWorkspace, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetWorkspaceState", nullptr, NativeGetWorkspaceState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveChapterLocation", nullptr, NativeResolveChapterLocation, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveVolumeLocation", nullptr, NativeResolveVolumeLocation, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
