// ── Chinese correction NAPI handlers ──
// Included by napi_init.cpp. Detection, ignored terms, dictionaries, revision
// validation and edit transactions all remain in WriterCore.

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
#include <rawfile/raw_file.h>
#include <rawfile/raw_file_manager.h>

enum class CorrectionOperation {
    SetModelPath,
    GetResourceStatus,
    GetModelPathStatus,
    CheckSegment,
    CheckEdits,
    CheckChapter,
    GetChapterSnapshot,
    Ignore,
    AddDictionaryEntry,
    Apply,
    PrepareBundledModel,
};

struct CorrectionAsyncWork {
    napi_deferred deferred = nullptr;
    napi_async_work work = nullptr;
    CorrectionOperation operation = CorrectionOperation::CheckSegment;
    uint64_t session_id = 0;
    uint64_t text_revision = 0;
    uint64_t base_revision = 0;
    uint32_t start_byte = 0;
    uint32_t end_byte = 0;
    std::string path;
    std::string project_id;
    std::string chapter_id;
    std::string text;
    std::string rule_id;
    std::string original;
    std::string replacement;
    std::string scope;
    std::string entry;
    std::string edits_json;
    char* json = nullptr;
    // PrepareBundledModel（Issue #958）：resource_manager 由 JS 线程创建并在完成
    // 回调（JS 线程）释放；其余字段在 JS 线程读入、异步线程只读。
    NativeResourceManager* resource_manager = nullptr;
    std::string raw_file_name;
    std::string destination_path;
    std::string expected_sha256;
    uint64_t expected_size = 0;
    std::string error_message;
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

// ── 内置纠错模型：rawfile → 沙箱 分块流式复制（Issue #958）──
//
// 模型二进制随 entry HAP 打包（resources/rawfile/people2014corpus_chars.klm），
// 运行时在这里按固定 1MB 缓冲分块读取，边读边写沙箱、边算 SHA-256，校验
// 字节数与摘要后原子改名落盘；沙箱内已有大小与摘要都匹配的文件直接复用。
// 刻意不用 ArkTS getRawFileContent（会把 ~148MB 读进 ArkTS 内存），也不把
// getRawFd 句柄当普通 fd 交给 fs.copyFile（华为 FAQ：内容错误）。

static constexpr size_t CORRECTION_MODEL_BUFFER_SIZE = 1024 * 1024;

// BEGIN self-contained SHA-256 (FIPS 180-4)
// 自包含 SHA-256：轮常量与初始向量取自 FIPS 180-4 标准；结构参考公有领域的
// SHA-256 参考实现（Brad Conte, crypto-algorithms，public domain），改写为
// 本翻译单元内的结构体，不依赖 OpenSSL。
struct CorrectionSha256State {
    uint32_t state[8];
    uint64_t total_bytes;
    unsigned char block[64];
    size_t block_length;

    CorrectionSha256State() : total_bytes(0), block_length(0) {
        state[0] = 0x6a09e667u; state[1] = 0xbb67ae85u; state[2] = 0x3c6ef372u;
        state[3] = 0xa54ff53au; state[4] = 0x510e527fu; state[5] = 0x9b05688cu;
        state[6] = 0x1f83d9abu; state[7] = 0x5be0cd19u;
    }

    static uint32_t RotateRight(uint32_t value, uint32_t bits) {
        return (value >> bits) | (value << (32 - bits));
    }

