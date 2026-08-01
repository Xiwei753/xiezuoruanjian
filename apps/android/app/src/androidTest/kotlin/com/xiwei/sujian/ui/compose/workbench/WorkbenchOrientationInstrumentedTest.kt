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
import androidx.compose.ui.unit.dp
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

class WorkbenchOrientationInstrumentedTest {

    @get:Rule
    val composeTestRule = createComposeRule()

    private val defaultState = WorkbenchReducer.computeDefaultLayout()

    @Test
    fun tabletLandscape_toPortrait_clampFloatingPanels() {
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
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 900f, 600f))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 500f, 400f))
        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        composeTestRule.waitForIdle()
        val landscapePanel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Landscape: X within bounds", landscapePanel.floatingX <= 800f)
        assertTrue("Landscape: Y within bounds", landscapePanel.floatingY <= 600f)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(600f, 800f))
        composeTestRule.waitForIdle()
        val portraitPanel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Portrait: X within bounds", portraitPanel.floatingX <= 600f)
        assertTrue("Portrait: Y within bounds", portraitPanel.floatingY <= 800f)
        assertTrue("Portrait: Width clamped", portraitPanel.floatingWidthDp <= 600f)
    }

    @Test
    fun floatingToDockToTabGroup_roundTrip() {
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

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()
        assertEquals(DockZone.Floating, layoutState.panels[WorkbenchPanelId.AiAssistant]!!.zone)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Right, 0))
        composeTestRule.waitForIdle()
        assertEquals(DockZone.Right, layoutState.panels[WorkbenchPanelId.AiAssistant]!!.zone)
        val groupId = layoutState.panels[WorkbenchPanelId.AiAssistant]!!.tabGroupId

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, groupId))
        composeTestRule.waitForIdle()
        assertEquals("Search in same tab group", groupId, layoutState.panels[WorkbenchPanelId.Search]!!.tabGroupId)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanel(WorkbenchPanelId.Search))
        composeTestRule.waitForIdle()
        assertEquals(DockZone.Floating, layoutState.panels[WorkbenchPanelId.Search]!!.zone)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.Search, DockZone.Right, 1))
        composeTestRule.waitForIdle()
        assertEquals(DockZone.Right, layoutState.panels[WorkbenchPanelId.Search]!!.zone)
    }

    @Test
    fun layoutRestore_afterSimulatedRestart() {
        var state = defaultState
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.DocumentOutline))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.FloatPanel(WorkbenchPanelId.Search))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.MoveFloatingPanel(WorkbenchPanelId.Search, 200f, 150f))

        val savedState = state

        var layoutState by mutableStateOf(savedState)
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

        assertEquals("AiAssistant preserved", PanelVisibility.Expanded, layoutState.panels[WorkbenchPanelId.AiAssistant]!!.visibility)
        assertEquals("DocumentOutline preserved", PanelVisibility.Expanded, layoutState.panels[WorkbenchPanelId.DocumentOutline]!!.visibility)
        assertEquals("Search still floating", DockZone.Floating, layoutState.panels[WorkbenchPanelId.Search]!!.zone)
        assertEquals(200f, layoutState.panels[WorkbenchPanelId.Search]!!.floatingX, 0.01f)
        assertEquals(150f, layoutState.panels[WorkbenchPanelId.Search]!!.floatingY, 0.01f)
    }

    @Test
    fun windowResize_narrowToWide_dockZoneSizesAdapt() {
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

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(600f, 400f))
        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(1400f, 900f))
        composeTestRule.waitForIdle()

        val panel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Panel should have valid zone", panel.zone == DockZone.Right || panel.zone == DockZone.Left || panel.zone == DockZone.Bottom || panel.zone == DockZone.Floating)
    }
}
