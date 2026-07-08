use std::time::Instant;

/// 动画类型
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextAnimationKind {
    Insert,
    Delete,
}

/// 分层动画模式 — 与 Core AnimationMode 对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AnimationMode {
    /// 逐字形动画：1–8 个 cluster，每个 glyph 独立飞入
    GlyphAnimation,
    /// 整簇动画：包含复杂 grapheme（emoji/ZWJ/variation selector/combining mark），整簇作为单个 ghost
    ClusterAnimation,
    /// 分组动画：9–40 个 cluster，按 run 分组动画
    RunAnimation,
    /// 行级 reflow 动画：包含换行，做行级 reflow
    LineReflowAnimation,
    /// 快照动画：>40 个 cluster，极端长文本用 snapshot 动画
    SnapshotAnimation,
    /// 系统抑制：动画关闭/滚动/加载/格式化/设置变化等，不创建 hidden range
    SystemSuppressed,
}

/// 单个活跃动画条目
#[derive(Clone, Debug)]
pub(crate) struct ActiveTextAnimation {
    pub transaction_id: Option<u64>,
    pub range_id: Option<u64>,
    pub kind: TextAnimationKind,
    pub byte_range: (usize, usize),
    pub reflow_hidden_ranges: Vec<ActiveReflowHiddenRange>,
    #[allow(dead_code)]
    pub animation_mode: AnimationMode,
    pub start_time: Instant,
    pub duration_ms: u64,
}

/// 单个 reflow hidden range。range_id 是 Linux 侧稳定生命周期 ID，
/// 用于在连续输入和 range 映射后仍能把某个 reflow skip range 绑定到
/// 原 insert transaction；byte_range 只用于静态层实际跳过绘制。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ActiveReflowHiddenRange {
    pub range_id: u64,
    pub byte_range: (usize, usize),
}

/// 文本动画状态机 — 独立于 Qt/SujianEditorItem 的动画生命周期管理。
///
/// 所有停止动画路径都必须立即清理 hidden range：
/// - scroll / reload / visual_change / typing_animation_disabled / timeout
///
/// Insert 动画期间，正文层跳过 inserted range 不绘制，由 QML overlay 显示 glyph。
/// Delete 动画不需要正文层跳过，但记录以跟踪活跃动画。
#[allow(dead_code)]
pub(crate) struct TextAnimationState {
    animations: Vec<ActiveTextAnimation>,
}

#[allow(dead_code)]
impl TextAnimationState {
    pub fn new() -> Self {
        Self {
            animations: Vec::new(),
        }
    }

    /// 开始一个 Insert 动画
    /// `reflow_byte_ranges`：受插入影响的 glyph 在新文本中的 byte ranges，
    /// 静态正文层在动画期间跳过这些 ranges，由 overlay reflow ghost 显示位移动画。
    #[allow(dead_code)]
    pub fn start_insert(
        &mut self,
        byte_range: (usize, usize),
        reflow_byte_ranges: Vec<(usize, usize)>,
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) {
        self.start_insert_with_ids(None, None, byte_range, reflow_byte_ranges, animation_mode, duration_ms);
    }

