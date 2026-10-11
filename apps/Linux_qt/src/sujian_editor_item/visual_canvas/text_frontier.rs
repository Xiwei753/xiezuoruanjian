//! Issue #901：二维文字前沿 —— 跟踪文字展开/收缩的前沿边界。
//!
//! ## 核心概念
//!
//! 文字不是一维逐字出现，而是**二维前沿**：
//! - **行内维度**：字符逐个出现/消失。
//! - **行间维度**：整行出现/消失。
//!
//! 前沿推进时优先按行边界跳跃——如果下一个行边界在推进范围内，直接跳到
//! 行边界（整行出现/消失），否则在行内逐字符推进。
//!
//! ## 吞字与吐字
//!
//! - **吐字**（前沿推进）：`target > visible`，新内容逐步出现。
//! - **吞字**（前沿收缩）：`target < visible`，旧内容逐步消失。
//!
//! ## Issue #901 问题4：cluster 几何分段
//!
//! 旧实现按全章 UTF-8 bytes/ms 跳行，不考虑字形实际宽度。新实现支持
//! Qt cluster 几何分段边界（`ClusterBoundary`），`visible_range_at_progress`
//! 根据 progress 和 cluster 宽度计算当前可见范围，而非按字节速率推进。
//!
//! ## 纯 Rust
//!
//! 本模块只做 byte range 算术和几何分段，不依赖 Qt 类型。

/// Issue #901 问题4：Qt cluster 几何分段边界。
///
/// 描述一个 cluster（字形段）的 byte range 和文档坐标宽度。
/// `visible_range_at_progress` 用此信息按宽度比例计算当前可见 byte range，
/// 而非按全章 UTF-8 bytes/ms 匀速推进。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClusterBoundary {
    /// 该 cluster 的起始 byte offset。
    pub byte_offset: usize,
    /// 该 cluster 的结束 byte offset（半开）。
    pub byte_end: usize,
    /// 该 cluster 在文档坐标系中的宽度（像素）。
    pub width: f64,
}

/// 二维文字前沿 —— 跟踪文字展开/收缩的可见范围边界。
///
/// 维护当前可见 byte range 和目标 byte range。`advance` 按推进速度
/// 将可见范围向目标推进，推进时优先按行边界跳跃（二维特性）。
#[derive(Clone, Debug)]
pub(crate) struct TextFrontier {
    /// 当前可见范围的起始 byte offset。
    visible_start: usize,
    /// 当前可见范围的结束 byte offset。
    visible_end: usize,
    /// 目标可见范围的起始 byte offset。
    target_start: usize,
    /// 目标可见范围的结束 byte offset。
    target_end: usize,
    /// 行边界列表 —— 每行结束的 byte offset（升序）。
    ///
    /// 用于行级前沿推进：推进时如果下一个行边界在范围内，直接跳到行边界。
    line_boundaries: Vec<usize>,
    /// 前沿推进速度（bytes per millisecond）。
    advance_rate_bytes_per_ms: f64,
    /// Issue #901 问题4：Qt cluster 几何分段边界列表。
    ///
    /// 从 `TargetGlyphRun` 的 `byte_range` 和 `document_rect` 推导。
    /// 非空时 `visible_range_at_progress` 按 cluster 宽度比例计算可见范围，
    /// 而非按字节速率匀速推进。
    cluster_boundaries: Vec<ClusterBoundary>,
}

impl TextFrontier {
    /// 创建一条新的文字前沿，初始可见范围和目标范围都为 `(start, end)`。
    ///
    /// `line_boundaries` 是每行结束的 byte offset 列表（升序），用于行级推进。
    /// `advance_rate_bytes_per_ms` 是前沿推进速度。
    pub(crate) fn new(
        start: usize,
        end: usize,
        line_boundaries: Vec<usize>,
        advance_rate_bytes_per_ms: f64,
    ) -> Self {
        Self {
            visible_start: start,
            visible_end: end,
            target_start: start,
            target_end: end,
            line_boundaries,
            advance_rate_bytes_per_ms: advance_rate_bytes_per_ms.max(0.0),
            cluster_boundaries: Vec::new(),
        }
    }

