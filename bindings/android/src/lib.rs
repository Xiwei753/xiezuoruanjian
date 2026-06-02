//! # Android JNI 桥接层（Binding 层）
//!
//! 本 crate 是 Android 客户端与 Rust Core 之间的桥梁。
//!
//! ## 架构定位
//!
//! ```text
//! Android Kotlin UI → NativeCoreBridge.kt → JNI Binding → WriterCoreApi → Facade/Core modules
//! ```
//!
//! ## 职责边界
//!
//! - **做**：JString ↔ String 转换、Result → 兼容 DTO 序列化、JNI 函数注册
//! - **不做**：业务逻辑（全部委托给 `WriterCoreApi`）
//! - **不做**：直接依赖 `writer_core::facade::WriterCore`。`WriterCore` 是 Core 内部协调层，不是平台稳定边界。JNI 只能调用 `WriterCoreApi`。
//! - **不做**：错误处理（错误通过稳定 code/message 返回给 Kotlin）
//!
//! ## 兼容协议
//!
//! 旧 JNI 函数仍返回 JSON 字符串，但错误必须包含稳定 code：
//! - 成功：`{ "success": true, "data": <实际数据> }`
//! - 失败：`{ "success": false, "code": "...", "error": "<错误信息>" }`
//!
//! ## 注意事项
//!
//! - 本 crate 不允许添加业务逻辑，只做类型转换
//! - 所有业务调用均通过 `WriterCoreApi` 暴露的稳定接口进行，禁止直接 `new()` Core。

use jni::objects::{JClass, JObject, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;
use serde::Serialize;
use writer_core::api::{ResultEnvelope, WriterCoreApi};

mod chapter_jni;
mod mindmap_jni;
mod misc_jni;
mod project_jni;
mod settings_jni;
mod starmap_jni;
mod sync_jni;
mod workspace_jni;
mod writing_stats_jni;

fn api_from_workspace(workspace_path: &str) -> WriterCoreApi {
    WriterCoreApi::new(workspace_path)
}

/// 将 JString 转换为 Rust String。
///
/// 如果 JString 为 null，返回空字符串。
fn jstring_to_string(env: &mut JNIEnv, jstr: &JString) -> Result<String, String> {
    if jstr.is_null() {
        return Ok(String::new());
    }
    env.get_string(jstr)
        .map(|s| s.into())
        .map_err(|e| format!("Failed to get string: {}", e))
}

/// 将 Core 层 Result 转换为 JSON 字符串返回给 Kotlin。
///
/// 成功/失败均返回标准 ResultEnvelope JSON。
fn result_to_jstring<T: Serialize>(
    env: &mut JNIEnv,
    result: Result<T, writer_core::api::error::WriterError>,
) -> jstring {
    let json_str = ResultEnvelope::from_api_result(result).to_json_string();

    string_to_jstring(env, json_str)
}

fn string_to_jstring(env: &mut JNIEnv, value: String) -> jstring {
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[cfg(test)]
mod architecture_tests {
    use std::fs;

    #[test]
    fn test_no_direct_facade_usage() {
        let src_dir = "src";
        let forbidden_facade = vec!["writer_core::", "facade::", "WriterCore"].join("");
        let forbidden_new = vec!["WriterCore::", "new("].join("");

        let mut violations = Vec::new();

        for entry in fs::read_dir(src_dir).expect("Failed to read src dir") {
            let entry = entry.expect("Failed to read entry");
            let path = entry.path();
            if path.extension().map_or(true, |e| e != "rs") {
                continue;
            }
            let content = fs::read_to_string(&path).expect("Failed to read file");
            let filename = path.file_name().unwrap().to_string_lossy();

            for (idx, line) in content.lines().enumerate() {
                if line.contains("forbidden_facade")
                    || line.contains("forbidden_new")
                    || line.trim().starts_with("//")
                    || line.trim().starts_with("panic!")
                {
                    continue;
                }
                if line.contains(&forbidden_facade) || line.contains(&forbidden_new) {
                    violations.push(format!("{}:{}: {}", filename, idx + 1, line));
                }
            }
        }

        if !violations.is_empty() {
            for v in &violations {
                eprintln!("{}", v);
            }
            panic!("Architecture violation: Found direct usage of Facade or WriterCore constructor in JNI layer.");
        }
    }
}
