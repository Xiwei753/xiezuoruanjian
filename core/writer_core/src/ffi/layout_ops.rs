//! FFI 层布局策略操作

use std::os::raw::c_char;

use crate::ffi::{c_str_to_rust, err_json, ok_json};
use crate::layout_policy::{resolve_layout, WindowMetrics};

/// # Safety
/// `metrics_json` must be a valid null-terminated UTF-8 C string containing JSON WindowMetrics.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_layout(metrics_json: *const c_char) -> *mut c_char {
    let json_str = match c_str_to_rust(metrics_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("metrics_json is null or invalid UTF-8: {}", e),
            );
        }
    };

    let metrics: WindowMetrics = match serde_json::from_str(&json_str) {
        Ok(m) => m,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse WindowMetrics JSON: {}", e),
            );
        }
    };

    let plan = resolve_layout(&metrics);
    ok_json(plan)
}
