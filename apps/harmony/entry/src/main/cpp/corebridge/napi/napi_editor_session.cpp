// ── Editor Session NAPI handlers ──
// Included by napi_init.cpp — expects ReturnJsonString, dup_napi_string and writer_core_bridge.h
// to be available. 与 Android TextEditSessionBridge 职责对齐。
//
// 参数约定：
// - uint64 参数（session_id / expected_revision / composition_*）通过 napi_get_value_int64
//   读取（ArkTS number；超 2^63 的 revision/session_id 不支持，需调用方保证）。
// - uint32 参数通过 napi_get_value_uint32 读取。
// - 字符串参数动态分配（dup_str），调用 C ABI 后立即 delete[]。
// - 所有 handler 返回 ReturnJsonString(env, json)，json 由 core 分配，须 writer_core_free_string 释放。

// ── Inline helpers（本域专用）──

// get_u64: 从 napi_value 读取 uint64_t（ArkTS number → int64 → uint64_t）。
// ArkTS 侧 sessionId/revision 用 number（JSON.parse 返回），实际值 < 2^63，cast 安全。
static uint64_t get_u64(napi_env env, napi_value value) {
    int64_t val = 0;
    napi_get_value_int64(env, value, &val);
    return static_cast<uint64_t>(val);
}

// get_u32: 从 napi_value 读取 uint32_t。
static uint32_t get_u32(napi_env env, napi_value value) {
    uint32_t val = 0;
    napi_get_value_uint32(env, value, &val);
    return val;
}

// dup_str: 动态分配并复制 NAPI string。返回 new char[]，调用方须 delete[]。
static char* dup_str(napi_env env, napi_value value) {
    size_t len = 0;
    napi_get_value_string_utf8(env, value, nullptr, 0, &len);
    char* buf = new char[len + 1];
    napi_get_value_string_utf8(env, value, buf, len + 1, &len);
    return buf;
}

// arg_u64: 取第 i 个 uint64 参数；i >= argc 时返回 0。
static uint64_t arg_u64(napi_env env, const napi_value* args, size_t argc, size_t i) {
    return (i < argc) ? get_u64(env, args[i]) : 0;
}

// arg_u32: 取第 i 个 uint32 参数；i >= argc 时返回 0。
static uint32_t arg_u32(napi_env env, const napi_value* args, size_t argc, size_t i) {
    return (i < argc) ? get_u32(env, args[i]) : 0;
}

// arg_str: 取第 i 个字符串参数（动态分配）；i >= argc 时返回空字符串（new char[1]）。
static char* arg_str(napi_env env, const napi_value* args, size_t argc, size_t i) {
    if (i >= argc) {
        char* empty = new char[1];
        empty[0] = '\0';
        return empty;
    }
    return dup_str(env, args[i]);
}

// ── Editor Session handlers ──

// NativeEditorSessionCreate(target_id, initial_text, initial_cursor_byte_offset, is_persistent)
static napi_value NativeEditorSessionCreate(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    char* target_id = arg_str(env, args, argc, 0);
    char* initial_text = arg_str(env, args, argc, 1);
    uint32_t initial_cursor_byte_offset = arg_u32(env, args, argc, 2);
    uint8_t is_persistent = static_cast<uint8_t>(arg_u32(env, args, argc, 3));

    napi_value result = ReturnJsonString(env,
        writer_core_editor_session_create(target_id, initial_text, initial_cursor_byte_offset, is_persistent));
    delete[] target_id;
    delete[] initial_text;
    return result;
}

// NativeEditorSessionClose(session_id)
static napi_value NativeEditorSessionClose(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    return ReturnJsonString(env, writer_core_editor_session_close(session_id));
}

// NativeEditorSessionSnapshot(session_id)
static napi_value NativeEditorSessionSnapshot(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    return ReturnJsonString(env, writer_core_editor_session_snapshot(session_id));
}

// NativeEditorSessionInsert(session_id, byte_offset, text, cause, expected_revision)
static napi_value NativeEditorSessionInsert(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t byte_offset = arg_u32(env, args, argc, 1);
    char* text = arg_str(env, args, argc, 2);
    char* cause = arg_str(env, args, argc, 3);
    uint64_t expected_revision = arg_u64(env, args, argc, 4);

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

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t byte_start = arg_u32(env, args, argc, 1);
    uint32_t byte_end_exclusive = arg_u32(env, args, argc, 2);
    char* cause = arg_str(env, args, argc, 3);
    uint64_t expected_revision = arg_u64(env, args, argc, 4);

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

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t byte_start = arg_u32(env, args, argc, 1);
    uint32_t byte_end_exclusive = arg_u32(env, args, argc, 2);
    char* replacement_text = arg_str(env, args, argc, 3);
    char* original_text = arg_str(env, args, argc, 4);
    char* cause = arg_str(env, args, argc, 5);
    uint64_t expected_revision = arg_u64(env, args, argc, 6);

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

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t anchor_byte_offset = arg_u32(env, args, argc, 1);
    uint32_t head_byte_offset = arg_u32(env, args, argc, 2);
    uint64_t expected_revision = arg_u64(env, args, argc, 3);

    return ReturnJsonString(env,
        writer_core_editor_session_set_selection(session_id, anchor_byte_offset, head_byte_offset, expected_revision));
}

// NativeEditorSessionUndo(session_id, expected_revision)
static napi_value NativeEditorSessionUndo(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint64_t expected_revision = arg_u64(env, args, argc, 1);
    return ReturnJsonString(env, writer_core_editor_session_undo(session_id, expected_revision));
}

