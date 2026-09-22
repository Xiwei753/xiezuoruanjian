use serde::Serialize;

/// Issue #735: Linux 私有动画模式 — 纯平台类型，不再从 Core `AnimationMode` 转换。
///
/// Core 已删除 `AnimationMode`。平台端自行决定动画模式：
/// - 正常编辑（typing / delete）→ `GlyphAnimation`
/// - 滚动 / 加载 / 格式化期间 → `SystemSuppressed`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AnimationMode {
    GlyphAnimation,
    ClusterAnimation,
    RunAnimation,
    LineReflowAnimation,
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