    /// 返回当前可见范围 `(byte_start, byte_end)`。
    pub(crate) fn visible_range(&self) -> (usize, usize) {
        (self.visible_start, self.visible_end)
    }

    /// 返回目标可见范围 `(byte_start, byte_end)`。
    pub(crate) fn target_range(&self) -> (usize, usize) {
        (self.target_start, self.target_end)
    }

    /// 前沿是否已到达目标（可见范围 == 目标范围）。
    pub(crate) fn is_settled(&self) -> bool {
        self.visible_start == self.target_start && self.visible_end == self.target_end
    }

    /// Issue #901 问题4：设置 Qt cluster 几何分段边界。
    ///
    /// 从 `TargetGlyphRun` 的 `byte_range` 和 `document_rect` 推导。
    /// 设置后 `visible_range_at_progress` 按 cluster 宽度比例计算可见范围。
    pub(crate) fn set_cluster_boundaries(&mut self, boundaries: Vec<ClusterBoundary>) {
        self.cluster_boundaries = boundaries;
    }

    /// Issue #901 问题4：根据 progress 和 cluster 宽度计算当前可见范围。
    ///
    /// 如果 `cluster_boundaries` 非空，按宽度比例在 target 范围内插值：
    /// - `progress=0` → 返回 `(target_start, target_start)`（空范围）。
    /// - `progress=1` → 返回 `(target_start, target_end)`（完整范围）。
    /// - 中间值按 cluster 宽度累加到 `progress * total_width` 处的 byte offset。
    ///
    /// 如果 `cluster_boundaries` 为空，回退到线性 byte range 插值。
    pub(crate) fn visible_range_at_progress(&self, progress: f64) -> (usize, usize) {
        let p = progress.clamp(0.0, 1.0);
        let target_span = self.target_end.saturating_sub(self.target_start);

        if self.cluster_boundaries.is_empty() || target_span == 0 {
            // 回退到线性插值。
            let visible_end = self.target_start + (target_span as f64 * p).round() as usize;
            return (self.target_start, visible_end.min(self.target_end));
        }

        // 按 cluster 宽度比例插值。
        // 只考虑落在 target 范围内的 cluster。
        let relevant: Vec<&ClusterBoundary> = self
            .cluster_boundaries
            .iter()
            .filter(|cb| cb.byte_end > self.target_start && cb.byte_offset < self.target_end)
            .collect();

        if relevant.is_empty() {
            let visible_end = self.target_start + (target_span as f64 * p).round() as usize;
            return (self.target_start, visible_end.min(self.target_end));
        }

        let total_width: f64 = relevant.iter().map(|cb| cb.width).sum();
        if total_width <= 0.0 {
            let visible_end = self.target_start + (target_span as f64 * p).round() as usize;
            return (self.target_start, visible_end.min(self.target_end));
        }

        let target_width = total_width * p;
        let mut acc_width = 0.0;
        let mut visible_end = self.target_start;

        for cb in &relevant {
            if acc_width + cb.width <= target_width {
                acc_width += cb.width;
                visible_end = cb.byte_end.min(self.target_end);
            } else {
                // 在该 cluster 内部，按比例插值 byte offset。
                let remaining = target_width - acc_width;
                if cb.width > 0.0 {
                    let frac = remaining / cb.width;
                    let cluster_span = cb.byte_end.saturating_sub(cb.byte_offset);
                    visible_end = cb.byte_offset + (cluster_span as f64 * frac).round() as usize;
                    visible_end = visible_end.min(self.target_end);
                }
                break;
            }
        }

        (self.target_start, visible_end.max(self.target_start))
    }