    void Transform() {
        static const uint32_t kRound[64] = {
            0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u,
            0x923f82a4u, 0xab1c5ed5u, 0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u,
            0x72be5d74u, 0x80deb1feu, 0x9bdc06a7u, 0xc19bf174u, 0xe49b69c1u, 0xefbe4786u,
            0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu, 0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau,
            0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u, 0xc6e00bf3u, 0xd5a79147u,
            0x06ca6351u, 0x14292967u, 0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu, 0x53380d13u,
            0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u, 0xa2bfe8a1u, 0xa81a664bu,
            0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u,
            0x19a4c116u, 0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au,
            0x5b9cca4fu, 0x682e6ff3u, 0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u,
            0x90befffau, 0xa4506cebu, 0xbef9a3f7u, 0xc67178f2u
        };
        uint32_t w[64];
        for (int i = 0; i < 16; i++) {
            w[i] = (static_cast<uint32_t>(block[i * 4]) << 24) |
                   (static_cast<uint32_t>(block[i * 4 + 1]) << 16) |
                   (static_cast<uint32_t>(block[i * 4 + 2]) << 8) |
                   static_cast<uint32_t>(block[i * 4 + 3]);
        }
        for (int i = 16; i < 64; i++) {
            const uint32_t s0 = RotateRight(w[i - 15], 7) ^ RotateRight(w[i - 15], 18) ^ (w[i - 15] >> 3);
            const uint32_t s1 = RotateRight(w[i - 2], 17) ^ RotateRight(w[i - 2], 19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16] + s0 + w[i - 7] + s1;
        }
        uint32_t a = state[0];
        uint32_t b = state[1];
        uint32_t c = state[2];
        uint32_t d = state[3];
        uint32_t e = state[4];
        uint32_t f = state[5];
        uint32_t g = state[6];
        uint32_t h = state[7];
        for (int i = 0; i < 64; i++) {
            const uint32_t s1 = RotateRight(e, 6) ^ RotateRight(e, 11) ^ RotateRight(e, 25);
            const uint32_t choice = (e & f) ^ (~e & g);
            const uint32_t temp1 = h + s1 + choice + kRound[i] + w[i];
            const uint32_t s0 = RotateRight(a, 2) ^ RotateRight(a, 13) ^ RotateRight(a, 22);
            const uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
            const uint32_t temp2 = s0 + majority;
            h = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }
        state[0] += a; state[1] += b; state[2] += c; state[3] += d;
        state[4] += e; state[5] += f; state[6] += g; state[7] += h;
    }

    void Update(const unsigned char* data, size_t length) {
        total_bytes += length;
        while (length > 0) {
            size_t take = 64 - block_length;
            if (take > length) {
                take = length;
            }
            memcpy(block + block_length, data, take);
            block_length += take;
            data += take;
            length -= take;
            if (block_length == 64) {
                Transform();
                block_length = 0;
            }
        }
    }

