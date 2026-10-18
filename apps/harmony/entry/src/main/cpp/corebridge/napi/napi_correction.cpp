// ── Chinese correction NAPI handlers ──
// Included by napi_init.cpp. Detection, ignored terms, dictionaries, revision
// validation and edit transactions all remain in WriterCore.

#include <cmath>

enum class CorrectionOperation {
    SetModelPath,
    GetResourceStatus,
    GetModelPathStatus,
    CheckSegment,
    CheckChapter,
    GetChapterSnapshot,
    Ignore,
    AddDictionaryEntry,
    Apply,
};

struct CorrectionAsyncWork {
    napi_deferred deferred = nullptr;
    napi_async_work work = nullptr;
    CorrectionOperation operation = CorrectionOperation::CheckSegment;
    uint64_t session_id = 0;
    uint64_t text_revision = 0;
    uint32_t start_byte = 0;
    uint32_t end_byte = 0;
    std::string path;
    std::string project_id;
    std::string chapter_id;
    std::string rule_id;
    std::string original;
    std::string replacement;
    std::string scope;
    std::string entry;
    char* json = nullptr;
};

static bool correction_read_string(napi_env env, napi_value value, std::string* output) {
    size_t length = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
        return false;
    }
    char* buffer = new char[length + 1];
    size_t copied = 0;
    const napi_status status = napi_get_value_string_utf8(env, value, buffer, length + 1, &copied);
    if (status != napi_ok || copied != length) {
        delete[] buffer;
        return false;
    }
    output->assign(buffer, length);
    delete[] buffer;
    return true;
}

static napi_value correction_invalid_args(napi_env env, const char* message) {
    napi_throw_error(env, nullptr, message);
    return nullptr;
}

static void correction_execute(napi_env /*env*/, void* data) {
    CorrectionAsyncWork* call = static_cast<CorrectionAsyncWork*>(data);
    switch (call->operation) {
        case CorrectionOperation::SetModelPath:
            call->json = writer_core_set_correction_model_path(call->path.c_str());
            break;
        case CorrectionOperation::GetResourceStatus:
            call->json = writer_core_get_correction_resource_status();
            break;
        case CorrectionOperation::GetModelPathStatus:
            call->json = writer_core_get_correction_model_path_status(call->path.c_str());
            break;
        case CorrectionOperation::CheckSegment:
            call->json = writer_core_check_correction_segment(
                call->session_id, call->project_id.c_str(), call->chapter_id.c_str(),
                call->text_revision, call->start_byte, call->end_byte);
            break;
        case CorrectionOperation::CheckChapter:
            call->json = writer_core_check_correction_chapter(
                call->session_id, call->project_id.c_str(), call->chapter_id.c_str(),
                call->text_revision);
            break;
        case CorrectionOperation::GetChapterSnapshot:
            call->json = writer_core_get_correction_chapter_snapshot(
                call->session_id, call->project_id.c_str(), call->chapter_id.c_str(),
                call->text_revision);
            break;
        case CorrectionOperation::Ignore:
            call->json = writer_core_ignore_correction(
                call->project_id.c_str(), call->chapter_id.c_str(), call->rule_id.c_str(),
                call->original.c_str(), call->scope.c_str(), call->text_revision,
                call->start_byte, call->end_byte);
            break;
        case CorrectionOperation::AddDictionaryEntry:
            call->json = writer_core_add_correction_dictionary_entry(
                call->project_id.c_str(), call->entry.c_str());
            break;
        case CorrectionOperation::Apply:
            call->json = writer_core_apply_correction(
                call->session_id, call->project_id.c_str(), call->chapter_id.c_str(),
                call->text_revision, call->start_byte, call->end_byte,
                call->original.c_str(), call->replacement.c_str());
            break;
    }
}

static void correction_complete(napi_env env, napi_status status, void* data) {
    CorrectionAsyncWork* call = static_cast<CorrectionAsyncWork*>(data);
    if (status != napi_ok) {
        napi_value message;
        napi_value error;
        napi_create_string_utf8(env, "Correction task failed", NAPI_AUTO_LENGTH, &message);
        napi_create_error(env, nullptr, message, &error);
        napi_reject_deferred(env, call->deferred, error);
    } else {
        const char* json = call->json;
        const char* fallback = "{\"success\":false,\"errorCode\":\"NULL_RESULT\"}";
        napi_value result;
        napi_create_string_utf8(env, json ? json : fallback,
                                json ? strlen(json) : strlen(fallback), &result);
        if (call->json != nullptr) {
            writer_core_free_string(call->json);
            call->json = nullptr;
        }
        napi_resolve_deferred(env, call->deferred, result);
    }
    if (call->json != nullptr) {
        writer_core_free_string(call->json);
        call->json = nullptr;
    }
    napi_delete_async_work(env, call->work);
    delete call;
}

