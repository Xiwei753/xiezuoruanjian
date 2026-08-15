// ── Chapter & RecentEdits NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString and writer_core_bridge.h to be available.

// ── Chapter ──

static napi_value NativeListChapters(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_list_chapters(project_id, volume_id));
}

static napi_value NativeCreateChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char name[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    }
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], name, sizeof(name), nullptr);
    }

    return ReturnJsonString(env, writer_core_create_chapter(project_id, volume_id, name));
}

static napi_value NativeOpenChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    }
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);
    }

    return ReturnJsonString(env, writer_core_open_chapter(project_id, volume_id, chapter_id));
}

static napi_value NativeSaveChapter(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    if (argc >= 1) {
        napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    }
    if (argc >= 2) {
        napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    }
    if (argc >= 3) {
        napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);
    }

    // Chapter content can be large, use dynamic allocation
    char* content = nullptr;
    if (argc >= 4) {
        size_t content_len = 0;
        napi_get_value_string_utf8(env, args[3], nullptr, 0, &content_len);
        content = new char[content_len + 1];
        napi_get_value_string_utf8(env, args[3], content, content_len + 1, &content_len);
    } else {
        content = new char[1];
        content[0] = '\0';
    }

    napi_value result = ReturnJsonString(env, writer_core_save_chapter(project_id, volume_id, chapter_id, content));
    delete[] content;
    return result;
}

static napi_value NativeRenameChapter(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    char new_name[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);
    if (argc >= 4) napi_get_value_string_utf8(env, args[3], new_name, sizeof(new_name), nullptr);

    return ReturnJsonString(env, writer_core_rename_chapter(project_id, volume_id, chapter_id, new_name));
}

static napi_value NativeDeleteChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);

    return ReturnJsonString(env, writer_core_delete_chapter(project_id, volume_id, chapter_id));
}

static napi_value NativeReorderChapters(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);

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

    napi_value result = ReturnJsonString(env, writer_core_reorder_chapters(project_id, volume_id, json));
    delete[] json;
    return result;
}

static napi_value NativeClearChapter(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    if (argc >= 1) napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    if (argc >= 2) napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    if (argc >= 3) napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);

    return ReturnJsonString(env, writer_core_clear_chapter(project_id, volume_id, chapter_id));
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
        {"nativeRenameChapter", nullptr, NativeRenameChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteChapter", nullptr, NativeDeleteChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeReorderChapters", nullptr, NativeReorderChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearChapter", nullptr, NativeClearChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetRecentEdits", nullptr, NativeGetRecentEdits, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
