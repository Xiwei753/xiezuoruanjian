#include <cstring>
#include <napi/native_api.h>
#include <hilog/log.h>
#include "writer_core_bridge.h"

#undef LOG_DOMAIN
#undef LOG_TAG
#define LOG_DOMAIN 0xFF00
#define LOG_TAG "WriterCoreNapi"

static char* dup_napi_string(napi_env env, napi_value value, char* buf, size_t buf_size) {
    size_t len = 0;
    napi_get_value_string_utf8(env, value, buf, buf_size, &len);
    return buf;
}

// ── Core lifecycle ──

static napi_value NativeInit(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    if (argc < 1) {
        napi_throw_error(env, nullptr, "Expected 1 argument: path");
        return nullptr;
    }

    char path[2048] = {0};
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

    size_t text_len = 0;
    napi_get_value_string_utf8(env, args[0], nullptr, 0, &text_len);
    char* text = new char[text_len + 1];
    napi_get_value_string_utf8(env, args[0], text, text_len + 1, &text_len);

    int32_t count = writer_core_calculate_word_count(text);
    delete[] text;

    napi_value result;
    napi_create_int32(env, count, &result);
    return result;
}

// ── JSON-returning helpers ──
// All return a JSON string that the ArkTS side parses into ResultEnvelope<T>.

static napi_value ReturnJsonString(napi_env env, char* json) {
    if (json == nullptr) {
        napi_value empty;
        napi_create_string_utf8(env, "{\"success\":false,\"errorCode\":\"NULL_RESULT\"}", 47, &empty);
        return empty;
    }
    napi_value result;
    napi_create_string_utf8(env, json, strlen(json), &result);
    writer_core_free_string(json);
    return result;
}

// ── Workspace ──

static napi_value NativeValidateWorkspace(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_validate_workspace());
}

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

// ── Recent edits ──

static napi_value NativeGetRecentEdits(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_recent_edits());
}

// ── Settings ──

static napi_value NativeLoadLocalSettings(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_load_local_settings());
}

static napi_value NativeSaveLocalSettings(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_local_settings(json));
    delete[] json;
    return result;
}

// ── Module registration ──

static napi_value Init(napi_env env, napi_value exports) {
    napi_property_descriptor desc[] = {
        // Lifecycle
        {"nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLoadStatus", nullptr, NativeGetLoadStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCalculateWordCount", nullptr, NativeCalculateWordCount, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Workspace
        {"nativeValidateWorkspace", nullptr, NativeValidateWorkspace, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Project
        {"nativeListProjects", nullptr, NativeListProjects, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetProjectTree", nullptr, NativeGetProjectTree, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateProject", nullptr, NativeCreateProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Volume
        {"nativeListVolumes", nullptr, NativeListVolumes, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateVolume", nullptr, NativeCreateVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Chapter
        {"nativeListChapters", nullptr, NativeListChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateChapter", nullptr, NativeCreateChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeOpenChapter", nullptr, NativeOpenChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveChapter", nullptr, NativeSaveChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Recent edits
        {"nativeGetRecentEdits", nullptr, NativeGetRecentEdits, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Settings
        {"nativeLoadLocalSettings", nullptr, NativeLoadLocalSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveLocalSettings", nullptr, NativeSaveLocalSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
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
