// ── StarMap NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

static napi_value NativeListStarMaps(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_starmaps());
}

static napi_value NativeListRootStarMaps(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_list_root_starmaps());
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

static napi_value NativeCloseStarMapStore(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);

    return ReturnJsonString(env, writer_core_close_starmap_store(starmap_id));
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

static napi_value NativeUpdateStarMapNode(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    char node_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], node_id, sizeof(node_id), nullptr);

    size_t json_len = 0;
    char* json = nullptr;
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], nullptr, 0, &json_len);
        json = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[2], json, json_len + 1, &json_len);
    } else {
        json = new char[1];
        json[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_update_starmap_node(starmap_id, node_id, json));
    delete[] json;
    return result;
}

static napi_value NativeAddStarMapEdge(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_edge(starmap_id, json));
    delete[] json;
    return result;
}

static napi_value NativeAddStarMapNode(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
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

    double x = 0.0;
    double y = 0.0;
    if (argc >= 3) napi_get_value_double(env, args[2], &x);
    if (argc >= 4) napi_get_value_double(env, args[3], &y);

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_node(starmap_id, json, (float)x, (float)y));
    delete[] json;
    return result;
}

static napi_value NativeAddStarMapEmbed(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_embed(starmap_id, json));
    delete[] json;
    return result;
}

static napi_value NativeCreateStarMapChildEmbed(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char host_starmap_id[256] = {0};
    char title[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], host_starmap_id, sizeof(host_starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], title, sizeof(title), nullptr);

    size_t json_len = 0;
    char* json = nullptr;
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], nullptr, 0, &json_len);
        json = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[2], json, json_len + 1, &json_len);
    } else {
        json = new char[1];
        json[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_create_starmap_child_embed(host_starmap_id, title, json));
    delete[] json;
    return result;
}

static napi_value NativeUpdateStarMapEmbed(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    char instance_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], instance_id, sizeof(instance_id), nullptr);

    size_t json_len = 0;
    char* json = nullptr;
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], nullptr, 0, &json_len);
        json = new char[json_len + 1];
        napi_get_value_string_utf8(env, args[2], json, json_len + 1, &json_len);
    } else {
        json = new char[1];
        json[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_update_starmap_embed(starmap_id, instance_id, json));
    delete[] json;
    return result;
}

static napi_value NativeResolveStarMapPath(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char root_starmap_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], root_starmap_id, sizeof(root_starmap_id), nullptr);

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

    napi_value result = ReturnJsonString(env, writer_core_resolve_starmap_path(root_starmap_id, json));
    delete[] json;
    return result;
}

static napi_value NativePlanCrossLayerConnection(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_plan_cross_layer_connection(json));
    delete[] json;
    return result;
}

static napi_value NativeListStarMapLibrary(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char scope_json[512] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], scope_json, sizeof(scope_json), nullptr);

    return ReturnJsonString(env, writer_core_list_starmap_library_json(scope_json));
}

static napi_value NativeCreateStarMapLibraryFolder(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char scope_json[512] = {0};
    char parent_id[256] = {0};
    char name[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], scope_json, sizeof(scope_json), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], parent_id, sizeof(parent_id), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], name, sizeof(name), nullptr);

    return ReturnJsonString(env, writer_core_create_starmap_library_folder_json(scope_json, parent_id, name));
}

static napi_value NativeMoveStarMapLibraryEntry(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char entry_id[256] = {0};
    char scope_json[512] = {0};
    char parent_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], entry_id, sizeof(entry_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], scope_json, sizeof(scope_json), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], parent_id, sizeof(parent_id), nullptr);

    return ReturnJsonString(env, writer_core_move_starmap_library_entry_json(entry_id, scope_json, parent_id));
}

static napi_value NativeAddStarMapNodeReference(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    char asset_id[256] = {0};
    double x = 0.0;
    double y = 0.0;
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], asset_id, sizeof(asset_id), nullptr);
    if (argc >= 3) napi_get_value_double(env, args[2], &x);
    if (argc >= 4) napi_get_value_double(env, args[3], &y);

    return ReturnJsonString(env, writer_core_add_starmap_node_reference(starmap_id, asset_id, x, y));
}

