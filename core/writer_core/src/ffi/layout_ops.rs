//! FFI 层布局契约操作（#610：输入为平台已判断好的窗口能力）

use std::os::raw::c_char;

use crate::ffi::{c_str_to_rust, err_json, ok_json};
use crate::presentation::layout_contract::{resolve_layout, WindowCapabilities};

/// # Safety
/// `capabilities_json` must be a valid null-terminated UTF-8 C string containing JSON WindowCapabilities.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_layout(
    capabilities_json: *const c_char,
) -> *mut c_char {
    let json_str = match c_str_to_rust(capabilities_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("capabilities_json is null or invalid UTF-8: {}", e),
            );
        }
    };

    let capabilities: WindowCapabilities = match serde_json::from_str(&json_str) {
        Ok(c) => c,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse WindowCapabilities JSON: {}", e),
            );
        }
    };

    let contract = resolve_layout(&capabilities);
    ok_json(contract)
}
