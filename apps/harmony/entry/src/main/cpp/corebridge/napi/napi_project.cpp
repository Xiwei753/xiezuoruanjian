// ── Project & Volume NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

// ── Project ──

static napi_value NativeListProjects(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_projects());
}

static napi_value NativeGetProjectTree(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_get_project_tree(project_id));
}

static napi_value NativeCreateProject(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char name[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], name, sizeof(name), nullptr);
    }

    return ReturnJsonString(env, writer_core_create_project(name));
}

static napi_value NativeRenameProject(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char new_name[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], new_name, sizeof(new_name), nullptr);

    return ReturnJsonString(env, writer_core_rename_project(project_id, new_name));
}

static napi_value NativeDeleteProject(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);

    return ReturnJsonString(env, writer_core_delete_project(project_id));
}

static napi_value NativeGetProjectStats(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);

    return ReturnJsonString(env, writer_core_get_project_stats(project_id));
}

// ── Volume ──

static napi_value NativeListVolumes(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_list_volumes(project_id));
}

static napi_value NativeCreateVolume(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char name[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], name, sizeof(name), nullptr);
    }

    return ReturnJsonString(env, writer_core_create_volume(project_id, name));
}

static napi_value NativeRenameVolume(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char new_name[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], new_name, sizeof(new_name), nullptr);

    return ReturnJsonString(env, writer_core_rename_volume(project_id, volume_id, new_name));
}

static napi_value NativeDeleteVolume(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);

    return ReturnJsonString(env, writer_core_delete_volume(project_id, volume_id));
}

static napi_value NativeReorderVolumes(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);

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

    napi_value result = ReturnJsonString(env, writer_core_reorder_volumes(project_id, json));
    delete[] json;
    return result;
}

// ── Project/Volume property descriptors ──

napi_property_descriptor* getProjectDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeListProjects", nullptr, NativeListProjects, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetProjectTree", nullptr, NativeGetProjectTree, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateProject", nullptr, NativeCreateProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameProject", nullptr, NativeRenameProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteProject", nullptr, NativeDeleteProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetProjectStats", nullptr, NativeGetProjectStats, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListVolumes", nullptr, NativeListVolumes, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateVolume", nullptr, NativeCreateVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameVolume", nullptr, NativeRenameVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteVolume", nullptr, NativeDeleteVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeReorderVolumes", nullptr, NativeReorderVolumes, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
