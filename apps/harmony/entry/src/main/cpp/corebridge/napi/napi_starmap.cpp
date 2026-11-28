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

    char* project_id = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &project_id)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_list_starmaps_for_project(project_id));
    delete[] project_id;
    return result;
}

static napi_value NativeGetStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &starmap_id)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_get_starmap(starmap_id));
    delete[] starmap_id;
    return result;
}

static napi_value NativeGetStarMapGraph(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &starmap_id)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_get_starmap_graph(starmap_id));
    delete[] starmap_id;
    return result;
}

static napi_value NativeCloseStarMapStore(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &starmap_id)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_close_starmap_store(starmap_id));
    delete[] starmap_id;
    return result;
}

static napi_value NativeCreateStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* title = nullptr;
    char* description = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &title)
        || !napi_arg_get_string(env, args[1], &description)) {
        delete[] title;
        delete[] description;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_create_starmap(title, description));
    delete[] title;
    delete[] description;
    return result;
}

static napi_value NativeDeleteStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &starmap_id)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_delete_starmap(starmap_id));
    delete[] starmap_id;
    return result;
}

static napi_value NativeRenameStarMap(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* new_title = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &new_title)) {
        delete[] starmap_id;
        delete[] new_title;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_rename_starmap(starmap_id, new_title));
    delete[] starmap_id;
    delete[] new_title;
    return result;
}

static napi_value NativeUpdateStarMapNode(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* node_id = nullptr;
    char* json = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &node_id)
        || !napi_arg_get_string(env, args[2], &json)) {
        delete[] starmap_id;
        delete[] node_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_update_starmap_node(starmap_id, node_id, json));
    delete[] starmap_id;
    delete[] node_id;
    delete[] json;
    return result;
}

// Issue #1060：只改共享素材本体（标题/正文/样式/portal/anchors），
// expectedDefinitionRevision 在 patch_json 内；不带 position。
static napi_value NativeUpdateStarMapNodeDefinition(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* node_id = nullptr;
    char* json = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &node_id)
        || !napi_arg_get_string(env, args[2], &json)) {
        delete[] starmap_id;
        delete[] node_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_update_starmap_node_definition(starmap_id, node_id, json));
    delete[] starmap_id;
    delete[] node_id;
    delete[] json;
    return result;
}

// Issue #1060：只移动画布实例；expectedInstanceRevision 来自 node.updatedAt，
// 以十进制字符串下发（空串 = 不校验，由 Core 语义决定）。
static napi_value NativeMoveStarMapNode(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* node_id = nullptr;
    char* json = nullptr;
    char* expected_revision = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &node_id)
        || !napi_arg_get_string(env, args[2], &json)) {
        delete[] starmap_id;
        delete[] node_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }
    if (argc >= 4 && !napi_arg_get_string(env, args[3], &expected_revision)) {
        delete[] starmap_id;
        delete[] node_id;
        delete[] json;
        delete[] expected_revision;
        return napi_arg_invalid_result(env);
    }

    const char* revision = (expected_revision != nullptr && expected_revision[0] != '\0')
        ? expected_revision : nullptr;
    napi_value result = ReturnJsonString(env,
        writer_core_move_starmap_node(starmap_id, node_id, json, revision));
    delete[] starmap_id;
    delete[] node_id;
    delete[] json;
    delete[] expected_revision;
    return result;
}

static napi_value NativeAddStarMapEdge(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* json = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &json)) {
        delete[] starmap_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_edge(starmap_id, json));
    delete[] starmap_id;
    delete[] json;
    return result;
}

static napi_value NativeAddStarMapNode(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* json = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &json)) {
        delete[] starmap_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    double x = 0.0;
    double y = 0.0;
    if (argc >= 3) napi_get_value_double(env, args[2], &x);
    if (argc >= 4) napi_get_value_double(env, args[3], &y);

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_node(starmap_id, json, (float)x, (float)y));
    delete[] starmap_id;
    delete[] json;
    return result;
}

static napi_value NativeAddStarMapEmbed(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* json = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &json)) {
        delete[] starmap_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_embed(starmap_id, json));
    delete[] starmap_id;
    delete[] json;
    return result;
}

static napi_value NativeCreateStarMapChildEmbed(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* host_starmap_id = nullptr;
    char* title = nullptr;
    char* json = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &host_starmap_id)
        || !napi_arg_get_string(env, args[1], &title)
        || !napi_arg_get_string(env, args[2], &json)) {
        delete[] host_starmap_id;
        delete[] title;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_create_starmap_child_embed(host_starmap_id, title, json));
    delete[] host_starmap_id;
    delete[] title;
    delete[] json;
    return result;
}

static napi_value NativeUpdateStarMapEmbed(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* instance_id = nullptr;
    char* json = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &instance_id)
        || !napi_arg_get_string(env, args[2], &json)) {
        delete[] starmap_id;
        delete[] instance_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_update_starmap_embed(starmap_id, instance_id, json));
    delete[] starmap_id;
    delete[] instance_id;
    delete[] json;
    return result;
}