    /// 开始一个 Insert 动画，并记录 Core 事务 ID / hidden range ID。
    /// 完成/跳过时按 ID 清理；byte range 只用于静态层绘制排除。
    pub fn start_insert_with_ids(
        &mut self,
        transaction_id: Option<u64>,
        range_id: Option<u64>,
        byte_range: (usize, usize),
        reflow_byte_ranges: Vec<(usize, usize)>,
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) {
        let reflow_hidden_ranges = reflow_byte_ranges
            .into_iter()
            .enumerate()
            .map(|(idx, byte_range)| ActiveReflowHiddenRange {
                range_id: Self::derive_reflow_range_id(transaction_id, range_id, idx),
                byte_range,
            })
            .collect();

        self.animations.push(ActiveTextAnimation {
            transaction_id,
            range_id,
            kind: TextAnimationKind::Insert,
            byte_range,
            reflow_hidden_ranges,
            animation_mode,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    /// 为 reflow hidden range 派生稳定 ID。
    ///
    /// Core 目前只给 inserted hidden range 下发 `HiddenVisualRange.id`；Linux 侧
    /// 对 reflow skip ranges 使用同一个 transaction/range 命名空间派生本地稳定 ID。
    /// 这些 ID 不依赖 byteStart/byteEnd，因此连续输入导致 byte range 映射后不会
    /// 把完成/跳过回调误清到其它动画。
    fn derive_reflow_range_id(
        transaction_id: Option<u64>,
        insert_range_id: Option<u64>,
        idx: usize,
    ) -> u64 {
        let base = insert_range_id.or(transaction_id).unwrap_or(0);
        if base == 0 {
            // legacy 路径没有 Core id 时仍给出进程内稳定非零 ID；byte range 仍是兜底。
            0x7fff_0000_u64 + idx as u64 + 1
        } else {
            base.saturating_mul(1_000_000).saturating_add(idx as u64 + 1)
        }
    }

    /// 开始一个 Delete 动画
    pub fn start_delete(&mut self, byte_range: (usize, usize), animation_mode: AnimationMode, duration_ms: u64) {
        self.animations.push(ActiveTextAnimation {
            transaction_id: None,
            range_id: None,
            kind: TextAnimationKind::Delete,
            byte_range,
            reflow_hidden_ranges: Vec::new(),
            animation_mode,
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
    #[allow(dead_code)]
    pub fn clear_on_scroll(&mut self) {
        self.clear();
    }

    #[allow(dead_code)]
    pub fn clear_on_reload(&mut self) {
        self.clear();
    }

    #[allow(dead_code)]
    pub fn clear_on_visual_change(&mut self) {
        self.clear();
    }

    /// 关闭打字动画时立即清理 hidden range，不依赖 timeout 恢复文字
    pub fn clear_on_typing_animation_disabled(&mut self) {
        self.clear();
    }

    /// QML 动画 overlay 通知 Insert 动画完成/跳过，优先按 transaction/range id 精确清理。
    ///
    /// 匹配逻辑：
    /// - 如果调用方提供了 range_id，只移除 range_id 匹配的动画
    /// - 如果调用方提供了 transaction_id（且 range_id 未匹配），只移除 transaction_id 匹配的动画
    /// - byte range 仅作为旧数据兜底：当动画自身没有 transaction_id 和 range_id 时，才按 byte range 匹配
    pub fn on_insert_animation_finished_by_id(
        &mut self,
        transaction_id: Option<u64>,
        range_id: Option<u64>,
        byte_start: usize,
        byte_end: usize,
    ) -> bool {
        let before = self.animations.len();
        self.animations.retain(|anim| {
            if anim.kind != TextAnimationKind::Insert {
                return true;
            }
            if let Some(range_id) = range_id {
                if anim.range_id == Some(range_id) {
                    return false;
                }
            }
            if let Some(transaction_id) = transaction_id {
                if anim.transaction_id == Some(transaction_id) {
                    return false;
                }
            }
            if anim.transaction_id.is_none() && anim.range_id.is_none() {
                return !(anim.byte_range == (byte_start, byte_end));
            }
            true
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

    /// 获取所有活跃 Insert 动画的 byte ranges
    pub fn active_insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animations
            .iter()
            .filter(|a| a.kind == TextAnimationKind::Insert)
            .map(|a| a.byte_range)
            .collect()
    }

    /// 获取所有活跃 Insert 动画的 reflow byte ranges
    /// 静态正文层在动画期间跳过这些 ranges，由 overlay reflow ghost 显示位移动画
    pub fn active_reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.animations
            .iter()
            .filter(|a| a.kind == TextAnimationKind::Insert)
            .flat_map(|a| a.reflow_hidden_ranges.iter().map(|r| r.byte_range))
            .collect()
    }

    /// 获取所有活跃 reflow hidden ranges 的稳定 ID 与 byte range。
    /// 渲染跳过仍使用 byte range；生命周期和测试优先使用 range_id。
    pub fn active_reflow_hidden_ranges(&self) -> Vec<ActiveReflowHiddenRange> {
        self.animations
            .iter()
            .filter(|a| a.kind == TextAnimationKind::Insert)
            .flat_map(|a| a.reflow_hidden_ranges.iter().cloned())
            .collect()
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

    /// 获取所有活跃动画的动画模式
    pub fn active_animation_modes(&self) -> Vec<AnimationMode> {
        self.animations.iter().map(|a| a.animation_mode).collect()
    }

    /// 是否有活跃的 LineReflow 动画
    pub fn has_active_line_reflow(&self) -> bool {
        self.animations
            .iter()
            .any(|a| a.animation_mode == AnimationMode::LineReflowAnimation)
    }

    /// 当新文本插入到 `pos` 位置、长度为 `len` 时，调整所有活跃动画的 byte ranges。
    ///
    /// 映射规则（对齐 CodeMirror/ProseMirror 的 decoration mapping 思路）：
    /// - `pos <= range.0`：range 整体后移 len → `(range.0 + len, range.1 + len)`
    /// - `pos >= range.1`：range 不受影响
    /// - `pos` 在 range 内部（`range.0 < pos < range.1`）：取消该动画（从列表中移除）
    ///
    /// 对每个动画的 `byte_range` 和 `reflow_byte_ranges` 都做映射。
    /// reflow_byte_ranges 中的每个 range 独立映射，如果某个 reflow range 被取消（相交），
    /// 则从 reflow_byte_ranges 中移除它（但不取消整个动画，除非 byte_range 本身被取消）。
    pub fn map_ranges_for_insert(&mut self, pos: usize, len: usize) {
        // 第一遍：计算每个动画的新 byte_range，标记需要取消的
        let new_ranges: Vec<Option<(usize, usize)>> = self
            .animations
            .iter()
            .map(|anim| map_range_for_insert(anim.byte_range, pos, len))
            .collect();

        // 第二遍：更新存活的动画并移除被取消的
        for (i, anim) in self.animations.iter_mut().enumerate() {
            if let Some(new_range) = new_ranges[i] {
                anim.byte_range = new_range;
            }
        }
        // 移除 byte_range 被取消的动画（new_ranges[i] == None）
        let mut i = 0;
        self.animations.retain(|_| {
            let keep = new_ranges[i].is_some();
            i += 1;
            keep
        });

        // 对存活的动画，映射 reflow hidden ranges；保留稳定 range_id，只更新 byte_range
        for anim in &mut self.animations {
            anim.reflow_hidden_ranges = anim
                .reflow_hidden_ranges
                .iter()
                .filter_map(|r| {
                    map_range_for_insert(r.byte_range, pos, len).map(|byte_range| ActiveReflowHiddenRange {
                        range_id: r.range_id,
                        byte_range,
                    })
                })
                .collect();
        }
    }

    /// 当文本从 `pos` 位置删除、长度为 `len` 时（删除范围 `[pos, pos+len)`），
    /// 调整所有活跃动画的 byte ranges。
    ///
    /// 映射规则：
    /// - 删除范围完全在 range 之前（`pos + len <= range.0`）：range 整体前移 → `(range.0 - len, range.1 - len)`
    /// - 删除范围完全在 range 之后（`pos >= range.1`）：range 不受影响
    /// - 删除范围和 range 相交：取消该动画（从列表中移除）
    ///
    /// 对每个动画的 `byte_range` 和 `reflow_byte_ranges` 都做映射。
    /// reflow_byte_ranges 中的每个 range 独立映射，如果某个 reflow range 被取消（相交），
    /// 则从 reflow_byte_ranges 中移除它（但不取消整个动画，除非 byte_range 本身被取消）。
    pub fn map_ranges_for_delete(&mut self, pos: usize, len: usize) {
        // 第一遍：计算每个动画的新 byte_range，标记需要取消的
        let new_ranges: Vec<Option<(usize, usize)>> = self
            .animations
            .iter()
            .map(|anim| map_range_for_delete(anim.byte_range, pos, len))
            .collect();

        // 第二遍：更新存活的动画并移除被取消的
        for (i, anim) in self.animations.iter_mut().enumerate() {
            if let Some(new_range) = new_ranges[i] {
                anim.byte_range = new_range;
            }
        }
        // 移除 byte_range 被取消的动画（new_ranges[i] == None）
        let mut i = 0;
        self.animations.retain(|_| {
            let keep = new_ranges[i].is_some();
            i += 1;
            keep
        });

        // 对存活的动画，映射 reflow hidden ranges；保留稳定 range_id，只更新 byte_range
        for anim in &mut self.animations {
            anim.reflow_hidden_ranges = anim
                .reflow_hidden_ranges
                .iter()
                .filter_map(|r| {
                    map_range_for_delete(r.byte_range, pos, len).map(|byte_range| ActiveReflowHiddenRange {
                        range_id: r.range_id,
                        byte_range,
                    })
                })
                .collect();
        }
    }
}

/// 单个 range 的 insert 映射。
///
/// - `pos <= range.0`：range 整体后移 len
/// - `pos >= range.1`：range 不受影响
/// - `pos` 在 range 内部：返回 None（取消）
fn map_range_for_insert(range: (usize, usize), pos: usize, len: usize) -> Option<(usize, usize)> {
    if pos <= range.0 {
        // insert 在 range 之前（或恰好在 range 起点），整体后移
        Some((range.0 + len, range.1 + len))
    } else if pos >= range.1 {
        // insert 在 range 之后，不受影响
        Some(range)
    } else {
        // insert 在 range 内部，取消
        None
    }
}

/// 单个 range 的 delete 映射。
///
/// - 删除范围完全在 range 之前（`pos + len <= range.0`）：range 整体前移
/// - 删除范围完全在 range 之后（`pos >= range.1`）：range 不受影响
/// - 删除范围和 range 相交：返回 None（取消）
fn map_range_for_delete(range: (usize, usize), pos: usize, len: usize) -> Option<(usize, usize)> {
    let delete_end = pos + len;
    if delete_end <= range.0 {
        // 删除范围完全在 range 之前，整体前移
        Some((range.0 - len, range.1 - len))
    } else if pos >= range.1 {
        // 删除范围完全在 range 之后，不受影响
        Some(range)
    } else {
        // 删除范围和 range 相交，取消
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_insert_creates_active_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        assert!(state.has_active_insert());
        assert!(!state.is_empty());
    }

    #[test]
    fn test_delete_does_not_create_hidden_range() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), AnimationMode::GlyphAnimation, 100);
        // Delete 动画不产生 hidden range
        assert!(state.active_insert_byte_ranges().is_empty());
        assert!(!state.has_active_insert());
        // 但状态机不为空（有 Delete 动画在跟踪）
        assert!(!state.is_empty());
    }

    #[test]
    fn test_clear_removes_all() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_delete((30, 40), AnimationMode::GlyphAnimation, 100);
        assert!(!state.is_empty());
        state.clear();
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
        assert!(!state.has_active_insert());
    }

    #[test]
    fn test_clear_on_typing_animation_disabled() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        state.clear_on_typing_animation_disabled();
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_clear_on_scroll() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        state.clear_on_scroll();
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_clear_on_reload() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        state.clear_on_reload();
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_clear_on_visual_change() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        state.clear_on_visual_change();
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_timeout_clears_animation() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        // duration=100, grace = 2*100 + 200 = 400ms
        // 超过宽限期后 tick 应清除
        let now = Instant::now() + Duration::from_millis(401);
        let cleared = state.tick(now);
        assert!(cleared);
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_within_grace_period_not_cleared() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        // duration=100, grace = 2*100 + 200 = 400ms
        // 在宽限期内 tick 不应清除
        let now = Instant::now() + Duration::from_millis(300);
        let cleared = state.tick(now);
        assert!(!cleared);
        assert!(!state.is_empty());
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
    }

    #[test]
    fn test_on_insert_animation_finished_removes_matching() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        state.on_insert_animation_finished_by_id(None, None, 10, 20);
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_on_insert_animation_finished_keeps_others() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_delete((30, 40), AnimationMode::GlyphAnimation, 100);
        // 完成 Insert (10,20)，Delete (30,40) 应保留
        state.on_insert_animation_finished_by_id(None, None, 10, 20);
        assert!(!state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
        assert!(!state.has_active_insert());
    }

    #[test]
    fn test_on_insert_animation_finished_returns_true_when_removed() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        let removed = state.on_insert_animation_finished_by_id(None, None, 10, 20);
        assert!(removed);
        assert!(state.is_empty());
    }

    #[test]
    fn test_on_insert_animation_finished_returns_false_when_no_match() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        let removed = state.on_insert_animation_finished_by_id(None, None, 30, 40);
        assert!(!removed);
        assert!(!state.is_empty());
        assert!(state.has_active_insert());
    }

    #[test]
    fn test_on_insert_animation_finished_multiple_inserts_repaints_each() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![], AnimationMode::GlyphAnimation, 100);
        // 完成第一个 Insert — removed=true，但还有另一个 Insert 活跃
        let removed1 = state.on_insert_animation_finished_by_id(None, None, 10, 20);
        assert!(removed1);
        assert!(state.has_active_insert());
        // 完成第二个 Insert — removed=true，现在没有活跃 Insert 了
        let removed2 = state.on_insert_animation_finished_by_id(None, None, 30, 40);
        assert!(removed2);
        assert!(!state.has_active_insert());
        assert!(state.is_empty());
    }

    #[test]
    fn test_active_insert_byte_ranges_returns_all() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![], AnimationMode::GlyphAnimation, 100);
        // 两个活跃 Insert 动画都应返回
        let ranges = state.active_insert_byte_ranges();
        assert_eq!(ranges.len(), 2);
        assert!(ranges.contains(&(10, 20)));
        assert!(ranges.contains(&(30, 40)));
        // 完成第一个后，只剩一个
        state.on_insert_animation_finished_by_id(None, None, 10, 20);
        let ranges2 = state.active_insert_byte_ranges();
        assert_eq!(ranges2, vec![(30, 40)]);
    }

    // --- Additional animation lifecycle tests ---

    // --- Reflow range tests ---

    #[test]
    fn test_insert_with_reflow_ranges() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(20, 25), (25, 30)]);
        let hidden = state.active_reflow_hidden_ranges();
        assert_eq!(hidden.len(), 2);
        assert!(hidden[0].range_id > 0);
        assert_ne!(hidden[0].range_id, hidden[1].range_id);
    }

    #[test]
    fn test_reflow_range_ids_survive_mapping_and_finish_uses_insert_id() {
        let mut state = TextAnimationState::new();
        state.start_insert_with_ids(
            Some(11),
            Some(22),
            (10, 20),
            vec![(20, 25), (25, 30)],
            AnimationMode::GlyphAnimation,
            100,
        );
        let ids_before: Vec<u64> = state
            .active_reflow_hidden_ranges()
            .iter()
            .map(|r| r.range_id)
            .collect();
        state.map_ranges_for_insert(0, 3);
        let mapped = state.active_reflow_hidden_ranges();
        assert_eq!(mapped.iter().map(|r| r.range_id).collect::<Vec<_>>(), ids_before);
        assert_eq!(mapped.iter().map(|r| r.byte_range).collect::<Vec<_>>(), vec![(23, 28), (28, 33)]);
        assert!(!state.on_insert_animation_finished_by_id(None, None, 13, 23));
        assert!(state.on_insert_animation_finished_by_id(Some(11), Some(22), 13, 23));
        assert!(state.active_reflow_hidden_ranges().is_empty());
    }

    #[test]
    fn test_byte_fallback_cannot_clear_id_owned_insert_animation() {
        let mut state = TextAnimationState::new();
        state.start_insert_with_ids(
            Some(101),
            Some(202),
            (10, 20),
            vec![(20, 25)],
            AnimationMode::GlyphAnimation,
            100,
        );
        assert!(!state.on_insert_animation_finished_by_id(None, None, 10, 20));
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        assert!(state.on_insert_animation_finished_by_id(Some(101), Some(202), 10, 20));
        assert!(state.is_empty());
    }

    #[test]
    fn test_reflow_ranges_cleared_on_insert_finished() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25)], AnimationMode::GlyphAnimation, 100);
        assert!(!state.active_reflow_byte_ranges().is_empty());
        state.on_insert_animation_finished_by_id(None, None, 10, 20);
        assert!(state.active_reflow_byte_ranges().is_empty());
    }

    #[test]
    fn test_reflow_ranges_cleared_on_clear() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25)], AnimationMode::GlyphAnimation, 100);
        state.clear();
        assert!(state.active_reflow_byte_ranges().is_empty());
    }

    #[test]
    fn test_multiple_inserts_reflow_ranges_merged() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25)], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![(40, 45)], AnimationMode::GlyphAnimation, 100);
        let reflow = state.active_reflow_byte_ranges();
        assert_eq!(reflow.len(), 2);
        assert!(reflow.contains(&(20, 25)));
        assert!(reflow.contains(&(40, 45)));
        // 完成第一个 insert 后，只剩第二个的 reflow
        state.on_insert_animation_finished_by_id(None, None, 10, 20);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(40, 45)]);
    }

    #[test]
    fn test_delete_animation_timeout_clears() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), AnimationMode::GlyphAnimation, 100);
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
        state.start_delete((5, 15), AnimationMode::GlyphAnimation, 100);
        assert!(!state.is_empty());
        state.clear_on_scroll();
        assert!(state.is_empty(), "Delete animation should be cleared on scroll");
    }

    #[test]
    fn test_delete_animation_cleared_on_typing_animation_disabled() {
        let mut state = TextAnimationState::new();
        state.start_delete((5, 15), AnimationMode::GlyphAnimation, 100);
        assert!(!state.is_empty());
        state.clear_on_typing_animation_disabled();
        assert!(state.is_empty(), "Delete animation should be cleared when typing animation disabled");
    }

    #[test]
    fn test_mixed_insert_delete_clear_all() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_delete((30, 40), AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        assert!(!state.is_empty());
        state.clear();
        assert!(state.is_empty(), "clear() should remove both Insert and Delete animations");
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    // --- Range mapping tests ---

    #[test]
    fn test_map_ranges_insert_before_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(5, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(13, 23)]);
    }

    #[test]
    fn test_map_ranges_insert_at_range_start() {
        // pos == range.0 算"在 range 之前"，整体后移
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(10, 4);
        assert_eq!(state.active_insert_byte_ranges(), vec![(14, 24)]);
    }

    #[test]
    fn test_map_ranges_insert_after_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(20, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
    }

    #[test]
    fn test_map_ranges_insert_beyond_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(25, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
    }

    #[test]
    fn test_map_ranges_insert_inside_range_cancels() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(15, 3);
        assert!(state.is_empty());
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_map_ranges_delete_before_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(5, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(7, 17)]);
    }

    #[test]
    fn test_map_ranges_delete_up_to_range_start() {
        // 删除范围 [5, 10)，range.0 = 10，delete_end = 10 <= range.0，整体前移
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(5, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(5, 15)]);
    }

    #[test]
    fn test_map_ranges_delete_after_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(20, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
    }

    #[test]
    fn test_map_ranges_delete_beyond_range() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(25, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
    }

    #[test]
    fn test_map_ranges_delete_overlapping_start_cancels() {
        // 删除范围 [8, 13)，和 range [10, 20) 相交
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(8, 5);
        assert!(state.is_empty());
    }

    #[test]
    fn test_map_ranges_delete_overlapping_end_cancels() {
        // 删除范围 [18, 25)，和 range [10, 20) 相交
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(18, 7);
        assert!(state.is_empty());
    }

    #[test]
    fn test_map_ranges_delete_inside_range_cancels() {
        // 删除范围 [12, 18)，完全在 range [10, 20) 内部，相交
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(12, 6);
        assert!(state.is_empty());
    }

    #[test]
    fn test_map_ranges_delete_superset_of_range_cancels() {
        // 删除范围 [5, 25)，完全包含 range [10, 20)，相交
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(5, 20);
        assert!(state.is_empty());
    }

    // --- Reflow range mapping tests ---

    #[test]
    fn test_map_ranges_insert_before_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(5, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(13, 23)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(23, 28), (28, 33)]);
    }

    #[test]
    fn test_map_ranges_insert_after_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(30, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(20, 25), (25, 30)]);
    }

    #[test]
    fn test_map_ranges_insert_inside_reflow_removes_that_reflow() {
        // insert 在 reflow range 内部，移除该 reflow range，但不取消整个动画
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(22, 3);
        // byte_range 不受影响（insert 在 byte_range 之后）
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        // reflow (20,25) 被 insert 在 22 处取消；reflow (25,30) 后移到 (28,33)
        assert_eq!(state.active_reflow_byte_ranges(), vec![(28, 33)]);
    }

    #[test]
    fn test_map_ranges_delete_before_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(5, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(7, 17)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(17, 22), (22, 27)]);
    }

    #[test]
    fn test_map_ranges_delete_after_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(30, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(20, 25), (25, 30)]);
    }

    #[test]
    fn test_map_ranges_delete_overlapping_reflow_removes_that_reflow() {
        // delete 和 reflow range 相交，移除该 reflow range，但不取消整个动画
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(22, 5);
        // byte_range 不受影响（delete 在 byte_range 之后）
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        // reflow (20,25) 被相交删除取消；reflow (25,30) 也被相交删除取消
        // delete [22, 27) 和 (20,25) 相交，和 (25,30) 也相交
        assert!(state.active_reflow_byte_ranges().is_empty());
    }

    #[test]
    fn test_map_ranges_delete_partial_reflow_overlap() {
        // delete 只和一个 reflow 相交
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (30, 35)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(22, 5);
        // byte_range 不受影响
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        // reflow (20,25) 被相交删除取消；
        // reflow (30,35)：delete [22,27) 完全在 (30,35) 之前，前移 → (25,30)
        assert_eq!(state.active_reflow_byte_ranges(), vec![(25, 30)]);
    }

    // --- Multiple animations mapping tests ---

    #[test]
    fn test_map_ranges_insert_multiple_animations() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(5, 3);
        let ranges = state.active_insert_byte_ranges();
        assert_eq!(ranges.len(), 2);
        assert!(ranges.contains(&(13, 23)));
        assert!(ranges.contains(&(33, 43)));
    }

    #[test]
    fn test_map_ranges_insert_cancels_one_of_multiple() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![], AnimationMode::GlyphAnimation, 100);
        // insert 在第一个 range 内部，取消第一个；第二个后移
        state.map_ranges_for_insert(15, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(33, 43)]);
    }

    #[test]
    fn test_map_ranges_delete_multiple_animations() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_delete(5, 3);
        let ranges = state.active_insert_byte_ranges();
        assert_eq!(ranges.len(), 2);
        assert!(ranges.contains(&(7, 17)));
        assert!(ranges.contains(&(27, 37)));
    }

    #[test]
    fn test_map_ranges_delete_cancels_one_of_multiple() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![], AnimationMode::GlyphAnimation, 100);
        // delete 和第一个 range 相交，取消第一个；第二个前移
        state.map_ranges_for_delete(8, 5);
        assert_eq!(state.active_insert_byte_ranges(), vec![(25, 35)]);
    }

    #[test]
    fn test_map_ranges_multiple_with_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25)], AnimationMode::GlyphAnimation, 100);
        state.start_insert((30, 40), vec![(40, 45)], AnimationMode::GlyphAnimation, 100);
        state.map_ranges_for_insert(5, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(13, 23), (33, 43)]);
        let reflow = state.active_reflow_byte_ranges();
        assert_eq!(reflow.len(), 2);
        assert!(reflow.contains(&(23, 28)));
        assert!(reflow.contains(&(43, 48)));
    }

    #[test]
    fn test_map_ranges_delete_animation_also_mapped() {
        // Delete 动画的 byte_range 也应该被映射
        let mut state = TextAnimationState::new();
        state.start_delete((10, 20), AnimationMode::GlyphAnimation, 100);
        // insert 在 delete range 之前
        state.map_ranges_for_insert(5, 3);
        // Delete 动画没有 active_insert_byte_ranges，但状态机不为空
        assert!(!state.is_empty());
        assert!(!state.has_active_insert());
    }

    #[test]
    fn test_map_ranges_insert_inside_delete_range_cancels() {
        let mut state = TextAnimationState::new();
        state.start_delete((10, 20), AnimationMode::GlyphAnimation, 100);
        // insert 在 delete range 内部，取消该 delete 动画
        state.map_ranges_for_insert(15, 3);
        assert!(state.is_empty());
    }

    // --- Unit tests for helper functions ---

    #[test]
    fn test_map_range_for_insert_before() {
        assert_eq!(map_range_for_insert((10, 20), 5, 3), Some((13, 23)));
    }

    #[test]
    fn test_map_range_for_insert_at_start() {
        assert_eq!(map_range_for_insert((10, 20), 10, 4), Some((14, 24)));
    }

    #[test]
    fn test_map_range_for_insert_after() {
        assert_eq!(map_range_for_insert((10, 20), 20, 5), Some((10, 20)));
    }

    #[test]
    fn test_map_range_for_insert_inside() {
        assert_eq!(map_range_for_insert((10, 20), 15, 3), None);
    }

    #[test]
    fn test_map_range_for_delete_before() {
        assert_eq!(map_range_for_delete((10, 20), 5, 3), Some((7, 17)));
    }

    #[test]
    fn test_map_range_for_delete_up_to_start() {
        assert_eq!(map_range_for_delete((10, 20), 5, 5), Some((5, 15)));
    }

    #[test]
    fn test_map_range_for_delete_after() {
        assert_eq!(map_range_for_delete((10, 20), 20, 5), Some((10, 20)));
    }

    #[test]
    fn test_map_range_for_delete_overlapping() {
        assert_eq!(map_range_for_delete((10, 20), 8, 5), None);
    }

    #[test]
    fn test_map_range_for_delete_inside() {
        assert_eq!(map_range_for_delete((10, 20), 12, 3), None);
    }

    #[test]
    fn test_map_range_for_delete_superset() {
        assert_eq!(map_range_for_delete((10, 20), 5, 20), None);
    }

    // --- record_transaction repaint-signal tests ---

    #[test]
    fn test_insert_finished_prefers_range_id_over_byte_range() {
        let mut state = TextAnimationState::new();
        state.start_insert_with_ids(Some(10), Some(100), (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        state.start_insert_with_ids(Some(11), Some(101), (5, 6), vec![], AnimationMode::GlyphAnimation, 100);

        let removed = state.on_insert_animation_finished_by_id(Some(11), Some(101), 5, 6);

        assert!(removed);
        assert_eq!(state.active_insert_byte_ranges(), vec![(5, 6)]);
        let remaining = state
            .animations
            .iter()
            .find(|a| a.kind == TextAnimationKind::Insert)
            .unwrap();
        assert_eq!(remaining.transaction_id, Some(10));
        assert_eq!(remaining.range_id, Some(100));
    }

    #[test]
    fn test_insert_finished_falls_back_to_byte_range_for_legacy_signal() {
        let mut state = TextAnimationState::new();
        state.start_insert_with_ids(None, None, (10, 12), vec![], AnimationMode::GlyphAnimation, 100);

        let removed = state.on_insert_animation_finished_by_id(None, None, 10, 12);

        assert!(removed);
        assert!(state.active_insert_byte_ranges().is_empty());
    }

    #[test]
    fn test_map_ranges_for_insert_triggers_repaint_on_cancel() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        // insert 在 range 内部，取消动画 → has_active_insert 变为 false（需要 repaint）
        state.map_ranges_for_insert(15, 3);
        assert!(!state.has_active_insert(), "Insert inside range should cancel animation, triggering repaint");
    }

    #[test]
    fn test_map_ranges_for_delete_triggers_repaint_on_cancel() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        // delete 和 range 相交，取消动画 → has_active_insert 变为 false（需要 repaint）
        state.map_ranges_for_delete(8, 5);
        assert!(!state.has_active_insert(), "Delete intersecting range should cancel animation, triggering repaint");
    }

    #[test]
    fn test_map_ranges_for_insert_no_repaint_when_no_cancel() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        // insert 在 range 之前，不取消 → has_active_insert 仍为 true（无需 repaint）
        state.map_ranges_for_insert(5, 3);
        assert!(state.has_active_insert(), "Insert before range should not cancel animation");
        assert_eq!(state.active_insert_byte_ranges(), vec![(13, 23)]);
    }

    #[test]
    fn test_map_ranges_for_delete_no_repaint_when_no_cancel() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(state.has_active_insert());
        // delete 在 range 之后，不取消 → has_active_insert 仍为 true（无需 repaint）
        state.map_ranges_for_delete(25, 5);
        assert!(state.has_active_insert(), "Delete after range should not cancel animation");
    }

    #[test]
    fn test_map_ranges_insert_before_with_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        // insert 在 byte_range 之前，byte_range 和 reflow_byte_ranges 都后移
        state.map_ranges_for_insert(5, 3);
        assert_eq!(state.active_insert_byte_ranges(), vec![(13, 23)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(23, 28), (28, 33)]);
    }

    #[test]
    fn test_map_ranges_delete_intersecting_reflow_cancels_that_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25), (30, 35)], AnimationMode::GlyphAnimation, 100);
        // delete [22, 27) 和 reflow (20,25) 相交，取消该 reflow；
        // reflow (30,35) 在 delete 之后，前移 → (25, 30)
        state.map_ranges_for_delete(22, 5);
        // byte_range 不受影响（delete 在 byte_range 之后）
        assert_eq!(state.active_insert_byte_ranges(), vec![(10, 20)]);
        assert_eq!(state.active_reflow_byte_ranges(), vec![(25, 30)]);
    }

    #[test]
    fn test_map_ranges_insert_cancels_animation_with_reflow() {
        let mut state = TextAnimationState::new();
        state.start_insert((10, 20), vec![(20, 25)], AnimationMode::GlyphAnimation, 100);
        // insert 在 byte_range 内部，取消整个动画（包括 reflow）
        state.map_ranges_for_insert(15, 3);
        assert!(state.is_empty(), "Insert inside byte_range should cancel entire animation");
        assert!(state.active_reflow_byte_ranges().is_empty(), "Reflow ranges should be cleared when animation is cancelled");
    }

    #[test]
    fn test_sequential_inserts_map_correctly() {
        let mut state = TextAnimationState::new();
        // 第一个动画
        state.start_insert((5, 10), vec![], AnimationMode::GlyphAnimation, 100);
        // insert 在第一个 range 之后（pos=10 >= range.1=10），range 不变
        state.map_ranges_for_insert(10, 1);
        assert_eq!(state.active_insert_byte_ranges(), vec![(5, 10)]);

        // 第二个动画
        state.start_insert((10, 11), vec![], AnimationMode::GlyphAnimation, 100);
        // insert 在第一个 range 之后、第二个 range 边界
        state.map_ranges_for_insert(11, 1);
        let ranges = state.active_insert_byte_ranges();
        assert_eq!(ranges.len(), 2);
        assert!(ranges.contains(&(5, 10)), "First animation range should remain (5, 10)");
        assert!(ranges.contains(&(10, 11)), "Second animation range should remain (10, 11)");
    }
}
