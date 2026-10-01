//! 光标控制器 — 管理光标位置、动画和闪烁状态。
//!
//! ## 坐标空间
//!
//! 所有坐标为文档坐标系（物理像素，不含滚动偏移）。
//! `target_x/y` 是光标应到达的目标位置（由布局引擎计算），
//! `visual_x/y` 是当前渲染位置（动画中间态可能与 target 不同）。
//!
//! ## 动画模型
//!
//! 光标移动走 Snap（瞬移）或 Tween（缓动）两种模式：
//! - Snap：跨行、大距离跳转、章节加载时使用，visual 立即设为 target
//! - Tween：同行小距离移动时使用，visual 从当前位置缓动到 target
//!
//! 动画中断时从当前 visual 位置 rebase 到新 target，保证无跳变。
//!
//! ## 闪烁模型
//!
//! `blink_visible` 控制光标是否可见（530ms 交替）。
//! 编辑操作触发 `blink_reset_requested`，使光标重新可见并重置闪烁计时器。
//! 滚动和动画期间闪烁暂停。

use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::rendering::CursorAnimationState;
use crate::editor::layout::CaretAffinity;
use std::time::{Duration, Instant};

const BLINK_INTERVAL_MS: u64 = 530;

/// 光标移动来源 — 决定跨行移动时走 Snap 还是 Tween。
///
/// Issue #712: 替代旧的 `cross_line_snap = dy > cursor_h * 3.0` 按距离猜用户意图的规则，
/// 改为按光标移动来源决定 Snap/Tween：
/// - `PointerClick` / `KeyboardNavigation`：smooth cursor 开启时允许跨行 Tween
/// - `DragSelection` / `LayoutChange` / `Scroll`：硬 Snap
/// - `TextTransaction`：由正文协同光标处理
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorMoveSource {
    /// 鼠标点击：smooth cursor 开启时允许跨行 Tween
    PointerClick,
    /// 方向键导航：smooth cursor 开启时允许跨行 Tween
    KeyboardNavigation,
    /// 拖选：Snap
    DragSelection,
    /// 布局变化（窗口宽度改变等）：Snap
    LayoutChange,
    /// 滚动：Snap
    Scroll,
    /// 正文事务（输入/删除等）：继续由正文协同光标处理
    TextTransaction,
}

/// Issue #810 评论 问题2: 光标可见性状态 — 区分"从未可见"、"正常可见"、"因选区暂时隐藏"。
///
/// 旧实现只用 `visible: bool`，无法区分"因选区隐藏"和"首次出现/不在视口隐藏"。
/// 这导致选区收起后 `!old_visible` 触发 hard_snap，光标瞬移而非 Tween。
///
/// - `Uninitialized`：从未可见，没有可信的 visual position
/// - `Visible`：正常可见，visual_x/visual_y 是当前绘制位置
/// - `HiddenBySelection`：因 has_selection 隐藏，但 visual_x/visual_y 保留为
///   选区 head 的视觉位置，选区收起后从此位置恢复 Tween
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorVisibilityState {
    Uninitialized,
    Visible,
    HiddenBySelection,
}

