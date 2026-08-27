@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.session.WindowBindingState
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #640 A.3：editorSurfaceMode 纯函数决策测试 —
 * 活动 target 始终组合 AndroidView（EditorHost），用 View.INVISIBLE 控制可见性。
 *
 * 活动章节从进入页面到稳定显示必须只有 SujianEditorView 一套正文 renderer。
 * - isActivePane=true → EditorHost（始终组合 AndroidView，用 View.INVISIBLE 控制可见性）；
 * - isActivePane=false + 匹配 Attaching/Attached/Committing/Cancelling → EditorHost；
 * - isActivePane=false + Idle/Detached/Detaching → Preview。
 */
class WritingEditorSurfaceTest {
    // ── active + 任意 bindingState → EditorHost（始终组合 AndroidView）──

    @Test
    fun activeTarget_idle_returnsEditorHost() {
        assertEquals(
            "活动 target 且 Idle → EditorHost，始终组合 AndroidView",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Idle,
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_detached_returnsEditorHost() {
        assertEquals(
            "活动 target 且 Detached → EditorHost，始终组合 AndroidView",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Detached("t1", 5UL, null),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_detaching_returnsEditorHost() {
        assertEquals(
            "活动 target 且 Detaching → EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Detaching(null),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingAttaching_returnsEditorHost() {
        assertEquals(
            "活动 target + 匹配窗口 Attaching → EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attaching("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingAttached_returnsEditorHost() {
        assertEquals(
            "活动 target + 匹配窗口 Attached → EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingCommitting_returnsEditorHost() {
        assertEquals(
            "活动 target + Committing（targetId 匹配）→ EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Committing("t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingCancelling_returnsEditorHost() {
        assertEquals(
            "活动 target + Cancelling（targetId 匹配）→ EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Cancelling("t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    // ── 非 active + Idle/Detached/Detaching → Preview ──

    @Test
    fun inactiveTarget_idle_returnsPreview() {
        assertEquals(
            "非活动 target + Idle → Preview（只读预览）",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Idle,
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    @Test
    fun inactiveTarget_detached_returnsPreview() {
        assertEquals(
            "非活动 target + Detached → Preview",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Detached("t1", 5UL, null),
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    @Test
    fun inactiveTarget_detaching_returnsPreview() {
        assertEquals(
            "非活动 target + Detaching → Preview",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Detaching(null),
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    // ── 旧 window Attached/Attaching + active → EditorHost（不冒充当前 EditorHost，始终组合 AndroidView）──

    @Test
    fun oldWindowAttached_activeTarget_returnsEditorHost() {
        assertEquals(
            "旧窗口 Attached 但 target 仍是 active → EditorHost，不冒充当前 EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("oldWindow", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun oldWindowAttaching_activeTarget_returnsEditorHost() {
        assertEquals(
            "旧窗口 Attaching 但 target 仍是 active → EditorHost，不冒充当前 EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attaching("oldWindow", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun oldWindowAttached_inactiveTarget_returnsPreview() {
        assertEquals(
            "旧窗口 Attached 且非活动 target → Preview",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("oldWindow", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    // ── isActivePane=false 时的退化（精确绑定态仍优先 → EditorHost）──

    @Test
    fun nullActiveTarget_idle_returnsPreview() {
        assertEquals(
            "isActivePane=false + Idle → Preview（不绘制任何编辑器）",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Idle,
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    @Test
    fun nullActiveTarget_matchingAttached_returnsEditorHost() {
        assertEquals(
            "isActivePane=false + 匹配 Attached → EditorHost（精确绑定态优先于 isActivePane）",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    // ── Committing/Cancelling 对其他 target → EditorHost（若 active）或 Preview ──

    @Test
    fun committing_otherTarget_active_returnsEditorHost() {
        assertEquals(
            "Committing targetId 不匹配 + 活动 → EditorHost",
            EditorSurfaceMode.EditorHost,
            editorSurfaceMode(
                bindingState = WindowBindingState.Committing("otherTarget", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun cancelling_otherTarget_inactive_returnsPreview() {
        assertEquals(
            "Cancelling targetId 不匹配 + 非活动 → Preview",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Cancelling("otherTarget", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }
}
