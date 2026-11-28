// ── 共享 NAPI 参数读取（章节/项目/卷/星图/编辑 session 域统一入口）──
//
// 由 napi_init.cpp 在 include 各域实现之前 include；napi_editor_session.cpp 也会
// include（同一翻译单元，include guard 保证只定义一次）。全部函数都是 static，
// 避免与其它 NAPI 域的同名 helper 冲突。
//
// 约定：
// - 读取失败（参数缺失、类型不符、范围非法、Node-API 调用失败）返回 false，
//   出参保持 0/nullptr；调用方必须显式返回 INVALID_ARGUMENT 错误 JSON，
//   不能把未初始化的 buffer 或 0 当成合法值继续调用 Rust C ABI。
// - JSON 返回型 handler 统一用 napi_arg_invalid_result() 构造 INVALID_ARGUMENT
//   信封；int32/其它非 JSON 返回值的 handler 用 napi_arg_throw_invalid() 抛
//   INVALID_ARGUMENT 异常，不能改变各自对 ArkTS 的返回值类型契约。
// - 字符串出参成功时为 new[] 分配的 NUL 结尾 UTF-8 副本，调用方用 delete[] 释放。
// - 字符串按 CStr 传给 Rust，因此**不支持内嵌 U+0000**：含 NUL 的 ArkTS 字符串
//   判为非法参数（返回 false，由调用方转 INVALID_ARGUMENT），不能静默截断。
//   若未来确实需要传递含 NUL 的原始文本，必须改用显式长度 ABI 或 JSON 转义，
//   不能继续走 CStr。
// - sessionId / revision / composition_* 等 u64 参数和 byte offset 等 u32 参数
//   必须落在 JS 安全整数范围内（Number.MAX_SAFE_INTEGER = 2^53 - 1），因为
//   ArkTS number 只在安全整数内精确；超出后 double 已经舍入，不能再当成 ID 使用。
//   ID/revision 不接受 2^53 及以上的值，也不接受 NaN/Infinity/负数/小数。

#ifndef NAPI_EDITOR_ARGS_H
#define NAPI_EDITOR_ARGS_H

#include <cmath>
#include <cstdint>
#include <cstring>
#include <new>

#include <napi/native_api.h>

// JS 安全整数上界：Number.MAX_SAFE_INTEGER == 2^53 - 1 == 9007199254740991，
// 该值在 double 中精确可表示；2^53 本身已超出安全整数范围。
static constexpr double kNapiArgMaxSafeInteger = 9007199254740991.0;

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
//   2. 第一次 napi_get_value_string_utf8 只查询字节长度并检查 status；长度为
//      SIZE_MAX 时 len + 1 会溢出，直接失败；
//   3. 用 nothrow new[] 分配 length + 1 字节，分配失败返回 false；
//   4. 第二次复制检查 status，且实际写入量不得超过查询长度；
//   5. 显式写 buffer[written] = '\0'，不依赖 Node-API 是否补终止符；
//   6. 在写入的 written 字节内用 memchr 检查内嵌 NUL：C ABI 只能传 NUL 结尾
//      字符串，内嵌 U+0000 会截断正文，故判为非法参数（见文件头约定）；
//   7. 任何失败都 delete[] buffer、保持 *out = nullptr 并返回 false，
//      不会把未初始化或已释放的指针交给调用方。
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
    if (length == SIZE_MAX) {
        // length + 1 溢出，无法分配终止符。
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
    // 显式补终止符，避免把未初始化字节当成字符串内容交给 Rust。
    buffer[written] = '\0';

    // written 字节内出现 NUL 说明原字符串包含 U+0000，CStr 无法无损表示。
    if (std::memchr(buffer, '\0', written) != nullptr) {
        delete[] buffer;
        *out = nullptr;
        return false;
    }

    *out = buffer;
    return true;
}

