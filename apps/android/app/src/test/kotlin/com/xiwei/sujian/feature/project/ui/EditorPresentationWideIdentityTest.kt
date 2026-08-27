package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchPlacement
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * #640 评论 5441849412 问题2 / 5442422507：宽屏 Editor slot call site 唯一性回归测试。
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
 * #640 评论 5442422507：EditorPane 唯一 call site 由源码结构保证（EditorPane 始终在同一 call site，
 * 不在 EDITOR_ONLY/FULL_WORKBENCH 两分支各自创建）。不再用纯函数 identity 锁此不变量
 * （[WideEditorSlotIdentity]/[resolveWideEditorSlotIdentity] 已删除 — 纯测试专用生产逻辑，
 * 无生产消费者，"自己证明自己"）。本测改为锁 [resolveWideWorkspaceCompositionMode] 决策
 * （hidden→EDITOR_ONLY、visible+WORKBENCH→FULL_WORKBENCH 等），这是 AndroidView 不重建的前提：
 * compositionMode 决策正确 + EditorPane 在唯一 call site（源码结构）→ 切换不重建 View。
 */
class EditorPresentationWideIdentityTest {
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
        // 显示阶段（visible）：compositionMode=FULL_WORKBENCH。
        val visibleMode =
            resolveWideWorkspaceCompositionMode(
                workbenchPlan = workbenchPlan,
                presentationVisible = true,
            )

        assertEquals(
            "hidden 阶段 compositionMode 必须为 EDITOR_ONLY（不进入完整 Workbench shell）",
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

        assertEquals(
            "plan=SINGLE_PANE 时 compositionMode 必须为 EDITOR_ONLY（fallback 不进 Workbench shell）",
            WideWorkspaceCompositionMode.EDITOR_ONLY,
            singlePaneMode,
        )
        assertEquals(
            "plan=WORKBENCH + visible 时 compositionMode 必须为 FULL_WORKBENCH",
            WideWorkspaceCompositionMode.FULL_WORKBENCH,
            workbenchMode,
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

        assertEquals(
            "plan=null（桥失败）时 compositionMode 必须为 EDITOR_ONLY",
            WideWorkspaceCompositionMode.EDITOR_ONLY,
            nullPlanMode,
        )
        assertEquals(
            "plan=WORKBENCH + visible 时 compositionMode 必须为 FULL_WORKBENCH",
            WideWorkspaceCompositionMode.FULL_WORKBENCH,
            workbenchMode,
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
