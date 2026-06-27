//! FFI 层页面策略操作

use std::os::raw::c_char;

use crate::ffi::{c_str_to_rust, err_json, ok_json};
use crate::layout_policy::ShellMode;
use crate::screen_policy::{resolve_screen_policy, ScreenRole};

/// # Safety
/// `screen_role_json` and `shell_mode_json` must be valid null-terminated UTF-8 C strings.
/// Returns a caller-owned C string. Free with `writer_core_free_string`.
#[no_mangle]
pub unsafe extern "C" fn writer_core_resolve_screen_policy(
    screen_role_json: *const c_char,
    shell_mode_json: *const c_char,
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

    let mode_str = match c_str_to_rust(shell_mode_json) {
        Ok(s) => s,
        Err(e) => {
            return err_json(
                "INVALID_INPUT",
                &format!("shell_mode_json is null or invalid UTF-8: {}", e),
            );
        }
    };

    let shell_mode: ShellMode = match serde_json::from_str(&mode_str) {
        Ok(m) => m,
        Err(e) => {
            return err_json(
                "PARSE_ERROR",
                &format!("Failed to parse ShellMode JSON: {}", e),
            );
        }
    };

    let action_slots = resolve_screen_policy(screen_role, shell_mode);

    use crate::api::types::screen_policy::*;
    let dto = ScreenPolicyDto {
        screen_role: screen_role.into(),
        action_slots: action_slots.into_iter().map(Into::into).collect(),
    };
    ok_json(dto)
}
