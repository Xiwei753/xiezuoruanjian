use std::time::Instant;

/// 动画类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextAnimationKind {
    Insert,
    Delete,
}

/// 统一动画判定结果。
///
/// Rust 创建 hidden range 前和 QML 创建 ghost 前必须使用同一套规则，
/// 避免 Rust 已经 hidden range 但 QML 跳过导致文字短暂消失。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimationDecision {
    /// 完全不做动画（不创建 hidden range，不创建 ghost）
    NoAnimation,
    /// 只做光标动画（不创建 hidden range，不创建文字 ghost）
    CursorOnly,
    /// 完整动画（创建 hidden range + 文字 ghost + 光标动画）
    FullAnimation,
}

/// 统一动画判定函数。
///
/// Rust 侧 `record_transaction` 和 QML 侧 `EditorAnimationOverlay` 必须使用
/// 相同的判定规则，确保 hidden range 和 ghost 的创建/跳过完全一致。
///
/// 规则：
/// - glyph 为空：不动画
/// - glyph 超过上限（8）：不动画
/// - 包含换行：文字不动画，但光标仍可动画（CursorOnly）
/// - scrolling/loading/settings/applyingFormat：不动画
/// - component not ready：不动画
/// - 不动画时绝对不能 start_insert hidden range
/// - 如果已经 start_insert 但 overlay 跳过，必须立即 clear_active_text_animations
pub(crate) fn should_create_text_animation(
    glyph_count: usize,
    contains_newline: bool,
    is_scrolling: bool,
    is_loading: bool,
    is_applying_format: bool,
    is_applying_settings: bool,
    animation_enabled: bool,
    component_ready: bool,
) -> AnimationDecision {
    const MAX_GLYPH_COUNT: usize = 8;

    // 全局抑制条件：这些情况下完全不做动画
    if !animation_enabled {
        return AnimationDecision::NoAnimation;
    }
    if is_scrolling || is_loading || is_applying_format || is_applying_settings {
        return AnimationDecision::NoAnimation;
    }
    if !component_ready {
        return AnimationDecision::NoAnimation;
    }

    // Glyph 为空：不动画
    if glyph_count == 0 {
        return AnimationDecision::NoAnimation;
    }

    // Glyph 超过上限：不动画
    if glyph_count > MAX_GLYPH_COUNT {
        return AnimationDecision::NoAnimation;
    }

    // 包含换行：文字不动画，但光标仍可动画
    if contains_newline {
        return AnimationDecision::CursorOnly;
    }

    // 所有条件通过：完整动画
    AnimationDecision::FullAnimation
}

/// 单个活跃动画条目
#[derive(Clone, Debug)]
pub(crate) struct ActiveTextAnimation {
    pub kind: TextAnimationKind,
    pub byte_range: (usize, usize),
    pub start_time: Instant,
    pub duration_ms: u64,
}

/// 文本动画状态机 — 独立于 Qt/SujianEditorItem 的动画生命周期管理。
///
/// 所有停止动画路径都必须立即清理 hidden range：
/// - scroll / reload / visual_change / typing_animation_disabled / timeout
///
/// Insert 动画期间，正文层跳过 inserted range 不绘制，由 QML overlay 显示 glyph。
/// Delete 动画不需要正文层跳过，但记录以跟踪活跃动画。
pub(crate) struct TextAnimationState {
    animations: Vec<ActiveTextAnimation>,
}