/// 光标状态 — 跟踪光标位置、动画和闪烁。
///
/// - `target_x/y`：光标应到达的位置（布局引擎计算结果）
/// - `visual_x/y`：当前渲染位置（动画中间态可能与 target 不同，动画结束后 visual == target）
/// - `affinity`：行末换行时光标偏向哪一端（Downstream=下一行行首，Upstream=当前行行末）
/// - `current_visual_line_id`：光标所在 visual line 的 ID，用于判断是否跨行移动
/// - `force_snap_next`：下次更新强制 Snap（跳过 Tween），用于章节加载、滚动恢复等场景
/// - `blink_reset_requested`：编辑操作后请求重置闪烁（使光标重新可见）
/// - `cursor_owner_epoch`：光标所有权版本号（Issue #705 评论 5717380886）。
///   非正文事务导致的逻辑 cursor 移动（鼠标点击、方向键、Home/End、拖选等）
///   会 bump 此 epoch；`PreparedTextVisualTransaction` 创建时记录当时的 epoch，
///   `animation_coordinator` 在驱动 coordinated caret 前检查事务记录的 epoch
///   是否仍等于当前 epoch。
///   Issue #727 约束 1: epoch 不一致后，这笔事务立刻失去 caret motion ownership；
///   InsertReveal / DeleteConceal 同时结束到 canonical 最终状态，不再继续播放。
///   ReflowMove / ReflowCrossFade 是否继续可以单独决定。
///   即：caret ownership 丢失 = reveal/conceal ownership 同时丢失。
pub struct CursorController {
    pub target_x: f64,
    pub target_y: f64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_h: f64,
    /// Issue #712 评论 5739517945: 当前光标 baseline 的视觉位置。
    /// Snap 时直接取 `plan.cursor_baseline_y`；Tween 时从 old_rect.baseline_y
    /// 缓动到 new_rect.baseline_y。替代旧的 `cursor_h * 0.8` 估算。
    pub visual_baseline_y: f64,
    pub visible: bool,
    pub dirty: bool,
    pub affinity: CaretAffinity,
    pub ime_cursor_rect_h: f64,
    pub anchor_visual_x: Option<f64>,
    pub anchor_visual_y: Option<f64>,
    pub animation: Option<CursorAnimationState>,
    pub force_snap_next: bool,
    pub blink_visible: bool,
    pub blink_last_toggle: Instant,
    pub blink_reset_requested: bool,
    /// Issue #705 评论 5717380886: 光标所有权版本号。
    /// 0 表示初始状态；任何非正文事务导致的逻辑 cursor 移动都应 bump。
    pub cursor_owner_epoch: u64,
    /// Issue #712: 光标移动来源，决定跨行移动时走 Snap 还是 Tween。
    /// 默认 LayoutChange（安全默认值，首次出现走 Snap）。
    pub last_move_source: CursorMoveSource,
    /// Issue #810 评论 问题2: 光标可见性状态。
    /// 区分"因选区隐藏"和"首次出现/不在视口隐藏"，选区收起后从保留位置恢复 Tween。
    pub visibility_state: CursorVisibilityState,
    /// Issue #810 评论 问题2: 选区结束时 selection head 的 visual rect (x, y, h, baseline_y)。
    ///
    /// 手势结束（end_selection_gesture）时保存当前 visual rect。
    /// 选区收起后（has_selection 从 true 变 false），从该位置恢复 Tween，
    /// 而非因 `!old_visible` 强制 Snap 瞬移。
    pub selection_head_rect: Option<(f64, f64, f64, f64)>,
}

impl Default for CursorController {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorController {
    pub fn new() -> Self {
        Self {
            target_x: 0.0,
            target_y: 0.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_h: 0.0,
            visual_baseline_y: 0.0,
            visible: false,
            dirty: false,
            affinity: CaretAffinity::Downstream,
            ime_cursor_rect_h: 0.0,
            anchor_visual_x: None,
            anchor_visual_y: None,
            animation: None,
            force_snap_next: false,
            blink_visible: true,
            blink_last_toggle: Instant::now(),
            blink_reset_requested: false,
            cursor_owner_epoch: 0,
            last_move_source: CursorMoveSource::LayoutChange,
            // Issue #810 评论 问题2: 初始未可见，无保存的 selection head rect。
            visibility_state: CursorVisibilityState::Uninitialized,
            selection_head_rect: None,
        }
    }

    /// Issue #705 评论 5717380886: bump 光标所有权版本号。
    ///
    /// 由 `editing.rs::begin_manual_cursor_move()` 统一调用，标记一次"非正文事务
    /// 导致的逻辑 cursor 移动"（鼠标点击、方向键、Home/End、拖选等）。之后
    /// `animation_coordinator` 在驱动 coordinated caret 前检查事务记录的 epoch
    /// 是否仍等于当前 epoch。
    ///
    /// Issue #727 约束 1: epoch 不一致后，这笔事务立刻失去 caret motion ownership；
    /// InsertReveal / DeleteConceal 同时结束到 canonical 最终状态，不再继续播放。
    /// 即：caret ownership 丢失 = reveal/conceal ownership 同时丢失。
    ///
    /// 使用 `wrapping_add` 避免 overflow panic（u64 在实际使用中不可能溢出，
    /// 但遵守 AGENTS.md "不用 unwrap/expect 代替错误处理" 的安全边界）。
    pub fn bump_cursor_owner_epoch(&mut self) {
        self.cursor_owner_epoch = self.cursor_owner_epoch.wrapping_add(1);
    }