// napi_arg_get_u64: 读取必填 uint64 参数（ArkTS number → int64 → uint64）。
//   ArkTS 的 sessionId/revision 用 number 表示：先要求有限、整数、非负且
//   <= Number.MAX_SAFE_INTEGER（2^53 - 1），再用 napi_get_value_int64 复核
//   数值相等后输出。2^53 及以上的 double 已经丢失整数精度，不能舍入后当 ID。
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
    if (!std::isfinite(as_double) || std::trunc(as_double) != as_double
        || as_double < 0 || as_double > kNapiArgMaxSafeInteger) {
        return false;
    }

    // int64 复核：值必须与 double 完全一致，避免任何隐式截断/回绕。
    int64_t as_int64 = 0;
    if (napi_get_value_int64(env, value, &as_int64) != napi_ok
        || as_int64 < 0 || static_cast<double>(as_int64) != as_double) {
        return false;
    }
    *out = static_cast<uint64_t>(as_int64);
    return true;
}

// napi_arg_get_u32: 读取必填 uint32 参数（byte offset 等）。
//   先按 u64 的同一套规则校验非负、有限、整数且 <= MAX_SAFE_INTEGER，
//   再额外要求 <= UINT32_MAX；最后用 int64 做范围与数值相等复核，
//   不依赖 napi_get_value_uint32 对负数/小数/超界值的隐式截断与回绕。
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
    if (!std::isfinite(as_double) || std::trunc(as_double) != as_double
        || as_double < 0 || as_double > kNapiArgMaxSafeInteger
        || as_double > static_cast<double>(UINT32_MAX)) {
        return false;
    }

    int64_t as_int64 = 0;
    if (napi_get_value_int64(env, value, &as_int64) != napi_ok
        || as_int64 < 0 || static_cast<uint64_t>(as_int64) > static_cast<uint64_t>(UINT32_MAX)
        || static_cast<double>(as_int64) != as_double) {
        return false;
    }
    *out = static_cast<uint32_t>(as_int64);
    return true;
}

// napi_arg_get_bool: 读取必填 bool 参数。
//   先校验 napi_typeof == napi_boolean，再读取；类型不符/读取失败返回 false，
//   *out 保持 false。用于 allow_empty_overwrite / is_embed 这类会改变业务语义的开关，
//   不能把非 bool 参数当成默认 false 继续执行。
static bool napi_arg_get_bool(napi_env env, napi_value value, bool* out) {
    if (out == nullptr) {
        return false;
    }
    *out = false;

    napi_valuetype type = napi_undefined;
    if (napi_typeof(env, value, &type) != napi_ok || type != napi_boolean) {
        return false;
    }
    return napi_get_value_bool(env, value, out) == napi_ok;
}

// napi_arg_invalid_result: JSON 返回型 handler 的统一 INVALID_ARGUMENT 返回值。
//   napi_editor_invalid_arg_json() 是静态字面量，不能交给 ReturnJsonString
//   （后者按 Core 所有权调用 writer_core_free_string），这里直接构造 NAPI 字符串。
//   字符串创建失败与其它 NAPI 字符串失败保持同一语义：抛 E_NAPI_STRING 并返回 nullptr。
static napi_value napi_arg_invalid_result(napi_env env) {
    napi_value result = nullptr;
    if (napi_create_string_utf8(env, napi_editor_invalid_arg_json(),
                                NAPI_AUTO_LENGTH, &result) != napi_ok) {
        napi_throw_error(env, "E_NAPI_STRING", "Failed to create INVALID_ARGUMENT JSON string");
        return nullptr;
    }
    return result;
}

// napi_arg_throw_invalid: 非 JSON 返回值 handler（如 nativeInit 的 int32 状态码）
//   的参数错误出口。这些 API 的 ArkTS 调用方按数值接收结果，不能塞错误信封字符串，
//   因此以 INVALID_ARGUMENT 错误码抛 JS 异常；调用方的 try/catch 会转成 NATIVE_ERROR。
static napi_value napi_arg_throw_invalid(napi_env env, const char* detail) {
    napi_throw_error(env, "INVALID_ARGUMENT",
                     detail != nullptr ? detail : "Invalid native argument");
    return nullptr;
}

#endif  // NAPI_EDITOR_ARGS_H
