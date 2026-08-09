package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.ui.shouldShowEditor
import com.xiwei.sujian.feature.editor.session.WindowBindingState
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 八：正文 Surface 渲染决策消费规范 [WindowBindingState] 的契约测试。
 *
 * WritingEditorSurface 用 [shouldShowEditor]（纯函数）消费会话层唯一规范状态机
 * [WindowBindingState]，决定显示编辑器还是预览：
 * - Attaching/Attached/Committing/Cancelling 且 targetId 匹配 → 显示编辑器
 * - Idle/Detaching/Detached → 显示预览
 *
 * 不再存在第二套 EditorAttachmentState 派生类型 — 临时失焦（动画暂停）不改变
 * binding 状态，Attached 时编辑器始终显示，暂停/恢复由 View 内部处理。
 */
class AttachmentStateConsumptionTest {
    @Test
    fun attachedWithMatchingTargetShowsEditor() {
        assertTrue(
            "Attached with matching target must show editor",
            shouldShowEditor(WindowBindingState.Attached("w1", "t1", 7UL), "t1"),
        )
    }

    @Test
    fun attachedWithDifferentTargetShowsPreview() {
        assertFalse(
            "Attached with different target must show preview",
            shouldShowEditor(WindowBindingState.Attached("w1", "t1", 7UL), "t2"),
        )
    }

    @Test
    fun attachingWithMatchingTargetShowsEditor() {
        assertTrue(
            "Attaching with matching target must show editor",
            shouldShowEditor(WindowBindingState.Attaching("w1", "t1", 7UL), "t1"),
        )
    }

    @Test
    fun committingWithMatchingTargetShowsEditor() {
        assertTrue(
            "Committing with matching target must keep editor visible",
            shouldShowEditor(WindowBindingState.Committing("t1", 7UL), "t1"),
        )
    }

    @Test
    fun cancellingWithMatchingTargetShowsEditor() {
        assertTrue(
            "Cancelling with matching target must keep editor visible",
            shouldShowEditor(WindowBindingState.Cancelling("t1", 7UL), "t1"),
        )
    }

    @Test
    fun detachedShowsPreview() {
        assertFalse(
            "Detached must show preview",
            shouldShowEditor(WindowBindingState.Detached("t1", 7UL, null), "t1"),
        )
    }

    @Test
    fun detachingShowsPreview() {
        assertFalse(
            "Detaching must show preview",
            shouldShowEditor(WindowBindingState.Detaching(null), "t1"),
        )
    }

    @Test
    fun idleShowsPreview() {
        assertFalse("Idle must show preview", shouldShowEditor(WindowBindingState.Idle, "t1"))
    }
}