    void Finalize(unsigned char* digest) {
        const uint64_t bit_length = total_bytes * 8;
        block[block_length++] = 0x80;
        if (block_length > 56) {
            while (block_length < 64) {
                block[block_length++] = 0;
            }
            Transform();
            block_length = 0;
        }
        while (block_length < 56) {
            block[block_length++] = 0;
        }
        for (int i = 0; i < 8; i++) {
            block[56 + i] = static_cast<unsigned char>((bit_length >> (56 - i * 8)) & 0xff);
        }
        Transform();
        block_length = 0;
        for (int i = 0; i < 8; i++) {
            digest[i * 4] = static_cast<unsigned char>((state[i] >> 24) & 0xff);
            digest[i * 4 + 1] = static_cast<unsigned char>((state[i] >> 16) & 0xff);
            digest[i * 4 + 2] = static_cast<unsigned char>((state[i] >> 8) & 0xff);
            digest[i * 4 + 3] = static_cast<unsigned char>(state[i] & 0xff);
        }
    }
};
// END self-contained SHA-256 (FIPS 180-4)

// 摘要转小写十六进制（与 sha256sum、打包锁文件格式一致）。
static std::string correction_sha256_hex(const unsigned char* digest) {
    static const char kHex[] = "0123456789abcdef";
    std::string hex(64, '0');
    for (size_t i = 0; i < 32; i++) {
        hex[i * 2] = kHex[(digest[i] >> 4) & 0x0f];
        hex[i * 2 + 1] = kHex[digest[i] & 0x0f];
    }
    return hex;
}

// 计算已有文件的字节数与 SHA-256；文件不存在或读取失败返回 false。
static bool correction_hash_file(const std::string& path, uint64_t* byte_count, std::string* hex_digest) {
    FILE* input = fopen(path.c_str(), "rb");
    if (input == nullptr) {
        return false;
    }
    CorrectionSha256State hasher;
    std::vector<unsigned char> buffer(CORRECTION_MODEL_BUFFER_SIZE);
    uint64_t total = 0;
    bool ok = true;
    while (true) {
        const size_t read = fread(buffer.data(), 1, buffer.size(), input);
        if (read > 0) {
            hasher.Update(buffer.data(), read);
            total += read;
        }
        if (read < buffer.size()) {
            if (ferror(input) != 0) {
                ok = false;
            }
            break;
        }
    }
    if (fclose(input) != 0) {
        ok = false;
    }
    if (!ok) {
        return false;
    }
    unsigned char digest[32];
    hasher.Finalize(digest);
    *byte_count = total;
    *hex_digest = correction_sha256_hex(digest);
    return true;
}

// 把 rawfile 流式复制到 destination_path.part，校验字节数与 SHA-256 后改名。
// 失败时删除 .part 并把可读原因写入 error_message。
static bool correction_copy_bundled_model(CorrectionAsyncWork* call) {
    uint64_t existing_bytes = 0;
    std::string existing_digest;
    if (correction_hash_file(call->destination_path, &existing_bytes, &existing_digest) &&
        existing_bytes == call->expected_size && existing_digest == call->expected_sha256) {
        // 沙箱内已有校验通过的成品：重复启动不重拷。
        return true;
    }

    RawFile* raw_file = OH_ResourceManager_OpenRawFile(call->resource_manager, call->raw_file_name.c_str());
    if (raw_file == nullptr) {
        call->error_message = "bundled correction model rawfile is missing from the entry HAP: " + call->raw_file_name;
        return false;
    }

    const std::string part_path = call->destination_path + ".part";
    FILE* output = fopen(part_path.c_str(), "wb");
    if (output == nullptr) {
        OH_ResourceManager_CloseRawFile(raw_file);
        call->error_message = "cannot open the staging file for the bundled correction model: " + part_path;
        return false;
    }

    CorrectionSha256State hasher;
    std::vector<unsigned char> buffer(CORRECTION_MODEL_BUFFER_SIZE);
    uint64_t total_bytes = 0;
    bool copied = true;
    while (true) {
        const int read = OH_ResourceManager_ReadRawFile(raw_file, buffer.data(), buffer.size());
        if (read < 0) {
            call->error_message = "reading the bundled correction model from the HAP failed: " + call->raw_file_name;
            copied = false;
            break;
        }
        if (read == 0) {
            break;
        }
        const size_t length = static_cast<size_t>(read);
        if (fwrite(buffer.data(), 1, length, output) != length) {
            call->error_message = "writing the bundled correction model into the sandbox failed: " + part_path;
            copied = false;
            break;
        }
        hasher.Update(buffer.data(), length);
        total_bytes += length;
    }
    if (fclose(output) != 0 && copied) {
        call->error_message = "closing the staged bundled correction model failed: " + part_path;
        copied = false;
    }
    OH_ResourceManager_CloseRawFile(raw_file);

    if (copied && total_bytes != call->expected_size) {
        call->error_message = "bundled correction model size mismatch: expected " +
            std::to_string(call->expected_size) + " bytes, copied " + std::to_string(total_bytes);
        copied = false;
    }
    if (copied) {
        unsigned char digest[32];
        hasher.Finalize(digest);
        const std::string actual_digest = correction_sha256_hex(digest);
        if (actual_digest != call->expected_sha256) {
            call->error_message = "bundled correction model SHA-256 mismatch: expected " +
                call->expected_sha256 + ", got " + actual_digest;
            copied = false;
        }
    }
    if (!copied) {
        std::remove(part_path.c_str());
        return false;
    }
    if (std::rename(part_path.c_str(), call->destination_path.c_str()) != 0) {
        call->error_message = "cannot move the verified bundled correction model into place: " + call->destination_path;
        std::remove(part_path.c_str());
        return false;
    }
    return true;
}

// 释放 JS 线程创建的 NativeResourceManager（完成回调与失败兜底共用）。
static void correction_release_resource_manager(CorrectionAsyncWork* call) {
    if (call->resource_manager != nullptr) {
        OH_ResourceManager_ReleaseNativeResourceManager(call->resource_manager);
        call->resource_manager = nullptr;
    }
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
        case CorrectionOperation::CheckEdits:
            call->json = writer_core_check_correction_edits(
                call->session_id, call->project_id.c_str(), call->chapter_id.c_str(),
                call->text_revision, call->base_revision, call->text.c_str(), call->edits_json.c_str());
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
        case CorrectionOperation::PrepareBundledModel:
            correction_copy_bundled_model(call);
            break;
    }
}

