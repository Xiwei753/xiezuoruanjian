// ── Editor Session NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString, ThrowNapiStringError and
// writer_core_bridge.h to be available. 与 Android TextEditSessionBridge 职责对齐。
//
// 参数约定（Issue #1063 评论 5）：
// - 参数读取统一走 napi_editor_args.h：字符串校验 napi_typeof、两次
//   napi_get_value_string_utf8 的 status 与写入量；uint64
//   （session_id / expected_revision / composition_*）拒绝负数/非整数/越界；
//   uint32（byte offset 等）拒绝负数与超出 UINT32_MAX 的值。
// - 参数个数不足或类型/范围非法时不调用 Rust，直接返回
//   INVALID_ARGUMENT ResultEnvelope JSON；任何失败路径都不会把 nullptr 或
//   未初始化 buffer 交给 C ABI。
// - 字符串参数动态分配（new[]），调用 C ABI 后立即 delete[]，
//   所有失败分支在返回前释放已分配的 buffer。
// - 所有 handler 返回 ReturnJsonString(env, json)，json 由 core 分配，须 writer_core_free_string 释放。

#include "napi_editor_args.h"

// ── Inline helpers（本域专用）──

// ReturnInvalidArg: 参数个数/类型/范围非法时的统一返回。
//   napi_editor_args.h 的 INVALID_ARGUMENT JSON 是静态字面量，不能交给
//   ReturnJsonString（后者按 Core 所有权用 writer_core_free_string 释放），
//   因此这里直接用 napi_create_string_utf8 构造返回值；创建失败与其它
//   NAPI 字符串创建失败保持同一语义（抛 E_NAPI_STRING 并返回 nullptr）。
static napi_value ReturnInvalidArg(napi_env env) {
    napi_value result;
    if (napi_create_string_utf8(env, napi_editor_invalid_arg_json(),
                                NAPI_AUTO_LENGTH, &result) != napi_ok) {
        return ThrowNapiStringError(env, "Failed to create INVALID_ARGUMENT JSON string");
    }
    return result;
}

// ── Editor Session handlers ──

// NativeEditorSessionCreate(target_id, initial_text, initial_cursor_byte_offset)
static napi_value NativeEditorSessionCreate(napi_env env, napi_callback_info info) {
    size_t argc = 3;
    napi_value args[3];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint32_t initial_cursor_byte_offset = 0;
    if (argc < 3 || !napi_arg_get_u32(env, args[2], &initial_cursor_byte_offset)) {
        return ReturnInvalidArg(env);
    }

    char* target_id = nullptr;
    if (!napi_arg_get_string(env, args[0], &target_id)) {
        return ReturnInvalidArg(env);
    }
    char* initial_text = nullptr;
    if (!napi_arg_get_string(env, args[1], &initial_text)) {
        delete[] target_id;
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_create(target_id, initial_text, initial_cursor_byte_offset));
    delete[] target_id;
    delete[] initial_text;
    return result;
}

// NativeEditorSessionClose(session_id)
static napi_value NativeEditorSessionClose(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    if (argc < 1 || !napi_arg_get_u64(env, args[0], &session_id)) {
        return ReturnInvalidArg(env);
    }
    return ReturnJsonString(env, writer_core_editor_session_close(session_id));
}

// NativeEditorSessionSnapshot(session_id)
static napi_value NativeEditorSessionSnapshot(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    if (argc < 1 || !napi_arg_get_u64(env, args[0], &session_id)) {
        return ReturnInvalidArg(env);
    }
    return ReturnJsonString(env, writer_core_editor_session_snapshot(session_id));
}