    /// Issue #810 评论 问题2: 记录选择手势结束时的 selection head visual rect。
    ///
    /// 由 `SujianEditorItem::end_selection_gesture()` 在 pointer release 时调用。
    /// 保存当前 visual_x/visual_y/visual_h/visual_baseline_y，选区收起后
    /// （has_selection 从 true 变 false）从该位置恢复 Tween，而非 Snap 瞬移。
    ///
    /// 注意：此方法只保存位置，不清 `force_snap_next`。`force_snap_next` 的清理
    /// 由 `take_force_snap_next()` 在下一次 `apply_plan()` 时统一处理。
    /// 手势结束后 `selection_gesture_active=false`，build_cursor_plan 不再因
    /// 手势强制 Snap，`force_snap_next` 只在仍有残留时生效一次。
    pub fn record_selection_head_rect(&mut self) {
        self.selection_head_rect = Some((
            self.visual_x,
            self.visual_y,
            self.visual_h,
            self.visual_baseline_y,
        ));
    }

    /// Issue #724 评论 5750911834 问题 2: 取出并立即重置 force_snap_next。
    ///
    /// 旧实现在 `apply_plan()` 后段才 `self.force_snap_next = false`，前面的
    /// `!should_be_visible` 提前返回会留下未消费的 force-snap，下一次正常光标
    /// 移动仍被直接 Snap。改用 `take_force_snap_next()` 在 `apply_plan()` 前段
    /// 消费 force_snap_next，确保任何返回路径都不会留下 force-snap。
    pub fn take_force_snap_next(&mut self) -> bool {
        let v = self.force_snap_next;
        self.force_snap_next = false;
        v
    }

    pub fn cursor_should_be_visible(&self) -> bool {
        self.visible
    }

    // 三个方法保持 pub(crate)：入参/返回的都是平台端内部光标动画状态，
    // 消费方只有 qquickitem_impl / properties / editing / rendering，
    // 集成测试只走 new() / bump_cursor_owner_epoch() / cursor_owner_epoch。
    pub(crate) fn cursor_blink_opacity(&self, blink_mode: CursorBlinkMode) -> f64 {
        if !self.visible {
            return 0.0;
        }
        if blink_mode == CursorBlinkMode::Suppressed {
            return 1.0;
        }
        if self.blink_visible {
            1.0
        } else {
            0.0
        }
    }

