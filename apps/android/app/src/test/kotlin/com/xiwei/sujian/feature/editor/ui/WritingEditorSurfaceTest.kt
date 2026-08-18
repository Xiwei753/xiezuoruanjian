@file:Suppress("StringLiteralDuplication") // 测试固件字符串天然重复

package com.xiwei.sujian.feature.editor.ui

import com.xiwei.sujian.feature.editor.session.WindowBindingState
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #630 R12：editorSurfaceMode 纯函数决策测试 —
 * 覆盖活动/非活动 target + 各种 bindingState 组合，验证双 renderer 切换已修复。
 *
 * 活动章节从进入页面到稳定显示必须只有 SujianEditorView 一套正文 renderer。
 * - activeTarget + Idle/Detached → Pending（不是 Preview）；
 * - activeTarget + 匹配 Attaching/Attached → Editor；
 * - 非 activeTarget + Idle/Detached → Preview；
 * - 旧 window Attached + activeTarget → Pending（不冒充当前 Editor，不画 Preview）。
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "other-target",
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
                activeTargetId = "other-target",
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
                activeTargetId = "other-target",
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
                activeTargetId = "t1",
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
                activeTargetId = "t1",
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
                activeTargetId = "other-target",
            ),
        )
    }

    // ── activeTargetId 为 null 时的退化 ──

    @Test
    fun nullActiveTarget_idle_returnsPreview() {
        assertEquals(
            "activeTargetId=null + Idle → Preview（不绘制任何编辑器）",
            EditorSurfaceMode.Preview,
            editorSurfaceMode(
                bindingState = WindowBindingState.Idle,
                windowId = "w1",
                targetId = "t1",
                activeTargetId = null,
            ),
        )
    }

    @Test
    fun nullActiveTarget_matchingAttached_returnsEditor() {
        assertEquals(
            "activeTargetId=null + 匹配 Attached → Editor（向后兼容旧 shouldShowEditor 行为）",
            EditorSurfaceMode.Editor,
            editorSurfaceMode(
                bindingState = WindowBindingState.Attached("w1", "t1", 5UL),
                windowId = "w1",
                targetId = "t1",
                activeTargetId = null,
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
                activeTargetId = "t1",
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
                activeTargetId = "other-target",
            ),
        )
    }

    // ── shouldShowEditor 向后兼容（activeTargetId=null 时等价旧逻辑）──

    @Test
    fun shouldShowEditor_backwardCompatible_matchesEditorSurfaceMode() {
        // shouldShowEditor 内部调用 editorSurfaceMode(activeTargetId=null)，
        // 验证所有 bindingState 分支行为一致。
        val testCases =
            listOf(
                Pair(WindowBindingState.Idle, false),
                Pair(WindowBindingState.Detached("t1", 5UL, null), false),
                Pair(WindowBindingState.Detaching(null), false),
                Pair(WindowBindingState.Attaching("w1", "t1", 5UL), true),
                Pair(WindowBindingState.Attached("w1", "t1", 5UL), true),
                Pair(WindowBindingState.Committing("t1", 5UL), true),
                Pair(WindowBindingState.Cancelling("t1", 5UL), true),
                // 旧窗口 Attached 不匹配
                Pair(WindowBindingState.Attached("oldWindow", "t1", 5UL), false),
                Pair(WindowBindingState.Attaching("oldWindow", "t1", 5UL), false),
            )

        for ((bindingState, expectedShowEditor) in testCases) {
            val expectedMode = if (expectedShowEditor) EditorSurfaceMode.Editor else EditorSurfaceMode.Preview
            assertEquals(
                "shouldShowEditor($bindingState) 应与 editorSurfaceMode(activeTargetId=null) 一致",
                expectedMode,
                editorSurfaceMode(bindingState, "w1", "t1", activeTargetId = null),
            )
            assertEquals(
                "shouldShowEditor($bindingState) = $expectedShowEditor",
                expectedShowEditor,
                shouldShowEditor(bindingState, "w1", "t1"),
            )
        }
    }
}
