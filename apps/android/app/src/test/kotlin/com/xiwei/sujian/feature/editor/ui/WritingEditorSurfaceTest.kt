@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.session.WindowBindingState
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #630 R12：editorSurfaceMode 纯函数决策测试 —
 * 覆盖活动/非活动 pane + 各种 bindingState 组合，验证双 renderer 切换已修复。
 *
 * 活动章节从进入页面到稳定显示必须只有 SujianEditorView 一套正文 renderer。
 * - isActivePane=true + Idle/Detached → Pending（不是 Preview）；
 * - isActivePane=true + 匹配 Attaching/Attached → Editor；
 * - isActivePane=false + Idle/Detached → Preview；
 * - 旧 window Attached + isActivePane=true → Pending（不冒充当前 Editor，不画 Preview）。
 */
class WritingEditorSurfaceTest {
    @Test
    fun activeTarget_idle_returnsPending() {
        assertEquals(
            "活动 target 且 Idle → Pending，绝不用第二套正文 renderer",
            EditorSurfaceMode.Pending,
            editorSurfaceMode(
                bindingState = WindowBindingState.Idle,
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_detached_returnsPending() {
        assertEquals(
            "活动 target 且 Detached → Pending，绝不用第二套正文 renderer",
            EditorSurfaceMode.Pending,
            editorSurfaceMode(
                bindingState = WindowBindingState.Detached("t1", 5UL, null),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_detaching_returnsPending() {
        assertEquals(
            "活动 target 且 Detaching → Pending",
            EditorSurfaceMode.Pending,
            editorSurfaceMode(
                bindingState = WindowBindingState.Detaching(null),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    // ── active + 匹配 Attaching/Attached → Editor ──

    @Test
    fun activeTarget_matchingAttaching_returnsEditor() {
        assertEquals(
            "活动 target + 匹配窗口 Attaching → Editor",
            EditorSurfaceMode.Editor,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attaching("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingAttached_returnsEditor() {
        assertEquals(
            "活动 target + 匹配窗口 Attached → Editor",
            EditorSurfaceMode.Editor,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingCommitting_returnsEditor() {
        assertEquals(
            "活动 target + Committing（targetId 匹配）→ Editor",
            EditorSurfaceMode.Editor,
            editorSurfaceMode(
                bindingState = WindowBindingState.Committing("t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun activeTarget_matchingCancelling_returnsEditor() {
        assertEquals(
            "活动 target + Cancelling（targetId 匹配）→ Editor",
            EditorSurfaceMode.Editor,
            editorSurfaceMode(
                bindingState = WindowBindingState.Cancelling("t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    // ── 非 active + Idle/Detached → Preview ──

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

    // ── 旧 window Attached + active → Pending（不冒充当前 Editor，不画 Preview）──

    @Test
    fun oldWindowAttached_activeTarget_returnsPending() {
        assertEquals(
            "旧窗口 Attached 但 target 仍是 active → Pending，不冒充当前 Editor",
            EditorSurfaceMode.Pending,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("oldWindow", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = true,
            ),
        )
    }

    @Test
    fun oldWindowAttaching_activeTarget_returnsPending() {
        assertEquals(
            "旧窗口 Attaching 但 target 仍是 active → Pending，不冒充当前 Editor",
            EditorSurfaceMode.Pending,
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

    // ── isActivePane=false 时的退化（精确绑定态仍优先 → Editor）──

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
    fun nullActiveTarget_matchingAttached_returnsEditor() {
        assertEquals(
            "isActivePane=false + 匹配 Attached → Editor（精确绑定态优先于 isActivePane）",
            EditorSurfaceMode.Editor,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                isActivePane = false,
            ),
        )
    }

    // ── Committing/Cancelling 对其他 target → Pending（若 active）或 Preview ──

    @Test
    fun committing_otherTarget_active_returnsPending() {
        assertEquals(
            "Committing targetId 不匹配 + 活动 → Pending",
            EditorSurfaceMode.Pending,
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