// NativeEditorSessionInsert(session_id, byte_offset, text, cause, expected_revision)
static napi_value NativeEditorSessionInsert(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t byte_offset = 0;
    uint64_t expected_revision = 0;
    // 先校验参数个数与全部数值参数，避免在可能失败的读取之后才分配字符串。
    if (argc < 5
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &byte_offset)
        || !napi_arg_get_u64(env, args[4], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    char* text = nullptr;
    if (!napi_arg_get_string(env, args[2], &text)) {
        return ReturnInvalidArg(env);
    }
    char* cause = nullptr;
    if (!napi_arg_get_string(env, args[3], &cause)) {
        delete[] text;
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_insert(session_id, byte_offset, text, cause, expected_revision));
    delete[] text;
    delete[] cause;
    return result;
}

// NativeEditorSessionDelete(session_id, byte_start, byte_end_exclusive, cause, expected_revision)
static napi_value NativeEditorSessionDelete(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t byte_start = 0;
    uint32_t byte_end_exclusive = 0;
    uint64_t expected_revision = 0;
    if (argc < 5
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &byte_start)
        || !napi_arg_get_u32(env, args[2], &byte_end_exclusive)
        || !napi_arg_get_u64(env, args[4], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    char* cause = nullptr;
    if (!napi_arg_get_string(env, args[3], &cause)) {
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_delete(session_id, byte_start, byte_end_exclusive, cause, expected_revision));
    delete[] cause;
    return result;
}

// NativeEditorSessionReplace(session_id, byte_start, byte_end_exclusive, replacement_text, original_text, cause, expected_revision)
static napi_value NativeEditorSessionReplace(napi_env env, napi_callback_info info) {
    size_t argc = 7;
    napi_value args[7];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t byte_start = 0;
    uint32_t byte_end_exclusive = 0;
    uint64_t expected_revision = 0;
    if (argc < 7
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &byte_start)
        || !napi_arg_get_u32(env, args[2], &byte_end_exclusive)
        || !napi_arg_get_u64(env, args[6], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    char* replacement_text = nullptr;
    if (!napi_arg_get_string(env, args[3], &replacement_text)) {
        return ReturnInvalidArg(env);
    }
    char* original_text = nullptr;
    if (!napi_arg_get_string(env, args[4], &original_text)) {
        delete[] replacement_text;
        return ReturnInvalidArg(env);
    }
    char* cause = nullptr;
    if (!napi_arg_get_string(env, args[5], &cause)) {
        delete[] replacement_text;
        delete[] original_text;
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_replace(session_id, byte_start, byte_end_exclusive,
                                            replacement_text, original_text, cause, expected_revision));
    delete[] replacement_text;
    delete[] original_text;
    delete[] cause;
    return result;
}

// NativeEditorSessionSetSelection(session_id, anchor_byte_offset, head_byte_offset, expected_revision)
static napi_value NativeEditorSessionSetSelection(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t anchor_byte_offset = 0;
    uint32_t head_byte_offset = 0;
    uint64_t expected_revision = 0;
    if (argc < 4
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &anchor_byte_offset)
        || !napi_arg_get_u32(env, args[2], &head_byte_offset)
        || !napi_arg_get_u64(env, args[3], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_set_selection(session_id, anchor_byte_offset, head_byte_offset, expected_revision));
}

// NativeEditorSessionUndo(session_id, expected_revision)
static napi_value NativeEditorSessionUndo(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t expected_revision = 0;
    if (argc < 2
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u64(env, args[1], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env, writer_core_editor_session_undo(session_id, expected_revision));
}

// NativeEditorSessionRedo(session_id, expected_revision)
static napi_value NativeEditorSessionRedo(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t expected_revision = 0;
    if (argc < 2
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u64(env, args[1], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env, writer_core_editor_session_redo(session_id, expected_revision));
}

// NativeEditorSessionCommitText(session_id, byte_start, byte_end_exclusive, replacement_text,
//   resulting_selection_anchor, resulting_selection_head, composition_session_id,
//   composition_base_revision, composition_generation, cause, expected_revision)
static napi_value NativeEditorSessionCommitText(napi_env env, napi_callback_info info) {
    size_t argc = 11;
    napi_value args[11];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t byte_start = 0;
    uint32_t byte_end_exclusive = 0;
    uint32_t resulting_selection_anchor = 0;
    uint32_t resulting_selection_head = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_base_revision = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (argc < 11
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &byte_start)
        || !napi_arg_get_u32(env, args[2], &byte_end_exclusive)
        || !napi_arg_get_u32(env, args[4], &resulting_selection_anchor)
        || !napi_arg_get_u32(env, args[5], &resulting_selection_head)
        || !napi_arg_get_u64(env, args[6], &composition_session_id)
        || !napi_arg_get_u64(env, args[7], &composition_base_revision)
        || !napi_arg_get_u64(env, args[8], &composition_generation)
        || !napi_arg_get_u64(env, args[10], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    char* replacement_text = nullptr;
    if (!napi_arg_get_string(env, args[3], &replacement_text)) {
        return ReturnInvalidArg(env);
    }
    char* cause = nullptr;
    if (!napi_arg_get_string(env, args[9], &cause)) {
        delete[] replacement_text;
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_commit_text(session_id, byte_start, byte_end_exclusive, replacement_text,
                                               resulting_selection_anchor, resulting_selection_head,
                                               composition_session_id, composition_base_revision,
                                               composition_generation, cause, expected_revision));
    delete[] replacement_text;
    delete[] cause;
    return result;
}

// NativeEditorSessionDeleteSurrounding(session_id, before_byte_start, before_byte_end_exclusive,
//   after_byte_start, after_byte_end_exclusive, cause, expected_revision)
static napi_value NativeEditorSessionDeleteSurrounding(napi_env env, napi_callback_info info) {
    size_t argc = 7;
    napi_value args[7];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t before_byte_start = 0;
    uint32_t before_byte_end_exclusive = 0;
    uint32_t after_byte_start = 0;
    uint32_t after_byte_end_exclusive = 0;
    uint64_t expected_revision = 0;
    if (argc < 7
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &before_byte_start)
        || !napi_arg_get_u32(env, args[2], &before_byte_end_exclusive)
        || !napi_arg_get_u32(env, args[3], &after_byte_start)
        || !napi_arg_get_u32(env, args[4], &after_byte_end_exclusive)
        || !napi_arg_get_u64(env, args[6], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    char* cause = nullptr;
    if (!napi_arg_get_string(env, args[5], &cause)) {
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_delete_surrounding(session_id, before_byte_start, before_byte_end_exclusive,
                                                      after_byte_start, after_byte_end_exclusive,
                                                      cause, expected_revision));
    delete[] cause;
    return result;
}

// NativeEditorSessionBeginComposition(session_id, replace_start, replace_end_exclusive, expected_revision)
static napi_value NativeEditorSessionBeginComposition(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t replace_start = 0;
    uint32_t replace_end_exclusive = 0;
    uint64_t expected_revision = 0;
    if (argc < 4
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &replace_start)
        || !napi_arg_get_u32(env, args[2], &replace_end_exclusive)
        || !napi_arg_get_u64(env, args[3], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_begin_composition(session_id, replace_start, replace_end_exclusive, expected_revision));
}

// NativeEditorSessionUpdateComposition(session_id, composition_session_id, composition_generation,
//   new_preedit_text, new_preedit_cursor_utf16, expected_revision)
static napi_value NativeEditorSessionUpdateComposition(napi_env env, napi_callback_info info) {
    size_t argc = 6;
    napi_value args[6];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint32_t new_preedit_cursor_utf16 = 0;
    uint64_t expected_revision = 0;
    if (argc < 6
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u64(env, args[1], &composition_session_id)
        || !napi_arg_get_u64(env, args[2], &composition_generation)
        || !napi_arg_get_u32(env, args[4], &new_preedit_cursor_utf16)
        || !napi_arg_get_u64(env, args[5], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    char* new_preedit_text = nullptr;
    if (!napi_arg_get_string(env, args[3], &new_preedit_text)) {
        return ReturnInvalidArg(env);
    }

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_update_composition(session_id, composition_session_id, composition_generation,
                                                      new_preedit_text, new_preedit_cursor_utf16, expected_revision));
    delete[] new_preedit_text;
    return result;
}

// NativeEditorSessionFinishComposition(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionFinishComposition(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (argc < 4
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u64(env, args[1], &composition_session_id)
        || !napi_arg_get_u64(env, args[2], &composition_generation)
        || !napi_arg_get_u64(env, args[3], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_finish_composition(session_id, composition_session_id,
                                                       composition_generation, expected_revision));
}

// NativeEditorSessionCancelComposition(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionCancelComposition(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (argc < 4
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u64(env, args[1], &composition_session_id)
        || !napi_arg_get_u64(env, args[2], &composition_generation)
        || !napi_arg_get_u64(env, args[3], &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_cancel_composition(session_id, composition_session_id,
                                                       composition_generation, expected_revision));
}

// NativeEditorSessionGetText(session_id)
static napi_value NativeEditorSessionGetText(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    if (argc < 1 || !napi_arg_get_u64(env, args[0], &session_id)) {
        return ReturnInvalidArg(env);
    }
    return ReturnJsonString(env, writer_core_editor_session_get_text(session_id));
}

// NativeEditorSessionGetRevision(session_id)
static napi_value NativeEditorSessionGetRevision(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    if (argc < 1 || !napi_arg_get_u64(env, args[0], &session_id)) {
        return ReturnInvalidArg(env);
    }
    return ReturnJsonString(env, writer_core_editor_session_get_revision(session_id));
}

// NativeEditorSessionPreviousGraphemeBoundary(session_id, byte_offset)
// #606: 返回严格在 byte_offset 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
static napi_value NativeEditorSessionPreviousGraphemeBoundary(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t byte_offset = 0;
    if (argc < 2
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &byte_offset)) {
        return ReturnInvalidArg(env);
    }
    return ReturnJsonString(env, writer_core_editor_session_previous_grapheme_boundary(session_id, byte_offset));
}

// NativeEditorSessionNextGraphemeBoundary(session_id, byte_offset)
// #606: 返回严格在 byte_offset 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
static napi_value NativeEditorSessionNextGraphemeBoundary(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint32_t byte_offset = 0;
    if (argc < 2
        || !napi_arg_get_u64(env, args[0], &session_id)
        || !napi_arg_get_u32(env, args[1], &byte_offset)) {
        return ReturnInvalidArg(env);
    }
    return ReturnJsonString(env, writer_core_editor_session_next_grapheme_boundary(session_id, byte_offset));
}

// 以下四个 composition grapheme 操作参数相同：
// (session_id, composition_session_id, composition_generation, expected_revision)。
static bool ReadCompositionGraphemeArgs(napi_env env, const napi_value* args, size_t argc,
                                        uint64_t* session_id, uint64_t* composition_session_id,
                                        uint64_t* composition_generation, uint64_t* expected_revision) {
    return argc >= 4
        && napi_arg_get_u64(env, args[0], session_id)
        && napi_arg_get_u64(env, args[1], composition_session_id)
        && napi_arg_get_u64(env, args[2], composition_generation)
        && napi_arg_get_u64(env, args[3], expected_revision);
}

// NativeEditorSessionCompositionMoveGraphemeLeft(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionCompositionMoveGraphemeLeft(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (!ReadCompositionGraphemeArgs(env, args, argc, &session_id, &composition_session_id,
                                     &composition_generation, &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_composition_move_grapheme_left(session_id, composition_session_id, composition_generation, expected_revision));
}

// NativeEditorSessionCompositionMoveGraphemeRight(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionCompositionMoveGraphemeRight(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (!ReadCompositionGraphemeArgs(env, args, argc, &session_id, &composition_session_id,
                                     &composition_generation, &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_composition_move_grapheme_right(session_id, composition_session_id, composition_generation, expected_revision));
}

// NativeEditorSessionCompositionDeleteGraphemeBackward(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionCompositionDeleteGraphemeBackward(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (!ReadCompositionGraphemeArgs(env, args, argc, &session_id, &composition_session_id,
                                     &composition_generation, &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_composition_delete_grapheme_backward(session_id, composition_session_id, composition_generation, expected_revision));
}

