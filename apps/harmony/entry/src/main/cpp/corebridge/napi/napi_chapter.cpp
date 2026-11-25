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

static napi_value NativeSaveChapterWithOptions(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5];
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

    // Chapter content can be large, use dynamic allocation.
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

    // Issue #1054：空覆盖意图必须由调用方显式给出布尔值；缺省按 false，
    // 保持 Core 对意外空覆盖的拒绝。
    bool allow_empty_overwrite = false;
    if (argc >= 5) {
        napi_get_value_bool(env, args[4], &allow_empty_overwrite);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_save_chapter_with_options(project_id, volume_id, chapter_id, content,
                                              allow_empty_overwrite));
    delete[] content;
    return result;
}

static char* ReadDynamicString(napi_env env, napi_value value) {
    size_t len = 0;
    napi_get_value_string_utf8(env, value, nullptr, 0, &len);
    char* out = new char[len + 1];
    napi_get_value_string_utf8(env, value, out, len + 1, &len);
    return out;
}

static napi_value NativeSaveChapterVersioned(napi_env env, napi_callback_info info) {
    size_t argc = 9;
    napi_value args[9];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc != 9) {
        napi_throw_type_error(env, nullptr,
            "saveChapterVersioned expects 9 arguments (pid, vid, cid, text, generation, revision, sha256, base, allowEmpty)");
        return nullptr;
    }

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    char expected_generation[128] = {0};
    char expected_sha256[128] = {0};
    napi_valuetype type = napi_undefined;
    for (size_t index = 0; index < 9; ++index) {
        napi_typeof(env, args[index], &type);
        bool expected_string = index <= 4 || index == 6 || index == 7;
        if ((expected_string && type != napi_string)
            || (index == 5 && type != napi_number)
            || (index == 8 && type != napi_boolean)) {
            napi_throw_type_error(env, nullptr, "saveChapterVersioned argument type mismatch");
            return nullptr;
        }
    }
    napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);
    napi_get_value_string_utf8(env, args[4], expected_generation, sizeof(expected_generation), nullptr);
    napi_get_value_string_utf8(env, args[6], expected_sha256, sizeof(expected_sha256), nullptr);
    int64_t expected_revision = 0;
    napi_get_value_int64(env, args[5], &expected_revision);
    bool allow_empty_overwrite = false;
    napi_get_value_bool(env, args[8], &allow_empty_overwrite);

    char* content = ReadDynamicString(env, args[3]);
    char* base_content = ReadDynamicString(env, args[7]);

    napi_value result = ReturnJsonString(env,
        writer_core_save_chapter_versioned(project_id, volume_id, chapter_id, content,
            expected_generation, expected_revision, expected_sha256, base_content,
            allow_empty_overwrite));
    delete[] content;
    delete[] base_content;
    return result;
}

static napi_value NativeClearChapterVersioned(napi_env env, napi_callback_info info) {
    size_t argc = 7;
    napi_value args[7];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc != 7) {
        napi_throw_type_error(env, nullptr,
            "clearChapterVersioned expects 7 arguments (pid, vid, cid, generation, revision, sha256, base)");
        return nullptr;
    }

    char project_id[256] = {0};
    char volume_id[256] = {0};
    char chapter_id[256] = {0};
    char expected_generation[128] = {0};
    char expected_sha256[128] = {0};
    napi_valuetype type = napi_undefined;
    for (size_t index = 0; index < 7; ++index) {
        napi_typeof(env, args[index], &type);
        bool expected_string = index <= 3 || index == 5 || index == 6;
        if ((expected_string && type != napi_string)
            || (index == 4 && type != napi_number)) {
            napi_throw_type_error(env, nullptr, "clearChapterVersioned argument type mismatch");
            return nullptr;
        }
    }
    napi_get_value_string_utf8(env, args[0], project_id, sizeof(project_id), nullptr);
    napi_get_value_string_utf8(env, args[1], volume_id, sizeof(volume_id), nullptr);
    napi_get_value_string_utf8(env, args[2], chapter_id, sizeof(chapter_id), nullptr);
    napi_get_value_string_utf8(env, args[3], expected_generation, sizeof(expected_generation), nullptr);
    napi_get_value_string_utf8(env, args[5], expected_sha256, sizeof(expected_sha256), nullptr);
    int64_t expected_revision = 0;
    napi_get_value_int64(env, args[4], &expected_revision);
    char* base_content = ReadDynamicString(env, args[6]);

    napi_value result = ReturnJsonString(env,
        writer_core_clear_chapter_versioned(project_id, volume_id, chapter_id,
            expected_generation, expected_revision, expected_sha256, base_content));
    delete[] base_content;
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
        {"nativeSaveChapterWithOptions", nullptr, NativeSaveChapterWithOptions, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveChapterVersioned", nullptr, NativeSaveChapterVersioned, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearChapterVersioned", nullptr, NativeClearChapterVersioned, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameChapter", nullptr, NativeRenameChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteChapter", nullptr, NativeDeleteChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeReorderChapters", nullptr, NativeReorderChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearChapter", nullptr, NativeClearChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetRecentEdits", nullptr, NativeGetRecentEdits, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(desc) / sizeof(desc[0]);
    return desc;
}