    /// 推进前沿 `delta_ms` 毫秒。
    ///
    /// 在 `delta_ms` 时间内推进 `delta_ms * advance_rate` 个 byte。
    /// `visible_start` 向 `target_start` 推进，`visible_end` 向 `target_end` 推进。
    /// 推进时优先按行边界跳跃（二维特性）。
    pub(crate) fn advance(&mut self, delta_ms: f64) {
        if delta_ms <= 0.0 || self.is_settled() {
            return;
        }
        let step = (delta_ms * self.advance_rate_bytes_per_ms).round() as usize;
        if step == 0 {
            return;
        }

        // 推进 visible_start 向 target_start。
        self.visible_start = self.advance_boundary(
            self.visible_start,
            self.target_start,
            step,
            false, // start 向前收缩：方向为减小
        );

        // 推进 visible_end 向 target_end。
        self.visible_end = self.advance_boundary(
            self.visible_end,
            self.target_end,
            step,
            true, // end 向后推进：方向为增大
        );
    }

    /// 重定向到新目标范围。
    ///
    /// 当前可见范围保持不变（不从 0 重新开始），只更新目标。
    /// 用于连续编辑：新编辑到来时只改变最终目标，不排"第几次按键的动画队列"。
    pub(crate) fn retarget(&mut self, new_target_range: (usize, usize)) {
        self.target_start = new_target_range.0;
        self.target_end = new_target_range.1;
    }

    /// 更新行边界列表。
    ///
    /// 排版变化时行边界可能改变，调用此方法更新。
    pub(crate) fn set_line_boundaries(&mut self, boundaries: Vec<usize>) {
        self.line_boundaries = boundaries;
    }

    /// 返回行边界列表的引用（仅供调试 / 测试）。
    #[cfg(test)]
    pub(crate) fn line_boundaries(&self) -> &[usize] {
        &self.line_boundaries
    }

    /// 推进单个边界值向目标靠近 `step` 个 byte，优先按行边界跳跃。
    ///
    /// `current`：当前值。`target`：目标值。`step`：最大推进量。
    /// `forward`：true 表示 target > current 时推进（end 增大），
    /// false 表示 target < current 时推进（start 减小）。
    fn advance_boundary(
        &self,
        current: usize,
        target: usize,
        step: usize,
        forward: bool,
    ) -> usize {
        if forward {
            // end 向 target 推进（增大方向）。
            if target <= current {
                // 吞字：end 向 target 收缩（减小方向）。
                let diff = current.saturating_sub(target);
                let move_amount = step.min(diff);
                let next = current - move_amount;
                // 按行边界对齐收缩。
                self.snap_to_line_boundary(next, target, false)
            } else {
                // 吐字：end 向 target 推进（增大方向）。
                let diff = target - current;
                let move_amount = step.min(diff);
                let next = current + move_amount;
                // 按行边界对齐推进。
                self.snap_to_line_boundary(next, target, true)
            }
        } else {
            // start 向 target 推进。
            if target >= current {
                // start 向 target 推进（增大方向，尾部收缩）。
                let diff = target - current;
                let move_amount = step.min(diff);
                let next = current + move_amount;
                self.snap_to_line_boundary(next, target, true)
            } else {
                // start 向 target 收缩（减小方向，头部扩展）。
                let diff = current.saturating_sub(target);
                let move_amount = step.min(diff);
                let next = current - move_amount;
                self.snap_to_line_boundary(next, target, false)
            }
        }
    }

