package com.xiwei.sujian.ui.compose.workbench

import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertFalse
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
    fun floatPanel_assignsZIndex() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.Statistics))
        assertTrue(result.panels[WorkbenchPanelId.Statistics]?.floatingZIndex!! >= 0)
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
    fun restoreLayout_returnsDefaultState() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.RestoreLayout)
        assertEquals(defaultState, result)
    }

    @Test
    fun resizePanel_enforcesEditorMinWidth_singleSide() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val availableWidth = 900f
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 500f, availableWidth))
        val maxSizeForEditor = availableWidth - 480f
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! <= maxSizeForEditor)
    }

    @Test
    fun resizePanel_enforcesEditorMinWidth_bothSides_actualSideWidth() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val availableWidth = 1200f
        val result = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 600f, availableWidth))
        val otherSideWidth = rightExpanded.actualSideWidthDp(DockZone.Right)
        val maxSizeForEditor = availableWidth - 480f - otherSideWidth
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! <= maxSizeForEditor)
    }

    @Test
    fun resizePanel_availableWidthDefault_noEditorConstraint() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 520f))
        assertEquals(520f, result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!!, 0.01f)
    }

    @Test
    fun resizePanel_bottomPanel_notAffectedByEditorMinWidth() {
        val moved = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val expanded = WorkbenchReducer.reduce(moved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.Statistics, 300f, 600f))
        assertEquals(300f, result.panels[WorkbenchPanelId.Statistics]?.sizeDp!!, 0.01f)
    }

    @Test
    fun movePanelToGroup_updatesTabGroupId() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "custom-group"))
        assertEquals("custom-group", result.panels[WorkbenchPanelId.Search]?.tabGroupId)
    }

    @Test
    fun movePanelToGroup_updatesActiveTab() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "custom-group"))
        assertEquals(WorkbenchPanelId.Search, result.activeTabByGroup["custom-group"])
    }

    @Test
    fun movePanelToGroup_inheritsTargetGroupZone() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "right-tools"))
        assertEquals(DockZone.Right, result.panels[WorkbenchPanelId.Search]?.zone)
    }

    @Test
    fun reorderPanel_updatesOrder() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ReorderPanel(WorkbenchPanelId.Search, 5))
        assertEquals(5, result.panels[WorkbenchPanelId.Search]?.order)
    }

    @Test
    fun bringFloatingToFront_updatesZIndex() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.BringFloatingToFront(WorkbenchPanelId.AiAssistant))
        assertTrue(result.panels[WorkbenchPanelId.AiAssistant]?.floatingZIndex!! > floated.panels[WorkbenchPanelId.AiAssistant]?.floatingZIndex!!)
    }

    @Test
    fun bringFloatingToFront_nonFloatingPanel_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.BringFloatingToFront(WorkbenchPanelId.AiAssistant))
        assertEquals(defaultState, result)
    }

    @Test
    fun resizeFloatingPanel_updatesDimensions() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 500f, 700f))
        assertEquals(500f, result.panels[WorkbenchPanelId.AiAssistant]?.floatingWidthDp!!, 0.01f)
        assertEquals(700f, result.panels[WorkbenchPanelId.AiAssistant]?.floatingHeightDp!!, 0.01f)
    }

    @Test
    fun resizeFloatingPanel_clampsMinSize() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 50f, 50f))
        assertTrue(result.panels[WorkbenchPanelId.AiAssistant]?.floatingWidthDp!! >= 200f)
        assertTrue(result.panels[WorkbenchPanelId.AiAssistant]?.floatingHeightDp!! >= 150f)
    }

    @Test
    fun resizeFloatingPanel_nonFloatingPanel_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 500f, 700f))
        assertEquals(defaultState, result)
    }

    @Test
    fun computePresentationState_overlayMode_compact() {
        val state = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val presentation = WorkbenchReducer.computePresentationState(state, 600f, 800f)
        assertTrue(presentation.isOverlayMode)
    }

    @Test
    fun computePresentationState_normalMode_expanded() {
        val state = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val presentation = WorkbenchReducer.computePresentationState(state, 1000f, 800f)
        assertTrue(!presentation.isOverlayMode)
    }

    @Test
    fun resizePanel_bottomPanel_clampsByMaxRatio() {
        val moved = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val expanded = WorkbenchReducer.reduce(moved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.Statistics, 600f, 800f))
        val maxBottom = 800f * 0.55f
        assertTrue(result.panels[WorkbenchPanelId.Statistics]?.sizeDp!! <= maxBottom)
    }

    @Test
    fun dockGroupsByZone_groupsByTabGroupId() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        val rightGroups = research.dockGroupsByZone(DockZone.Right)
        assertTrue(rightGroups.isNotEmpty())
        val searchGroup = rightGroups.find { it.panelIds.contains(WorkbenchPanelId.Search) }
        assertTrue(searchGroup != null)
    }

    @Test
    fun floatPanelAt_setsPositionAndFloating() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 150f, 250f))
        assertEquals(DockZone.Floating, result.panels[WorkbenchPanelId.AiAssistant]?.zone)
        assertEquals(150f, result.panels[WorkbenchPanelId.AiAssistant]?.floatingX!!, 0.01f)
        assertEquals(250f, result.panels[WorkbenchPanelId.AiAssistant]?.floatingY!!, 0.01f)
        assertEquals(PanelVisibility.Expanded, result.panels[WorkbenchPanelId.AiAssistant]?.visibility)
    }

    @Test
    fun activateOverlayPanel_updatesActiveOverlay() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.AiAssistant))
        assertEquals(WorkbenchPanelId.AiAssistant, result.activeOverlayPanelId)
    }

    @Test
    fun activateOverlayPanel_ignoresNonExpandedPanel() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.AiAssistant))
        assertEquals(defaultState.activeOverlayPanelId, result.activeOverlayPanelId)
    }

    @Test
    fun createDockGroup_savesGroupSize() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.CreateDockGroup("new-group", DockZone.Left, 0))
        assertTrue(result.dockGroupSizes.containsKey("new-group"))
        assertTrue((result.dockGroupSizes["new-group"] ?: 0f) >= 280f)
    }

    @Test
    fun createDockGroup_duplicateId_noChange() {
        val first = WorkbenchReducer.reduce(defaultState, WorkbenchAction.CreateDockGroup("g1", DockZone.Left, 0))
        val second = WorkbenchReducer.reduce(first, WorkbenchAction.CreateDockGroup("g1", DockZone.Left, 1))
        assertEquals(first.dockGroupSizes, second.dockGroupSizes)
    }

    @Test
    fun resizeDockGroup_accumulatesDelta() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val initialSize = state.dockGroupSizes["left-nav"] ?: 320f
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockGroup("left-nav", DockZone.Left, 30f))
        assertEquals(initialSize + 30f, result.dockGroupSizes["left-nav"]!!, 0.01f)
    }

    @Test
    fun resizeDockGroup_clampsMinSize() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockGroup("left-nav", DockZone.Left, -1000f))
        assertTrue((result.dockGroupSizes["left-nav"] ?: 0f) >= 280f)
    }

    @Test
    fun clampFloatingPanels_clampsPositionAndSize() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, -100f, -50f))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingX >= 0f)
        assertTrue(panel.floatingY >= 0f)
        assertTrue(panel.floatingWidthDp <= 800f)
        assertTrue(panel.floatingHeightDp <= 600f)
    }

    @Test
    fun clampFloatingPanels_clampsOversizedPanel() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val resized = WorkbenchReducer.reduce(floated, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 1000f, 900f))
        val result = WorkbenchReducer.reduce(resized, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingWidthDp <= 800f)
        assertTrue(panel.floatingHeightDp <= 600f)
    }

    @Test
    fun actualSideWidthDp_returnsMaxGroupSizeRatio() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        val rightWidth = research.actualSideWidthDp(DockZone.Right)
        val expectedRatio = research.dockGroupSizes["research-right"] ?: 380f
        assertEquals(expectedRatio, rightWidth, 0.01f)
    }

    @Test
    fun actualSideWidthDp_noExpandedPanels_returnsZero() {
        assertEquals(0f, defaultState.actualSideWidthDp(DockZone.Left), 0.01f)
        assertEquals(0f, defaultState.actualSideWidthDp(DockZone.Right), 0.01f)
    }

    @Test
    fun computePresentationState_respectsActiveOverlayPanelId() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.AiAssistant))
        val presentation = WorkbenchReducer.computePresentationState(withActive, 600f, 800f)
        assertEquals(WorkbenchPanelId.AiAssistant, presentation.activeOverlayPanelId)
    }

    @Test
    fun hidePanel_clearsActiveOverlayIfSame() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val withActive = WorkbenchReducer.reduce(expanded, WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.HidePanel(WorkbenchPanelId.ChapterNavigator))
        assertTrue(result.activeOverlayPanelId != WorkbenchPanelId.ChapterNavigator)
    }

    @Test
    fun resizePanelDelta_accumulatesFromCurrentSize() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val initialSize = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp ?: 320f
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, 30f))
        assertEquals(initialSize + 30f, result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!!, 0.01f)
    }

    @Test
    fun resizePanelDelta_clampsByEditorMinWidth() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, 500f, 900f))
        val maxForEditor = 900f - 480f
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! <= maxForEditor)
    }

    @Test
    fun resizePanelDelta_negativeDeltaWithBothSides_enforcesEditorMin() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(expanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, -1000f, 1200f))
        val rightWidth = rightExpanded.actualSideWidthDp(DockZone.Right)
        val maxForEditor = 1200f - 480f - rightWidth
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! >= 280f)
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! <= maxForEditor)
    }

    @Test
    fun resizePanelDelta_negativeDelta_clampsMin() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, -1000f))
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! >= 280f)
    }

    @Test
    fun resizePanelDelta_bottomPanel_usesAvailableHeight() {
        val moved = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val expanded = WorkbenchReducer.reduce(moved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.Statistics, 50f, 800f))
        assertTrue(result.panels[WorkbenchPanelId.Statistics]?.sizeDp!! >= 220f)
    }

    @Test
    fun clampFloatingPosition_withinBounds() {
        val (x, y) = WorkbenchReducer.clampFloatingPosition(-50f, -30f, 400f, 500f, 800f, 600f)
        assertTrue(x >= 0f)
        assertTrue(y >= 0f)
    }

    @Test
    fun resizePanel_syncsDockGroupSizes() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 400f))
        val groupSize = result.dockGroupSizes["left-nav"]!!
        val panelSize = result.panels[WorkbenchPanelId.ChapterNavigator]!!.sizeDp
        assertEquals(panelSize, groupSize, 0.01f)
    }

    @Test
    fun resizePanelDelta_syncsDockGroupSizes() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, 50f))
        val groupSize = result.dockGroupSizes["left-nav"]!!
        val panelSize = result.panels[WorkbenchPanelId.ChapterNavigator]!!.sizeDp
        assertEquals(panelSize, groupSize, 0.01f)
    }

    @Test
    fun actualSideWidthDp_usesDockGroupSizes() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val rightWidth = state.actualSideWidthDp(DockZone.Right)
        val expectedWidth = state.dockGroupSizes["right-tools"] ?: 400f
        assertEquals(expectedWidth, rightWidth, 0.01f)
    }

    @Test
    fun researchWritingPreset_initializesDockGroupSizes() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        assertTrue(research.dockGroupSizes.containsKey("research-right"))
        assertEquals(380f, research.dockGroupSizes["research-right"]!!, 0.01f)
    }

    @Test
    fun resizePanel_usesActualSideWidth_notSumOfAllPanels() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        val statsExpanded = WorkbenchReducer.reduce(research, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val availableWidth = 1200f
        val result = WorkbenchReducer.reduce(statsExpanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 500f, availableWidth))
        val rightWidth = statsExpanded.actualSideWidthDp(DockZone.Right)
        val maxAllowed = availableWidth - 480f - rightWidth
        assertTrue(result.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!! <= maxAllowed)
    }

    @Test
    fun dockGroupsByZone_usesDockGroupSizes_asSizeRatio() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val groups = state.dockGroupsByZone(DockZone.Left)
        for (group in groups) {
            val expectedRatio = state.dockGroupSizes[group.id] ?: 280f
            assertEquals(expectedRatio, group.sizeRatio, 0.01f)
        }
    }

    @Test
    fun dockGroupsByZone_sizeRatioZero_defaultsTo280() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val groups = state.dockGroupsByZone(DockZone.Right)
        for (group in groups) {
            assertTrue(group.sizeRatio >= 280f)
        }
    }

    @Test
    fun resizeDockGroup_accumulatesAndUpdatesDockGroupSizes() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val initialSize = state.dockGroupSizes["left-nav"] ?: 320f
        val afterFirst = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockGroup("left-nav", DockZone.Left, 30f))
        assertEquals(initialSize + 30f, afterFirst.dockGroupSizes["left-nav"]!!, 0.01f)
        val afterSecond = WorkbenchReducer.reduce(afterFirst, WorkbenchAction.ResizeDockGroup("left-nav", DockZone.Left, -20f))
        assertEquals(initialSize + 10f, afterSecond.dockGroupSizes["left-nav"]!!, 0.01f)
    }

    @Test
    fun resizeDockGroup_updatesSizePropagatesToDockGroupsByZone() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val resized = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockGroup("left-nav", DockZone.Left, 80f))
        val leftGroups = resized.dockGroupsByZone(DockZone.Left)
        val navGroup = leftGroups.find { it.id == "left-nav" }
        assertNotNull(navGroup)
        val expectedRatio = state.dockGroupSizes["left-nav"]!! + 80f
        assertEquals(expectedRatio, navGroup!!.sizeRatio, 0.01f)
    }

    @Test
    fun resizeDockGroup_negativeDelta_clampsMin() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockGroup("left-nav", DockZone.Left, -1000f))
        assertTrue((result.dockGroupSizes["left-nav"] ?: 0f) >= 280f)
    }

    @Test
    fun resizeDockGroup_bottomGroup_clampsByMax() {
        val state = WorkbenchReducer.computeDefaultLayout()
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockGroup("bottom-group", DockZone.Bottom, 5000f))
        assertTrue((result.dockGroupSizes["bottom-group"] ?: 0f) <= 2000f)
    }

    @Test
    fun movePanelToGroup_initializesDockGroupSizesForNewGroup() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "new-group"))
        assertTrue(result.dockGroupSizes.containsKey("new-group"))
        assertTrue((result.dockGroupSizes["new-group"] ?: 0f) >= 280f)
    }

    @Test
    fun computePresentationState_switchesOverlayViaActivateOverlayPanel() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded3 = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val withActive = WorkbenchReducer.reduce(expanded3, WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.Search))
        val presentation = WorkbenchReducer.computePresentationState(withActive, 600f, 800f)
        assertEquals(WorkbenchPanelId.Search, presentation.activeOverlayPanelId)
        assertTrue(presentation.overlayPanelIds.contains(WorkbenchPanelId.Search))
    }

    @Test
    fun clampFloatingPanels_usesPanelActualDimensions() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, -100f, -50f))
        val resized = WorkbenchReducer.reduce(floated, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 300f, 400f))
        val result = WorkbenchReducer.reduce(resized, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertEquals(0f, panel.floatingX, 0.01f)
        assertEquals(0f, panel.floatingY, 0.01f)
        assertEquals(300f, panel.floatingWidthDp, 0.01f)
        assertEquals(400f, panel.floatingHeightDp, 0.01f)
    }

    @Test
    fun computePresentationState_mediumDualSide_rightPanelsInOverlay() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val bothExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val presentation = WorkbenchReducer.computePresentationState(bothExpanded, 1000f, 800f)
        assertFalse(presentation.isOverlayMode)
        assertTrue(presentation.overlayPanelIds.isNotEmpty())
        assertTrue(presentation.overlayPanelIds.contains(WorkbenchPanelId.AiAssistant))
        assertFalse(presentation.overlayPanelIds.contains(WorkbenchPanelId.ChapterNavigator))
    }

    @Test
    fun computePresentationState_mediumSingleSide_noOverlay() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val presentation = WorkbenchReducer.computePresentationState(expanded, 1000f, 800f)
        assertTrue(presentation.overlayPanelIds.isEmpty())
    }

    @Test
    fun computePresentationState_largeDualSide_noOverlay() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val bothExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val presentation = WorkbenchReducer.computePresentationState(bothExpanded, 1300f, 800f)
        assertTrue(presentation.overlayPanelIds.isEmpty())
    }

    @Test
    fun dockPanel_sameZone_noTabGroupIdChange() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val originalGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanel(WorkbenchPanelId.ChapterNavigator, DockZone.Left))
        assertEquals(originalGroupId, result.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId)
    }

    @Test
    fun dockPanel_assignsNewTabGroupId_forDifferentZone() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanel(WorkbenchPanelId.AiAssistant, DockZone.Left))
        val tabGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId
        assertNotNull(tabGroupId)
        assertTrue(tabGroupId!!.contains("left"))
        assertTrue(result.dockGroupSizes.containsKey(tabGroupId))
    }

    @Test
    fun dockPanel_reusesExistingGroupInZone() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val leftGroupId = chapterExpanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.DockPanel(WorkbenchPanelId.ProjectNavigator, DockZone.Left))
        assertEquals(leftGroupId, result.panels[WorkbenchPanelId.ProjectNavigator]?.tabGroupId)
    }

    @Test
    fun dockPanel_floatingToBottom_createsBottomGroup() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val tabGroupId = result.panels[WorkbenchPanelId.Statistics]?.tabGroupId
        assertNotNull(tabGroupId)
        assertTrue(tabGroupId!!.contains("bottom"))
        assertTrue(result.dockGroupSizes.containsKey(tabGroupId))
        val groupSize = result.dockGroupSizes[tabGroupId] ?: 0f
        assertTrue(groupSize >= 220f)
    }

    @Test
    fun computePresentationState_mediumWithBottom_noOverlayForBottom() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val bottomMoved = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val bothExpanded = WorkbenchReducer.reduce(bottomMoved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val presentation = WorkbenchReducer.computePresentationState(bothExpanded, 1000f, 800f)
        assertTrue(presentation.overlayPanelIds.isEmpty())
    }

    @Test
    fun computePresentationState_mediumDualSide_activeOverlayRespected() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val withActive = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.Search))
        val presentation = WorkbenchReducer.computePresentationState(withActive, 1000f, 800f)
        assertEquals(WorkbenchPanelId.Search, presentation.activeOverlayPanelId)
        assertTrue(presentation.overlayPanelIds.contains(WorkbenchPanelId.Search))
        assertFalse(presentation.overlayPanelIds.contains(WorkbenchPanelId.ChapterNavigator))
    }

    @Test
    fun computePresentationState_mediumDualSide_bottomNotInOverlay() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val bothExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val bottomMoved = WorkbenchReducer.reduce(bothExpanded, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val allExpanded = WorkbenchReducer.reduce(bottomMoved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val presentation = WorkbenchReducer.computePresentationState(allExpanded, 1000f, 800f)
        assertTrue(presentation.overlayPanelIds.contains(WorkbenchPanelId.AiAssistant))
        assertFalse(presentation.overlayPanelIds.contains(WorkbenchPanelId.Statistics))
    }
}
