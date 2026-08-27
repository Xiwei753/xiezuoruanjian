package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchPlacement
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #640 C：大屏工作台组合模式决策回归测试。
 *
 * - plan=null/SINGLE_PANE → SINGLE_PANE_WITH_TOP_BAR（无论 hidden/visible，在 Rust Editor free-region
 *   内测量 singlePaneTopBar + EditorPane body）；
 * - plan=WORKBENCH + presentationVisible=false → EDITOR_ONLY_PREWARM（只组合/测量唯一 EditorPane，
 *   不进入完整 Workbench shell，避免隐藏阶段预热 toolbar / 第二份 ChapterTree /
 *   syncStatusRepository collector / WritingToolRail / WritingToolPane）；
 * - plan=WORKBENCH + presentationVisible=true → FULL_WORKBENCH。
 *
 * EditorPane 始终在 WideWritingWorkspace 唯一 call site，三种模式切换不重建 AndroidView。
 * 直接调用生产入口 [resolveWideWorkspaceCompositionMode]；不为测试重复决策逻辑。
 */
class WideWritingWorkspaceCompositionModeTest {
    @Test
    fun hidden_workbenchPlan_returnsEditorOnlyPrewarm() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH)
        assertEquals(
            WideWorkspaceCompositionMode.EDITOR_ONLY_PREWARM,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = false),
        )
    }

    @Test
    fun hidden_singlePanePlan_returnsSinglePaneWithTopBar() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.SINGLE_PANE)
        assertEquals(
            WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = false),
        )
    }

    @Test
    fun hidden_nullPlan_returnsSinglePaneWithTopBar() {
        assertEquals(
            WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR,
            resolveWideWorkspaceCompositionMode(null, presentationVisible = false),
        )
    }

    @Test
    fun visible_workbenchPlan_returnsFullWorkbench() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH)
        assertEquals(
            WideWorkspaceCompositionMode.FULL_WORKBENCH,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = true),
        )
    }

    @Test
    fun visible_singlePanePlan_returnsSinglePaneWithTopBar() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.SINGLE_PANE)
        assertEquals(
            WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = true),
        )
    }

    @Test
    fun visible_nullPlan_returnsSinglePaneWithTopBar() {
        assertEquals(
            WideWorkspaceCompositionMode.SINGLE_PANE_WITH_TOP_BAR,
            resolveWideWorkspaceCompositionMode(null, presentationVisible = true),
        )
    }

    private fun workbenchPlan(mode: AndroidResolvedWorkspaceMode): AndroidWorkbenchLayoutPlan =
        AndroidWorkbenchLayoutPlan(
            placements =
                listOf(
                    AndroidWorkbenchPlacement(
                        role = AndroidWorkbenchRole.EDITOR,
                        bounds = AndroidLayoutRect(leftDp = 0f, topDp = 0f, rightDp = 100f, bottomDp = 100f),
                    ),
                ),
            mode = mode,
        )
}