    /// 将 `next` 按行边界对齐（二维前沿：行级推进优先于行内推进）。
    ///
    /// `forward = true`：推进方向为增大，snap 到 **>= next** 的最小行边界
    ///   （向前跳跃到下一个行边界，整行出现）。不超过 `target`。
    /// `forward = false`：收缩方向为减小，snap 到 **<= next** 的最大行边界
    ///   （向后跳跃到上一个行边界，整行消失）。不小于 `target`。
    ///
    /// 如果没有合适的行边界，返回 `next`（行内逐字符推进）。
    fn snap_to_line_boundary(&self, next: usize, target: usize, forward: bool) -> usize {
        if self.line_boundaries.is_empty() {
            return next;
        }

        if forward {
            // 推进方向：找 >= next 的最小行边界（向前跳跃到下一个行边界，整行出现）。
            // 但不超过 target。
            let candidate = self
                .line_boundaries
                .iter()
                .copied()
                .filter(|&b| b >= next && b <= target)
                .min();
            candidate.unwrap_or(next)
        } else {
            // 收缩方向：找 <= next 的最大行边界（向后跳跃到上一个行边界，整行消失）。
            // 但不小于 target。
            let candidate = self
                .line_boundaries
                .iter()
                .copied()
                .filter(|&b| b <= next && b >= target)
                .max();
            candidate.unwrap_or(next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_frontier_is_settled() {
        let f = TextFrontier::new(0, 10, vec![5, 10], 1.0);
        assert!(f.is_settled());
        assert_eq!(f.visible_range(), (0, 10));
        assert_eq!(f.target_range(), (0, 10));
    }

    #[test]
    fn advance_nothing_when_settled() {
        let mut f = TextFrontier::new(0, 10, vec![5, 10], 1.0);
        f.advance(100.0);
        assert_eq!(f.visible_range(), (0, 10));
    }

    #[test]
    fn advance_zero_delta_no_change() {
        let mut f = TextFrontier::new(0, 0, vec![5, 10], 1.0);
        f.retarget((0, 10));
        f.advance(0.0);
        assert_eq!(f.visible_range(), (0, 0));
    }

    #[test]
    fn retarget_preserves_visible_range() {
        let mut f = TextFrontier::new(0, 5, vec![5, 10], 1.0);
        f.retarget((0, 10));
        // 可见范围不变，只更新目标。
        assert_eq!(f.visible_range(), (0, 5));
        assert_eq!(f.target_range(), (0, 10));
        assert!(!f.is_settled());
    }

    #[test]
    fn advance_tu_zi_progress_forward() {
        // 吐字：前沿从 (0, 0) 推进到 (0, 10)。
        let mut f = TextFrontier::new(0, 0, vec![], 2.0); // 2 bytes/ms
        f.retarget((0, 10));
        f.advance(3.0); // 3ms * 2 bytes/ms = 6 bytes
        let (start, end) = f.visible_range();
        assert_eq!(start, 0);
        assert!(end > 0, "frontier must advance forward");
        assert!(end <= 10, "frontier must not overshoot target");
    }

    #[test]
    fn advance_tun_zi_contract_backward() {
        // 吞字：前沿从 (0, 10) 收缩到 (0, 0)。
        let mut f = TextFrontier::new(0, 10, vec![], 2.0);
        f.retarget((0, 0));
        f.advance(3.0); // 6 bytes
        let (_, end) = f.visible_range();
        assert!(end < 10, "frontier must contract backward");
    }

    #[test]
    fn advance_reaches_target_eventually() {
        let mut f = TextFrontier::new(0, 0, vec![], 10.0);
        f.retarget((0, 100));
        // 推进足够长时间。
        f.advance(1000.0);
        assert!(f.is_settled(), "frontier must reach target eventually");
        assert_eq!(f.visible_range(), (0, 100));
    }

    #[test]
    fn advance_snaps_to_line_boundary_forward() {
        // 行边界 [5, 10]，推进 3 bytes 应该跳到行边界 5。
        let mut f = TextFrontier::new(0, 0, vec![5, 10], 3.0);
        f.retarget((0, 10));
        f.advance(1.0); // 3 bytes → 应该 snap 到 5
        let (_, end) = f.visible_range();
        assert_eq!(end, 5, "frontier should snap to line boundary 5");
    }

    #[test]
    fn advance_snaps_to_line_boundary_backward() {
        // 行边界 [5, 10]，从 10 收缩 3 bytes 应该跳到行边界 5。
        let mut f = TextFrontier::new(0, 10, vec![5, 10], 3.0);
        f.retarget((0, 0));
        f.advance(1.0); // 3 bytes → 应该 snap 到 5
        let (_, end) = f.visible_range();
        assert_eq!(end, 5, "frontier should snap to line boundary 5 when contracting");
    }

    #[test]
    fn advance_both_start_and_end() {
        // start 从 0 推进到 3，end 从 10 推进到 15。
        let mut f = TextFrontier::new(0, 10, vec![], 5.0);
        f.retarget((3, 15));
        f.advance(1.0); // 5 bytes
        let (start, end) = f.visible_range();
        assert!(start > 0 || end > 10, "at least one boundary must move");
    }

    #[test]
    fn retarget_then_advance_continuous() {
        // 连续 retarget 不从 0 重新开始。
        let mut f = TextFrontier::new(0, 0, vec![], 10.0);
        f.retarget((0, 100));
        f.advance(2.0); // 20 bytes → visible = (0, 20)
        assert_eq!(f.visible_range(), (0, 20));

        // 新目标到来：不从 0 重新开始。
        f.retarget((0, 200));
        assert_eq!(f.visible_range(), (0, 20), "visible range preserved after retarget");
        f.advance(2.0); // 再推进 20 bytes → visible = (0, 40)
        assert_eq!(f.visible_range(), (0, 40));
    }

    #[test]
    fn set_line_boundaries_updates() {
        let mut f = TextFrontier::new(0, 0, vec![], 10.0);
        f.set_line_boundaries(vec![3, 6, 9]);
        assert_eq!(f.line_boundaries(), &[3, 6, 9]);
    }

    #[test]
    fn advance_clamped_to_target() {
        let mut f = TextFrontier::new(0, 0, vec![], 100.0);
        f.retarget((0, 5));
        f.advance(1000.0); // 远超目标
        assert_eq!(f.visible_range(), (0, 5), "must not overshoot target");
    }

    #[test]
    fn visible_range_at_progress_without_clusters_falls_back_to_linear() {
        // 无 cluster_boundaries 时回退到线性插值。
        let mut f = TextFrontier::new(0, 0, vec![], 10.0);
        f.retarget((0, 100));
        let (s, e) = f.visible_range_at_progress(0.5);
        assert_eq!(s, 0);
        assert!((e as f64 - 50.0).abs() < 1.0, "linear fallback: expected ~50, got {e}");
    }

    #[test]
    fn visible_range_at_progress_with_clusters() {
        // 有 cluster_boundaries 时按宽度比例插值。
        let mut f = TextFrontier::new(0, 0, vec![], 10.0);
        f.retarget((0, 10));
        // 两个 cluster：byte [0,5) 宽 30px，byte [5,10) 宽 70px。
        // total_width=100, progress=0.3 → target_width=30 → 刚好到第一个 cluster 末尾。
        f.set_cluster_boundaries(vec![
            ClusterBoundary { byte_offset: 0, byte_end: 5, width: 30.0 },
            ClusterBoundary { byte_offset: 5, byte_end: 10, width: 70.0 },
        ]);
        let (s, e) = f.visible_range_at_progress(0.3);
        assert_eq!(s, 0);
        assert_eq!(e, 5, "progress=0.3 (30px) should reach first cluster end (byte 5)");

        // progress=0.5 → target_width=50 → 在第二个 cluster 内 20px 处。
        // cluster_span=5, frac=20/70≈0.286, offset=5+5*0.286≈6.43→round=6
        let (s2, e2) = f.visible_range_at_progress(0.5);
        assert_eq!(s2, 0);
        assert!(e2 > 5 && e2 < 10, "progress=0.5 should be within second cluster, got {e2}");

        // progress=1.0 → 完整范围。
        let (s3, e3) = f.visible_range_at_progress(1.0);
        assert_eq!((s3, e3), (0, 10), "progress=1.0 must reach full target range");
    }
}