static napi_value correction_queue(napi_env env, CorrectionAsyncWork* call) {
    napi_value promise;
    if (napi_create_promise(env, &call->deferred, &promise) != napi_ok) {
        delete call;
        return nullptr;
    }
    napi_value resource_name;
    if (napi_create_string_utf8(env, "WriterCoreCorrection", NAPI_AUTO_LENGTH, &resource_name) != napi_ok ||
        napi_create_async_work(env, nullptr, resource_name, correction_execute,
                               correction_complete, call, &call->work) != napi_ok) {
        napi_value message;
        napi_value error;
        napi_create_string_utf8(env, "Could not queue correction task", NAPI_AUTO_LENGTH, &message);
        napi_create_error(env, nullptr, message, &error);
        napi_reject_deferred(env, call->deferred, error);
        delete call;
        return promise;
    }
    if (napi_queue_async_work(env, call->work) != napi_ok) {
        napi_value message;
        napi_value error;
        napi_create_string_utf8(env, "Could not start correction task", NAPI_AUTO_LENGTH, &message);
        napi_create_error(env, nullptr, message, &error);
        napi_reject_deferred(env, call->deferred, error);
        napi_delete_async_work(env, call->work);
        delete call;
    }
    return promise;
}

static bool correction_read_uint64(napi_env env, napi_value value, uint64_t* output) {
    double number = 0.0;
    if (napi_get_value_double(env, value, &number) != napi_ok || !std::isfinite(number) ||
        number < 0.0 || std::floor(number) != number || number > 9007199254740991.0) {
        return false;
    }
    *output = static_cast<uint64_t>(number);
    return true;
}

static bool correction_read_uint32(napi_env env, napi_value value, uint32_t* output) {
    double number = 0.0;
    if (napi_get_value_double(env, value, &number) != napi_ok || !std::isfinite(number) ||
        number < 0.0 || std::floor(number) != number || number > 4294967295.0) {
        return false;
    }
    *output = static_cast<uint32_t>(number);
    return true;
}

static bool correction_read_session_and_revision(napi_env env, napi_value session_value,
                                                 napi_value revision_value,
                                                 CorrectionAsyncWork* call) {
    if (!correction_read_uint64(env, session_value, &call->session_id) ||
        !correction_read_uint64(env, revision_value, &call->text_revision)) {
        return false;
    }
    return true;
}

