package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.session.WindowBindingState
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 八：正文 Surface 渲染决策消费规范 [WindowBindingState] 的契约测试。
 *
 * WritingEditorSurface 用 [shouldShowEditor]（纯函数）消费会话层唯一规范状态机
 * [WindowBindingState]，决定显示编辑器还是预览：
 * - Attaching/Attached 且 windowId + targetId 都匹配 → 显示编辑器
 * - Committing/Cancelling 且 targetId 匹配 → 编辑器保持显示
 * - Idle/Detaching/Detached → 显示预览
 *
 * #623 评论5：Attaching/Attached 的判定必须带窗口身份 — 残留自其他窗口的绑定
 * （旧窗口 release 与新窗口附着之间的竞态）对新窗口不算已绑定，显示预览，
 * 不得创建未绑定 session 的编辑器 View。
 *
 * 不再存在第二套 EditorAttachmentState 派生类型 — 临时失焦（动画暂停）不改变
 * binding 状态，Attached 时编辑器始终显示，暂停/恢复由 View 内部处理。
 */
class AttachmentStateConsumptionTest {
    @Test
    fun attachedWithMatchingWindowAndTargetShowsEditor() {
        assertTrue(
            "Attached with matching window+target must show editor",
            shouldShowEditor(WindowBindingState.Attached("w1", "t1", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun attachedWithDifferentWindowShowsPreview() {
        assertFalse(
            "Attached from a different window must show preview — the current window has no bound view",
            shouldShowEditor(WindowBindingState.Attached("w1", "t1", 7UL), "w2", "t1"),
        )
    }

    @Test
    fun attachedWithDifferentTargetShowsPreview() {
        assertFalse(
            "Attached with different target must show preview",
            shouldShowEditor(WindowBindingState.Attached("w1", "t1", 7UL), "w1", "t2"),
        )
    }

    @Test
    fun attachingWithMatchingWindowAndTargetShowsEditor() {
        assertTrue(
            "Attaching with matching window+target must show editor",
            shouldShowEditor(WindowBindingState.Attaching("w1", "t1", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun attachingWithDifferentWindowShowsPreview() {
        assertFalse(
            "Attaching from a different window (e.g. prepared pre-binding before the pane attaches) " +
                "must show preview until the current window re-stamps its own Attaching",
            shouldShowEditor(WindowBindingState.Attaching("w1", "t1", 7UL), "w2", "t1"),
        )
    }

    @Test
    fun committingWithMatchingTargetKeepsEditorVisible() {
        assertTrue(
            "Committing with matching target must keep editor visible",
            shouldShowEditor(WindowBindingState.Committing("t1", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun cancellingWithMatchingTargetKeepsEditorVisible() {
        assertTrue(
            "Cancelling with matching target must keep editor visible",
            shouldShowEditor(WindowBindingState.Cancelling("t1", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun detachedShowsPreview() {
        assertFalse(
            "Detached must show preview",
            shouldShowEditor(WindowBindingState.Detached("t1", 7UL, null), "w1", "t1"),
        )
    }

    @Test
    fun detachingShowsPreview() {
        assertFalse(
            "Detaching must show preview",
            shouldShowEditor(WindowBindingState.Detaching(null), "w1", "t1"),
        )
    }

    @Test
    fun idleShowsPreview() {
        assertFalse("Idle must show preview", shouldShowEditor(WindowBindingState.Idle, "w1", "t1"))
    }

    // ── #624 评论16 问题3：confirmEditorAttached 只在真正 Attached 且 windowId+targetId 匹配时调用 ──

    @Test
    fun shouldConfirmEditorAttached_falseWhenAttaching() {
        assertFalse(
            "Attaching 状态不得 confirmEditorAttached — View 尚未真实绑定",
            shouldConfirmEditorAttached(WindowBindingState.Attaching("w1", "t1", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun shouldConfirmEditorAttached_falseWhenIdle() {
        assertFalse(
            "Idle 状态不得 confirmEditorAttached — beginEdit 尚未成功",
            shouldConfirmEditorAttached(WindowBindingState.Idle, "w1", "t1"),
        )
    }

    @Test
    fun shouldConfirmEditorAttached_falseWhenDetached() {
        assertFalse(
            "Detached 状态不得 confirmEditorAttached",
            shouldConfirmEditorAttached(WindowBindingState.Detached("t1", 7UL, null), "w1", "t1"),
        )
    }

    @Test
    fun shouldConfirmEditorAttached_falseWhenAttachedDifferentWindow() {
        assertFalse(
            "Attached 但 windowId 不匹配不得 confirmEditorAttached",
            shouldConfirmEditorAttached(WindowBindingState.Attached("w2", "t1", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun shouldConfirmEditorAttached_falseWhenAttachedDifferentTarget() {
        assertFalse(
            "Attached 但 targetId 不匹配不得 confirmEditorAttached",
            shouldConfirmEditorAttached(WindowBindingState.Attached("w1", "t2", 7UL), "w1", "t1"),
        )
    }

    @Test
    fun shouldConfirmEditorAttached_trueOnlyWhenAttachedAndMatching() {
        assertTrue(
            "Attached 且 windowId + targetId 都匹配时才 confirmEditorAttached",
            shouldConfirmEditorAttached(WindowBindingState.Attached("w1", "t1", 7UL), "w1", "t1"),
        )
    }
}