    pub(crate) fn apply_plan(&mut self, plan: &CursorAnimationPlan) -> CursorUpdateResult {
        // Issue #724 评论 5750911834 问题 2: 在 apply_plan() 前段消费 force_snap_next，
        // 避免前面的 !should_be_visible 提前返回留下未消费的 force-snap，
        // 下一次正常光标移动仍被直接 Snap。
        // plan 已由 build_cursor_plan() 接收 force_snap_next 参数并做出决策，
        // apply_plan() 不需要再读 self.force_snap_next，这里只负责消费重置。
        let _ = self.take_force_snap_next();

        let old_x = self.target_x;
        let old_y = self.target_y;
        let old_visible = self.visible;
        let old_blink_visible = self.blink_visible;

        // Issue #709 评论 issue-body-709: 明确的"刚启动或 rebase 一次光标 Tween"信号。
        // 旧实现只在 pos_changed == true 时设置 blink_visible/dirty，但新建 Tween 第一帧
        // visual_x/visual_y 还停在上一帧位置，pos_changed 很可能是 false。如果用户点击
        // 时正处在 blink 的隐藏半周期，Tween 已开始但 opacity 还是 0，看起来像"这次点击
        // 动画没触发"。新建或 rebase Tween 本身就是明确的状态变化信号，不依赖坐标是否
        // 已经移动。
        let mut started_or_rebased_tween = false;

        self.target_x = plan.cursor_x;
        self.target_y = plan.cursor_y;
        self.visual_h = plan.cursor_h;
        self.ime_cursor_rect_h = plan.cursor_h;
        self.visible = plan.should_be_visible;

        let visibility_changed = old_visible != plan.should_be_visible;

        if !plan.should_be_visible {
            // Issue #810 评论 问题2: 区分光标隐藏原因。
            //
            // 旧实现不区分隐藏原因，should_be_visible=false 时一律 visual 落到 target、
            // 清 animation。这导致选区收起后 old_visible=false 触发 build_cursor_plan
            // 的 !old_visible hard_snap，光标瞬移而非 Tween。
            //
            // 新逻辑：
            // - hidden_by_selection（因 has_selection 隐藏）：visual_x/visual_y 更新到
            //   plan.cursor_x/cursor_y（= selection head 位置，保持有效值），设
            //   HiddenBySelection 状态。选区收起后 build_cursor_plan 不再因 !old_visible
            //   强制 Snap，从 old_visual_x/old_visual_y（= selection head 位置）建 Tween。
            // - 非选区原因隐藏（editor disabled / 不在视口）：维持原行为。
            if plan.hidden_by_selection {
                self.visibility_state = CursorVisibilityState::HiddenBySelection;
                // visual 更新到当前 cursor 位置（selection head），保持有效值。
                // 不把 visual_x/visual_y 丢弃为无意义值，供恢复时作为 Tween 起点。
                self.visual_x = plan.cursor_x;
                self.visual_y = plan.cursor_y;
                self.visual_baseline_y = plan.cursor_baseline_y;
                // 清 animation：选区期间光标不绘制，不需要动画推进。
                // 恢复时从 visual_x/visual_y（selection head 位置）建新 Tween，
                // 不走旧 animation 的 finished/rebase 分支避免跳到旧 target。
                self.animation = None;
                if old_visible {
                    self.dirty = true;
                }
                let position_changed =
                    (old_x - plan.cursor_x).abs() > 0.01 || (old_y - plan.cursor_y).abs() > 0.01;
                return CursorUpdateResult {
                    ime_needs_update: position_changed,
                    needs_repaint: old_visible,
                    visibility_changed,
                    blink_changed: false,
                    visual_position_changed: position_changed,
                };
            }

            self.animation = None;
            self.visual_x = plan.cursor_x;
            self.visual_y = plan.cursor_y;
            self.visual_baseline_y = plan.cursor_baseline_y;
            self.blink_visible = true;
            // Issue #810 评论 问题2: 非选区原因隐藏，重置为 Uninitialized。
            self.visibility_state = CursorVisibilityState::Uninitialized;
            if old_visible {
                self.dirty = true;
            }
            let position_changed =
                (old_x - plan.cursor_x).abs() > 0.01 || (old_y - plan.cursor_y).abs() > 0.01;
            return CursorUpdateResult {
                ime_needs_update: position_changed,
                needs_repaint: old_visible,
                visibility_changed,
                blink_changed: old_blink_visible != self.blink_visible,
                visual_position_changed: position_changed,
            };
        }

        // Issue #810 评论 问题2: 光标可见时设 Visible 状态。
        // 从 HiddenBySelection 恢复到 Visible 时，build_cursor_plan 已不再因
        // !old_visible 强制 Snap（render_plan_builder 改动），走正常 Tween 判断，
        // 从 old_visual_x/old_visual_y（= selection head 位置）Tween 到新 target。
        if self.visibility_state == CursorVisibilityState::HiddenBySelection {
            // 从选区隐藏恢复：清除 selection_head_rect（已通过 visual_x/visual_y
            // 传递给 build_cursor_plan 作为 Tween 起点），标记恢复发生。
            self.selection_head_rect = None;
        }
        self.visibility_state = CursorVisibilityState::Visible;

        match &plan.transition {
            CursorTransition::Snap => {
                self.visual_x = plan.cursor_x;
                self.visual_y = plan.cursor_y;
                self.visual_baseline_y = plan.cursor_baseline_y;
                self.animation = None;
            }
            CursorTransition::Tween {
                old_rect,
                new_rect,
                duration_ms,
            } => {
                let start_x = old_rect.x;
                let start_y = old_rect.top;
                let target_x = new_rect.x;
                let target_y = new_rect.top;

                if let Some(ref anim) = self.animation {
                    // Issue #679 评论 5658087764 (2): 事务换了，即使目标点一样，
                    // 也必须从当前视觉位置 rebase 到新 driver，不能继续挂旧事务。
                    // Issue #686 评论 5664857575 领域2：连续 Insert/Delete 的 driver 切换时，
                    // 从 `anim.current_position()` rebase 到最新 target，
                    // 不把视觉光标先落回旧 old_cursor_rect（start_x/start_y 不用于此分支）。
                    // Issue #702 评论 5707449688 问题 2: 不再比较 driver_key，
                    // 纯光标移动只看 target 是否变化决定是否 rebase。
                    let target_changed = (anim.target_x - target_x).abs() > 0.01
                        || (anim.target_y - target_y).abs() > 0.01;

                    if target_changed {
                        // 从当前视觉位置 rebase，不落回 old_rect。
                        let (cur_x, cur_y) = anim.current_position();
                        self.animation = Some(CursorAnimationState {
                            start_x: cur_x,
                            start_y: cur_y,
                            target_x,
                            target_y,
                            progress: 0.0,
                            // Issue #702: 纯光标移动自己的 timeline。
                            // rebase 时重置 started_at 为 None，等下一帧 frame_now 启动。
                            started_at: None,
                            duration_ms: *duration_ms,
                        });
                        self.visual_x = cur_x;
                        self.visual_y = cur_y;
                        // Issue #712 评论 5739517945: rebase 时 baseline 从当前视觉
                        // baseline 继续，目标 baseline 取 new_rect.baseline_y。
                        self.visual_baseline_y = new_rect.baseline_y;
                        // Issue #709 评论 issue-body-709: rebase Tween 是明确的状态变化信号。
                        started_or_rebased_tween = true;
                    } else if anim.is_finished() {
                        self.visual_x = anim.target_x;
                        self.visual_y = anim.target_y;
                        self.visual_baseline_y = new_rect.baseline_y;
                        self.animation = None;
                    } else {
                        let (cur_x, cur_y) = anim.current_position();
                        self.visual_x = cur_x;
                        self.visual_y = cur_y;
                        // Issue #712 评论 5739517945: 动画进行中，baseline 取目标值。
                        self.visual_baseline_y = new_rect.baseline_y;
                    }
                } else {
                    // Issue #687: animation == None 分支永远从当前屏幕帧继续。
                    // 只要光标已经可见且 visual_x/visual_y 是有效当前屏幕位置，
                    // 新 Tween 的 start 永远取 self.visual_x/self.visual_y。
                    // old_rect 只作为首次出现、尚无可信 visual position 时的初始化来源，
                    // 不再作为连续编辑动画的回退起点。
                    let prev_vx = self.visual_x;
                    let prev_vy = self.visual_y;
                    if (prev_vx - target_x).abs() > 0.01 || (prev_vy - target_y).abs() > 0.01 {
                        let (init_x, init_y) = if old_visible {
                            (prev_vx, prev_vy)
                        } else {
                            // 首次出现：尚无可信 visual position，用 old_rect 初始化。
                            (start_x, start_y)
                        };
                        self.animation = Some(CursorAnimationState {
                            start_x: init_x,
                            start_y: init_y,
                            target_x,
                            target_y,
                            progress: 0.0,
                            // Issue #702: 纯光标移动自己的 timeline。
                            // started_at 留 None，等下一帧 frame_now 启动。
                            started_at: None,
                            duration_ms: *duration_ms,
                        });
                        // Issue #712 评论 5739517945: 新建 Tween 时 baseline 从
                        // old_rect.baseline_y 缓动到 new_rect.baseline_y。
                        self.visual_baseline_y = old_rect.baseline_y;
                        // Issue #709 评论 issue-body-709: 新建 Tween 是明确的状态变化信号。
                        started_or_rebased_tween = true;
                    } else {
                        self.visual_x = target_x;
                        self.visual_y = target_y;
                        self.visual_baseline_y = new_rect.baseline_y;
                    }
                }
            }
        }

        // Issue #724 评论 5750911834 问题 2: force_snap_next 已在 apply_plan() 前段
        // 由 take_force_snap_next() 消费重置，不再在此后段重置。

        let pos_changed = (self.visual_x - old_x).abs() > 0.01
            || (self.visual_y - old_y).abs() > 0.01
            || !old_visible;
        if pos_changed {
            self.dirty = true;
            self.blink_visible = true;
            self.blink_last_toggle = Instant::now();
        }

        // Issue #709 评论 issue-body-709: 新建或 rebase Tween 是额外的独立触发条件，
        // 不依赖坐标是否已经移动。第一帧 visual_x/visual_y 还停在上一帧位置，
        // pos_changed 很可能是 false，但用户确实刚启动了一次光标移动。
        // 此时必须立即 blink_visible = true / blink_last_toggle = now / dirty = true，
        // 否则用户点击时正处在 blink 的隐藏半周期，Tween 已开始但 opacity 还是 0，
        // 看起来像"这次点击动画没触发"。
        if started_or_rebased_tween {
            self.blink_visible = true;
            self.blink_last_toggle = Instant::now();
            self.dirty = true;
        }

        let blink_changed = old_blink_visible != self.blink_visible;

        let position_changed =
            (old_x - plan.cursor_x).abs() > 0.01 || (old_y - plan.cursor_y).abs() > 0.01;

        CursorUpdateResult {
            ime_needs_update: position_changed,
            // Issue #709 评论 issue-body-709: needs_repaint 必须包含 started_or_rebased_tween，
            // 即使第一帧坐标没移动也要触发重绘以应用新的 blink 状态。
            needs_repaint: pos_changed || self.animation.is_some() || started_or_rebased_tween,
            visibility_changed,
            blink_changed,
            visual_position_changed: pos_changed,
        }
    }