static napi_value NativeResolveStarMapPath(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* root_starmap_id = nullptr;
    char* json = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &root_starmap_id)
        || !napi_arg_get_string(env, args[1], &json)) {
        delete[] root_starmap_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_resolve_starmap_path(root_starmap_id, json));
    delete[] root_starmap_id;
    delete[] json;
    return result;
}

static napi_value NativePlanCrossLayerConnection(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* json = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &json)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_plan_cross_layer_connection(json));
    delete[] json;
    return result;
}

static napi_value NativeListStarMapLibrary(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* scope_json = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &scope_json)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_list_starmap_library_json(scope_json));
    delete[] scope_json;
    return result;
}

static napi_value NativeCreateStarMapLibraryFolder(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* scope_json = nullptr;
    char* parent_id = nullptr;
    char* name = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &scope_json)
        || !napi_arg_get_string(env, args[1], &parent_id)
        || !napi_arg_get_string(env, args[2], &name)) {
        delete[] scope_json;
        delete[] parent_id;
        delete[] name;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_create_starmap_library_folder_json(scope_json, parent_id, name));
    delete[] scope_json;
    delete[] parent_id;
    delete[] name;
    return result;
}

static napi_value NativeMoveStarMapLibraryEntry(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* entry_id = nullptr;
    char* scope_json = nullptr;
    char* parent_id = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &entry_id)
        || !napi_arg_get_string(env, args[1], &scope_json)
        || !napi_arg_get_string(env, args[2], &parent_id)) {
        delete[] entry_id;
        delete[] scope_json;
        delete[] parent_id;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_move_starmap_library_entry_json(entry_id, scope_json, parent_id));
    delete[] entry_id;
    delete[] scope_json;
    delete[] parent_id;
    return result;
}

static napi_value NativeAddStarMapNodeReference(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* asset_id = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &asset_id)) {
        delete[] starmap_id;
        delete[] asset_id;
        return napi_arg_invalid_result(env);
    }

    double x = 0.0;
    double y = 0.0;
    if (argc >= 3) napi_get_value_double(env, args[2], &x);
    if (argc >= 4) napi_get_value_double(env, args[3], &y);

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_node_reference(starmap_id, asset_id, x, y));
    delete[] starmap_id;
    delete[] asset_id;
    return result;
}

static napi_value NativeAddStarMapReference(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* host_starmap_id = nullptr;
    char* target_starmap_id = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &host_starmap_id)
        || !napi_arg_get_string(env, args[1], &target_starmap_id)) {
        delete[] host_starmap_id;
        delete[] target_starmap_id;
        return napi_arg_invalid_result(env);
    }

    double x = 0.0;
    double y = 0.0;
    if (argc >= 3) napi_get_value_double(env, args[2], &x);
    if (argc >= 4) napi_get_value_double(env, args[3], &y);

    napi_value result = ReturnJsonString(env, writer_core_add_starmap_reference(host_starmap_id, target_starmap_id, x, y));
    delete[] host_starmap_id;
    delete[] target_starmap_id;
    return result;
}

static napi_value NativeCopyStarMapLibraryAsset(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* target_json = nullptr;
    char* scope_json = nullptr;
    char* title = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &target_json)
        || !napi_arg_get_string(env, args[1], &scope_json)
        || !napi_arg_get_string(env, args[2], &title)) {
        delete[] target_json;
        delete[] scope_json;
        delete[] title;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_copy_starmap_library_asset_json(target_json, scope_json, title));
    delete[] target_json;
    delete[] scope_json;
    delete[] title;
    return result;
}

static napi_value NativeListStarMapAssetReferences(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* target_json = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &target_json)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_list_starmap_asset_references_json(target_json));
    delete[] target_json;
    return result;
}

static napi_value NativeDeleteStarMapLibraryAsset(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* target_json = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &target_json)) {
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_delete_starmap_library_asset_json(target_json));
    delete[] target_json;
    return result;
}

static napi_value NativeRemoveStarMapInstance(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* starmap_id = nullptr;
    char* instance_id = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &starmap_id)
        || !napi_arg_get_string(env, args[1], &instance_id)) {
        delete[] starmap_id;
        delete[] instance_id;
        return napi_arg_invalid_result(env);
    }

    bool is_embed = false;
    if (argc >= 3 && !napi_arg_get_bool(env, args[2], &is_embed)) {
        delete[] starmap_id;
        delete[] instance_id;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_remove_starmap_instance(starmap_id, instance_id, is_embed));
    delete[] starmap_id;
    delete[] instance_id;
    return result;
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
        {"nativeUpdateStarMapNodeDefinition", nullptr, NativeUpdateStarMapNodeDefinition, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeMoveStarMapNode", nullptr, NativeMoveStarMapNode, nullptr, nullptr, nullptr, napi_default, nullptr},
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
