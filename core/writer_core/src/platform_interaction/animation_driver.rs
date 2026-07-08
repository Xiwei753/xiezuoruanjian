//! AnimationDriver — 动画语义与帧驱动分离
//!
//! Core 只负责 `EditorVisualTransaction` 语义（Insert/Delete/Cursor/Reflow/SystemSuppressed），
//! 平台负责坐标填充、帧驱动、绘制方式、暂停策略。
//! 滚动、加载、切章节、改字号、改主题、应用设置时统一进入 SystemSuppressed。

use crate::editor::{AnimationMode, EditorVisualTransaction};
use serde::{Deserialize, Serialize};

/// 动画暂停原因 — 平台适配层报告给 Core
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AnimationSuppressReason {
    Scrolling,
    LoadingChapter,
    SwitchingChapter,
    ChangingFontSize,
    ChangingTheme,
    ApplyingSettings,
    AnimationDisabled,
    WindowMinimized,
    WindowHidden,
}

/// 动画驱动请求 — Core → 平台适配层
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimationDriveRequest {
    /// 视觉事务（Core 填充语义，平台填充坐标）
    pub transaction: EditorVisualTransaction,
    /// 是否需要立即完成（跳过动画）
    pub skip_animation: bool,
}

/// 动画完成回调 — 平台适配层 → Core
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimationCompletion {
    /// 事务 ID
    pub transaction_id: u64,
    /// 是否正常完成
    pub completed_normally: bool,
}

/// AnimationDriver trait — 平台适配层实现
///
/// 平台端实现此 trait，负责：
/// 1. 坐标填充（glyph rects, cursor rects, reflow rects）
/// 2. 帧驱动（requestAnimationFrame / vsync / timer）
/// 3. 绘制方式（QML overlay / Canvas / Composition animation）
/// 4. 暂停策略（滚动/加载/字号变化时进入 SystemSuppressed）
pub trait AnimationDriver {
    /// 驱动一个视觉事务动画
    ///
    /// Core 调用此方法传入语义事务，平台填充坐标后驱动动画。
    /// 如果 skip_animation 为 true，平台应立即完成事务而不播放动画。
    fn drive_animation(&mut self, request: AnimationDriveRequest);

    /// 报告当前是否应该抑制动画
    fn should_suppress_animation(&self) -> bool {
        false
    }

    /// 获取当前抑制原因
    fn current_suppress_reason(&self) -> Option<AnimationSuppressReason> {
        None
    }

    /// 通知动画暂停（滚动/加载/字号变化等）
    fn notify_animation_suppressed(&mut self, reason: AnimationSuppressReason);

    /// 通知动画恢复
    fn notify_animation_resumed(&mut self);

    /// 取消所有进行中的动画
    fn cancel_all_animations(&mut self);

    /// 请求立即完成所有动画（跳过动画但保留最终状态）
    fn finish_all_animations(&mut self);

    /// 根据抑制原因计算动画模式
    ///
    /// 如果当前有抑制原因，返回 SystemSuppressed；
    /// 否则返回 Core 决定的 animation_mode。
    fn effective_animation_mode(
        &self,
        core_mode: AnimationMode,
    ) -> AnimationMode {
        if self.should_suppress_animation() {
            AnimationMode::SystemSuppressed
        } else {
            core_mode
        }
    }
}

/// 判断抑制原因是否应该阻止动画
pub fn should_suppress_for_reason(reason: AnimationSuppressReason) -> bool {
    matches!(
        reason,
        AnimationSuppressReason::Scrolling
            | AnimationSuppressReason::LoadingChapter
            | AnimationSuppressReason::SwitchingChapter
            | AnimationSuppressReason::ChangingFontSize
            | AnimationSuppressReason::ChangingTheme
            | AnimationSuppressReason::ApplyingSettings
            | AnimationSuppressReason::AnimationDisabled
            | AnimationSuppressReason::WindowMinimized
            | AnimationSuppressReason::WindowHidden
    )
}

/// 判断 EditorTransactionCause 是否应该进入 SystemSuppressed
pub fn cause_should_suppress(cause: crate::editor::EditorTransactionCause) -> bool {
    matches!(
        cause,
        crate::editor::EditorTransactionCause::Load
            | crate::editor::EditorTransactionCause::Format
            | crate::editor::EditorTransactionCause::Programmatic
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_suppress_reasons_should_suppress() {
        let reasons = [
            AnimationSuppressReason::Scrolling,
            AnimationSuppressReason::LoadingChapter,
            AnimationSuppressReason::SwitchingChapter,
            AnimationSuppressReason::ChangingFontSize,
            AnimationSuppressReason::ChangingTheme,
            AnimationSuppressReason::ApplyingSettings,
            AnimationSuppressReason::AnimationDisabled,
            AnimationSuppressReason::WindowMinimized,
            AnimationSuppressReason::WindowHidden,
        ];
        for reason in reasons {
            assert!(should_suppress_for_reason(reason), "{:?} should suppress", reason);
        }
    }

    #[test]
    fn effective_mode_returns_system_suppressed_when_suppressed() {
        struct SuppressedDriver;
        impl AnimationDriver for SuppressedDriver {
            fn drive_animation(&mut self, _request: AnimationDriveRequest) {}
            fn should_suppress_animation(&self) -> bool { true }
            fn current_suppress_reason(&self) -> Option<AnimationSuppressReason> {
                Some(AnimationSuppressReason::Scrolling)
            }
            fn notify_animation_suppressed(&mut self, _reason: AnimationSuppressReason) {}
            fn notify_animation_resumed(&mut self) {}
            fn cancel_all_animations(&mut self) {}
            fn finish_all_animations(&mut self) {}
        }
        let driver = SuppressedDriver;
        assert_eq!(
            driver.effective_animation_mode(AnimationMode::GlyphAnimation),
            AnimationMode::SystemSuppressed
        );
    }

    #[test]
    fn effective_mode_returns_core_mode_when_not_suppressed() {
        struct ActiveDriver;
        impl AnimationDriver for ActiveDriver {
            fn drive_animation(&mut self, _request: AnimationDriveRequest) {}
            fn should_suppress_animation(&self) -> bool { false }
            fn current_suppress_reason(&self) -> Option<AnimationSuppressReason> { None }
            fn notify_animation_suppressed(&mut self, _reason: AnimationSuppressReason) {}
            fn notify_animation_resumed(&mut self) {}
            fn cancel_all_animations(&mut self) {}
            fn finish_all_animations(&mut self) {}
        }
        let driver = ActiveDriver;
        assert_eq!(
            driver.effective_animation_mode(AnimationMode::GlyphAnimation),
            AnimationMode::GlyphAnimation
        );
    }

    #[test]
    fn cause_should_suppress_load_format_programmatic() {
        assert!(cause_should_suppress(crate::editor::EditorTransactionCause::Load));
        assert!(cause_should_suppress(crate::editor::EditorTransactionCause::Format));
        assert!(cause_should_suppress(crate::editor::EditorTransactionCause::Programmatic));
        assert!(!cause_should_suppress(crate::editor::EditorTransactionCause::Typing));
        assert!(!cause_should_suppress(crate::editor::EditorTransactionCause::Delete));
        assert!(!cause_should_suppress(crate::editor::EditorTransactionCause::Paste));
    }
}
