// ── Editor Session NAPI 参数读取（编辑 session 域统一入口）──
//
// 由 napi_editor_session.cpp include（该文件又被 napi_init.cpp #include，属于同一
// 翻译单元）。全部函数都是 static，避免与其它 NAPI 域的同名 helper 冲突。
//
// 约定：
// - 读取失败（参数缺失、类型不符、范围非法、Node-API 调用失败）返回 false，
//   出参保持 0/nullptr；调用方必须显式返回 INVALID_ARGUMENT 错误 JSON，
//   不能把未初始化的 buffer 或 0 当成合法值继续调用 Rust C ABI。
// - 字符串出参成功时为 new[] 分配的 NUL 结尾 UTF-8 副本，调用方用 delete[] 释放。

#ifndef NAPI_EDITOR_ARGS_H
#define NAPI_EDITOR_ARGS_H

#include <cmath>
#include <cstdint>
#include <new>

#include <napi/native_api.h>

// 编辑 session 参数非法时的统一 ResultEnvelope 错误字面量。
// 字段与 Core ffi/mod.rs 的 err_json（success/errorCode/userMessage）以及
// ArkTS ResultEnvelope 对齐；INVALID_ARGUMENT 已在 ResultEnvelope.ErrorCode 中登记。
// 注意：这是静态字面量，不能交给 ReturnJsonString —— 后者按 Core 所有权对其
// 参数调用 writer_core_free_string，只能接收 writer_core_* 返回的 char*。
static const char* napi_editor_invalid_arg_json() {
    return "{\"success\":false,\"errorCode\":\"INVALID_ARGUMENT\","
           "\"userMessage\":\"Invalid native argument\"}";
}

// napi_arg_get_string: 读取必填字符串参数并复制为 NUL 结尾的 UTF-8 C 字符串。
//   1. 校验 napi_typeof == napi_string；
//   2. 第一次 napi_get_value_string_utf8 只查询字节长度并检查 status；
//   3. 分配 length + 1 字节；
//   4. 第二次复制检查 status，且实际写入量不得超过查询长度；
//   5. 任何失败都释放 buffer、保持 *out = nullptr 并返回 false。
static bool napi_arg_get_string(napi_env env, napi_value value, char** out) {
    if (out == nullptr) {
        return false;
    }
    *out = nullptr;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, value, &type) != napi_ok || type != napi_string) {
        return false;
    }

    // 第一次调用只查询 UTF-8 字节长度（不含终止符）。
    size_t length = 0;
    if (napi_get_value_string_utf8(env, value, nullptr, 0, &length) != napi_ok) {
        return false;
    }

    char* buffer = new (std::nothrow) char[length + 1];
    if (buffer == nullptr) {
        return false;
    }

    size_t written = 0;
    if (napi_get_value_string_utf8(env, value, buffer, length + 1, &written) != napi_ok
        || written > length) {
        delete[] buffer;
        return false;
    }
    // buffer 足够时 Node-API 已写入终止符；这里再显式补一次，避免把未初始化字节
    // 当成字符串内容交给 Rust。
    buffer[written] = '\0';
    *out = buffer;
    return true;
}

// napi_arg_get_u64: 读取必填 uint64 参数（ArkTS number → int64 → uint64）。
//   ArkTS 的 sessionId/revision 用 number 表示：这里拒绝负数、非整数、NaN/Infinity
//   以及超出 int64 上界（2^63）的值，避免把非数字静默当成 0。
static bool napi_arg_get_u64(napi_env env, napi_value value, uint64_t* out) {
    if (out == nullptr) {
        return false;
    }
    *out = 0;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, value, &type) != napi_ok || type != napi_number) {
        return false;
    }

    double as_double = 0;
    if (napi_get_value_double(env, value, &as_double) != napi_ok) {
        return false;
    }
    // 9223372036854775808.0 == 2^63，double 可精确表示 int64 的上界。
    if (!std::isfinite(as_double) || std::floor(as_double) != as_double
        || as_double < 0 || as_double >= 9223372036854775808.0) {
        return false;
    }

    int64_t as_int64 = 0;
    if (napi_get_value_int64(env, value, &as_int64) != napi_ok || as_int64 < 0) {
        return false;
    }
    *out = static_cast<uint64_t>(as_int64);
    return true;
}

// napi_arg_get_u32: 读取必填 uint32 参数（byte offset 等）。
//   先用 double 校验非负、有限、整数且在 [0, UINT32_MAX]，再用 int64 做范围复核，
//   不依赖 napi_get_value_uint32 对负数/小数的隐式截断与回绕。
static bool napi_arg_get_u32(napi_env env, napi_value value, uint32_t* out) {
    if (out == nullptr) {
        return false;
    }
    *out = 0;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, value, &type) != napi_ok || type != napi_number) {
        return false;
    }

    double as_double = 0;
    if (napi_get_value_double(env, value, &as_double) != napi_ok) {
        return false;
    }
    if (!std::isfinite(as_double) || std::floor(as_double) != as_double
        || as_double < 0 || as_double > static_cast<double>(UINT32_MAX)) {
        return false;
    }

    int64_t as_int64 = 0;
    if (napi_get_value_int64(env, value, &as_int64) != napi_ok
        || as_int64 < 0 || static_cast<uint64_t>(as_int64) > static_cast<uint64_t>(UINT32_MAX)) {
        return false;
    }
    *out = static_cast<uint32_t>(as_int64);
    return true;
}

#endif  // NAPI_EDITOR_ARGS_H
