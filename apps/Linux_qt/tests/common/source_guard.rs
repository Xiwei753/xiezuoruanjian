//! 源码字符串守卫 helper — 供旧 WHITE_BOX 测试使用。
//!
//! Issue #707 评论 5723616999: 原 `qt_runtime.rs` 拆分为两部分：
//! - 本文件保留源码读取/函数窗口/模式匹配等 WHITE_BOX 辅助，供
//!   issue702/issue705/repro_issue_687 等旧测试继续使用。
//! - `qt_runtime.rs` 重写为真实 Qt runtime helper（`ensure_qt_application`），
//!   供 #707 真实行为测试使用。
//!
//! 旧测试通过 `#[path = "common/source_guard.rs"] mod source_guard;` 引入。

use std::path::PathBuf;

pub fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

pub fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// 在源码中统计某个模式的出现次数（非重叠）。
pub fn count_occurrences(src: &str, needle: &str) -> usize {
    src.matches(needle).count()
}

/// Issue #984：普通节点的 delegate 已并入 `StarMapActiveScene.qml`，
/// 旧 `StarMapNode.qml` 删除。白盒守卫统一截取这段 delegate，避免把
/// 活动画布的场景级逻辑（例如日志、进出场动画）误当成节点自身行为。
pub fn starmap_node_delegate_src() -> String {
    let src = read_src("qml/StarMapActiveScene.qml");
    let start = src
        .find("id: nodeItem")
        .expect("StarMapActiveScene.qml 必须包含节点 delegate id: nodeItem");
    let end = src[start..]
        .find("SequentialAnimation on _wobbleAnimX {")
        .map(|i| start + i)
        .expect("节点 delegate 必须以 wobble 动画收尾");
    src[start..end].to_string()
}

/// 从 `src` 中定位 `fn_marker` 并返回从该处起 `window_chars` 字符的函数体窗口。
/// 窗口结束位置回退到最近的 UTF-8 字符边界。
pub fn function_window(src: &str, fn_marker: &str, window_chars: usize) -> String {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let target_end = pos + window_chars;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    src[pos..window_end].to_string()
}

/// 检查窗口内是否出现任何"光标所有权 epoch / 失效"机制标识符。
pub fn has_cursor_owner_epoch_guard(window: &str) -> bool {
    let markers = [
        "cursor_owner_epoch",
        "owner_epoch",
        "cursor_ownership",
        "pointer_owns_cursor",
        "cursor_owner",
        "owns_cursor",
        "cursor_claim",
        "claim_epoch",
        "invalidate_cursor_owner",
        "release_cursor_owner",
        "renounce_cursor",
        "cursor_authority",
        "pointer_generation",
        "click_generation",
        "cursor_handoff",
        "handoff_epoch",
        "cursor_release",
        "release_text_transaction_cursor",
        "drop_cursor_claim",
        "cursor_claim_invalidated",
        "pointer_took_cursor",
    ];
    markers.iter().any(|m| window.contains(m))
}