static napi_value NativeCorrectionSetModelPath(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 1) {
        return correction_invalid_args(env, "Expected 1 argument: modelPath");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::SetModelPath;
    if (!correction_read_string(env, args[0], &call->path) || call->path.empty()) {
        delete call;
        return correction_invalid_args(env, "modelPath must be a non-empty local file path");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionGetResourceStatus(napi_env env, napi_callback_info /*info*/) {
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::GetResourceStatus;
    return correction_queue(env, call);
}

static napi_value NativeCorrectionGetModelPathStatus(napi_env env, napi_callback_info info) {
    size_t argc = 1;
    napi_value args[1];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 1) {
        return correction_invalid_args(env, "Expected 1 argument: modelPath");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::GetModelPathStatus;
    if (!correction_read_string(env, args[0], &call->path) || call->path.empty()) {
        delete call;
        return correction_invalid_args(env, "modelPath must be a non-empty local file path");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionCheckSegment(napi_env env, napi_callback_info info) {
    size_t argc = 6;
    napi_value args[6];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 6) {
        return correction_invalid_args(env, "Expected 6 arguments: sessionId, projectId, chapterId, textRevision, startByte, endByte");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::CheckSegment;
    if (!correction_read_session_and_revision(env, args[0], args[3], call) ||
        !correction_read_uint32(env, args[4], &call->start_byte) ||
        !correction_read_uint32(env, args[5], &call->end_byte) ||
        !correction_read_string(env, args[1], &call->project_id) ||
        !correction_read_string(env, args[2], &call->chapter_id)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction segment request");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionCheckChapter(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 4) {
        return correction_invalid_args(env, "Expected 4 arguments: sessionId, projectId, chapterId, textRevision");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::CheckChapter;
    if (!correction_read_session_and_revision(env, args[0], args[3], call) ||
        !correction_read_string(env, args[1], &call->project_id) ||
        !correction_read_string(env, args[2], &call->chapter_id)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction chapter request");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionGetChapterSnapshot(napi_env env, napi_callback_info info) {
    size_t argc = 4;
    napi_value args[4];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 4) {
        return correction_invalid_args(env, "Expected 4 arguments: sessionId, projectId, chapterId, textRevision");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::GetChapterSnapshot;
    if (!correction_read_session_and_revision(env, args[0], args[3], call) ||
        !correction_read_string(env, args[1], &call->project_id) ||
        !correction_read_string(env, args[2], &call->chapter_id)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction chapter snapshot request");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionIgnore(napi_env env, napi_callback_info info) {
    size_t argc = 8;
    napi_value args[8];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 8) {
        return correction_invalid_args(env, "Expected 8 arguments: projectId, chapterId, ruleId, original, scope, textRevision, startByte, endByte");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::Ignore;
    if (!correction_read_string(env, args[0], &call->project_id) ||
        !correction_read_string(env, args[1], &call->chapter_id) ||
        !correction_read_string(env, args[2], &call->rule_id) ||
        !correction_read_string(env, args[3], &call->original) ||
        !correction_read_string(env, args[4], &call->scope)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction ignore request");
    }
    if (!correction_read_uint64(env, args[5], &call->text_revision)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction ignore revision");
    }
    if (!correction_read_uint32(env, args[6], &call->start_byte) ||
        !correction_read_uint32(env, args[7], &call->end_byte)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction ignore range");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionAddDictionaryEntry(napi_env env, napi_callback_info info) {
    size_t argc = 2;
    napi_value args[2];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 2) {
        return correction_invalid_args(env, "Expected 2 arguments: projectId, entry");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::AddDictionaryEntry;
    if (!correction_read_string(env, args[0], &call->project_id) ||
        !correction_read_string(env, args[1], &call->entry) || call->entry.empty()) {
        delete call;
        return correction_invalid_args(env, "Invalid correction dictionary entry");
    }
    return correction_queue(env, call);
}

static napi_value NativeCorrectionApply(napi_env env, napi_callback_info info) {
    size_t argc = 8;
    napi_value args[8];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 8) {
        return correction_invalid_args(env, "Expected 8 arguments: sessionId, projectId, chapterId, textRevision, startByte, endByte, original, replacement");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::Apply;
    if (!correction_read_session_and_revision(env, args[0], args[3], call) ||
        !correction_read_uint32(env, args[4], &call->start_byte) ||
        !correction_read_uint32(env, args[5], &call->end_byte) ||
        !correction_read_string(env, args[1], &call->project_id) ||
        !correction_read_string(env, args[2], &call->chapter_id) ||
        !correction_read_string(env, args[6], &call->original) ||
        !correction_read_string(env, args[7], &call->replacement)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction apply request");
    }
    return correction_queue(env, call);
}

napi_property_descriptor* getCorrectionDescriptors(size_t* count) {
    static const napi_property_descriptor kDesc[] = {
        {"nativeCorrectionSetModelPath", nullptr, NativeCorrectionSetModelPath, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionGetResourceStatus", nullptr, NativeCorrectionGetResourceStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionGetModelPathStatus", nullptr, NativeCorrectionGetModelPathStatus, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionCheckSegment", nullptr, NativeCorrectionCheckSegment, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionCheckChapter", nullptr, NativeCorrectionCheckChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionGetChapterSnapshot", nullptr, NativeCorrectionGetChapterSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionIgnore", nullptr, NativeCorrectionIgnore, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionAddDictionaryEntry", nullptr, NativeCorrectionAddDictionaryEntry, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionApply", nullptr, NativeCorrectionApply, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(kDesc) / sizeof(kDesc[0]);
    napi_property_descriptor* out = new napi_property_descriptor[*count];
    memcpy(out, kDesc, *count * sizeof(napi_property_descriptor));
    return out;
}
