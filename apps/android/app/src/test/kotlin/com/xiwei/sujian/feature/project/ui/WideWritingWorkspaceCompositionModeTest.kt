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
 * presentationVisible=false 时即使 plan=WORKBENCH 也必须 EDITOR_ONLY，
 * 不进入完整 Workbench shell（避免隐藏阶段预热 toolbar / 第二份 ChapterTree /
 * syncStatusRepository collector / WritingToolRail / WritingToolPane）。
 * presentationVisible=true 且 plan=WORKBENCH 才 FULL_WORKBENCH。
 *
 * 直接调用生产入口 [resolveWideWorkspaceCompositionMode]；不为测试重复决策逻辑。
 */
class WideWritingWorkspaceCompositionModeTest {
    @Test
    fun hidden_presentationVisibleFalse_workbenchPlan_returnsEditorOnly() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH)
        assertEquals(
            WideWorkspaceCompositionMode.EDITOR_ONLY,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = false),
        )
    }

    @Test
    fun hidden_presentationVisibleFalse_singlePanePlan_returnsEditorOnly() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.SINGLE_PANE)
        assertEquals(
            WideWorkspaceCompositionMode.EDITOR_ONLY,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = false),
        )
    }

    @Test
    fun hidden_presentationVisibleFalse_nullPlan_returnsEditorOnly() {
        assertEquals(
            WideWorkspaceCompositionMode.EDITOR_ONLY,
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
    fun visible_singlePanePlan_returnsEditorOnly() {
        val plan = workbenchPlan(AndroidResolvedWorkspaceMode.SINGLE_PANE)
        assertEquals(
            WideWorkspaceCompositionMode.EDITOR_ONLY,
            resolveWideWorkspaceCompositionMode(plan, presentationVisible = true),
        )
    }

    @Test
    fun visible_nullPlan_returnsEditorOnly() {
        assertEquals(
            WideWorkspaceCompositionMode.EDITOR_ONLY,
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
