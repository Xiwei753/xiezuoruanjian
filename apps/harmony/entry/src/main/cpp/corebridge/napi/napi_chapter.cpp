// ── Chapter & RecentEdits NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

// ── Chapter ──

static napi_value NativeListChapters(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    if (argc < 2 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)) {
        delete[] project_id;
        delete[] volume_id;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_list_chapters(project_id, volume_id));
    delete[] project_id;
    delete[] volume_id;
    return result;
}

static napi_value NativeCreateChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* name = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &name)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] name;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_create_chapter(project_id, volume_id, name));
    delete[] project_id;
    delete[] volume_id;
    delete[] name;
    return result;
}

static napi_value NativeOpenChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* chapter_id = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &chapter_id)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_open_chapter(project_id, volume_id, chapter_id));
    delete[] project_id;
    delete[] volume_id;
    delete[] chapter_id;
    return result;
}

static napi_value NativeSaveChapter(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* chapter_id = nullptr;
    // Issue #1069 评论 4：正文走 helper 的已初始化、NUL 结尾 buffer；两次
    // napi_get_value_string_utf8 的 status/分配失败都统一转 INVALID_ARGUMENT，
    // 绝不再把未初始化字节交给 writer_core_save_chapter。
    char* content = nullptr;
    if (argc < 4 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &chapter_id)
        || !napi_arg_get_string(env, args[3], &content)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        delete[] content;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_save_chapter(project_id, volume_id, chapter_id, content));
    delete[] project_id;
    delete[] volume_id;
    delete[] chapter_id;
    delete[] content;
    return result;
}

static napi_value NativeSaveChapterWithOptions(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* chapter_id = nullptr;
    char* content = nullptr;
    if (argc < 4 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &chapter_id)
        || !napi_arg_get_string(env, args[3], &content)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        delete[] content;
        return napi_arg_invalid_result(env);
    }

    // Issue #1054：空覆盖意图必须由调用方显式给出布尔值；缺省按 false，
    // 保持 Core 对意外空覆盖的拒绝。给了但不是 bool 时不能当 false 继续。
    bool allow_empty_overwrite = false;
    if (argc >= 5 && !napi_arg_get_bool(env, args[4], &allow_empty_overwrite)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        delete[] content;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_save_chapter_with_options(project_id, volume_id, chapter_id, content,
                                              allow_empty_overwrite));
    delete[] project_id;
    delete[] volume_id;
    delete[] chapter_id;
    delete[] content;
    return result;
}

static char* SaveChapterVersionedJsonWorker(const char* input) {
    return writer_core_save_chapter_versioned_json(input);
}

static napi_value NativeSaveChapterVersioned(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    // Issue #1069 评论 4：版本化保存的 JSON payload 同样走统一字符串读取，
    // 非字符串/内嵌 NUL/分配失败直接 INVALID_ARGUMENT，不把空串送进异步工作。
    char* input = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &input)) {
        return napi_arg_invalid_result(env);
    }
    napi_value result = NapiJsonAsyncSubmit(env, "saveChapterVersioned", input,
                                            SaveChapterVersionedJsonWorker);
    delete[] input;
    return result;
}

// Issue #1063：只写编辑会话冲突材料，不发布正文、不做版本 CAS。
static char* WriteEditorConflictMaterialJsonWorker(const char* input) {
    return writer_core_write_editor_conflict_material_json(input);
}

static napi_value NativeWriteEditorConflictMaterial(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* input = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &input)) {
        return napi_arg_invalid_result(env);
    }
    napi_value result = NapiJsonAsyncSubmit(env, "writeEditorConflictMaterial", input,
                                            WriteEditorConflictMaterialJsonWorker);
    delete[] input;
    return result;
}

static napi_value NativeClearChapterVersioned(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* input = nullptr;
    if (argc < 1 || !napi_arg_get_string(env, args[0], &input)) {
        return napi_arg_invalid_result(env);
    }
    napi_value result = NapiJsonAsyncSubmit(env, "clearChapterVersioned", input,
                                            SaveChapterVersionedJsonWorker);
    delete[] input;
    return result;
}

static napi_value NativeRenameChapter(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* chapter_id = nullptr;
    char* new_name = nullptr;
    if (argc < 4 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &chapter_id)
        || !napi_arg_get_string(env, args[3], &new_name)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        delete[] new_name;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_rename_chapter(project_id, volume_id, chapter_id, new_name));
    delete[] project_id;
    delete[] volume_id;
    delete[] chapter_id;
    delete[] new_name;
    return result;
}

static napi_value NativeDeleteChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* chapter_id = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &chapter_id)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_delete_chapter(project_id, volume_id, chapter_id));
    delete[] project_id;
    delete[] volume_id;
    delete[] chapter_id;
    return result;
}

static napi_value NativeReorderChapters(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* json = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &json)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] json;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_reorder_chapters(project_id, volume_id, json));
    delete[] project_id;
    delete[] volume_id;
    delete[] json;
    return result;
}

static napi_value NativeClearChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* project_id = nullptr;
    char* volume_id = nullptr;
    char* chapter_id = nullptr;
    if (argc < 3 || !napi_arg_get_string(env, args[0], &project_id)
        || !napi_arg_get_string(env, args[1], &volume_id)
        || !napi_arg_get_string(env, args[2], &chapter_id)) {
        delete[] project_id;
        delete[] volume_id;
        delete[] chapter_id;
        return napi_arg_invalid_result(env);
    }

    napi_value result = ReturnJsonString(env, writer_core_clear_chapter(project_id, volume_id, chapter_id));
    delete[] project_id;
    delete[] volume_id;
    delete[] chapter_id;
    return result;
}

// ── Recent edits ──

static napi_value NativeGetRecentEdits(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_recent_edits());
}

// ── Chapter property descriptors ──

napi_property_descriptor* getChapterDescriptors(size_t* count) {
    static napi_property_descriptor desc[] = {
        {"nativeListChapters", nullptr, NativeListChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateChapter", nullptr, NativeCreateChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeOpenChapter", nullptr, NativeOpenChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveChapter", nullptr, NativeSaveChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveChapterWithOptions", nullptr, NativeSaveChapterWithOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveChapterVersioned", nullptr, NativeSaveChapterVersioned, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearChapterVersioned", nullptr, NativeClearChapterVersioned, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameChapter", nullptr, NativeRenameChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteChapter", nullptr, NativeDeleteChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeReorderChapters", nullptr, NativeReorderChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearChapter", nullptr, NativeClearChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetRecentEdits", nullptr, NativeGetRecentEdits, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeWriteEditorConflictMaterial", nullptr, NativeWriteEditorConflictMaterial, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
