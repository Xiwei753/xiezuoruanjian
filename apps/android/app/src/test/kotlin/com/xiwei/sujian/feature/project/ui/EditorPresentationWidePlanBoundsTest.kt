package com.xiwei.sujian.feature.project.ui

import com.xiwei.sujian.app.presentation.layout.AndroidLayoutRect
import com.xiwei.sujian.app.presentation.layout.AndroidResolvedWorkspaceMode
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchLayoutPlan
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchPlacement
import com.xiwei.sujian.app.presentation.layout.AndroidWorkbenchRole
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * #640 A：宽屏 host plan bounds 决策回归测试。
 *
 * 宽屏 EditorPresentationHost 直接使用最终 Rust AndroidWorkbenchLayoutPlan 的 Editor bounds，
 * 根 host 不经过 outer top bar/NavigationRail。bounds 解析由纯函数 [resolveWideEditorBounds]
 * 统一表达，生产 host 与单测共用。
 *
 * - plan 非 null 且 Editor bounds 非空 → 返回该 bounds（不 fillMaxSize、不经过 outer chrome）；
 * - plan null → 返回 null（调用方回落 fillMaxSize，无遮挡信息）；
 * - Editor bounds 空 → 返回 null（不放置零尺寸 editor）；
 * - 返回的 bounds 必须来自 plan.placementFor(EDITOR)，不来自其它角色。
 */
class EditorPresentationWidePlanBoundsTest {
    private val editorBounds = AndroidLayoutRect(leftDp = 10f, topDp = 20f, rightDp = 110f, bottomDp = 220f)

    @Test
    fun workbenchPlan_returnsEditorBounds() {
        val plan = planWithEditorBounds(editorBounds, AndroidResolvedWorkspaceMode.WORKBENCH)
        val resolved = resolveWideEditorBounds(plan)
        assertNotNull(resolved)
        assertEquals(editorBounds, resolved)
    }

    @Test
    fun singlePanePlan_returnsEditorBounds() {
        val plan = planWithEditorBounds(editorBounds, AndroidResolvedWorkspaceMode.SINGLE_PANE)
        val resolved = resolveWideEditorBounds(plan)
        assertNotNull(resolved)
        assertEquals(editorBounds, resolved)
    }

    @Test
    fun nullPlan_returnsNull() {
        assertEquals(null, resolveWideEditorBounds(null))
    }

    @Test
    fun emptyEditorBounds_returnsNull() {
        val empty = AndroidLayoutRect(leftDp = 0f, topDp = 0f, rightDp = 0f, bottomDp = 0f)
        val plan = planWithEditorBounds(empty, AndroidResolvedWorkspaceMode.WORKBENCH)
        assertEquals("零尺寸 bounds 不得放置 editor", null, resolveWideEditorBounds(plan))
    }

    @Test
    fun resolvedBounds_doesNotComeFromOtherRoles() {
        val toolbarBounds = AndroidLayoutRect(leftDp = 0f, topDp = 0f, rightDp = 200f, bottomDp = 10f)
        val plan =
            AndroidWorkbenchLayoutPlan(
                placements =
                    listOf(
                        AndroidWorkbenchPlacement(
                            role = AndroidWorkbenchRole.TOOLBAR_LEADING,
                            bounds = toolbarBounds,
                        ),
                        AndroidWorkbenchPlacement(
                            role = AndroidWorkbenchRole.EDITOR,
                            bounds = editorBounds,
                        ),
                    ),
                mode = AndroidResolvedWorkspaceMode.WORKBENCH,
            )
        val resolved = resolveWideEditorBounds(plan)
        assertNotNull(resolved)
        assertTrue("bounds 必须来自 EDITOR 角色", resolved == editorBounds)
        assertFalse("bounds 不得来自 TOOLBAR", resolved == toolbarBounds)
    }

    private fun planWithEditorBounds(
        bounds: AndroidLayoutRect,
        mode: AndroidResolvedWorkspaceMode,
    ): AndroidWorkbenchLayoutPlan =
        AndroidWorkbenchLayoutPlan(
            placements =
                listOf(
                    AndroidWorkbenchPlacement(
                        role = AndroidWorkbenchRole.EDITOR,
                        bounds = bounds,
                    ),
                ),
            mode = mode,
        )
}