impl TextAnimationState {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
        }
    }

    /// 开始一个 Insert 动画
    pub fn start_insert(&mut self, byte_range: (usize, usize), duration_ms: u64) {
        self.animations.push(ActiveTextAnimation {
            kind: TextAnimationKind::Insert,
            byte_range,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    /// 开始一个 Delete 动画
    pub fn start_delete(&mut self, byte_range: (usize, usize), duration_ms: u64) {
        self.animations.push(ActiveTextAnimation {
            kind: TextAnimationKind::Delete,
            byte_range,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    /// 清空所有动画（立即清理 hidden range）
    /// 用于：set_plain_text / reload / visual_changed / typing_animation_disabled / scroll
    pub fn clear(&mut self) {
        self.animations.clear();
    }

    /// 滚动时立即清理（等价于 clear，语义明确）
    pub fn clear_on_scroll(&mut self) {
        self.clear();
    }

    /// 重新加载时立即清理
    pub fn clear_on_reload(&mut self) {
        self.clear();
    }

    /// 视觉变更（字号/行距/缩进）时立即清理
    pub fn clear_on_visual_change(&mut self) {
        self.clear();
    }

    /// 关闭打字动画时立即清理 hidden range，不依赖 timeout 恢复文字
    pub fn clear_on_typing_animation_disabled(&mut self) {
        self.clear();
    }

    /// QML 动画 overlay 通知 Insert 动画完成，清除对应的隐藏 range。
    /// 返回 true 表示确实移除了匹配的 Insert 动画（需要触发重绘）。
    pub fn on_insert_animation_finished(&mut self, byte_start: usize, byte_end: usize) -> bool {
        let before = self.animations.len();
        self.animations.retain(|anim| {
            !(anim.kind == TextAnimationKind::Insert && anim.byte_range == (byte_start, byte_end))
        });
        self.animations.len() < before
    }

    /// 超时安全机制：检查活跃文本动画是否超时，超时则清除。
    /// 给 2x duration + 200ms 作为宽限期，防止 QML 动画和 Rust 超时不同步。
    /// 返回 true 如果有动画被清除（需要触发重绘）。
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.animations.is_empty() {
            return false;
        }
        let before = self.animations.len();
        self.animations.retain(|anim| {
            let elapsed = now.duration_since(anim.start_time).as_millis() as u64;
            elapsed < anim.duration_ms * 2 + 200
        });
        self.animations.len() != before
    }

    /// 获取活跃 Insert 动画的 byte range
    pub fn active_insert_byte_range(&self) -> Option<(usize, usize)> {
        self.animations
            .iter()
            .find(|a| a.kind == TextAnimationKind::Insert)
            .map(|a| a.byte_range)
    }

    /// 是否有活跃的 Insert 动画
    pub fn has_active_insert(&self) -> bool {
        self.animations
            .iter()
            .any(|a| a.kind == TextAnimationKind::Insert)
    }

    /// 是否没有任何活跃动画
    pub fn is_empty(&self) -> bool {
        self.animations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_insert_creates_active_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        assert_eq!(state.active_insert_byte_range(), Some((10, 20)));
        assert!(state.has_active_insert());
        assert!(!state.is_empty());
    }

    #[test]
    fn test_delete_does_not_create_hidden_range() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), 100);
        // Delete 动画不产生 hidden range
        assert_eq!(state.active_insert_byte_range(), None);
        assert!(!state.has_active_insert());
        // 但状态机不为空（有 Delete 动画在跟踪）
        assert!(!state.is_empty());
    }

    #[test]
    fn test_clear_removes_all() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        state.start_delete((30, 40), 100);
        assert!(!state.is_empty());
        state.clear();
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
        assert!(!state.has_active_insert());
    }

    #[test]
    fn test_clear_on_typing_animation_disabled() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        assert!(state.has_active_insert());
        state.clear_on_typing_animation_disabled();
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    #[test]
    fn test_clear_on_scroll() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        assert!(state.has_active_insert());
        state.clear_on_scroll();
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    #[test]
    fn test_clear_on_reload() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        assert!(state.has_active_insert());
        state.clear_on_reload();
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    #[test]
    fn test_clear_on_visual_change() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        assert!(state.has_active_insert());
        state.clear_on_visual_change();
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    #[test]
    fn test_timeout_clears_animation() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        // duration=100, grace = 2*100 + 200 = 400ms
        // 超过宽限期后 tick 应清除
        let now = Instant::now() + Duration::from_millis(401);
        let cleared = state.tick(now);
        assert!(cleared);
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    #[test]
    fn test_within_grace_period_not_cleared() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        // duration=100, grace = 2*100 + 200 = 400ms
        // 在宽限期内 tick 不应清除
        let now = Instant::now() + Duration::from_millis(300);
        let cleared = state.tick(now);
        assert!(!cleared);
        assert!(!state.is_empty());
        assert_eq!(state.active_insert_byte_range(), Some((10, 20)));
    }

    #[test]
    fn test_on_insert_animation_finished_removes_matching() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        assert!(state.has_active_insert());
        state.on_insert_animation_finished(10, 20);
        assert!(state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
    }

    #[test]
    fn test_on_insert_animation_finished_keeps_others() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        state.start_delete((30, 40), 100);
        // 完成 Insert (10,20)，Delete (30,40) 应保留
        state.on_insert_animation_finished(10, 20);
        assert!(!state.is_empty());
        assert_eq!(state.active_insert_byte_range(), None);
        assert!(!state.has_active_insert());
    }

    #[test]
    fn test_on_insert_animation_finished_returns_true_when_removed() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        let removed = state.on_insert_animation_finished(10, 20);
        assert!(removed);
        assert!(state.is_empty());
    }

    #[test]
    fn test_on_insert_animation_finished_returns_false_when_no_match() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        let removed = state.on_insert_animation_finished(30, 40);
        assert!(!removed);
        assert!(!state.is_empty());
        assert!(state.has_active_insert());
    }

    #[test]
    fn test_on_insert_animation_finished_multiple_inserts_repaints_each() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        state.start_insert((30, 40), 100);
        // 完成第一个 Insert — removed=true，但还有另一个 Insert 活跃
        let removed1 = state.on_insert_animation_finished(10, 20);
        assert!(removed1);
        assert!(state.has_active_insert());
        // 完成第二个 Insert — removed=true，现在没有活跃 Insert 了
        let removed2 = state.on_insert_animation_finished(30, 40);
        assert!(removed2);
        assert!(!state.has_active_insert());
        assert!(state.is_empty());
    }

    // --- Additional animation lifecycle tests ---

    #[test]
    fn test_delete_animation_timeout_clears() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), 100);
        assert!(!state.is_empty());
        // duration=100, grace = 2*100 + 200 = 400ms
        let now = Instant::now() + Duration::from_millis(401);
        let cleared = state.tick(now);
        assert!(cleared, "Delete animation should be cleared after timeout");
        assert!(state.is_empty());
    }

    #[test]
    fn test_delete_animation_cleared_on_scroll() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), 100);
        assert!(!state.is_empty());
        state.clear_on_scroll();
        assert!(state.is_empty(), "Delete animation should be cleared on scroll");
    }

    #[test]
    fn test_delete_animation_cleared_on_typing_animation_disabled() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), 100);
        assert!(!state.is_empty());
        state.clear_on_typing_animation_disabled();
        assert!(state.is_empty(), "Delete animation should be cleared when typing animation disabled");
    }

    #[test]
    fn test_mixed_insert_delete_clear_all() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), 100);
        state.start_delete((30, 40), 100);
        assert!(state.has_active_insert());
        assert!(!state.is_empty());
        state.clear();
        assert!(state.is_empty(), "clear() should remove both Insert and Delete animations");
        assert_eq!(state.active_insert_byte_range(), None);
    }

    // --- should_create_text_animation unified decision tests ---

    #[test]
    fn test_should_create_animation_full() {
        let decision = should_create_text_animation(
            3,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::FullAnimation);
    }

    #[test]
    fn test_should_create_animation_empty_glyphs() {
        let decision = should_create_text_animation(
            0,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_glyph_over_limit() {
        let decision = should_create_text_animation(
            9,    // glyph_count > 8
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_contains_newline() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            true, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::CursorOnly);
    }

    #[test]
    fn test_should_create_animation_scrolling() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            false, // contains_newline
            true, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_loading() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            true, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_disabled() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            false, // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_component_not_ready() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            false, // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_exactly_at_limit() {
        // glyph_count == 8 should still be FullAnimation
        let decision = should_create_text_animation(
            8,    // glyph_count == MAX_GLYPH_COUNT
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::FullAnimation);
    }

    #[test]
    fn test_should_create_animation_applying_format() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            true, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_applying_settings() {
        let decision = should_create_text_animation(
            1,    // glyph_count
            false, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            true, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::NoAnimation);
    }

    #[test]
    fn test_should_create_animation_newline_overrides_glyph_count() {
        // Even with valid glyph_count, newline means CursorOnly
        let decision = should_create_text_animation(
            5,    // glyph_count
            true, // contains_newline
            false, // is_scrolling
            false, // is_loading
            false, // is_applying_format
            false, // is_applying_settings
            true,  // animation_enabled
            true,  // component_ready
        );
        assert_eq!(decision, AnimationDecision::CursorOnly);
    }
}
