//! FFI 层布局契约操作（：输入为原始窗口尺寸，不再接收平台已判断好的窗口能力）
//!
//! Issue #753：入口与出口都用 `api/types/platform.rs` 的 camelCase DTO，
//! FFI 只做搬运，不定义第二套字段。

use std::os::raw::c_char;

use crate::api::types::{LayoutContractDto, WindowViewportDto};
use crate::ffi::{c_str_to_rust, err_json, ok_json};

/// # Safety
/// `viewport_json` must be a valid null-terminated UTF-8 C string containing JSON `WindowViewportDto`.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_layout(viewport_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(viewport_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("viewport_json is null or invalid UTF-8: {}", e),
            );
        }
    };

    let viewport: WindowViewportDto = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse WindowViewportDto JSON: {e}"),
            );
        }
    };

    let contract = crate::presentation::layout::resolve_layout(&viewport.into());
    ok_json(LayoutContractDto::from(contract))
}
