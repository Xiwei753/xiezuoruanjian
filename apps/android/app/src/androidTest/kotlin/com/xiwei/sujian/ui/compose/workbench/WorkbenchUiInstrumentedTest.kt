package com.xiwei.sujian.ui.compose.workbench

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
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
    fun dockResizeHandle_existsAndIsDisplayed() {
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
        composeTestRule.waitForIdle()

        val resizeTag = SujianSemanticIds.dockResizeHandle(DockZone.Left.name)
        composeTestRule.onNodeWithTag(resizeTag).assertExists()
    }

    @Test
    fun dockSplitHandle_existsWithMultipleGroups() {
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

        val leftGroups = layoutState.dockGroupWeights.entries.filter { layoutState.dockGroupMeta[it.key]?.zone == DockZone.Left }
        if (leftGroups.size >= 2) {
            val beforeGroupId = leftGroups[0].key
            val splitTag = SujianSemanticIds.dockSplitHandle(beforeGroupId)
            composeTestRule.onNodeWithTag(splitTag).assertExists()
        }
    }

    @Test
    fun floatingPanel_existsWhenPanelIsFloating() {
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

        val floatingTag = SujianSemanticIds.floatingPanel(WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(floatingTag).assertExists()

        val resizeHandleTag = SujianSemanticIds.floatingResizeHandle(WorkbenchPanelId.AiAssistant.name)
        composeTestRule.onNodeWithTag(resizeHandleTag).assertExists()
    }
}
