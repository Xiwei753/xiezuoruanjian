//! Issue #701 评论 5705239656 结构守卫 — 程序化光标跟随滚动不清文字动画。
//!
//! WHITE_BOX 验证：set_scroll_y() 不再无条件 clear_active_text_animations() /
//! force_snap_next=true。scroll_y 只是 viewport transform，真实用户滚动抑制
//! 由 set_is_scrolling(true) 独立路径处理。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 取出某个方法从签名到函数体结束（首个 4 空格缩进的 `}`）之间的文本。
fn method_body(src: &str, signature: &str) -> String {
    let start = src
        .find(signature)
        .unwrap_or_else(|| panic!("method `{}` must exist", signature));
    let rest = &src[start..];
    let end = rest
        .find("\n    }\n")
        .unwrap_or_else(|| panic!("method `{}` body end not found", signature));
    rest[..end].to_string()
}

#[test]
fn issue701_set_scroll_y_does_not_clear_text_animations() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let body = method_body(&src, "fn set_scroll_y(");
    assert!(
        !body.contains("clear_active_text_animations"),
        "Issue #701 评论 5705239656: set_scroll_y() 不得无条件清文字动画。\
         程序化 ensureCursorVisible() 滚动会通过 scroll_y 绑定回流到此，\
         清动画会把刚建立的 VisualTransaction 清掉。\
         真实用户滚动抑制由 set_is_scrolling(true) 独立路径处理。\
         set_scroll_y body=\n{}",
        body
    );
    println!("[BUGFIX_701_VERIFY] set_scroll_y 不再清文字动画 (FIXED)");
}

#[test]
fn issue701_set_scroll_y_does_not_force_snap() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let body = method_body(&src, "fn set_scroll_y(");
    assert!(
        !body.contains("force_snap_next"),
        "Issue #701 评论 5705239656: set_scroll_y() 不得设 force_snap_next=true。\
         程序化 auto-follow 期间光标应按当前动画进度采样，不应强制瞬移。\
         set_scroll_y body=\n{}",
        body
    );
    println!("[BUGFIX_701_VERIFY] set_scroll_y 不再强制 snap (FIXED)");
}

#[test]
fn issue701_set_scroll_y_still_updates_viewport() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let body = method_body(&src, "fn set_scroll_y(");
    assert!(
        body.contains("self.current_scroll_y = value"),
        "Issue #701: set_scroll_y() 必须仍更新 current_scroll_y"
    );
    assert!(
        body.contains("self.update_cursor_visual_position()"),
        "Issue #701: set_scroll_y() 必须仍更新 caret viewport 坐标"
    );
    assert!(
        body.contains("self.request_frame_update()"),
        "Issue #701: set_scroll_y() 必须仍请求下一帧"
    );
    println!("[BUGFIX_701_VERIFY] set_scroll_y 仍做 viewport transform 更新 (OK)");
}

#[test]
fn issue701_user_scroll_suppression_still_in_is_scrolling_path() {
    let src = read_src("src/sujian_editor_item/properties.rs");
    let body = method_body(&src, "fn set_is_scrolling(");
    assert!(
        body.contains("pause_all"),
        "Issue #701: 用户滚动动画抑制必须仍在 set_is_scrolling(true) 路径（pause_all）"
    );
    assert!(
        body.contains("resume_all"),
        "Issue #701: 用户滚动恢复必须仍在 set_is_scrolling(false) 路径（resume_all）"
    );
    println!("[BUGFIX_701_VERIFY] 用户滚动抑制仍在 set_is_scrolling 独立路径 (OK)");
}
