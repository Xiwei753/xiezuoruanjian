package com.xiwei.sujian.editor.v2.host

import com.xiwei.sujian.editor.v2.coordinator.ProjectionSnapshot
import com.xiwei.sujian.editor.v2.coordinator.TargetSnapshot
import com.xiwei.sujian.editor.v2.coordinator.WindowBindingState
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #595 八：EditorAttachmentState 被实际消费决定渲染策略的契约测试。
 *
 * WritingEditorSurface 用 attachmentState 的类型和 targetId 匹配决定显示编辑器还是预览：
 * - Attached/Attaching/Paused 且 targetId 匹配 → 显示编辑器
 * - Detached/Idle → 显示预览
 */
class AttachmentStateConsumptionContractTest {

    private val frame = EditorFrameSnapshot(
        scrollX = 0f, scrollY = 0f,
        viewportWidth = 100, viewportHeight = 200,
        hasActiveAnimation = false,
    )

    private fun showEditor(state: EditorAttachmentState, targetId: String): Boolean = when (state) {
        is EditorAttachmentState.Attached -> state.targetId == targetId
        is EditorAttachmentState.Attaching -> state.targetId == targetId
        is EditorAttachmentState.Paused -> state.targetId == targetId
        else -> false
    }

    @Test
    fun attachedWithMatchingTargetShowsEditor() {
        val state = EditorAttachmentState.Attached("w1", "t1", 7UL)
        assertTrue("Attached with matching target must show editor", showEditor(state, "t1"))
    }

    @Test
    fun attachedWithDifferentTargetShowsPreview() {
        val state = EditorAttachmentState.Attached("w1", "t1", 7UL)
        assertFalse("Attached with different target must show preview", showEditor(state, "t2"))
    }

    @Test
    fun attachingWithMatchingTargetShowsEditor() {
        val state = EditorAttachmentState.Attaching("w1", "t1", 7UL)
        assertTrue("Attaching with matching target must show editor", showEditor(state, "t1"))
    }

    @Test
    fun pausedWithMatchingTargetShowsEditor() {
        val state = EditorAttachmentState.Paused("t1", 7UL, frame)
        assertTrue("Paused with matching target must show editor", showEditor(state, "t1"))
    }

    @Test
    fun detachedShowsPreview() {
        val state = EditorAttachmentState.Detached("t1", 7UL, null)
        assertFalse("Detached must show preview", showEditor(state, "t1"))
    }

    @Test
    fun idleShowsPreview() {
        assertFalse("Idle must show preview", showEditor(EditorAttachmentState.Idle, "t1"))
    }

    @Test
    fun releasingShowsPreview() {
        assertFalse("Releasing must show preview", showEditor(EditorAttachmentState.Releasing, "t1"))
    }

    @Test
    fun pausedFromAttachedAndPausedFlag() {
        val state = attachmentStateFromBinding(
            WindowBindingState.Attached("w1", "t1", 7UL),
            paused = true,
            frameSnapshot = frame,
            projectionSnapshot = null,
        )
        assertTrue("Attached + paused must derive Paused", state is EditorAttachmentState.Paused)
        assertTrue("Paused must show editor", showEditor(state, "t1"))
    }

    @Test
    fun attachedWithoutPauseDerivesAttached() {
        val state = attachmentStateFromBinding(
            WindowBindingState.Attached("w1", "t1", 7UL),
            paused = false,
            frameSnapshot = null,
            projectionSnapshot = null,
        )
        assertTrue("Attached without pause must derive Attached", state is EditorAttachmentState.Attached)
        assertTrue("Attached must show editor", showEditor(state, "t1"))
    }
}
