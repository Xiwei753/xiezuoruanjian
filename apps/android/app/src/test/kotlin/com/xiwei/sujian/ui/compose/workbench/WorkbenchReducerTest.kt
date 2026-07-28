package com.xiwei.sujian.ui.compose.workbench

import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class WorkbenchReducerTest {

    private val defaultState = WorkbenchReducer.computeDefaultLayout()

    @Test
    fun defaultLayout_allPanelsPresent() {
        for (id in WorkbenchPanelId.entries) {
            assertTrue("Default layout should contain $id", defaultState.panels.containsKey(id))
        }
    }

    @Test
    fun defaultLayout_allPanelsCollapsed() {
        for (panel in defaultState.panels.values) {
            assertEquals(
                "Default panel ${panel.id} should be collapsed",
                PanelVisibility.Collapsed,
                panel.visibility
            )
        }
    }

    @Test
    fun defaultLayout_presetIsFocusWriting() {
        assertEquals(WorkbenchPreset.FocusWriting, defaultState.preset)
    }

    @Test
    fun togglePanel_hiddenToExpanded() {
        val state = defaultState
        val panelId = WorkbenchPanelId.ChapterNavigator
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.TogglePanel(panelId))
        assertEquals(PanelVisibility.Expanded, result.panels[panelId]?.visibility)
    }

    @Test
    fun togglePanel_expandedToCollapsed() {
        val state = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.TogglePanel(WorkbenchPanelId.ChapterNavigator))
        assertEquals(PanelVisibility.Collapsed, result.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
    }

    @Test
    fun togglePanel_collapsedToExpanded() {
        val state = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val collapsed = WorkbenchReducer.reduce(state, WorkbenchAction.CollapsePanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(collapsed, WorkbenchAction.TogglePanel(WorkbenchPanelId.ChapterNavigator))
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
    }

    @Test
    fun expandPanel_setsExpanded() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.AiAssistant]?.visibility)
    }

    @Test
    fun collapsePanel_setsCollapsed() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.CollapsePanel(WorkbenchPanelId.AiAssistant))
        assertEquals(PanelVisibility.Collapsed, result.panels[WorkbenchPanelId.AiAssistant]?.visibility)
    }

    @Test
    fun hidePanel_setsHidden() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.HidePanel(WorkbenchPanelId.Search))
        assertEquals(PanelVisibility.Hidden, result.panels[WorkbenchPanelId.Search]?.visibility)
    }

    @Test
    fun movePanel_changesZone() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.AiAssistant, DockZone.Bottom))
        assertEquals(DockZone.Bottom, result.panels[WorkbenchPanelId.AiAssistant]?.zone)
    }

    @Test
    fun resizePanel_clampsSidePanel() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 600f))
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! <= 520f)
    }

    @Test
    fun resizePanel_clampsSidePanelMin() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 100f))
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! >= 280f)
    }

    @Test
    fun floatPanel_setsZoneAndExpanded() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.Statistics))
        assertEquals(DockZone.Floating, result.panels[WorkbenchPanelId.Statistics]?.zone)
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.Statistics]?.visibility)
    }

    @Test
    fun dockPanel_setsZoneAndExpanded() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanel(WorkbenchPanelId.StarMap, DockZone.Right))
        assertEquals(DockZone.Right, result.panels[WorkbenchPanelId.StarMap]?.zone)
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.StarMap]?.visibility)
    }

    @Test
    fun moveFloatingPanel_updatesPosition() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.MoveFloatingPanel(WorkbenchPanelId.AiAssistant, 100f, 200f))
        assertEquals(100f, result.panels[WorkbenchPanelId.AiAssistant]?.floatingX!!, 0.01f)
        assertEquals(200f, result.panels[WorkbenchPanelId.AiAssistant]?.floatingY!!, 0.01f)
    }

    @Test
    fun applyPreset_focusWriting_collapsesAll() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ApplyPreset(WorkbenchPreset.FocusWriting))
        for (panel in result.panels.values) {
            assertEquals(PanelVisibility.Collapsed, panel.visibility)
        }
        assertEquals(WorkbenchPreset.FocusWriting, result.preset)
    }

    @Test
    fun applyPreset_chapterWriting_expandsChapterNavigator() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ChapterWriting))
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
        assertEquals(DockZone.Left, result.panels[WorkbenchPanelId.ChapterNavigator]?.zone)
        assertEquals(WorkbenchPreset.ChapterWriting, result.preset)
    }

    @Test
    fun applyPreset_aiWriting_expandsAiAssistant() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.AiWriting))
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.AiAssistant]?.visibility)
        assertEquals(DockZone.Right, result.panels[WorkbenchPanelId.AiAssistant]?.zone)
        assertEquals(WorkbenchPreset.AiWriting, result.preset)
    }

    @Test
    fun applyPreset_researchWriting_expandsChapterAndSearch() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.Search]?.visibility)
        assertEquals(DockZone.Left, result.panels[WorkbenchPanelId.ChapterNavigator]?.zone)
        assertEquals(DockZone.Right, result.panels[WorkbenchPanelId.Search]?.zone)
        assertEquals(WorkbenchPreset.ResearchWriting, result.preset)
    }

    @Test
    fun actionMarksPresetCustom() {
        val focusWriting = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.FocusWriting))
        val result = WorkbenchReducer.reduce(focusWriting, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        assertEquals(WorkbenchPreset.Custom, result.preset)
    }

    @Test
    fun activateTab_updatesActiveTabByGroup() {
        val researchWriting = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        val searchTabGroup = researchWriting.panels[WorkbenchPanelId.Search]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(researchWriting, WorkbenchAction.ActivateTab(searchTabGroup, WorkbenchPanelId.Statistics))
        assertEquals(WorkbenchPanelId.Statistics, result.activeTabByGroup[searchTabGroup])
    }

    @Test
    fun restoreLayout_returnsStateUnchanged() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.RestoreLayout)
        assertEquals(defaultState, result)
    }
}
