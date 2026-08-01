package com.xiwei.sujian.ui.compose.workbench

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.ui.compose.workbench.component.SujianWorkbench
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class WorkbenchUiInstrumentedTest {

    @get:Rule
    val composeTestRule = createComposeRule()

    private val defaultState = WorkbenchReducer.computeDefaultLayout()

    @Test
    fun floatingPanel_roundTrip_toDockAndBack() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { _, _ -> },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()
        val expanded = layoutState
        assertTrue("Panel should be expanded", expanded.panels[WorkbenchPanelId.AiAssistant]!!.visibility == PanelVisibility.Expanded)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()
        val floated = layoutState
        assertEquals("Panel should be floating", DockZone.Floating, floated.panels[WorkbenchPanelId.AiAssistant]!!.zone)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Right, 0))
        composeTestRule.waitForIdle()
        val docked = layoutState
        assertEquals("Panel should be docked right", DockZone.Right, docked.panels[WorkbenchPanelId.AiAssistant]!!.zone)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()
        val reFloated = layoutState
        assertEquals("Panel should be floating again", DockZone.Floating, reFloated.panels[WorkbenchPanelId.AiAssistant]!!.zone)
    }

    @Test
    fun clampFloatingPanels_afterRotation_composeRerender() {
        var layoutState by mutableStateOf(defaultState)
        var windowSize by mutableStateOf(DpSize(1200.dp, 800.dp))

        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { _, _ -> },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 500f, 400f))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 600f, 500f))
        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        composeTestRule.waitForIdle()
        val panel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Width should be clamped", panel.floatingWidthDp <= 800f)
        assertTrue("Height should be clamped", panel.floatingHeightDp <= 600f)
        assertTrue("X should be within bounds", panel.floatingX >= 0f)
        assertTrue("Y should be within bounds", panel.floatingY >= 0f)

        windowSize = DpSize(800.dp, 1200.dp)
        composeTestRule.waitForIdle()
    }

    @Test
    fun layoutRestore_afterRestart_stateMatchesSnapshot() {
        var state = defaultState
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.DocumentOutline))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.FloatPanel(WorkbenchPanelId.Search))

        val snapshot = state
        val restoredAi = snapshot.panels[WorkbenchPanelId.AiAssistant]!!
        val restoredOutline = snapshot.panels[WorkbenchPanelId.DocumentOutline]!!
        val restoredSearch = snapshot.panels[WorkbenchPanelId.Search]!!

        assertEquals(PanelVisibility.Expanded, restoredAi.visibility)
        assertEquals(PanelVisibility.Expanded, restoredOutline.visibility)
        assertEquals(DockZone.Floating, restoredSearch.zone)

        var layoutState by mutableStateOf(snapshot)
        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { _, _ -> },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }
        composeTestRule.waitForIdle()
        assertEquals("Restored state should match snapshot", snapshot, layoutState)
    }

    @Test
    fun tabGroup_twoPanelsDockedTogether_activateTabSwitches() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { _, _ -> },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val groupId = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.tabGroupId
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, groupId))
        composeTestRule.waitForIdle()

        val afterMerge = layoutState
        assertEquals("Search should be in same group", groupId, afterMerge.panels[WorkbenchPanelId.Search]!!.tabGroupId)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.Search))
        composeTestRule.waitForIdle()
        assertEquals("Search should be active tab", WorkbenchPanelId.Search, layoutState.activeTabByGroup[groupId])
    }

    @Test
    fun dockZoneResize_extremeSmallScreen_noCrash() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { _, _ -> },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Bottom, 0))
        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ResizeDockZone(DockZone.Bottom, 10f, 300f))
        composeTestRule.waitForIdle()
        val bottomSize = layoutState.dockZoneSizeDp[DockZone.Bottom]!!
        assertTrue("Bottom size should be non-negative", bottomSize >= 0f)
    }
}
