// ── StarMap NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

static napi_value NativeListStarMaps(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_starmaps());
}

static napi_value NativeListStarMapsForProject(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);

    return ReturnJsonString(env, writer_core_list_starmaps_for_project(project_id));
}

static napi_value NativeGetStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

    return ReturnJsonString(env, writer_core_get_starmap(starmap_id));
}

static napi_value NativeGetStarMapGraph(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

    return ReturnJsonString(env, writer_core_get_starmap_graph(starmap_id));
}

static napi_value NativeCreateStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char title[256] = {0};
    char description[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], title, sizeof(title), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], description, sizeof(description), nullptr);

    return ReturnJsonString(env, writer_core_create_starmap(title, description));
}

static napi_value NativeDeleteStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

    return ReturnJsonString(env, writer_core_delete_starmap(starmap_id));
}

static napi_value NativeRenameStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    char new_title[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], new_title, sizeof(new_title), nullptr);

    return ReturnJsonString(env, writer_core_rename_starmap(starmap_id, new_title));
}

static napi_value NativeGetStarMapMotionPolicy(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_starmap_motion_policy());
}

static napi_value NativeGetStarMapLayout(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

    return ReturnJsonString(env, writer_core_get_starmap_layout(starmap_id));
}

static napi_value NativeSaveStarMapLayout(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

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

    napi_value result = ReturnJsonString(env, writer_core_save_starmap_layout(starmap_id, json));
    delete[] json;
    return result;
}

static napi_value NativeSaveStarMapViewport(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

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

    napi_value result = ReturnJsonString(env, writer_core_save_starmap_viewport(starmap_id, json));
    delete[] json;
    return result;
}

static napi_value NativeComputeStarMapEdgeRenders(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_compute_starmap_edge_renders(json));
    delete[] json;
    return result;
}

// ── StarMap property descriptors ──

napi_property_descriptor* getStarMapDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeListStarMaps", nullptr, NativeListStarMaps, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListStarMapsForProject", nullptr, NativeListStarMapsForProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMap", nullptr, NativeGetStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMapGraph", nullptr, NativeGetStarMapGraph, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMapMotionPolicy", nullptr, NativeGetStarMapMotionPolicy, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMapLayout", nullptr, NativeGetStarMapLayout, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveStarMapLayout", nullptr, NativeSaveStarMapLayout, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveStarMapViewport", nullptr, NativeSaveStarMapViewport, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeComputeStarMapEdgeRenders", nullptr, NativeComputeStarMapEdgeRenders, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateStarMap", nullptr, NativeCreateStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteStarMap", nullptr, NativeDeleteStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameStarMap", nullptr, NativeRenameStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
