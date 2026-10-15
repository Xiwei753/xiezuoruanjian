// =============================================================================
// native/kde_color_scheme.rs — KDE 应用配色方案桥（Rust 侧）
// =============================================================================
//
// 包装 C++ 侧 `sujian_kde_color_scheme_apply`（见 kde_color_scheme_bridge.cpp）。
// 该符号由 build.rs 把 native/*.cpp 编入同一静态库后链接得到。
// =============================================================================

use std::ffi::CString;

extern "C" {
    fn sujian_kde_color_scheme_apply(mode: *const std::os::raw::c_char) -> std::os::raw::c_int;
}

/// 按主题模式切换 KDE 应用配色方案。
///
/// `mode` 只接受 `"system"` / `"dark"` / `"light"`；其他值视为失败。
/// 返回 `true` 表示已成功应用（含 system 复位），`false` 表示未切换任何方案。
pub(crate) fn apply_appearance_mode(mode: &str) -> bool {
    let Ok(c_mode) = CString::new(mode) else {
        return false;
    };
    // SAFETY: c_mode 是以 NUL 结尾的有效 C 字符串，且在本调用期间保持存活；
    // 桥函数只读取该指针，不持有它。
    let result = unsafe { sujian_kde_color_scheme_apply(c_mode.as_ptr()) };
    result == 0
}
