//! FFI 层页面契约操作（#610 / #628：动作区域与顺序是产品语义，不随壳层变化；
//! ScreenPolicy 含 show_primary_navigation 由 Rust 决定）

use std::os::raw::c_char;

use crate::ffi::{c_str_to_rust, err_json, ok_json};
use crate::presentation::screen::{resolve_screen_policy, ScreenRole};

/// # Safety
/// `screen_role_json` must be a valid null-terminated UTF-8 C string.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_screen_policy(
    screen_role_json: *const c_char,
) -> *mut c_char {
    let role_str = match c_str_to_rust(screen_role_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("screen_role_json is null or invalid UTF-8: {}", e),
            );
        }
    };

    let screen_role: ScreenRole = match serde_json::from_str(&role_str) {
        Ok(r) => r,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse ScreenRole JSON: {}", e),
            );
        }
    };

    let policy = resolve_screen_policy(screen_role);

    use crate::api::types::screen_policy::*;
    let dto = ScreenPolicyDto {
        screen_role: policy.screen_role.into(),
        action_slots: policy.action_slots.into_iter().map(Into::into).collect(),
        show_primary_navigation: policy.show_primary_navigation,
    };
    ok_json(dto)
}