    pub fn update_animation_progress(&mut self, progress: f64) -> bool {
        if let Some(ref mut anim) = self.animation {
            anim.progress = progress.clamp(0.0, 1.0);
            if anim.is_finished() {
                self.visual_x = anim.target_x;
                self.visual_y = anim.target_y;
                self.animation = None;
                self.dirty = false;
                false
            } else {
                let (cx, cy) = anim.current_position();
                self.visual_x = cx;
                self.visual_y = cy;
                true
            }
        } else {
            self.dirty = false;
            false
        }
    }

    /// Issue #679 评论 5657313927: driver key 已不存在（Timeline 已结束/取消）时，
    /// 把 visual_x/y 精确落到 target_x/y 并删除 animation。
    /// 返回 true 表示发生过位置收尾，需要请求下一帧/重绘。
    pub fn finish_animation_to_target(&mut self) -> bool {
        if let Some(anim) = self.animation.take() {
            self.visual_x = anim.target_x;
            self.visual_y = anim.target_y;
            self.dirty = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn tick_blink(&mut self, blink_mode: CursorBlinkMode) -> bool {
        if !self.visible {
            if !self.blink_visible {
                return false;
            }
            self.blink_visible = false;
            return true;
        }

        if blink_mode == CursorBlinkMode::Suppressed {
            if !self.blink_visible {
                self.blink_visible = true;
                return true;
            }
            return false;
        }

        if self.blink_reset_requested {
            self.blink_visible = true;
            self.blink_last_toggle = Instant::now();
            self.blink_reset_requested = false;
            return true;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.blink_last_toggle);
        if elapsed >= Duration::from_millis(BLINK_INTERVAL_MS) {
            self.blink_visible = !self.blink_visible;
            self.blink_last_toggle = now;
            return true;
        }
        false
    }
}

/// 光标更新结果 — 描述一次光标更新后需要通知平台层的 UI 变化。
///
/// - `ime_needs_update`：光标位置变化需要通知输入法（QInputMethod::update）
/// - `needs_repaint`：光标视觉状态变化需要重绘
/// - `visibility_changed`：光标可见性变化（显示/隐藏切换）
/// - `blink_changed`：闪烁状态切换（可见↔不可见）
/// - `visual_position_changed`：光标渲染位置变化（动画帧推进或目标位置更新）
pub struct CursorUpdateResult {
    pub ime_needs_update: bool,
    pub needs_repaint: bool,
    pub visibility_changed: bool,
    pub blink_changed: bool,
    pub visual_position_changed: bool,
}
