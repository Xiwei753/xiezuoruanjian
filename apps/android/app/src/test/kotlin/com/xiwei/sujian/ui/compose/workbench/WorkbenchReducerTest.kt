package com.xiwei.sujian.ui.compose.workbench

import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.DragDropTarget
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.TabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchDragState
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
    fun defaultLayout_hasDockZoneSizeDp() {
        assertTrue(defaultState.dockZoneSizeDp.containsKey(DockZone.Left))
        assertTrue(defaultState.dockZoneSizeDp.containsKey(DockZone.Right))
        assertEquals(320f, defaultState.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
        assertEquals(400f, defaultState.dockZoneSizeDp[DockZone.Right]!!, 0.01f)
    }

    @Test
    fun defaultLayout_hasDockGroupWeights() {
        assertTrue(defaultState.dockGroupWeights.containsKey("left-nav"))
        assertTrue(defaultState.dockGroupWeights.containsKey("right-tools"))
        assertTrue(defaultState.dockGroupWeights.containsKey("right-outline"))
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
    fun createDockGroup_savesGroupWeight() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.CreateDockGroup("new-group", DockZone.Left, 0))
        assertTrue(result.dockGroupWeights.containsKey("new-group"))
        assertEquals(1f, result.dockGroupWeights["new-group"]!!, 0.01f)
    }

    @Test
    fun createDockGroup_duplicateId_noChange() {
        val first = WorkbenchReducer.reduce(defaultState, WorkbenchAction.CreateDockGroup("g1", DockZone.Left, 0))
        val second = WorkbenchReducer.reduce(first, WorkbenchAction.CreateDockGroup("g1", DockZone.Left, 1))
        assertEquals(first.dockGroupWeights, second.dockGroupWeights)
        val firstGroupCount = first.dockGroupsByZone(DockZone.Left).size
        val secondGroupCount = second.dockGroupsByZone(DockZone.Left).size
        assertEquals(firstGroupCount, secondGroupCount)
    }

    @Test
    fun clampFloatingPanels_clampsPositionAndSize() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, -100f, -50f))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingX >= -(panel.floatingWidthDp - 32f))
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
    fun actualSideWidthDp_readsDockZoneSizeDp() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        val rightWidth = research.actualSideWidthDp(DockZone.Right)
        assertEquals(research.dockZoneSizeDp[DockZone.Right]!!, rightWidth, 0.01f)
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
        assertTrue(x >= -(400f - 32f))
        assertTrue(y >= 0f)
    }

    @Test
    fun resizePanel_updatesDockZoneSizeDp() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 400f))
        assertEquals(400f, result.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
    }

    @Test
    fun resizePanelDelta_noDoubleAccumulation() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val first = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, 30f))
        val second = WorkbenchReducer.reduce(first, WorkbenchAction.ResizePanelDelta(WorkbenchPanelId.ChapterNavigator, 20f))
        val initialSize = chapterExpanded.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!!
        assertEquals(initialSize + 50f, second.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!!, 0.01f)
    }

    @Test
    fun dockGroupsByZone_usesDockGroupWeights_asWeight() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val groups = chapterExpanded.dockGroupsByZone(DockZone.Left)
        assertTrue(groups.isNotEmpty())
        for (group in groups) {
            val expectedWeight = chapterExpanded.dockGroupWeights[group.id] ?: 1f
            assertEquals(expectedWeight, group.weight, 0.01f)
        }
    }

    @Test
    fun dockGroupsByZone_weightMissing_defaultsTo1() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val stateWithoutWeight = chapterExpanded.copy(dockGroupWeights = chapterExpanded.dockGroupWeights - "left-nav")
        val groups = stateWithoutWeight.dockGroupsByZone(DockZone.Left)
        assertTrue(groups.isNotEmpty())
        for (group in groups) {
            if (group.id == "left-nav") {
                assertEquals(1f, group.weight, 0.01f)
            }
        }
    }

    @Test
    fun resizeDockSplit_adjustsWeightsAndPreservesSum() {
        val state = defaultState.copy(
            dockGroupWeights = defaultState.dockGroupWeights + ("left-nav" to 2f) + ("left-extra" to 1f),
            dockGroupMeta = defaultState.dockGroupMeta + ("left-extra" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-extra", DockZone.Left, 1)),
        )
        val totalBefore = state.dockGroupWeights["left-nav"]!! + state.dockGroupWeights["left-extra"]!!
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "left-nav", "left-extra", 50f, 320f))
        val totalAfter = result.dockGroupWeights["left-nav"]!! + result.dockGroupWeights["left-extra"]!!
        assertEquals(totalBefore, totalAfter, 0.01f)
    }

    @Test
    fun resizeDockSplit_respectsMinWeight() {
        val state = defaultState.copy(
            dockGroupWeights = defaultState.dockGroupWeights + ("left-nav" to 1f) + ("left-extra" to 1f),
            dockGroupMeta = defaultState.dockGroupMeta + ("left-extra" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-extra", DockZone.Left, 1)),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "left-nav"
            )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "left-nav", "left-extra", -1000f, 320f))
        val zoneTotalWeight = 1f + 1f
        val minWeight = 80f * zoneTotalWeight / 320f
        assertTrue(result.dockGroupWeights["left-nav"]!! >= minWeight - 0.01f)
        assertTrue(result.dockGroupWeights["left-extra"]!! >= minWeight - 0.01f)
    }

    @Test
    fun resizeDockSplit_unknownGroup_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizeDockSplit(DockZone.Left, "unknown1", "unknown2", 50f, 320f))
        assertEquals(defaultState, result)
    }

    @Test
    fun resizeDockSplit_zeroAvailableAxis_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizeDockSplit(DockZone.Left, "left-nav", "right-tools", 50f, 0f))
        assertEquals(defaultState, result)
    }

    @Test
    fun resizeDockZone_updatesDockZoneSizeDp() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 50f, 1200f))
        val expectedSize = (chapterExpanded.dockZoneSizeDp[DockZone.Left] ?: 320f) + 50f
        assertEquals(expectedSize, result.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
    }

    @Test
    fun resizeDockZone_enforcesEditorMinWidth() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val availableWidth = 1200f
        val result = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 500f, availableWidth))
        val rightWidth = result.actualSideWidthDp(DockZone.Right)
        val maxForEditor = availableWidth - 480f - rightWidth
        assertTrue(result.dockZoneSizeDp[DockZone.Left]!! <= maxForEditor)
    }

    @Test
    fun resizeDockZone_bottomZone_clampsByMaxRatio() {
        val moved = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val expanded = WorkbenchReducer.reduce(moved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizeDockZone(DockZone.Bottom, 500f, 800f))
        val maxBottom = 800f * 0.55f
        assertTrue(result.dockZoneSizeDp[DockZone.Bottom]!! <= maxBottom)
    }

    @Test
    fun resizeDockZone_emptyZone_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizeDockZone(DockZone.Left, 50f, 1200f))
        assertEquals(defaultState, result)
    }

    @Test
    fun resizeDockZone_zonesAreIndependent() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 50f, 1200f))
        val rightSizeBefore = chapterExpanded.dockZoneSizeDp[DockZone.Right]
        val rightSizeAfter = result.dockZoneSizeDp[DockZone.Right]
        assertEquals(rightSizeBefore, rightSizeAfter)
    }

    @Test
    fun clampFloatingPosition_allowsPartialLeftOverflow() {
        val (x, y) = WorkbenchReducer.clampFloatingPosition(-300f, 10f, 400f, 500f, 800f, 600f)
        assertTrue(x >= -(400f - 32f))
        assertTrue(x <= 800f - 32f)
    }

    @Test
    fun clampFloatingPosition_clampsRightEdge() {
        val (x, _) = WorkbenchReducer.clampFloatingPosition(900f, 10f, 400f, 500f, 800f, 600f)
        assertTrue(x <= 800f - 32f)
    }

    @Test
    fun clampFloatingPosition_clampsBottomEdge() {
        val (_, y) = WorkbenchReducer.clampFloatingPosition(10f, 700f, 400f, 500f, 800f, 600f)
        assertTrue(y <= 600f - 40f)
    }

    @Test
    fun moveFloatingPanel_clampsPosition() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.MoveFloatingPanel(WorkbenchPanelId.AiAssistant, -500f, -200f))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingX >= -(panel.floatingWidthDp - 32f))
        assertTrue(panel.floatingY >= 0f)
    }

    @Test
    fun clampFloatingPanels_usesClampFloatingPosition() {
        val floated = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, -100f, -50f))
        val result = WorkbenchReducer.reduce(floated, WorkbenchAction.ClampFloatingPanels(800f, 600f))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingY >= 0f)
        assertTrue(panel.floatingWidthDp <= 800f)
        assertTrue(panel.floatingHeightDp <= 600f)
    }

    @Test
    fun resolveDropTarget_tabGroupHitAreaTakesPriority() {
        val hitArea = TabGroupHitArea("test-group", 100f, 100f, 300f, 200f)
        val dragState = WorkbenchDragState(
            isDragging = true,
            draggedPanelId = WorkbenchPanelId.Search,
            pointerX = 200f,
            pointerY = 150f,
            tabGroupHitAreas = listOf(hitArea),
        )
        val (target, groupId) = dragState.resolveDropTarget(800f, 600f)
        assertEquals(DragDropTarget.TabGroup, target)
        assertEquals("test-group", groupId)
    }

    @Test
    fun resolveDropTarget_dockMarginsWhenNotInHitArea() {
        val dragState = WorkbenchDragState(
            isDragging = true,
            draggedPanelId = WorkbenchPanelId.Search,
            pointerX = 50f,
            pointerY = 300f,
            tabGroupHitAreas = emptyList(),
        )
        val (target, _) = dragState.resolveDropTarget(800f, 600f)
        assertEquals(DragDropTarget.DockLeft, target)
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
        assertTrue(result.dockGroupWeights.containsKey(tabGroupId))
    }

    @Test
    fun dockPanel_reusesExistingGroupInZone() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val leftGroupId = chapterExpanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.DockPanel(WorkbenchPanelId.AiAssistant, DockZone.Left))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertEquals("DockPanel cross-zone should join existing group when available", leftGroupId, newGroupId)
        assertTrue(result.dockGroupWeights.containsKey(newGroupId))
        assertTrue(result.dockGroupMeta.containsKey(newGroupId))
    }

    @Test
    fun dockPanel_floatingToBottom_createsBottomGroup() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val tabGroupId = result.panels[WorkbenchPanelId.Statistics]?.tabGroupId
        assertNotNull(tabGroupId)
        assertTrue(tabGroupId!!.contains("bottom"))
        assertTrue(result.dockGroupWeights.containsKey(tabGroupId))
        assertTrue(result.dockZoneSizeDp.containsKey(DockZone.Bottom))
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

    @Test
    fun movePanelToGroup_initializesDockGroupWeightsForNewGroup() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "new-group"))
        assertTrue(result.dockGroupWeights.containsKey("new-group"))
        assertEquals(1f, result.dockGroupWeights["new-group"]!!, 0.01f)
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
        assertTrue(panel.floatingX >= -(300f - 32f))
        assertEquals(0f, panel.floatingY, 0.01f)
        assertEquals(300f, panel.floatingWidthDp, 0.01f)
        assertEquals(400f, panel.floatingHeightDp, 0.01f)
    }

    @Test
    fun dockPanel_crossZone_updatesZoneAndTabGroupId() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanel(WorkbenchPanelId.AiAssistant, DockZone.Left))
        assertEquals(DockZone.Left, result.panels[WorkbenchPanelId.AiAssistant]?.zone)
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId!!
        assertTrue("DockPanel to empty zone should create new group", result.dockGroupMeta.containsKey(newGroupId))
    }

    @Test
    fun createDockGroup_persistsZoneAndOrder() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.CreateDockGroup("new-group", DockZone.Bottom, 3))
        assertTrue(result.dockGroupWeights.containsKey("new-group"))
        val groups = result.dockGroupsByZone(DockZone.Bottom)
        val newGroup = groups.find { it.id == "new-group" }
        assertNotNull(newGroup)
        assertEquals(DockZone.Bottom, newGroup!!.zone)
        assertEquals(3, newGroup.order)
    }

    @Test
    fun movePanelToGroup_bottomZone_usesBottomDefaultSize() {
        val bottomExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val expanded = WorkbenchReducer.reduce(bottomExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val bottomGroupId = expanded.panels[WorkbenchPanelId.Statistics]?.tabGroupId!!
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, bottomGroupId))
        assertTrue(result.dockGroupWeights.containsKey(bottomGroupId))
    }

    @Test
    fun expandPanel_initializesDockGroupWeights() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        assertTrue(result.dockGroupWeights.containsKey("left-nav"))
    }

    @Test
    fun applyPreset_setsDockZoneSizeDp() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        assertEquals(320f, research.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
        assertEquals(380f, research.dockZoneSizeDp[DockZone.Right]!!, 0.01f)
    }

    @Test
    fun migrateFromV1_convertsDockGroupSizesToZoneSizeAndWeights() {
        val panels = WorkbenchPanelId.entries.associateWith { id ->
            com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState(
                id = id,
                zone = when (id) {
                    WorkbenchPanelId.ChapterNavigator -> DockZone.Left
                    WorkbenchPanelId.Search -> DockZone.Right
                    else -> DockZone.Right
                },
                visibility = PanelVisibility.Collapsed,
                sizeDp = 320f,
                tabGroupId = when (id) {
                    WorkbenchPanelId.ChapterNavigator -> "left-nav"
                    WorkbenchPanelId.Search -> "research-right"
                    else -> "right-tools"
                },
                order = id.ordinal,
            )
        }
        val dockGroupSizes = mapOf("left-nav" to 320f, "research-right" to 380f, "right-tools" to 400f)
        val result = WorkbenchReducer.migrateFromV1(
            panels = panels,
            activeTabByGroup = emptyMap(),
            preset = WorkbenchPreset.Custom,
            dockGroupSizes = dockGroupSizes,
            activeOverlayPanelId = null,
        )
        assertEquals(320f, result.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
        assertEquals(400f, result.dockZoneSizeDp[DockZone.Right]!!, 0.01f)
        assertEquals(1f, result.dockGroupWeights["left-nav"]!!, 0.01f)
        assertEquals(1f, result.dockGroupWeights["research-right"]!!, 0.01f)
        assertTrue(result.dockGroupMeta.containsKey("left-nav"))
        assertTrue(result.dockGroupMeta.containsKey("research-right"))
        assertTrue(result.nextFloatingZIndex >= 1)
    }

    @Test
    fun migrateFromV1_nextFloatingZIndex_atLeastMaxPlusOne() {
        val panels = WorkbenchPanelId.entries.associateWith { id ->
            com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState(
                id = id,
                zone = if (id == WorkbenchPanelId.AiAssistant) DockZone.Floating else DockZone.Right,
                visibility = if (id == WorkbenchPanelId.AiAssistant) PanelVisibility.Expanded else PanelVisibility.Collapsed,
                sizeDp = 320f,
                tabGroupId = "default",
                order = id.ordinal,
                floatingZIndex = if (id == WorkbenchPanelId.AiAssistant) 5 else 0,
            )
        }
        val result = WorkbenchReducer.migrateFromV1(
            panels = panels,
            activeTabByGroup = emptyMap(),
            preset = WorkbenchPreset.Custom,
            dockGroupSizes = emptyMap(),
            activeOverlayPanelId = null,
        )
        assertTrue(result.nextFloatingZIndex >= 6)
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
    fun nextFloatingZIndex_incrementsOnFloatPanel() {
        val first = WorkbenchReducer.reduce(defaultState, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val firstZ = first.nextFloatingZIndex
        val second = WorkbenchReducer.reduce(first, WorkbenchAction.FloatPanel(WorkbenchPanelId.Search))
        assertTrue(second.nextFloatingZIndex > firstZ)
    }

    @Test
    fun resizeDockSplit_weightConversion_usesZoneTotalWeight() {
        val state = defaultState.copy(
            dockGroupWeights = defaultState.dockGroupWeights + ("left-nav" to 1f) + ("left-extra" to 1f),
            dockGroupMeta = defaultState.dockGroupMeta + ("left-extra" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-extra", DockZone.Left, 1)),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "left-nav"
            )) + (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "left-extra"
            )),
        )
        val availableMainAxisDp = 600f
        val deltaDp = 60f
        val zoneTotalWeight = 2f
        val expectedDeltaWeight = deltaDp * zoneTotalWeight / availableMainAxisDp
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "left-nav", "left-extra", deltaDp, availableMainAxisDp))
        val actualDelta = result.dockGroupWeights["left-nav"]!! - state.dockGroupWeights["left-nav"]!!
        assertEquals(expectedDeltaWeight, actualDelta, 0.01f)
    }

    @Test
    fun resizeDockSplit_threeGroups_correctWeightConversion() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f, "g3" to 1f),
            dockGroupMeta = mapOf(
                "g1" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1),
                "g3" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g3", DockZone.Left, 2),
            ),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
            )) + (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
            )) + (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g3"
            )),
        )
        val availableMainAxisDp = 600f
        val deltaDp = 60f
        val zoneTotalWeight = 3f
        val expectedDeltaWeight = deltaDp * zoneTotalWeight / availableMainAxisDp
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", deltaDp, availableMainAxisDp))
        val actualDelta = result.dockGroupWeights["g1"]!! - state.dockGroupWeights["g1"]!!
        assertEquals(expectedDeltaWeight, actualDelta, 0.01f)
    }

    @Test
    fun resizeDockSplit_nonNormalizedWeights_correctConversion() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 2f, "g2" to 3f),
            dockGroupMeta = mapOf(
                "g1" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1),
            ),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
            )) + (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
            )),
        )
        val availableMainAxisDp = 500f
        val deltaDp = 50f
        val zoneTotalWeight = 5f
        val expectedDeltaWeight = deltaDp * zoneTotalWeight / availableMainAxisDp
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", deltaDp, availableMainAxisDp))
        val actualDelta = result.dockGroupWeights["g1"]!! - state.dockGroupWeights["g1"]!!
        assertEquals(expectedDeltaWeight, actualDelta, 0.01f)
    }

    @Test
    fun dockPanel_edgeDock_createsNewGroup() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val existingGroupId = chapterExpanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 1))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("Edge dock should create new group, not merge into existing", newGroupId != existingGroupId)
        assertTrue(result.dockGroupMeta.containsKey(newGroupId))
        assertTrue(result.dockGroupWeights.containsKey(newGroupId))
    }

    @Test
    fun dockPanel_edgeDock_newGroupOrderIsAfterExisting() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 1))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val newMeta = result.dockGroupMeta[newGroupId]!!
        val existingGroups = chapterExpanded.dockGroupsByZone(DockZone.Left)
        val maxExistingOrder = existingGroups.maxOfOrNull { it.order } ?: -1
        assertTrue("New group order should be after existing groups", newMeta.order > maxExistingOrder)
    }

    @Test
    fun dockPanel_edgeDock_setsActiveTab() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 0))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertEquals(WorkbenchPanelId.AiAssistant, result.activeTabByGroup[newGroupId])
    }

    @Test
    fun movePanelToGroup_mergesIntoExistingGroup() {
        val chapterExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val leftGroupId = chapterExpanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(chapterExpanded, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.AiAssistant, leftGroupId))
        assertEquals(leftGroupId, result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId)
    }

    @Test
    fun reorderDockGroup_updatesMetaOrder() {
        val state = defaultState.copy(
            dockGroupMeta = defaultState.dockGroupMeta + ("left-nav" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-nav", DockZone.Left, 0)),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("left-nav", 5))
        val order = result.dockGroupMeta["left-nav"]?.order
        assertTrue("Single group reorder should result in order 0 after reindex", order == 0)
    }

    @Test
    fun reorderDockGroup_unknownGroup_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ReorderDockGroup("unknown-group", 5))
        assertEquals(defaultState, result)
    }

    @Test
    fun reorderDockGroup_doesNotChangePanelOrder() {
        val state = defaultState.copy(
            dockGroupMeta = defaultState.dockGroupMeta + ("left-nav" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-nav", DockZone.Left, 0)),
        )
        val panelOrderBefore = state.panels[WorkbenchPanelId.ChapterNavigator]?.order
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("left-nav", 5))
        assertEquals(panelOrderBefore, result.panels[WorkbenchPanelId.ChapterNavigator]?.order)
    }

    @Test
    fun reorderDockGroup_changesRenderOrder() {
        val meta0 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0)
        val meta1 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1)
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf("g1" to meta0, "g2" to meta1),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1")) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2")),
        )
        val groupsBefore = state.dockGroupsByZone(DockZone.Left).map { it.id }
        assertEquals(listOf("g1", "g2"), groupsBefore)
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("g1", 2))
        val groupsAfter = result.dockGroupsByZone(DockZone.Left).map { it.id }
        assertEquals(listOf("g2", "g1"), groupsAfter)
    }

    @Test
    fun resizeDockSplit_negativeAvailableAxis_noChange() {
        val state = defaultState.copy(
            dockGroupWeights = defaultState.dockGroupWeights + ("left-nav" to 1f) + ("left-extra" to 1f),
            dockGroupMeta = defaultState.dockGroupMeta + ("left-extra" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-extra", DockZone.Left, 1)),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "left-nav", "left-extra", 50f, -100f))
        assertEquals(state.dockGroupWeights, result.dockGroupWeights)
    }

    @Test
    fun resizeDockSplit_invalidGroup_noChange() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ResizeDockSplit(DockZone.Left, "nonexistent1", "nonexistent2", 50f, 320f))
        assertEquals(defaultState, result)
    }

    // --- Item 1: Drag direction tests ---

    @Test
    fun resizeDockSplit_leftZone_positiveDelta_increasesBeforeWeight() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf(
                "g1" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1),
            ),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
            )) + (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
            )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", 50f, 600f))
        assertTrue("Positive delta (drag down) should increase before weight", result.dockGroupWeights["g1"]!! > state.dockGroupWeights["g1"]!!)
        assertTrue("Positive delta (drag down) should decrease after weight", result.dockGroupWeights["g2"]!! < state.dockGroupWeights["g2"]!!)
    }

    @Test
    fun resizeDockSplit_bottomZone_positiveDelta_increasesBeforeWeight() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf(
                "g1" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Bottom, 0),
                "g2" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Bottom, 1),
            ),
            panels = defaultState.panels + (WorkbenchPanelId.Statistics to defaultState.panels[WorkbenchPanelId.Statistics]!!.copy(
                zone = DockZone.Bottom, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
            )) + (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                zone = DockZone.Bottom, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
            )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Bottom, "g1", "g2", 50f, 600f))
        assertTrue("Positive delta (drag right) should increase before weight", result.dockGroupWeights["g1"]!! > state.dockGroupWeights["g1"]!!)
    }

    // --- Item 2: Visible groups only ---

    @Test
    fun resizeDockSplit_ignoresHiddenGroupsInTotalWeight() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f, "g3" to 1f),
            dockGroupMeta = mapOf(
                "g1" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1),
                "g3" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g3", DockZone.Left, 2),
            ),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
            )) + (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
            )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", 60f, 600f))
        val zoneTotalWeight = 2f
        val expectedDeltaWeight = 60f * zoneTotalWeight / 600f
        val actualDelta = result.dockGroupWeights["g1"]!! - state.dockGroupWeights["g1"]!!
        assertEquals("Total weight should only count visible groups", expectedDeltaWeight, actualDelta, 0.01f)
    }

    @Test
    fun resizeDockSplit_nonAdjacentGroups_noChange() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f, "g3" to 1f),
            dockGroupMeta = mapOf(
                "g1" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1),
                "g3" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g3", DockZone.Left, 2),
            ),
            panels = defaultState.panels + (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
            )) + (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
            )) + (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g3"
            )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g3", 50f, 600f))
        assertEquals("Non-adjacent groups should not be resized", state.dockGroupWeights, result.dockGroupWeights)
    }

    // --- Item 3: DockPanelAsNewGroup ---

    @Test
    fun dockPanelAsNewGroup_createsNewGroupWithUniqueId() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val existingGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 1))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("New group ID must differ from existing", newGroupId != existingGroupId)
        assertTrue(result.dockGroupMeta.containsKey(newGroupId))
        assertTrue(result.dockGroupWeights.containsKey(newGroupId))
        assertTrue("New group ID should be generated, not fixed pattern", newGroupId.startsWith("left-group-"))
    }

    @Test
    fun dockPanelAsNewGroup_sameZone_createsNewSplitGroup() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val originalGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.ChapterNavigator, DockZone.Left, 1))
        val newGroupId = result.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        assertTrue("Same zone edge dock should create new group", newGroupId != originalGroupId)
    }

    @Test
    fun dockPanel_sameZone_doesNotCreateNewGroup() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val originalGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanel(WorkbenchPanelId.ChapterNavigator, DockZone.Left))
        assertEquals("DockPanel same zone should keep tabGroupId", originalGroupId, result.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId)
    }

    @Test
    fun dockPanelAsNewGroup_setsActiveTab() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 0))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertEquals(WorkbenchPanelId.AiAssistant, result.activeTabByGroup[newGroupId])
    }

    // --- Item 4: movePanelBetweenGroups and old group cleanup ---

    @Test
    fun floatPanel_cleansUpEmptyOldGroup() {
        val docked = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 0))
        val groupId = docked.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("Group should exist before float", docked.dockGroupMeta.containsKey(groupId))
        val result = WorkbenchReducer.reduce(docked, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        assertFalse("Empty group meta should be cleaned up", result.dockGroupMeta.containsKey(groupId))
        assertFalse("Empty group weights should be cleaned up", result.dockGroupWeights.containsKey(groupId))
        assertFalse("Empty group activeTab should be cleaned up", result.activeTabByGroup.containsKey(groupId))
    }

    @Test
    fun movePanelToGroup_cleansUpEmptyOldGroup() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val oldGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "new-target-group"))
        assertFalse("Old empty group meta should be removed", result.dockGroupMeta.containsKey(oldGroupId))
        assertFalse("Old empty group weights should be removed", result.dockGroupWeights.containsKey(oldGroupId))
    }

    @Test
    fun movePanelToGroup_nonLastPanel_switchesActiveTab() {
        val research = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        val groupId = research.panels[WorkbenchPanelId.Search]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(research, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "new-group"))
        val remainingActive = result.activeTabByGroup[groupId]
        assertTrue("Active tab should switch to remaining panel in group", remainingActive != WorkbenchPanelId.Search)
    }

    @Test
    fun floatPanelAt_cleansUpEmptyOldGroup() {
        val docked = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.ChapterNavigator, DockZone.Left, 0))
        val groupId = docked.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(docked, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.ChapterNavigator, 100f, 200f))
        assertFalse("Empty group should be cleaned after FloatPanelAt", result.dockGroupMeta.containsKey(groupId))
    }

    @Test
    fun dockPanelAsNewGroup_cleansUpOldEmptyGroup() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val oldGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.ChapterNavigator, DockZone.Right, 0))
        assertFalse("Old empty group should be cleaned when panel moves to new group", result.dockGroupMeta.containsKey(oldGroupId))
    }

    // --- Item 5: persistLayout atomic key+state (tested in ViewModel test) ---

    @Test
    fun dockPanel_crossZone_createsNewGroupWithUniquePrefix() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanel(WorkbenchPanelId.AiAssistant, DockZone.Bottom))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("Cross-zone dock should create new group", newGroupId.startsWith("bottom-group-"))
        assertTrue(result.dockGroupMeta.containsKey(newGroupId))
    }

    // --- Item 6: ReorderDockGroup real remove/insert ---

    @Test
    fun reorderDockGroup_realRemoveInsert_continuousOrder() {
        val meta0 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0)
        val meta1 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1)
        val meta2 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g3", DockZone.Left, 2)
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f, "g3" to 1f),
            dockGroupMeta = mapOf("g1" to meta0, "g2" to meta1, "g3" to meta2),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1")) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2")) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g3")),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("g1", 2))
        val orders = result.dockGroupMeta.values.filter { it.zone == DockZone.Left }.sortedBy { it.order }.map { it.order }
        assertEquals("Orders should be continuous 0..n-1", listOf(0, 1, 2), orders)
        val ids = result.dockGroupMeta.values.filter { it.zone == DockZone.Left }.sortedBy { it.order }.map { it.id }
        assertEquals("g1 should move to position 2", listOf("g2", "g3", "g1"), ids)
    }

    @Test
    fun reorderDockGroup_moveToFront() {
        val meta0 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0)
        val meta1 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1)
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf("g1" to meta0, "g2" to meta1),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1")) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2")),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("g2", 0))
        val ids = result.dockGroupMeta.values.filter { it.zone == DockZone.Left }.sortedBy { it.order }.map { it.id }
        assertEquals(listOf("g2", "g1"), ids)
    }

    @Test
    fun reorderDockGroup_clampsOutOfBoundsOrder() {
        val meta0 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0)
        val meta1 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1)
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf("g1" to meta0, "g2" to meta1),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1")) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2")),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("g1", 100))
        val ids = result.dockGroupMeta.values.filter { it.zone == DockZone.Left }.sortedBy { it.order }.map { it.id }
        assertEquals("Out-of-bounds order should be clamped", listOf("g2", "g1"), ids)
    }

    @Test
    fun reorderDockGroup_noDuplicateOrders() {
        val meta0 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g1", DockZone.Left, 0)
        val meta1 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g2", DockZone.Left, 1)
        val meta2 = com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("g3", DockZone.Left, 2)
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f, "g3" to 1f),
            dockGroupMeta = mapOf("g1" to meta0, "g2" to meta1, "g3" to meta2),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1")) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2")) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g3")),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("g3", 1))
        val orders = result.dockGroupMeta.values.filter { it.zone == DockZone.Left }.map { it.order }
        assertEquals("No duplicate orders", orders.distinct().size, orders.size)
    }

    @Test
    fun reorderDockGroup_singleGroup_noChange() {
        val state = defaultState.copy(
            dockGroupMeta = defaultState.dockGroupMeta + ("left-nav" to com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta("left-nav", DockZone.Left, 0)),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("left-nav", 5))
        assertEquals(0, result.dockGroupMeta["left-nav"]?.order)
    }
}