static napi_value NativeAddStarMapReference(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char host_starmap_id[256] = {0};
    char target_starmap_id[256] = {0};
    double x = 0.0;
    double y = 0.0;
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], host_starmap_id, sizeof(host_starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], target_starmap_id, sizeof(target_starmap_id), nullptr);
    if (argc >= 3) napi_get_value_double(env, args[2], &x);
    if (argc >= 4) napi_get_value_double(env, args[3], &y);

    return ReturnJsonString(env, writer_core_add_starmap_reference(host_starmap_id, target_starmap_id, x, y));
}

static napi_value NativeCopyStarMapLibraryAsset(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char target_json[512] = {0};
    char scope_json[512] = {0};
    char title[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], target_json, sizeof(target_json), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], scope_json, sizeof(scope_json), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], title, sizeof(title), nullptr);

    return ReturnJsonString(env, writer_core_copy_starmap_library_asset_json(target_json, scope_json, title));
}

static napi_value NativeListStarMapAssetReferences(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char target_json[512] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], target_json, sizeof(target_json), nullptr);

    return ReturnJsonString(env, writer_core_list_starmap_asset_references_json(target_json));
}

static napi_value NativeDeleteStarMapLibraryAsset(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char target_json[512] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], target_json, sizeof(target_json), nullptr);

    return ReturnJsonString(env, writer_core_delete_starmap_library_asset_json(target_json));
}

static napi_value NativeRemoveStarMapInstance(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char starmap_id[256] = {0};
    char instance_id[256] = {0};
    bool is_embed = false;
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], starmap_id, sizeof(starmap_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], instance_id, sizeof(instance_id), nullptr);
    if (argc >= 3) napi_get_value_bool(env, args[2], &is_embed);

    return ReturnJsonString(env, writer_core_remove_starmap_instance(starmap_id, instance_id, is_embed));
}

// ── StarMap property descriptors ──

napi_property_descriptor* getStarMapDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeListStarMaps", nullptr, NativeListStarMaps, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListRootStarMaps", nullptr, NativeListRootStarMaps, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListStarMapsForProject", nullptr, NativeListStarMapsForProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMap", nullptr, NativeGetStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMapGraph", nullptr, NativeGetStarMapGraph, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCloseStarMapStore", nullptr, NativeCloseStarMapStore, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeUpdateStarMapNode", nullptr, NativeUpdateStarMapNode, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeAddStarMapEdge", nullptr, NativeAddStarMapEdge, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeAddStarMapNode", nullptr, NativeAddStarMapNode, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeAddStarMapEmbed", nullptr, NativeAddStarMapEmbed, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateStarMapChildEmbed", nullptr, NativeCreateStarMapChildEmbed, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeUpdateStarMapEmbed", nullptr, NativeUpdateStarMapEmbed, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveStarMapPath", nullptr, NativeResolveStarMapPath, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativePlanCrossLayerConnection", nullptr, NativePlanCrossLayerConnection, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListStarMapLibrary", nullptr, NativeListStarMapLibrary, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateStarMapLibraryFolder", nullptr, NativeCreateStarMapLibraryFolder, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeMoveStarMapLibraryEntry", nullptr, NativeMoveStarMapLibraryEntry, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeAddStarMapNodeReference", nullptr, NativeAddStarMapNodeReference, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeAddStarMapReference", nullptr, NativeAddStarMapReference, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCopyStarMapLibraryAsset", nullptr, NativeCopyStarMapLibraryAsset, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListStarMapAssetReferences", nullptr, NativeListStarMapAssetReferences, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteStarMapLibraryAsset", nullptr, NativeDeleteStarMapLibraryAsset, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRemoveStarMapInstance", nullptr, NativeRemoveStarMapInstance, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateStarMap", nullptr, NativeCreateStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteStarMap", nullptr, NativeDeleteStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameStarMap", nullptr, NativeRenameStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