// NativeEditorSessionRedo(session_id, expected_revision)
static napi_value NativeEditorSessionRedo(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint64_t expected_revision = arg_u64(env, args, argc, 1);
    return ReturnJsonString(env, writer_core_editor_session_redo(session_id, expected_revision));
}

// NativeEditorSessionCommitText(session_id, byte_start, byte_end_exclusive, replacement_text,
//   resulting_selection_anchor, resulting_selection_head, composition_session_id,
//   composition_base_revision, composition_generation, cause, expected_revision)
static napi_value NativeEditorSessionCommitText(napi_env env, napi_callback_info info) {
    size_t argc = 11;
    napi_value args[11];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t byte_start = arg_u32(env, args, argc, 1);
    uint32_t byte_end_exclusive = arg_u32(env, args, argc, 2);
    char* replacement_text = arg_str(env, args, argc, 3);
    uint32_t resulting_selection_anchor = arg_u32(env, args, argc, 4);
    uint32_t resulting_selection_head = arg_u32(env, args, argc, 5);
    uint64_t composition_session_id = arg_u64(env, args, argc, 6);
    uint64_t composition_base_revision = arg_u64(env, args, argc, 7);
    uint64_t composition_generation = arg_u64(env, args, argc, 8);
    char* cause = arg_str(env, args, argc, 9);
    uint64_t expected_revision = arg_u64(env, args, argc, 10);

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

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t before_byte_start = arg_u32(env, args, argc, 1);
    uint32_t before_byte_end_exclusive = arg_u32(env, args, argc, 2);
    uint32_t after_byte_start = arg_u32(env, args, argc, 3);
    uint32_t after_byte_end_exclusive = arg_u32(env, args, argc, 4);
    char* cause = arg_str(env, args, argc, 5);
    uint64_t expected_revision = arg_u64(env, args, argc, 6);

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

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t replace_start = arg_u32(env, args, argc, 1);
    uint32_t replace_end_exclusive = arg_u32(env, args, argc, 2);
    uint64_t expected_revision = arg_u64(env, args, argc, 3);

    return ReturnJsonString(env,
        writer_core_editor_session_begin_composition(session_id, replace_start, replace_end_exclusive, expected_revision));
}

// NativeEditorSessionUpdateComposition(session_id, composition_session_id, composition_generation,
//   new_preedit_text, new_preedit_cursor_utf16, expected_revision)
static napi_value NativeEditorSessionUpdateComposition(napi_env env, napi_callback_info info) {
    size_t argc = 6;
    napi_value args[6];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint64_t composition_session_id = arg_u64(env, args, argc, 1);
    uint64_t composition_generation = arg_u64(env, args, argc, 2);
    char* new_preedit_text = arg_str(env, args, argc, 3);
    uint32_t new_preedit_cursor_utf16 = arg_u32(env, args, argc, 4);
    uint64_t expected_revision = arg_u64(env, args, argc, 5);

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

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint64_t composition_session_id = arg_u64(env, args, argc, 1);
    uint64_t composition_generation = arg_u64(env, args, argc, 2);
    uint64_t expected_revision = arg_u64(env, args, argc, 3);

    return ReturnJsonString(env,
        writer_core_editor_session_finish_composition(session_id, composition_session_id,
                                                       composition_generation, expected_revision));
}

// NativeEditorSessionCancelComposition(session_id, composition_session_id, composition_generation, expected_revision)
static napi_value NativeEditorSessionCancelComposition(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint64_t composition_session_id = arg_u64(env, args, argc, 1);
    uint64_t composition_generation = arg_u64(env, args, argc, 2);
    uint64_t expected_revision = arg_u64(env, args, argc, 3);

    return ReturnJsonString(env,
        writer_core_editor_session_cancel_composition(session_id, composition_session_id,
                                                       composition_generation, expected_revision));
}

// NativeEditorSessionGetText(session_id)
static napi_value NativeEditorSessionGetText(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    return ReturnJsonString(env, writer_core_editor_session_get_text(session_id));
}

// NativeEditorSessionGetRevision(session_id)
static napi_value NativeEditorSessionGetRevision(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    return ReturnJsonString(env, writer_core_editor_session_get_revision(session_id));
}

// NativeEditorSessionPreviousGraphemeBoundary(session_id, byte_offset)
// #606: 返回严格在 byte_offset 之前的最近 grapheme cluster 边界（UTF-8 byte offset）。
static napi_value NativeEditorSessionPreviousGraphemeBoundary(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t byte_offset = arg_u32(env, args, argc, 1);
    return ReturnJsonString(env, writer_core_editor_session_previous_grapheme_boundary(session_id, byte_offset));
}

// NativeEditorSessionNextGraphemeBoundary(session_id, byte_offset)
// #606: 返回严格在 byte_offset 之后的最近 grapheme cluster 边界（UTF-8 byte offset）。
static napi_value NativeEditorSessionNextGraphemeBoundary(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);

    uint64_t session_id = arg_u64(env, args, argc, 0);
    uint32_t byte_offset = arg_u32(env, args, argc, 1);
    return ReturnJsonString(env, writer_core_editor_session_next_grapheme_boundary(session_id, byte_offset));
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
    };
    *count = sizeof(kDesc) / sizeof(kDesc[0]);
    napi_property_descriptor* out = new napi_property_descriptor[*count];
    memcpy(out, kDesc, *count * sizeof(napi_property_descriptor));
    return out;
}
