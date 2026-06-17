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
        OH_LOG_ERROR(LOG_APP, "NativeInit: expected 1 argument (path), got %{public}zu", argc);
        napi_throw_error(env, nullptr, "Expected 1 argument: path");
        return nullptr;
    }

    char path[2048] = {0};
    size_t path_len = 0;
    napi_get_value_string_utf8(env, args[0], path, sizeof(path), &path_len);

    OH_LOG_INFO(LOG_APP, "NativeInit: calling writer_core_init with path='%{public}s'", path);
    int32_t result = writer_core_init(path);
    OH_LOG_INFO(LOG_APP, "NativeInit: writer_core_init returned %{public}d", result);

    if (result != 0) {
        OH_LOG_ERROR(LOG_APP, "NativeInit: FAILED with code %{public}d (path='%{public}s')", result, path);
    }

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

static napi_value NativeGetLastError(napi_env env, napi_callback_info info) {
    char* err = writer_core_get_last_error();
    if (err == nullptr) {
        napi_value empty;
        napi_create_string_utf8(env, "", 0, &empty);
        return empty;
    }

    napi_value result;
    napi_create_string_utf8(env, err, strlen(err), &result);
    writer_core_free_string(err);
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

// ── Layout Policy ──

static napi_value NativeResolveLayout(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_resolve_layout(json));
    delete[] json;
    return result;
}

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

// ── Project mutations ──

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

// ── Volume mutations ──

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

// ── Chapter mutations ──

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

// ── StarMap ──

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

// ── Syncable Settings ──

static napi_value NativeLoadSyncableSettings(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_load_syncable_settings());
}

static napi_value NativeSaveSyncableSettings(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_syncable_settings(json));
    delete[] json;
    return result;
}

// ── Sync ──

static napi_value NativeLoadSyncConfig(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_load_sync_config());
}

static napi_value NativeSaveSyncConfig(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_save_sync_config(json));
    delete[] json;
    return result;
}

static napi_value NativeSyncDryRun(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_sync_dry_run());
}

static napi_value NativeSyncDiagnostics(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_sync_diagnostics());
}

static napi_value NativePerformSync(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_perform_sync());
}

// ── Writing Stats ──

static napi_value NativeGetWritingStats(napi_env env, napi_callback_info info) {
    return ReturnJsonString(env, writer_core_get_writing_stats());
}

static napi_value NativeProcessWritingEvent(napi_env env, napi_callback_info info) {
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

    napi_value result = ReturnJsonString(env, writer_core_process_writing_event(json));
    delete[] json;
    return result;
}

// ── Misc ──

static napi_value NativeIsAiAvailable(napi_env env, napi_callback_info info) {
    int32_t available = writer_core_is_ai_available();
    napi_value result;
    napi_create_int32(env, available, &result);
    return result;
}

// ── Module registration ──

static napi_value Init(napi_env env, napi_value exports) {
    napi_property_descriptor desc[] = {
        // Lifecycle
        {"nativeInit", nullptr, NativeInit, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLoadStatus", nullptr, NativeGetLoadStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetLastError", nullptr, NativeGetLastError, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCalculateWordCount", nullptr, NativeCalculateWordCount, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Layout Policy
        {"nativeResolveLayout", nullptr, NativeResolveLayout, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Workspace
        {"nativeValidateWorkspace", nullptr, NativeValidateWorkspace, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListWorkspaces", nullptr, NativeListWorkspaces, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeOpenWorkspace", nullptr, NativeOpenWorkspace, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetWorkspaceState", nullptr, NativeGetWorkspaceState, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveChapterLocation", nullptr, NativeResolveChapterLocation, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeResolveVolumeLocation", nullptr, NativeResolveVolumeLocation, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Project
        {"nativeListProjects", nullptr, NativeListProjects, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetProjectTree", nullptr, NativeGetProjectTree, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateProject", nullptr, NativeCreateProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameProject", nullptr, NativeRenameProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteProject", nullptr, NativeDeleteProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetProjectStats", nullptr, NativeGetProjectStats, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Volume
        {"nativeListVolumes", nullptr, NativeListVolumes, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateVolume", nullptr, NativeCreateVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameVolume", nullptr, NativeRenameVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteVolume", nullptr, NativeDeleteVolume, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeReorderVolumes", nullptr, NativeReorderVolumes, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Chapter
        {"nativeListChapters", nullptr, NativeListChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateChapter", nullptr, NativeCreateChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeOpenChapter", nullptr, NativeOpenChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveChapter", nullptr, NativeSaveChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameChapter", nullptr, NativeRenameChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteChapter", nullptr, NativeDeleteChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeReorderChapters", nullptr, NativeReorderChapters, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeClearChapter", nullptr, NativeClearChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Recent edits
        {"nativeGetRecentEdits", nullptr, NativeGetRecentEdits, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Settings
        {"nativeLoadLocalSettings", nullptr, NativeLoadLocalSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveLocalSettings", nullptr, NativeSaveLocalSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeLoadSyncableSettings", nullptr, NativeLoadSyncableSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncableSettings", nullptr, NativeSaveSyncableSettings, nullptr, nullptr, nullptr, napi_default, nullptr},
        // StarMap
        {"nativeListStarMaps", nullptr, NativeListStarMaps, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeListStarMapsForProject", nullptr, NativeListStarMapsForProject, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMap", nullptr, NativeGetStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeGetStarMapGraph", nullptr, NativeGetStarMapGraph, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCreateStarMap", nullptr, NativeCreateStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeDeleteStarMap", nullptr, NativeDeleteStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeRenameStarMap", nullptr, NativeRenameStarMap, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Sync
        {"nativeLoadSyncConfig", nullptr, NativeLoadSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSaveSyncConfig", nullptr, NativeSaveSyncConfig, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDryRun", nullptr, NativeSyncDryRun, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeSyncDiagnostics", nullptr, NativeSyncDiagnostics, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativePerformSync", nullptr, NativePerformSync, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Stats
        {"nativeGetWritingStats", nullptr, NativeGetWritingStats, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeProcessWritingEvent", nullptr, NativeProcessWritingEvent, nullptr, nullptr, nullptr, napi_default, nullptr},
        // Misc
        {"nativeIsAiAvailable", nullptr, NativeIsAiAvailable, nullptr, nullptr, nullptr, napi_default, nullptr},
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
