//! 诊断导出 C-ABI 层 — 为 HarmonyOS NAPI 桥接提供 set_config / flush / clear / export 四个入口。
//!
//! 所有函数均为 `#[no_mangle] pub unsafe extern "C"`，符号由 `lib.rs` re-export
//! 拉入 cdylib，供 `napi_init.cpp` 链接期解析。

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::PathBuf;

use base64::Engine;
use writer_diagnostics::PlatformAttachment;

/// JSON 反序列化用的附件 DTO — `content` 是 base64 编码的字符串，
/// 解码后转为 `PlatformAttachment.content: Vec<u8>`。
#[derive(serde::Deserialize)]
struct AttachmentDto {
    relative_path: String,
    content: String,
}

/// C-ABI: 更新诊断配置（运行时切换 enabled / verbose）。
///
/// ## 参数
///
/// - `enabled`: 非 0 表示启用日志
/// - `verbose`: 非 0 表示输出 debug/trace 级别
///
/// ## 返回码
///
/// - `0` 成功
///
/// ## Safety
///
/// 无指针参数，任意 i32 值均可安全传入。
#[no_mangle]
pub unsafe extern "C" fn writer_core_set_diagnostics_config(enabled: i32, verbose: i32) -> i32 {
    writer_diagnostics::set_config(enabled != 0, verbose != 0);
    0
}

/// C-ABI: flush barrier — 阻塞直到前序日志落盘。
///
/// ## 返回码
///
/// - `0` 成功
/// - `-1` flush 失败（writer 死亡超时或写盘失败）
///
/// ## Safety
///
/// 无指针参数，安全调用。
#[no_mangle]
pub unsafe extern "C" fn writer_core_flush_diagnostics() -> i32 {
    if writer_diagnostics::flush() {
        0
    } else {
        -1
    }
}

/// C-ABI: clear barrier — 清空日志文件。
///
/// ## 返回码
///
/// - `0` 成功
/// - `-1` clear 失败（超时/中断/删除失败）
///
/// ## Safety
///
/// 无指针参数，安全调用。
#[no_mangle]
pub unsafe extern "C" fn writer_core_clear_diagnostics() -> i32 {
    if writer_diagnostics::clear() {
        0
    } else {
        -1
    }
}

/// C-ABI: 导出诊断包到 `output_dir`，返回生成的 zip 文件路径。
///
/// ## 参数
///
/// - `output_dir`: NUL-terminated UTF-8 C 字符串，导出目录路径
/// - `attachments_json`: NUL-terminated UTF-8 C 字符串，JSON 数组，
///   每项为 `{ "relative_path": String, "content": String }`，
///   `content` 为 base64 编码的二进制数据
///
/// ## 返回值
///
/// - 成功：heap-allocated C string（zip 文件路径），调用方须通过
///   `writer_core_free_string` 释放
/// - 失败：null
///
/// ## Safety
///
/// 调用方必须保证非 null 参数指向有效的 NUL-terminated UTF-8 C 字符串。
/// 返回的非 null 指针必须通过 `writer_core_free_string` 释放，不可重复释放。
#[no_mangle]
pub unsafe extern "C" fn writer_core_export_diagnostics(
    output_dir: *const c_char,
    attachments_json: *const c_char,
) -> *mut c_char {
    // SAFETY: 参数 null 检查后再 CStr::from_ptr；调用方保证非 null 指针指向有效 NUL-terminated UTF-8。
    let output_dir_str = match c_str_to_rust(output_dir) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let attachments_str = match c_str_to_rust(attachments_json) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    // 解析 attachments JSON → 解码 base64 → 构造 PlatformAttachment 列表
    let attachments = match parse_attachments(&attachments_str) {
        Ok(v) => v,
        Err(_) => return std::ptr::null_mut(),
    };

    let output_dir_path = PathBuf::from(output_dir_str);
    match writer_diagnostics::export(&output_dir_path, &attachments) {
        Ok(zip_path) => {
            let zip_str = zip_path.to_string_lossy().into_owned();
            // SAFETY: CString::new 分配 heap 内存；into_raw 把所有权转移给调用方。
            // 调用方通过 writer_core_free_string (CString::from_raw) 释放。
            match CString::new(zip_str) {
                Ok(c_str) => c_str.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}

/// 将 C string 转换为 Rust `String`。
///
/// ## 错误
///
/// - 空指针
/// - 无效 UTF-8
fn c_str_to_rust(s: *const c_char) -> Result<String, ()> {
    if s.is_null() {
        return Err(());
    }
    // SAFETY: s is null-checked above; the C ABI caller guarantees a valid NUL-terminated UTF-8 string.
    match unsafe { CStr::from_ptr(s) }.to_str() {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(()),
    }
}

/// 解析 attachments JSON 数组，解码 base64 content，构造 `PlatformAttachment` 列表。
fn parse_attachments(json: &str) -> Result<Vec<PlatformAttachment>, ()> {
    let dtos: Vec<AttachmentDto> = serde_json::from_str(json).map_err(|_| ())?;
    let mut attachments = Vec::with_capacity(dtos.len());
    for dto in dtos {
        let content = base64::engine::general_purpose::STANDARD
            .decode(&dto.content)
            .map_err(|_| ())?;
        attachments.push(PlatformAttachment {
            relative_path: dto.relative_path,
            content,
        });
    }
    Ok(attachments)
}