// NativeEditorSessionCompositionDeleteGraphemeForward(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionCompositionDeleteGraphemeForward(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = 0;
    uint64_t composition_session_id = 0;
    uint64_t composition_generation = 0;
    uint64_t expected_revision = 0;
    if (!ReadCompositionGraphemeArgs(env, args, argc, &session_id, &composition_session_id,
                                     &composition_generation, &expected_revision)) {
        return ReturnInvalidArg(env);
    }

    return ReturnJsonString(env,
        writer_core_editor_session_composition_delete_grapheme_forward(session_id, composition_session_id, composition_generation, expected_revision));
}

// ── Editor Session property descriptors ──
// 返回 new[] 的副本，调用方（napi_init.cpp Init）负责 delete[]。

napi_property_descriptor* getEditorSessionDescriptors(size_t* count) {
    static const napi_property_descriptor kDesc[] = {
        {"nativeEditorSessionCreate",            nullptr, NativeEditorSessionCreate,            nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionClose",             nullptr, NativeEditorSessionClose,             nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionSnapshot",          nullptr, NativeEditorSessionSnapshot,          nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionInsert",            nullptr, NativeEditorSessionInsert,            nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionDelete",            nullptr, NativeEditorSessionDelete,            nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionReplace",           nullptr, NativeEditorSessionReplace,           nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionSetSelection",      nullptr, NativeEditorSessionSetSelection,      nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionUndo",              nullptr, NativeEditorSessionUndo,              nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionRedo",              nullptr, NativeEditorSessionRedo,              nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionCommitText",        nullptr, NativeEditorSessionCommitText,        nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionDeleteSurrounding", nullptr, NativeEditorSessionDeleteSurrounding, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionBeginComposition",  nullptr, NativeEditorSessionBeginComposition,  nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionUpdateComposition", nullptr, NativeEditorSessionUpdateComposition, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionFinishComposition", nullptr, NativeEditorSessionFinishComposition, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionCancelComposition", nullptr, NativeEditorSessionCancelComposition, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionGetText",           nullptr, NativeEditorSessionGetText,           nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionGetRevision",       nullptr, NativeEditorSessionGetRevision,       nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionPreviousGraphemeBoundary", nullptr, NativeEditorSessionPreviousGraphemeBoundary, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionNextGraphemeBoundary",     nullptr, NativeEditorSessionNextGraphemeBoundary,     nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionCompositionMoveGraphemeLeft",     nullptr, NativeEditorSessionCompositionMoveGraphemeLeft,     nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionCompositionMoveGraphemeRight",    nullptr, NativeEditorSessionCompositionMoveGraphemeRight,    nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionCompositionDeleteGraphemeBackward", nullptr, NativeEditorSessionCompositionDeleteGraphemeBackward, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeEditorSessionCompositionDeleteGraphemeForward",  nullptr, NativeEditorSessionCompositionDeleteGraphemeForward,  nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(kDesc) / sizeof(kDesc[0]);
    napi_property_descriptor* out = new napi_property_descriptor[*count];
    memcpy(out, kDesc, *count * sizeof(napi_property_descriptor));
    return out;
}
