package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchPlacement
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 评论 5441849412 问题2：宽屏 Editor slot call site 唯一性回归测试。
 *
 * 根因：之前宽屏隐藏预热走 `WideWritingWorkspace → EDITOR_ONLY → EditorPane → AndroidView`；
 * 导航显示 Workbench 后变成 `WideWritingWorkspace → FULL_WORKBENCH → Layout → WorkbenchSlots →
 * WorkbenchContentSlots → EditorPane → AndroidView`。这不是同一个 Compose call site。
 * Compose composable 实例由 call site 识别；AndroidView 默认不会自动复用，包含它的 composition
 * hierarchy 改变时 View 会被丢弃并重新 factory。所以平板/大屏仍是"预热一个 View，显示时再造一个 View"。
 *
 * 重构后 EditorPane 始终在 [WideWritingWorkspace] 的 Layout content 唯一 call site（layoutId=EDITOR），
 * 切换 compositionMode 只增删 Workbench chrome slot，不移动 Editor 本身。
 *
 * 本测锁住不变量：
 * - EDITOR_ONLY 和 FULL_WORKBENCH 的 [WideEditorSlotIdentity.layoutIdKey] 相同（call site 不变）；
 * - [WideEditorSlotIdentity.isUniqueCallSite] 恒 true（EditorPane 不在 if 分支里各自创建）；
 * - presentationVisible=false → true、plan=WORKBENCH 前后 Editor slot identity 不变 → AndroidView 不重建。
 */
class EditorPresentationWideIdentityTest {
    @Test
    fun editorOnly_slotIdentity_isEditorAndUnique() {
        val identity =
            resolveWideEditorSlotIdentity(WideWorkspaceCompositionMode.EDITOR_ONLY)
        assertEquals("Editor slot layoutIdKey 必须恒为 EDITOR", "EDITOR", identity.layoutIdKey)
        assertTrue("Editor slot 必须在唯一 call site", identity.isUniqueCallSite)
    }

    @Test
    fun fullWorkbench_slotIdentity_isEditorAndUnique() {
        val identity =
            resolveWideEditorSlotIdentity(WideWorkspaceCompositionMode.FULL_WORKBENCH)
        assertEquals("Editor slot layoutIdKey 必须恒为 EDITOR", "EDITOR", identity.layoutIdKey)
        assertTrue("Editor slot 必须在唯一 call site", identity.isUniqueCallSite)
    }

    @Test
    fun editorOnly_andFullWorkbench_slotIdentity_identical() {
        val editorOnlyIdentity =
            resolveWideEditorSlotIdentity(WideWorkspaceCompositionMode.EDITOR_ONLY)
        val fullWorkbenchIdentity =
            resolveWideEditorSlotIdentity(WideWorkspaceCompositionMode.FULL_WORKBENCH)
        assertEquals(
            "EDITOR_ONLY 和 FULL_WORKBENCH 的 Editor slot identity 必须完全相同（call site 不变）",
            editorOnlyIdentity,
            fullWorkbenchIdentity,
        )
    }

    @Test
    fun hiddenToVisible_presenterVisibleFalseToTrue_slotIdentityUnchanged() {
        // 模拟 presentationVisible=false → true 切换（plan=WORKBENCH）。
        // 预热阶段（hidden）：compositionMode=EDITOR_ONLY。
        val workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH)
        val hiddenMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = workbenchPlan,
                presentationVisible = false,
            )
        val hiddenIdentity = resolveWideEditorSlotIdentity(hiddenMode)

        // 显示阶段（visible）：compositionMode=FULL_WORKBENCH。
        val visibleMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = workbenchPlan,
                presentationVisible = true,
            )
        val visibleIdentity = resolveWideEditorSlotIdentity(visibleMode)

        assertEquals(
            "presentationVisible=false→true 切换后 Editor slot identity 必须不变（AndroidView 不重建）",
            hiddenIdentity,
            visibleIdentity,
        )
        assertEquals(
            "hidden 阶段 compositionMode 必须为 EDITOR_ONLY",
            WideWorkspaceCompositionMode.EDITOR_ONLY,
            hiddenMode,
        )
        assertEquals(
            "visible 阶段 compositionMode 必须为 FULL_WORKBENCH",
            WideWorkspaceCompositionMode.FULL_WORKBENCH,
            visibleMode,
        )
    }

    @Test
    fun planSwitch_singlePaneToWorkbench_slotIdentityUnchanged() {
        // 模拟 plan 从 SINGLE_PANE 切到 WORKBENCH（presentationVisible=true）。
        val singlePanePlan = workbenchPlan(AndroidResolvedWorkspaceMode.SINGLE_PANE)
        val workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH)

        val singlePaneMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = singlePanePlan,
                presentationVisible = true,
            )
        val workbenchMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = workbenchPlan,
                presentationVisible = true,
            )

        val singlePaneIdentity = resolveWideEditorSlotIdentity(singlePaneMode)
        val workbenchIdentity = resolveWideEditorSlotIdentity(workbenchMode)

        assertEquals(
            "plan SINGLE_PANE→WORKBENCH 切换后 Editor slot identity 必须不变（AndroidView 不重建）",
            singlePaneIdentity,
            workbenchIdentity,
        )
    }

    @Test
    fun nullPlanToWorkbench_slotIdentityUnchanged() {
        // 模拟桥失败（plan=null）切到 WORKBENCH（presentationVisible=true）。
        val nullPlanMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = null,
                presentationVisible = true,
            )
        val workbenchMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = workbenchPlan(AndroidResolvedWorkspaceMode.WORKBENCH),
                presentationVisible = true,
            )

        val nullPlanIdentity = resolveWideEditorSlotIdentity(nullPlanMode)
        val workbenchIdentity = resolveWideEditorSlotIdentity(workbenchMode)

        assertEquals(
            "plan null→WORKBENCH 切换后 Editor slot identity 必须不变（AndroidView 不重建）",
            nullPlanIdentity,
            workbenchIdentity,
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
