package com.xiwei.sujian.platform

/**
 * Android AnimationDriver 接口
 *
 * Core 只负责 EditorVisualTransaction 语义（Insert/Delete/Cursor/Reflow/SystemSuppressed），
 * 平台负责坐标填充、帧驱动、绘制方式、暂停策略。
 * 滚动、加载、切章节、改字号、改主题、应用设置时统一进入 SystemSuppressed。
 */
interface AnimationDriver {
    /** 驱动一个视觉事务动画 */
    fun driveAnimation(
        transaction: Any,
        skipAnimation: Boolean,
    )

    /** 报告当前是否应该抑制动画 */
    fun shouldSuppressAnimation(): Boolean

    /** 通知动画暂停 */
    fun notifyAnimationSuppressed(reason: AnimationSuppressReason)

    /** 通知动画恢复 */
    fun notifyAnimationResumed()

    /** 取消所有进行中的动画 */
    fun cancelAllAnimations()

    /** 请求立即完成所有动画 */
    fun finishAllAnimations()
}

/** 动画暂停原因 */
enum class AnimationSuppressReason {
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
