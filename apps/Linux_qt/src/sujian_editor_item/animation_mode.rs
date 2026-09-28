use serde::Serialize;

/// Issue #735: Linux 私有动画模式 — 纯平台类型，不再从 Core `AnimationMode` 转换。
///
/// Core 已删除 `AnimationMode`。平台端自行决定动画模式，只保留两种：
/// - 正常编辑（typing / delete）→ `GlyphAnimation`
/// - 滚动 / 加载 / 格式化期间 → `SystemSuppressed`
///
/// 原先还有 `ClusterAnimation` / `RunAnimation` / `LineReflowAnimation`
/// 三个 cluster 级、run 级、整行 reflow 变体，但 `from_context()` 从来没有
/// 产出过它们（#785 起文字 unit 一律走 Timed timing），三个变体是纯死状态，
/// 按仓库约定"被新实现替代的旧入口直接删除"移除。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AnimationMode {
    GlyphAnimation,
    SystemSuppressed,
}

impl AnimationMode {
    /// Issue #735: 从编辑上下文派生动画模式。
    ///
    /// 滚动 / 加载 / 格式化期间返回 `SystemSuppressed`，否则返回 `GlyphAnimation`。
    pub fn from_context(is_scrolling: bool, is_loading: bool, is_applying_format: bool) -> Self {
        if is_scrolling || is_loading || is_applying_format {
            AnimationMode::SystemSuppressed
        } else {
            AnimationMode::GlyphAnimation
        }
    }

    pub fn should_create_transaction(&self) -> bool {
        !matches!(self, AnimationMode::SystemSuppressed)
    }
}
