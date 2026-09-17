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

use super::animation_coordinator::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::rendering::CursorAnimationState;
use crate::editor::layout::CaretAffinity;
use std::time::{Duration, Instant};

const BLINK_INTERVAL_MS: u64 = 530;

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
///   是否仍等于当前 epoch。epoch 不一致时，文字事务继续播自己的 glyph/reflow，
///   但不再驱动 caret（caret 改由纯光标移动 / 鼠标目标驱动）。
pub struct CursorController {
    pub target_x: f64,
    pub target_y: f64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_h: f64,
    pub visible: bool,
    pub dirty: bool,
    pub affinity: CaretAffinity,
    pub last_scroll_y: f64,
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
}

impl CursorController {
    pub fn new() -> Self {
        Self {
            target_x: 0.0,
            target_y: 0.0,
            visual_x: 0.0,
            visual_y: 0.0,
            visual_h: 0.0,
            visible: false,
            dirty: false,
            affinity: CaretAffinity::Downstream,
            last_scroll_y: 0.0,
            ime_cursor_rect_h: 0.0,
            anchor_visual_x: None,
            anchor_visual_y: None,
            animation: None,
            force_snap_next: false,
            blink_visible: true,
            blink_last_toggle: Instant::now(),
            blink_reset_requested: false,
            cursor_owner_epoch: 0,
        }
    }

    /// Issue #705 评论 5717380886: bump 光标所有权版本号。
    ///
    /// 由 `editing.rs::begin_manual_cursor_move()` 统一调用，标记一次"非正文事务
    /// 导致的逻辑 cursor 移动"（鼠标点击、方向键、Home/End、拖选等）。之后
    /// `animation_coordinator` 在驱动 coordinated caret 前检查事务记录的 epoch
    /// 是否仍等于当前 epoch，不一致则不驱动 caret（文字事务继续播自己的
    /// glyph/reflow，但 caret 改由纯光标移动 / 鼠标目标驱动）。
    ///
    /// 使用 `wrapping_add` 避免 overflow panic（u64 在实际使用中不可能溢出，
    /// 但遵守 AGENTS.md "不用 unwrap/expect 代替错误处理" 的安全边界）。
    pub fn bump_cursor_owner_epoch(&mut self) {
        self.cursor_owner_epoch = self.cursor_owner_epoch.wrapping_add(1);
    }

    pub fn cursor_should_be_visible(&self) -> bool {
        self.visible
    }

    pub fn cursor_blink_opacity(&self, blink_mode: CursorBlinkMode) -> f64 {
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

    pub fn apply_plan(&mut self, plan: &CursorAnimationPlan) -> CursorUpdateResult {
        let old_x = self.target_x;
        let old_y = self.target_y;
        let old_visible = self.visible;
        let old_blink_visible = self.blink_visible;

        self.target_x = plan.cursor_x;
        self.target_y = plan.cursor_y;
        self.visual_h = plan.cursor_h;
        self.ime_cursor_rect_h = plan.cursor_h;
        self.visible = plan.should_be_visible;

        let visibility_changed = old_visible != plan.should_be_visible;

        if !plan.should_be_visible {
            self.animation = None;
            self.visual_x = plan.cursor_x;
            self.visual_y = plan.cursor_y;
            self.blink_visible = true;
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

        match &plan.transition {
            CursorTransition::Snap => {
                self.visual_x = plan.cursor_x;
                self.visual_y = plan.cursor_y;
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
                    } else if anim.is_finished() {
                        self.visual_x = anim.target_x;
                        self.visual_y = anim.target_y;
                        self.animation = None;
                    } else {
                        let (cur_x, cur_y) = anim.current_position();
                        self.visual_x = cur_x;
                        self.visual_y = cur_y;
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
                    } else {
                        self.visual_x = target_x;
                        self.visual_y = target_y;
                    }
                }
            }
        }

        self.force_snap_next = false;

        let pos_changed = (self.visual_x - old_x).abs() > 0.01
            || (self.visual_y - old_y).abs() > 0.01
            || !old_visible;
        if pos_changed {
            self.dirty = true;
            self.blink_visible = true;
            self.blink_last_toggle = Instant::now();
        }

        let blink_changed = old_blink_visible != self.blink_visible;

        let position_changed =
            (old_x - plan.cursor_x).abs() > 0.01 || (old_y - plan.cursor_y).abs() > 0.01;

        CursorUpdateResult {
            ime_needs_update: position_changed,
            needs_repaint: pos_changed || self.animation.is_some(),
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

    pub fn tick_blink(&mut self, blink_mode: CursorBlinkMode) -> bool {
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
