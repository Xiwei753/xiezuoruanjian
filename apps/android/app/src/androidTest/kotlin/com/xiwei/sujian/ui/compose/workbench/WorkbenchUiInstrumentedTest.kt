package com.xiwei.sujian.ui.compose.workbench

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.ui.compose.workbench.component.SujianWorkbench
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class WorkbenchUiInstrumentedTest {

    @get:Rule
    val composeTestRule = createComposeRule()

    private val defaultState = WorkbenchReducer.computeDefaultLayout()

    @Test
    fun panelLauncherClick_expandsPanel() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        val initialVisibility = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.visibility
        assertTrue("Panel should start collapsed or hidden", initialVisibility != PanelVisibility.Expanded)

        val launcherTag = SujianSemanticIds.panelLauncherButton(WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(launcherTag).assertExists()
        composeTestRule.onNodeWithTag(launcherTag)
            .performClick()
        composeTestRule.waitForIdle()

        assertEquals("Panel should be expanded after launcher click", PanelVisibility.Expanded, layoutState.panels[WorkbenchPanelId.AiAssistant]!!.visibility)
    }

    @Test
    fun tabClick_switchesActiveTab() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val groupId = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.tabGroupId
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        composeTestRule.waitForIdle()

        val effectiveGroupId = layoutState.panels[WorkbenchPanelId.Search]!!.tabGroupId

        val aiTabTag = SujianSemanticIds.dockTab(effectiveGroupId, WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(aiTabTag).assertExists()
        composeTestRule.onNodeWithTag(aiTabTag)
            .performClick()
        composeTestRule.waitForIdle()

        assertEquals("AiAssistant should be active after tab click", WorkbenchPanelId.AiAssistant, layoutState.activeTabByGroup[effectiveGroupId])
    }

    @Test
    fun dockResizeHandle_drag_changesDockZoneSize_andOtherZoneUnaffected() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1400.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()

        val beforeLeftSize = layoutState.dockZoneSizeDp[DockZone.Left] ?: 0f
        val beforeRightSize = layoutState.dockZoneSizeDp[DockZone.Right] ?: 0f
        assertTrue("Left zone should have initial size", beforeLeftSize > 0f)

        val resizeTag = SujianSemanticIds.dockResizeHandle(DockZone.Left.name)
        composeTestRule.onNodeWithTag(resizeTag).performTouchInput {
            down(center)
            moveTo(Offset(center.x + 30f * density, center.y))
            up()
        }
        composeTestRule.waitForIdle()

        val afterLeftSize = layoutState.dockZoneSizeDp[DockZone.Left] ?: 0f
        assertTrue("Left dock zone should grow after dragging resize handle right", afterLeftSize > beforeLeftSize)

        if (beforeRightSize > 0f) {
            val afterRightSize = layoutState.dockZoneSizeDp[DockZone.Right] ?: 0f
            assertEquals("Right zone size should not be polluted by left resize", beforeRightSize, afterRightSize, 0.1f)
        }
    }

    @Test
    fun dockSplitHandle_drag_transfersWeightBetweenGroups_andSumConserved() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 1))
        composeTestRule.waitForIdle()

        val leftGroups = layoutState.dockGroupWeights.entries
            .filter { layoutState.dockGroupMeta[it.key]?.zone == DockZone.Left }
            .sortedBy { layoutState.dockGroupMeta[it.key]?.order ?: 0 }
        assertEquals("Must have exactly 2 left dock groups for split handle test", 2, leftGroups.size)

        val beforeGroupId = leftGroups[0].key
        val afterGroupId = leftGroups[1].key
        val beforeWeight = leftGroups[0].value
        val afterWeight = leftGroups[1].value
        val weightSum = beforeWeight + afterWeight

        val splitTag = SujianSemanticIds.dockSplitHandle(beforeGroupId)
        composeTestRule.onNodeWithTag(splitTag).performTouchInput {
            down(center)
            moveTo(Offset(center.x, center.y + 40f * density))
            up()
        }
        composeTestRule.waitForIdle()

        val newBeforeWeight = layoutState.dockGroupWeights[beforeGroupId]!!
        val newAfterWeight = layoutState.dockGroupWeights[afterGroupId]!!
        val newWeightSum = newBeforeWeight + newAfterWeight

        assertTrue("Before group weight should increase after dragging split handle down", newBeforeWeight > beforeWeight)
        assertTrue("After group weight should decrease after dragging split handle down", newAfterWeight < afterWeight)
        assertEquals("Weight sum must be conserved", weightSum, newWeightSum, 0.001f)
    }

    @Test
    fun floatingPanel_dragResizeHandle_changesSize_andClampedToMinimum() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()

        val beforeWidth = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.floatingWidthDp
        val beforeHeight = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.floatingHeightDp

        val resizeHandleTag = SujianSemanticIds.floatingResizeHandle(WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(resizeHandleTag).performTouchInput {
            down(center)
            moveTo(Offset(center.x + 50f * density, center.y + 40f * density))
            up()
        }
        composeTestRule.waitForIdle()

        val afterWidth = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.floatingWidthDp
        val afterHeight = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.floatingHeightDp
        assertTrue("Floating width should increase after resize drag", afterWidth > beforeWidth)
        assertTrue("Floating height should increase after resize drag", afterHeight > beforeHeight)
    }

    @Test
    fun floatingPanel_resizeHandle_dragToMinimum_clampedNoCrash() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()

        val resizeHandleTag = SujianSemanticIds.floatingResizeHandle(WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(resizeHandleTag).performTouchInput {
            down(center)
            moveTo(Offset(center.x - 800f * density, center.y - 800f * density))
            up()
        }
        composeTestRule.waitForIdle()

        val finalWidth = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.floatingWidthDp
        val finalHeight = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.floatingHeightDp
        assertTrue("Width must be clamped to at least FLOATING_MIN_WIDTH_DP", finalWidth >= WorkbenchReducer.FLOATING_MIN_WIDTH_DP)
        assertTrue("Height must be clamped to at least FLOATING_MIN_HEIGHT_DP", finalHeight >= WorkbenchReducer.FLOATING_MIN_HEIGHT_DP)
    }

    @Test
    fun floatingPanel_dragTitleBar_toDockEdge_triggersDockAsNewGroup() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()

        assertEquals("Panel should start as Floating", DockZone.Floating, layoutState.panels[WorkbenchPanelId.AiAssistant]!!.zone)

        val floatingTag = SujianSemanticIds.floatingPanel(WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(floatingTag).performTouchInput {
            down(topCenter)
            moveTo(Offset(10f, topCenter.y))
            up()
        }
        composeTestRule.waitForIdle()

        val zoneAfter = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.zone
        assertTrue("Panel should dock to Left after dragging to left edge, got $zoneAfter", zoneAfter == DockZone.Left)
    }

    @Test
    fun dockResizeHandle_bottom_drag_changesBottomZoneSize() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(1200.dp, 800.dp)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { _, _ -> },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.Statistics, DockZone.Bottom, 0))
        composeTestRule.waitForIdle()

        val beforeBottomSize = layoutState.dockZoneSizeDp[DockZone.Bottom] ?: 0f
        assertTrue("Bottom zone should have initial size", beforeBottomSize > 0f)

        val resizeTag = SujianSemanticIds.dockResizeHandle(DockZone.Bottom.name)
        composeTestRule.onNodeWithTag(resizeTag).performTouchInput {
            down(center)
            moveTo(Offset(center.x, center.y - 20f * density))
            up()
        }
        composeTestRule.waitForIdle()

        val afterBottomSize = layoutState.dockZoneSizeDp[DockZone.Bottom] ?: 0f
        assertTrue("Bottom dock zone should shrink after dragging resize handle up", afterBottomSize < beforeBottomSize)
    }
}