static void correction_complete(napi_env env, napi_status status, void* data) {
    CorrectionAsyncWork* call = static_cast<CorrectionAsyncWork*>(data);
    if (call->operation == CorrectionOperation::PrepareBundledModel) {
        if (status == napi_ok && call->error_message.empty()) {
            // 成功 resolve 最终文件绝对路径；沙箱内文件已按字节数与 SHA-256 校验。
            napi_value result;
            napi_create_string_utf8(env, call->destination_path.c_str(),
                                    call->destination_path.size(), &result);
            napi_resolve_deferred(env, call->deferred, result);
        } else {
            const std::string message = call->error_message.empty()
                ? std::string("Correction task failed") : call->error_message;
            napi_value reason;
            napi_value error;
            napi_create_string_utf8(env, message.c_str(), message.size(), &reason);
            napi_create_error(env, nullptr, reason, &error);
            napi_reject_deferred(env, call->deferred, error);
            OH_LOG_WARN(LOG_APP, "NativeCorrectionPrepareBundledModel failed: %{public}s", message.c_str());
        }
    } else if (status != napi_ok) {
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
    correction_release_resource_manager(call);
    napi_delete_async_work(env, call->work);
    delete call;
}

static napi_value correction_queue(napi_env env, CorrectionAsyncWork* call) {
    napi_value promise;
    if (napi_create_promise(env, &call->deferred, &promise) != napi_ok) {
        correction_release_resource_manager(call);
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
        correction_release_resource_manager(call);
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
        correction_release_resource_manager(call);
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

static napi_value NativeCorrectionCheckEdits(napi_env env, napi_callback_info info) {
    size_t argc = 7;
    napi_value args[7];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 7) {
        return correction_invalid_args(env, "Expected 7 arguments: sessionId, projectId, chapterId, textRevision, baseRevision, text, editsJson");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::CheckEdits;
    if (!correction_read_session_and_revision(env, args[0], args[3], call) ||
        !correction_read_uint64(env, args[4], &call->base_revision) ||
        !correction_read_string(env, args[1], &call->project_id) ||
        !correction_read_string(env, args[2], &call->chapter_id) ||
        !correction_read_string(env, args[5], &call->text) ||
        !correction_read_string(env, args[6], &call->edits_json)) {
        delete call;
        return correction_invalid_args(env, "Invalid correction edits request");
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

// prepareBundledCorrectionModel(resourceManager, rawFileName, destinationPath,
//   expectedSize, expectedSha256): Promise<string>
//   JS 线程取 NativeResourceManager，异步线程把 HAP 内置 rawfile 分块复制到
//   destinationPath（先写 .part，校验字节数与 SHA-256 后改名），成功 resolve
//   最终文件绝对路径，失败 reject Error。resourceManager 在完成回调里释放。
static napi_value NativeCorrectionPrepareBundledModel(napi_env env, napi_callback_info info) {
    size_t argc = 5;
    napi_value args[5];
    napi_get_cb_info(env, info, &argc, args, nullptr, nullptr);
    if (argc < 5) {
        return correction_invalid_args(env,
            "Expected 5 arguments: resourceManager, rawFileName, destinationPath, expectedSize, expectedSha256");
    }
    CorrectionAsyncWork* call = new CorrectionAsyncWork();
    call->operation = CorrectionOperation::PrepareBundledModel;
    call->resource_manager = OH_ResourceManager_InitNativeResourceManager(env, args[0]);
    if (call->resource_manager == nullptr) {
        delete call;
        return correction_invalid_args(env, "resourceManager must be a valid ResourceManager object");
    }
    const bool valid = correction_read_string(env, args[1], &call->raw_file_name) &&
        !call->raw_file_name.empty() &&
        correction_read_string(env, args[2], &call->destination_path) &&
        !call->destination_path.empty() &&
        correction_read_uint64(env, args[3], &call->expected_size) &&
        call->expected_size > 0 &&
        correction_read_string(env, args[4], &call->expected_sha256) &&
        !call->expected_sha256.empty();
    if (!valid) {
        correction_release_resource_manager(call);
        delete call;
        return correction_invalid_args(env, "Invalid bundled correction model request");
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
        {"nativeCorrectionCheckEdits", nullptr, NativeCorrectionCheckEdits, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionCheckChapter", nullptr, NativeCorrectionCheckChapter, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionGetChapterSnapshot", nullptr, NativeCorrectionGetChapterSnapshot, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionIgnore", nullptr, NativeCorrectionIgnore, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionAddDictionaryEntry", nullptr, NativeCorrectionAddDictionaryEntry, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionApply", nullptr, NativeCorrectionApply, nullptr, nullptr, nullptr, napi_default, nullptr},
        {"nativeCorrectionPrepareBundledModel", nullptr, NativeCorrectionPrepareBundledModel, nullptr, nullptr, nullptr, napi_default, nullptr},
    };
    *count = sizeof(kDesc) / sizeof(kDesc[0]);
    napi_property_descriptor* out = new napi_property_descriptor[*count];
    memcpy(out, kDesc, *count * sizeof(napi_property_descriptor));
    return out;
}
