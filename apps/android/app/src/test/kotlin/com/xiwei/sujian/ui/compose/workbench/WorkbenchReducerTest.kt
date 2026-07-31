package com.xiwei.sujian.ui.compose.workbench

import com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.DragDropTarget
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.TabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchDragState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.model.filterTabGroupHitAreas
import com.xiwei.sujian.ui.compose.workbench.model.upsertTabGroupHitArea
import com.xiwei.sujian.ui.compose.workbench.component.SplitHandleOrientation
import com.xiwei.sujian.ui.compose.workbench.component.splitHandleOrientation
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.min

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
    fun movePanelToGroup_updatesTabGroupId() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "custom-group"))
        assertEquals("custom-group", result.panels[WorkbenchPanelId.Search]?.tabGroupId)
    }

    @Test
    fun movePanelToGroup_updatesActiveTab() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "custom-group"))
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
    fun clampFloatingPosition_withinBounds() {
        val (x, y) = WorkbenchReducer.clampFloatingPosition(-50f, -30f, 400f, 500f, 800f, 600f)
        assertTrue(x >= -(400f - 32f))
        assertTrue(y >= 0f)
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
        assertEquals(0, newGroup.order)
    }

    @Test
    fun createDockGroup_duplicateOrder_reindexes() {
        val first = WorkbenchReducer.reduce(defaultState, WorkbenchAction.CreateDockGroup("g1", DockZone.Left, 0))
        val second = WorkbenchReducer.reduce(first, WorkbenchAction.CreateDockGroup("g2", DockZone.Left, 0))
        val leftGroups = second.dockGroupsByZone(DockZone.Left)
        val orders = leftGroups.map { it.order }
        assertEquals((0 until leftGroups.size).toList(), orders.sorted())
        val orderSet = orders.toSet()
        assertEquals(orders.size, orderSet.size)
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

    @Test
    fun dockPanelAsNewGroup_doubleDock_samePanel_sameZone_distinctGroupIds() {
        val first = WorkbenchReducer.reduce(defaultState, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 0))
        val firstGroupId = first.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val second = WorkbenchReducer.reduce(first, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 0))
        val secondGroupId = second.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("Repeated edge dock must create a fresh group, not reuse the previous one", secondGroupId != firstGroupId)
        assertTrue("Old group must be cleaned up once its only panel moves away", !second.dockGroupMeta.containsKey(firstGroupId))
        assertTrue(!second.dockGroupWeights.containsKey(firstGroupId))
        assertTrue(second.dockGroupMeta.containsKey(secondGroupId))
    }

    @Test
    fun floatPanel_removesEmptyMiddleGroup_reindexesRemainingOrders() {
        var state = defaultState
        state = WorkbenchReducer.reduce(state, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.ChapterNavigator, DockZone.Left, Int.MAX_VALUE))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, Int.MAX_VALUE))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.Search, DockZone.Left, Int.MAX_VALUE))
        val middleGroupId = state.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue(state.dockGroupMeta.containsKey(middleGroupId))
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        assertFalse("Empty middle group meta should be removed", result.dockGroupMeta.containsKey(middleGroupId))
        assertFalse("Empty middle group weights should be removed", result.dockGroupWeights.containsKey(middleGroupId))
        assertFalse("Empty middle group activeTab should be removed", result.activeTabByGroup.containsKey(middleGroupId))
        val remaining = result.dockGroupsByZone(DockZone.Left)
        val expandedGroups = remaining.filter { it.panelIds.isNotEmpty() }
        assertEquals("Two expanded groups should remain", 2, expandedGroups.size)
        assertTrue(expandedGroups.none { it.id == middleGroupId })
        val allLeftOrders = result.dockGroupMeta.values.filter { it.zone == DockZone.Left }.map { it.order }.sorted()
        assertEquals("All Left zone group orders should be continuous 0..n-1", allLeftOrders, (0 until allLeftOrders.size).toList())
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
        val state = defaultState.copy(
            panels = defaultState.panels + (WorkbenchPanelId.ProjectNavigator to defaultState.panels[WorkbenchPanelId.ProjectNavigator]!!.copy(
                tabGroupId = "right-tools",
            )),
        )
        val expanded = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val oldGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "new-target-group"))
        assertFalse("Old group with no panel left of any visibility should be removed", result.dockGroupMeta.containsKey(oldGroupId))
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
        val state = defaultState.copy(
            panels = defaultState.panels + (WorkbenchPanelId.ProjectNavigator to defaultState.panels[WorkbenchPanelId.ProjectNavigator]!!.copy(
                tabGroupId = "right-tools",
            )),
        )
        val expanded = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val oldGroupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.ChapterNavigator, DockZone.Right, 0))
        assertFalse("Old group with no panel left of any visibility should be cleaned when panel moves to new group", result.dockGroupMeta.containsKey(oldGroupId))
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
            dockGroupMeta = defaultState.dockGroupMeta + ("left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0)),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ReorderDockGroup("left-nav", 5))
        assertEquals(0, result.dockGroupMeta["left-nav"]?.order)
    }

    // --- Item 1 (follow-up): dockGroupsForHost uses meta order, not first-panel order ---

    @Test
    fun dockGroupsForHost_followsMetaOrderNotPanelOrder() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 1),
                "g2" to DockGroupMeta("g2", DockZone.Left, 0),
            ),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1", order = 0
                )) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2", order = 1
                )),
        )
        val groups = state.dockGroupsForHost(DockZone.Left, emptySet())
        assertEquals(
            "group render order must follow dockGroupMeta.order, not the first panel's order",
            listOf("g2", "g1"), groups.map { it.id }
        )
    }

    @Test
    fun dockGroupsForHost_insertionOrderReflected() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Left, 0))
        val newGroupId = result.panels[WorkbenchPanelId.AiAssistant]!!.tabGroupId
        val groups = result.dockGroupsForHost(DockZone.Left, emptySet())
        assertEquals("insertionOrder 0 must put the new group first in the host list", newGroupId, groups.first().id)
    }

    @Test
    fun dockGroupsForHost_filtersOverlayAndEmptyGroups() {
        val state = defaultState.copy(
            dockGroupMeta = defaultState.dockGroupMeta +
                ("left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0)) +
                ("left-extra" to DockGroupMeta("left-extra", DockZone.Left, 1)) +
                ("left-empty" to DockGroupMeta("left-empty", DockZone.Left, 2)),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "left-nav"
                )) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "left-extra"
                )),
        )
        val groups = state.dockGroupsForHost(DockZone.Left, setOf(WorkbenchPanelId.AiAssistant))
        assertEquals(
            "only groups with at least one panel outside the overlay may reach the host",
            listOf("left-nav"), groups.map { it.id }
        )
    }

    // --- Item 2 (follow-up): split handle orientation per zone ---

    @Test
    fun splitHandleOrientation_leftAndRight_areHorizontal() {
        assertEquals(SplitHandleOrientation.Horizontal, splitHandleOrientation(DockZone.Left))
        assertEquals(SplitHandleOrientation.Horizontal, splitHandleOrientation(DockZone.Right))
    }

    @Test
    fun splitHandleOrientation_bottom_isVertical() {
        assertEquals(SplitHandleOrientation.Vertical, splitHandleOrientation(DockZone.Bottom))
    }

    // --- Item 3 (follow-up): edge targets beat tab groups; hit areas are filtered/upserted ---

    @Test
    fun resolveDropTarget_edgeBeatsTabGroup() {
        val hitArea = TabGroupHitArea("test-group", 50f, 100f, 300f, 200f)
        val dragState = WorkbenchDragState(
            isDragging = true,
            draggedPanelId = WorkbenchPanelId.Search,
            pointerX = 60f,
            pointerY = 150f,
            tabGroupHitAreas = listOf(hitArea),
        )
        val (target, _) = dragState.resolveDropTarget(800f, 600f)
        assertEquals(
            "the 72dp screen edge must take priority even when the pointer is inside a tab group hit area",
            DragDropTarget.DockLeft, target
        )
    }

    @Test
    fun resolveDropTarget_contentArea_notMerged() {
        val hitArea = TabGroupHitArea("test-group", 100f, 0f, 500f, 40f)
        val dragState = WorkbenchDragState(
            isDragging = true,
            draggedPanelId = WorkbenchPanelId.Search,
            pointerX = 300f,
            pointerY = 300f,
            tabGroupHitAreas = listOf(hitArea),
        )
        val (target, _) = dragState.resolveDropTarget(800f, 600f)
        assertEquals(
            "content below the tab strip/title bar must not act as a merge target",
            DragDropTarget.None, target
        )
    }

    @Test
    fun filterTabGroupHitAreas_removesStaleGroupIds() {
        val areas = listOf(
            TabGroupHitArea("g1", 0f, 0f, 100f, 50f),
            TabGroupHitArea("g2", 0f, 0f, 100f, 50f),
            TabGroupHitArea("g3", 0f, 0f, 100f, 50f),
        )
        val filtered = filterTabGroupHitAreas(areas, setOf("g1", "g3"))
        assertEquals(listOf("g1", "g3"), filtered.map { it.groupId })
    }

    @Test
    fun upsertTabGroupHitArea_replacesSameGroup() {
        val areas = listOf(
            TabGroupHitArea("g1", 0f, 0f, 100f, 50f),
            TabGroupHitArea("g2", 0f, 0f, 100f, 50f),
        )
        val updated = upsertTabGroupHitArea(areas, TabGroupHitArea("g1", 10f, 10f, 90f, 40f))
        assertEquals(listOf("g2", "g1"), updated.map { it.groupId })
        assertEquals(10f, updated.last().left, 0.01f)
    }

    // --- Item 5 (follow-up): cleanUpOldGroup only deletes a truly empty group ---

    @Test
    fun cleanUpOldGroup_keepsGroupWithCollapsedPanelsRemain() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "target" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "target" to DockGroupMeta("target", DockZone.Left, 1),
            ),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Collapsed, tabGroupId = "g1"
                )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "target"))
        assertTrue("group with a collapsed panel still present must keep its meta", result.dockGroupMeta.containsKey("g1"))
        assertTrue("group with a collapsed panel still present must keep its weight", result.dockGroupWeights.containsKey("g1"))
        assertEquals("the collapsed panel must stay in its old group", "g1", result.panels[WorkbenchPanelId.Search]?.tabGroupId)
    }

    @Test
    fun cleanUpOldGroup_keepsGroupWithHiddenPanelsRemain() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "target" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "target" to DockGroupMeta("target", DockZone.Left, 1),
            ),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Hidden, tabGroupId = "g1"
                )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "target"))
        assertTrue("group with a hidden panel still present must keep its meta", result.dockGroupMeta.containsKey("g1"))
        assertTrue("group with a hidden panel still present must keep its weight", result.dockGroupWeights.containsKey("g1"))
        assertEquals("the hidden panel must stay in its old group", "g1", result.panels[WorkbenchPanelId.Search]?.tabGroupId)
    }

    @Test
    fun cleanUpOldGroup_clearsActiveTabButKeepsGroup() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "target" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "target" to DockGroupMeta("target", DockZone.Left, 1),
            ),
            activeTabByGroup = mapOf("g1" to WorkbenchPanelId.ChapterNavigator),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Collapsed, tabGroupId = "g1"
                )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "target"))
        assertTrue("no expanded panel remains, the stale activeTab must be cleared", result.activeTabByGroup["g1"] == null)
        assertTrue("but the group layout must survive", result.dockGroupMeta.containsKey("g1"))
        assertTrue(result.dockGroupWeights.containsKey("g1"))
    }

    @Test
    fun cleanUpOldGroup_deletesOnlyWhenTrulyEmpty() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "target" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "target" to DockGroupMeta("target", DockZone.Left, 1),
            ),
            activeTabByGroup = mapOf("g1" to WorkbenchPanelId.ChapterNavigator),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "target"))
        assertFalse("group without any panel left (of any visibility) must be deleted", result.dockGroupMeta.containsKey("g1"))
        assertFalse(result.dockGroupWeights.containsKey("g1"))
        assertFalse(result.activeTabByGroup.containsKey("g1"))
    }

    @Test
    fun cleanUpOldGroup_panelWithMatchingTabGroupIdButDifferentZone_notCountedAsRemaining() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "target" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "target" to DockGroupMeta("target", DockZone.Left, 1),
            ),
            activeTabByGroup = mapOf("g1" to WorkbenchPanelId.ChapterNavigator),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                    zone = DockZone.Floating, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )),
        )
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.ChapterNavigator, "target"))
        assertFalse("group g1 has no remaining panel in Left zone (Search is Floating), so it must be deleted", result.dockGroupMeta.containsKey("g1"))
        assertFalse(result.dockGroupWeights.containsKey("g1"))
    }

    // --- Item 6 (follow-up): ResizeDockSplit space-insufficiency safety ---

    @Test
    fun resizeDockSplit_insufficientSpace_normalizesEqualWeights() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 3f, "g2" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to DockGroupMeta("g2", DockZone.Left, 1),
            ),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
                )),
        )
        val totalBefore = state.dockGroupWeights["g1"]!! + state.dockGroupWeights["g2"]!!
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", -1000f, 150f))
        assertEquals("150dp < 2*80dp must normalize both groups to equal weights", 2f, result.dockGroupWeights["g1"]!!, 0.01f)
        assertEquals(2f, result.dockGroupWeights["g2"]!!, 0.01f)
        val totalAfter = result.dockGroupWeights["g1"]!! + result.dockGroupWeights["g2"]!!
        assertEquals("weight sum must be preserved", totalBefore, totalAfter, 0.01f)
    }

    @Test
    fun resizeDockSplit_tinyAvailable_noNegativeWeights() {
        val state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 10f, "g2" to 1f, "g3" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to DockGroupMeta("g2", DockZone.Left, 1),
                "g3" to DockGroupMeta("g3", DockZone.Left, 2),
            ),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
                )) +
                (WorkbenchPanelId.Search to defaultState.panels[WorkbenchPanelId.Search]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g3"
                )),
        )
        val totalBefore = state.dockGroupWeights.values.sum()
        val result = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", 1000f, 10f))
        for (id in listOf("g1", "g2", "g3")) {
            assertTrue("weight for $id must never go negative", result.dockGroupWeights[id]!! >= 0f)
        }
        assertEquals("tiny space must normalize to equal weights", 4f, result.dockGroupWeights["g1"]!!, 0.01f)
        assertEquals(4f, result.dockGroupWeights["g2"]!!, 0.01f)
        assertEquals(4f, result.dockGroupWeights["g3"]!!, 0.01f)
        assertEquals("weight sum must be preserved", totalBefore, result.dockGroupWeights.values.sum(), 0.01f)
    }

    @Test
    fun resizeDockSplit_repeatedExtremeDrags_noNegativeWeights() {
        var state = defaultState.copy(
            dockGroupWeights = mapOf("g1" to 1f, "g2" to 1f),
            dockGroupMeta = mapOf(
                "g1" to DockGroupMeta("g1", DockZone.Left, 0),
                "g2" to DockGroupMeta("g2", DockZone.Left, 1),
            ),
            panels = defaultState.panels +
                (WorkbenchPanelId.ChapterNavigator to defaultState.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g1"
                )) +
                (WorkbenchPanelId.AiAssistant to defaultState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                    zone = DockZone.Left, visibility = PanelVisibility.Expanded, tabGroupId = "g2"
                )),
        )
        for (i in 0 until 25) {
            state = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", 1000f, 170f))
            state = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "g1", "g2", -1000f, 170f))
            assertTrue(state.dockGroupWeights["g1"]!! >= 0f)
            assertTrue(state.dockGroupWeights["g2"]!! >= 0f)
            assertEquals("adjacent pair sum must stay constant across repeated drags", 2f, state.dockGroupWeights["g1"]!! + state.dockGroupWeights["g2"]!!, 0.01f)
        }
    }

    // --- Item 7 (follow-up): presets fully rebuild layout state from canonical defaults ---

    private fun pollutedCustomState(): WorkbenchLayoutState {
        var state = WorkbenchReducer.computeDefaultLayout()
        state = WorkbenchReducer.reduce(state, WorkbenchAction.CreateDockGroup("custom-a", DockZone.Left, 2))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "custom-a"))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "custom-a", "left-nav", -1000f, 320f))
        return state
    }

    private fun assertPresetRebuildsCompletely(result: WorkbenchLayoutState, expected: WorkbenchLayoutState) {
        assertEquals("panels must be rebuilt panel by panel", expected.panels, result.panels)
        assertEquals("activeTabByGroup must be rebuilt", expected.activeTabByGroup, result.activeTabByGroup)
        assertEquals("dockZoneSizeDp must be rebuilt", expected.dockZoneSizeDp, result.dockZoneSizeDp)
        assertEquals("dockGroupWeights must be rebuilt", expected.dockGroupWeights, result.dockGroupWeights)
        assertEquals("dockGroupMeta must be rebuilt", expected.dockGroupMeta, result.dockGroupMeta)
        assertEquals("preset must be set", expected.preset, result.preset)
        assertTrue("stale custom groups must be gone", result.dockGroupMeta.keys.none { it.startsWith("custom-") })
        assertTrue(result.dockGroupWeights.keys.none { it.startsWith("custom-") })
    }

    @Test
    fun applyPreset_focusWriting_exactStateFromPollutedCustom() {
        val expected = WorkbenchReducer.computeDefaultLayout()
        val result = WorkbenchReducer.reduce(pollutedCustomState(), WorkbenchAction.ApplyPreset(WorkbenchPreset.FocusWriting))
        assertPresetRebuildsCompletely(result, expected)
    }

    @Test
    fun applyPreset_chapterWriting_exactStateFromPollutedCustom() {
        val base = WorkbenchReducer.computeDefaultLayout()
        val expected = base.copy(
            panels = base.panels + (WorkbenchPanelId.ChapterNavigator to base.panels.getValue(WorkbenchPanelId.ChapterNavigator).copy(
                visibility = PanelVisibility.Expanded, sizeDp = 320f
            )),
            activeTabByGroup = mapOf("left-nav" to WorkbenchPanelId.ChapterNavigator),
            dockZoneSizeDp = mapOf(DockZone.Left to 320f, DockZone.Right to 400f),
            dockGroupWeights = base.dockGroupWeights,
            dockGroupMeta = base.dockGroupMeta,
            preset = WorkbenchPreset.ChapterWriting,
        )
        val result = WorkbenchReducer.reduce(pollutedCustomState(), WorkbenchAction.ApplyPreset(WorkbenchPreset.ChapterWriting))
        assertPresetRebuildsCompletely(result, expected)
    }

    @Test
    fun applyPreset_aiWriting_exactStateFromPollutedCustom() {
        val base = WorkbenchReducer.computeDefaultLayout()
        val expected = base.copy(
            panels = base.panels + (WorkbenchPanelId.AiAssistant to base.panels.getValue(WorkbenchPanelId.AiAssistant).copy(
                visibility = PanelVisibility.Expanded, sizeDp = 400f
            )),
            activeTabByGroup = mapOf("right-tools" to WorkbenchPanelId.AiAssistant),
            dockZoneSizeDp = mapOf(DockZone.Left to 320f, DockZone.Right to 400f),
            dockGroupWeights = base.dockGroupWeights,
            dockGroupMeta = base.dockGroupMeta,
            preset = WorkbenchPreset.AiWriting,
        )
        val result = WorkbenchReducer.reduce(pollutedCustomState(), WorkbenchAction.ApplyPreset(WorkbenchPreset.AiWriting))
        assertPresetRebuildsCompletely(result, expected)
    }

    @Test
    fun applyPreset_researchWriting_exactStateFromPollutedCustom() {
        val base = WorkbenchReducer.computeDefaultLayout()
        val expected = base.copy(
            panels = base.panels
                + (WorkbenchPanelId.ChapterNavigator to base.panels.getValue(WorkbenchPanelId.ChapterNavigator).copy(
                    visibility = PanelVisibility.Expanded, sizeDp = 320f
                ))
                + (WorkbenchPanelId.Search to base.panels.getValue(WorkbenchPanelId.Search).copy(
                    visibility = PanelVisibility.Expanded, sizeDp = 380f, tabGroupId = "research-right"
                ))
                + (WorkbenchPanelId.Statistics to base.panels.getValue(WorkbenchPanelId.Statistics).copy(
                    tabGroupId = "research-right"
                )),
            activeTabByGroup = mapOf(
                "left-nav" to WorkbenchPanelId.ChapterNavigator,
                "research-right" to WorkbenchPanelId.Search,
            ),
            dockZoneSizeDp = mapOf(DockZone.Left to 320f, DockZone.Right to 380f),
            dockGroupWeights = base.dockGroupWeights + ("research-right" to 1f),
            dockGroupMeta = base.dockGroupMeta + ("research-right" to DockGroupMeta("research-right", DockZone.Right, 2)),
            preset = WorkbenchPreset.ResearchWriting,
        )
        val result = WorkbenchReducer.reduce(pollutedCustomState(), WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        assertPresetRebuildsCompletely(result, expected)
    }

    @Test
    fun researchWritingPreset_preservesAllDefaultGroupMeta() {
        val preset = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ResearchWriting))
        assertTrue("researchWritingPreset should retain right-tools meta", preset.dockGroupMeta.containsKey("right-tools"))
        assertTrue("researchWritingPreset should retain right-outline meta", preset.dockGroupMeta.containsKey("right-outline"))
        assertTrue("researchWritingPreset should have research-right meta", preset.dockGroupMeta.containsKey("research-right"))
        val rightOrders = preset.dockGroupMeta.values.filter { it.zone == DockZone.Right }.map { it.order }.sorted()
        assertEquals("right zone group orders should be distinct and contiguous", listOf(0, 1, 2), rightOrders)
    }

    // --- Fix #2: activeTab normalization after visibility change ---

    @Test
    fun collapsePanel_activeTabBecomesCollapsed_normalizesToNextExpanded() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ProjectNavigator))
        val groupId = expanded2.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.ChapterNavigator))
        assertEquals(WorkbenchPanelId.ChapterNavigator, withActive.activeTabByGroup[groupId])
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.CollapsePanel(WorkbenchPanelId.ChapterNavigator))
        val newActive = result.activeTabByGroup[groupId]
        assertTrue("activeTab must switch away from collapsed panel", newActive != WorkbenchPanelId.ChapterNavigator)
        assertTrue("activeTab must point to an expanded panel in the same group", newActive != null && result.panels[newActive]?.visibility == PanelVisibility.Expanded)
    }

    @Test
    fun hidePanel_activeTabBecomesHidden_normalizesToNextExpanded() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.HidePanel(WorkbenchPanelId.AiAssistant))
        val newActive = result.activeTabByGroup[groupId]
        assertEquals("activeTab must switch to the remaining expanded panel (Search)", WorkbenchPanelId.Search, newActive)
    }

    @Test
    fun togglePanel_collapseActiveTab_normalizesActiveTab() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val groupId = expanded.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        assertEquals(WorkbenchPanelId.ChapterNavigator, expanded.activeTabByGroup[groupId])
        val collapsed = WorkbenchReducer.reduce(expanded, WorkbenchAction.TogglePanel(WorkbenchPanelId.ChapterNavigator))
        assertEquals(PanelVisibility.Collapsed, collapsed.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
        assertTrue("activeTab for a group with no expanded panels should be cleared", collapsed.activeTabByGroup[groupId] == null)
    }

    @Test
    fun expandPanel_setsActiveTabForGroup() {
        val result = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val groupId = result.panels[WorkbenchPanelId.ChapterNavigator]?.tabGroupId ?: ""
        assertEquals("expanding a panel should set it as the active tab", WorkbenchPanelId.ChapterNavigator, result.activeTabByGroup[groupId])
    }

    // --- Fix #3: ResizeDockZone with actualOtherSideWidthDp ---

    @Test
    fun resizeDockZone_usesActualOtherSideWidth_whenProvided() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val availableWidth = 1200f
        val actualRightWidth = 0f
        val currentLeftSize = rightExpanded.dockZoneSizeDp[DockZone.Left] ?: 320f
        val deltaDp = 500f
        val newSize = currentLeftSize + deltaDp
        val maxForEditor = availableWidth - 480f - actualRightWidth
        val expectedClamped = newSize.coerceIn(280f, min(520f, maxForEditor))
        val result = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, deltaDp, availableWidth, actualRightWidth))
        assertEquals("with other side = 0f, left zone should be clamped to exact expected value", expectedClamped, result.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
    }

    @Test
    fun resizeDockZone_nullOtherSide_fallsBackToPersistedWidth() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val availableWidth = 1200f
        val resultNull = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 500f, availableWidth, null))
        val resultExplicit = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 500f, availableWidth, rightExpanded.actualSideWidthDp(DockZone.Right)))
        assertEquals("null should behave same as explicit persisted width", resultExplicit.dockZoneSizeDp[DockZone.Left]!!, resultNull.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
    }

    @Test
    fun resizeDockZone_actualOtherSideWidth_overridesPersistedWidth() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val availableWidth = 1200f
        val resultSmallOther = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 300f, availableWidth, 100f))
        val resultLargeOther = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 300f, availableWidth, 400f))
        assertTrue("smaller other side width should allow larger left zone", resultSmallOther.dockZoneSizeDp[DockZone.Left]!! > resultLargeOther.dockZoneSizeDp[DockZone.Left]!!)
    }

    // --- Fix #5: computeDefaultLayout has complete dockGroupMeta ---

    @Test
    fun defaultLayout_hasCompleteDockGroupMeta() {
        assertTrue(defaultState.dockGroupMeta.containsKey("left-nav"))
        assertTrue(defaultState.dockGroupMeta.containsKey("right-tools"))
        assertTrue(defaultState.dockGroupMeta.containsKey("right-outline"))
        assertEquals(DockZone.Left, defaultState.dockGroupMeta["left-nav"]?.zone)
        assertEquals(DockZone.Right, defaultState.dockGroupMeta["right-tools"]?.zone)
        assertEquals(DockZone.Right, defaultState.dockGroupMeta["right-outline"]?.zone)
        assertEquals(0, defaultState.dockGroupMeta["right-tools"]?.order)
        assertEquals(1, defaultState.dockGroupMeta["right-outline"]?.order)
    }

    @Test
    fun expandPanel_newGroupMeta_orderIsAfterExisting() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val rightZoneMaxOrder = expanded.dockGroupMeta.values.filter { it.zone == DockZone.Right }.maxOfOrNull { it.order } ?: -1
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.Search, DockZone.Right, rightZoneMaxOrder + 1))
        val newGroupId = result.panels[WorkbenchPanelId.Search]?.tabGroupId ?: ""
        val existingGroupId = expanded.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("new group must have a different id from existing groups ($newGroupId vs $existingGroupId)",
            newGroupId != existingGroupId)
        assertTrue("new group must exist in dockGroupMeta (keys: ${result.dockGroupMeta.keys})",
            newGroupId in result.dockGroupMeta)
        val newMeta = result.dockGroupMeta[newGroupId]
        assertTrue("new group order (${newMeta?.order}) must be after existing max ($rightZoneMaxOrder)", (newMeta?.order ?: -1) > rightZoneMaxOrder)
    }

    @Test
    fun togglePanel_expandPath_normalizesStaleActiveTabsInOtherGroups() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val rightGroupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActiveRight = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(rightGroupId, WorkbenchPanelId.AiAssistant))
        val hidden = WorkbenchReducer.reduce(withActiveRight, WorkbenchAction.HidePanel(WorkbenchPanelId.AiAssistant))
        assertTrue("after hiding AiAssistant, right group activeTab should not be AiAssistant",
            hidden.activeTabByGroup[rightGroupId] != WorkbenchPanelId.AiAssistant)
        val manuallyCorrupted = hidden.copy(
            activeTabByGroup = hidden.activeTabByGroup + (rightGroupId to WorkbenchPanelId.AiAssistant)
        )
        val collapsedLeft = manuallyCorrupted.copy(
            panels = manuallyCorrupted.panels + (WorkbenchPanelId.ChapterNavigator to manuallyCorrupted.panels[WorkbenchPanelId.ChapterNavigator]!!.copy(visibility = PanelVisibility.Collapsed))
        )
        val result = WorkbenchReducer.reduce(collapsedLeft, WorkbenchAction.TogglePanel(WorkbenchPanelId.ChapterNavigator))
        assertTrue("togglePanel(expand) should normalize stale activeTab in other groups",
            result.activeTabByGroup[rightGroupId] != WorkbenchPanelId.AiAssistant)
    }

    @Test
    fun floatPanel_normalizesActiveTabInSourceGroup() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val rightGroupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(rightGroupId, WorkbenchPanelId.AiAssistant))
        assertEquals(WorkbenchPanelId.AiAssistant, withActive.activeTabByGroup[rightGroupId])
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        assertEquals("floatPanel must normalize activeTab to remaining expanded panel",
            WorkbenchPanelId.Search, result.activeTabByGroup[rightGroupId])
    }

    @Test
    fun expandPanel_noDuplicateOrdersInSameZone() {
        var state = defaultState
        for (id in WorkbenchPanelId.entries) {
            state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(id))
        }
        state = WorkbenchReducer.reduce(state, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.AiAssistant, DockZone.Right, 10))
        val aiGroupId = state.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        assertTrue("new group must differ from right-tools", aiGroupId != "right-tools")
        val rightOrders = state.dockGroupMeta.values.filter { it.zone == DockZone.Right }.map { it.order }
        assertEquals("no duplicate orders in Right zone after creating new group", rightOrders.distinct().size, rightOrders.size)
        val leftOrders = state.dockGroupMeta.values.filter { it.zone == DockZone.Left }.map { it.order }
        assertEquals("no duplicate orders in Left zone", leftOrders.distinct().size, leftOrders.size)
    }

    // --- Fix: floatPanel/floatPanelAt activeTab switches to remaining panel in group ---

    @Test
    fun floatPanel_activeTabSwitchesToRemainingExpandedPanel() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        assertEquals(WorkbenchPanelId.AiAssistant, withActive.activeTabByGroup[groupId])
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val newActive = result.activeTabByGroup[groupId]
        assertTrue("activeTab must switch away from floated panel", newActive != WorkbenchPanelId.AiAssistant)
        assertTrue("activeTab must point to remaining expanded panel in group", newActive != null && result.panels[newActive]?.visibility == PanelVisibility.Expanded)
    }

    @Test
    fun floatPanelAt_activeTabSwitchesToRemainingExpandedPanel() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 100f, 200f))
        val newActive = result.activeTabByGroup[groupId]
        assertEquals("activeTab must switch to the remaining expanded panel (Search)", WorkbenchPanelId.Search, newActive)
    }

    // --- Defect: movePanel does not clean up old group activeTab ---

    @Test
    fun movePanel_cleansUpOldGroupActiveTab() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        assertEquals(WorkbenchPanelId.AiAssistant, withActive.activeTabByGroup[groupId])
        val result = WorkbenchReducer.reduce(withActive, WorkbenchAction.MovePanel(WorkbenchPanelId.AiAssistant, DockZone.Bottom))
        val newActive = result.activeTabByGroup[groupId]
        assertTrue("movePanel away from group should switch activeTab to remaining panel", newActive != WorkbenchPanelId.AiAssistant)
        if (result.panels.values.any { it.tabGroupId == groupId && it.zone != DockZone.Floating && it.visibility == PanelVisibility.Expanded }) {
            assertNotNull("activeTab should point to a remaining expanded panel in the old group", newActive)
        }
    }

    // --- Defect: normalizeActiveTabs does not check zone ---

    @Test
    fun normalizeActiveTabs_repairsActiveTabPointingToFloatingPanel() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        val floated = WorkbenchReducer.reduce(withActive, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val activeTab = floated.activeTabByGroup[groupId]
        assertTrue("after floatPanel, activeTab must not point to the floated panel (zone mismatch)", activeTab != WorkbenchPanelId.AiAssistant)
    }

    @Test
    fun normalizeActiveTabs_zoneCheck_floatingPanelNotCountedAsGroupMember() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        val floated = WorkbenchReducer.reduce(withActive, WorkbenchAction.FloatPanel(WorkbenchPanelId.AiAssistant))
        val zone = floated.dockGroupMeta[groupId]?.zone
        if (zone != null) {
            val activeTab = floated.activeTabByGroup[groupId]
            val activePanel = if (activeTab != null) floated.panels[activeTab] else null
            assertTrue("activeTab panel must be in the same zone as the group", activePanel == null || activePanel.zone == zone)
        }
    }

    @Test
    fun normalizeActiveTabs_repairsStaleActiveTabAfterManualZoneChange() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val groupId = expanded2.panels[WorkbenchPanelId.AiAssistant]?.tabGroupId ?: ""
        val withActive = WorkbenchReducer.reduce(expanded2, WorkbenchAction.ActivateTab(groupId, WorkbenchPanelId.AiAssistant))
        val moved = WorkbenchReducer.reduce(withActive, WorkbenchAction.MovePanel(WorkbenchPanelId.AiAssistant, DockZone.Left))
        val activeTab = moved.activeTabByGroup[groupId]
        val activePanel = if (activeTab != null) moved.panels[activeTab] else null
        val groupZone = moved.dockGroupMeta[groupId]?.zone
        if (groupZone != null && activePanel != null) {
            assertTrue("after movePanel away, activeTab should not point to panel in different zone (activeTab=$activeTab, panelZone=${activePanel.zone}, groupZone=$groupZone)", activePanel.zone == groupZone || activeTab != WorkbenchPanelId.AiAssistant)
        }
    }

    // --- Defect: Bottom resize handle direction is reversed ---

    @Test
    fun resizeDockZone_bottomPositiveDelta_increasesBottomHeight() {
        val moved = WorkbenchReducer.reduce(defaultState, WorkbenchAction.MovePanel(WorkbenchPanelId.Statistics, DockZone.Bottom))
        val expanded = WorkbenchReducer.reduce(moved, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Statistics))
        val beforeSize = expanded.dockZoneSizeDp[DockZone.Bottom] ?: 220f
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.ResizeDockZone(DockZone.Bottom, 50f, 800f))
        val afterSize = result.dockZoneSizeDp[DockZone.Bottom]!!
        assertTrue("positive delta should increase bottom height (before=$beforeSize, after=$afterSize)", afterSize > beforeSize)
    }

    // --- Defect: SujianWorkbench passes 0f instead of null for actualOtherSideWidthDp ---
    // This is a UI-layer defect; test the Reducer contract: 0f means zero, null means fallback

    // --- Defect: Presets lose dockGroupMeta for inactive groups ---

    @Test
    fun chapterWritingPreset_preservesAllDefaultGroupMeta() {
        val preset = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.ChapterWriting))
        assertTrue("chapterWritingPreset should retain right-tools meta", preset.dockGroupMeta.containsKey("right-tools"))
        assertTrue("chapterWritingPreset should retain right-outline meta", preset.dockGroupMeta.containsKey("right-outline"))
    }

    @Test
    fun aiWritingPreset_preservesAllDefaultGroupMeta() {
        val preset = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ApplyPreset(WorkbenchPreset.AiWriting))
        assertTrue("aiWritingPreset should retain left-nav meta", preset.dockGroupMeta.containsKey("left-nav"))
        assertTrue("aiWritingPreset should retain right-outline meta", preset.dockGroupMeta.containsKey("right-outline"))
    }

    // --- Defect: movePanelToGroup creates new group meta with order=0 causing duplicate ---

    @Test
    fun movePanelToGroup_newGroupMeta_orderNotDuplicateWithExisting() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.DocumentOutline))
        val rightToolsGroup = expanded2.panels[WorkbenchPanelId.AiAssistant]!!.tabGroupId
        val rightOutlineGroup = expanded2.panels[WorkbenchPanelId.DocumentOutline]!!.tabGroupId
        val newGroupId = "right-new-dynamic"
        val moved = WorkbenchReducer.reduce(expanded2, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, newGroupId))
        val newMeta = moved.dockGroupMeta[newGroupId]
        assertNotNull("new group meta should exist", newMeta)
        val existingOrders = moved.dockGroupMeta.values
            .filter { it.zone == DockZone.Right && it.id != newGroupId }
            .map { it.order }
        if (newMeta != null) {
            assertFalse("new group order=${newMeta.order} should not duplicate existing orders $existingOrders", newMeta.order in existingOrders)
        }
    }

    // --- Defect: movePanelBetweenGroups creates new group meta with order=0 causing duplicate ---

    @Test
    fun movePanelBetweenGroups_newGroupMeta_orderNotDuplicateWithExisting() {
        val expanded1 = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val expanded2 = WorkbenchReducer.reduce(expanded1, WorkbenchAction.ExpandPanel(WorkbenchPanelId.DocumentOutline))
        val newGroupId = "right-dynamic-via-between"
        val stateWithNewGroup = expanded2.copy(
            dockGroupMeta = expanded2.dockGroupMeta + (newGroupId to DockGroupMeta(newGroupId, DockZone.Right, 2)),
            dockGroupWeights = expanded2.dockGroupWeights + (newGroupId to 1f),
        )
        val moved = WorkbenchReducer.reduce(stateWithNewGroup, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, newGroupId))
        val newMeta = moved.dockGroupMeta[newGroupId]
        assertNotNull("new group meta should exist", newMeta)
        val existingOrders = moved.dockGroupMeta.values
            .filter { it.zone == DockZone.Right && it.id != newGroupId }
            .map { it.order }
        if (newMeta != null) {
            assertFalse("new group order=${newMeta.order} should not duplicate existing orders $existingOrders", newMeta.order in existingOrders)
        }
    }

    @Test
    fun resizeDockZone_zeroOtherSide_allowsLargerZoneThanPersistedWidth() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val availableWidth = 1200f
        val resultZero = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 500f, availableWidth, 0f))
        val resultNull = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 500f, availableWidth, null))
        val resultWithOther = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ResizeDockZone(DockZone.Left, 300f, availableWidth, 400f))
        assertTrue("0f (other side truly absent) should allow larger left zone than null (fallback to persisted right width)", resultZero.dockZoneSizeDp[DockZone.Left]!! >= resultNull.dockZoneSizeDp[DockZone.Left]!!)
        assertTrue("0f should allow larger left zone than when other side is 400dp", resultZero.dockZoneSizeDp[DockZone.Left]!! > resultWithOther.dockZoneSizeDp[DockZone.Left]!!)
    }

    // --- cleanUpOldGroup should normalize active tabs across all groups ---

    @Test
    fun movePanel_normalizesStaleActiveTabInOtherGroup() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val outlineExpanded = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.DocumentOutline))
        val staleState = outlineExpanded.copy(
            activeTabByGroup = outlineExpanded.activeTabByGroup + ("left-nav" to WorkbenchPanelId.AiAssistant)
        )
        val moved = WorkbenchReducer.reduce(staleState, WorkbenchAction.MovePanel(WorkbenchPanelId.AiAssistant, DockZone.Bottom))
        val leftActive = moved.activeTabByGroup["left-nav"]
        val leftActivePanel = moved.panels[leftActive]
        if (leftActive != null) {
            assertTrue("left-nav activeTab should point to an Expanded panel in Left zone, got ${leftActivePanel?.zone}/${leftActivePanel?.visibility}", leftActivePanel?.zone == DockZone.Left && leftActivePanel?.visibility == PanelVisibility.Expanded)
        }
    }

    @Test
    fun floatPanel_normalizesStaleActiveTabInOtherGroup() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val searchExpanded = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val staleState = searchExpanded.copy(
            activeTabByGroup = searchExpanded.activeTabByGroup + ("left-nav" to WorkbenchPanelId.AiAssistant)
        )
        val floated = WorkbenchReducer.reduce(staleState, WorkbenchAction.FloatPanel(WorkbenchPanelId.Search))
        val leftActive = floated.activeTabByGroup["left-nav"]
        val leftActivePanel = floated.panels[leftActive]
        if (leftActive != null) {
            assertTrue("left-nav activeTab should point to an Expanded panel in Left zone, got ${leftActivePanel?.zone}/${leftActivePanel?.visibility}", leftActivePanel?.zone == DockZone.Left && leftActivePanel?.visibility == PanelVisibility.Expanded)
        }
    }

    @Test
    fun movePanelToGroup_normalizesStaleActiveTabInOtherGroup() {
        val leftExpanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val rightExpanded = WorkbenchReducer.reduce(leftExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val searchExpanded = WorkbenchReducer.reduce(rightExpanded, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        val staleState = searchExpanded.copy(
            activeTabByGroup = searchExpanded.activeTabByGroup + ("left-nav" to WorkbenchPanelId.AiAssistant)
        )
        val moved = WorkbenchReducer.reduce(staleState, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "right-outline"))
        val leftActive = moved.activeTabByGroup["left-nav"]
        val leftActivePanel = moved.panels[leftActive]
        if (leftActive != null) {
            assertTrue("left-nav activeTab should point to an Expanded panel in Left zone, got ${leftActivePanel?.zone}/${leftActivePanel?.visibility}", leftActivePanel?.zone == DockZone.Left && leftActivePanel?.visibility == PanelVisibility.Expanded)
        }
    }

    @Test
    fun movePanel_crossZone_tabGroupIdConsistentWithNewZone() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanel(WorkbenchPanelId.AiAssistant, DockZone.Bottom))
        val panel = result.panels[WorkbenchPanelId.AiAssistant]!!
        assertEquals("panel zone should be Bottom", DockZone.Bottom, panel.zone)
        val meta = result.dockGroupMeta[panel.tabGroupId]
        assertNotNull("dockGroupMeta must exist for the panel's tabGroupId", meta)
        if (meta != null) {
            assertEquals("dockGroupMeta zone must match panel zone after cross-zone move", DockZone.Bottom, meta.zone)
        }
    }

    @Test
    fun movePanel_crossZone_noOrphanedTabGroupIdInWrongZone() {
        val expanded = WorkbenchReducer.reduce(defaultState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        val result = WorkbenchReducer.reduce(expanded, WorkbenchAction.MovePanel(WorkbenchPanelId.AiAssistant, DockZone.Bottom))
        for ((groupId, meta) in result.dockGroupMeta) {
            val panelsInMetaZone = result.panels.values.filter { it.tabGroupId == groupId && it.zone == meta.zone }
            val panelsInOtherZone = result.panels.values.filter { it.tabGroupId == groupId && it.zone != meta.zone && it.zone != DockZone.Floating }
            assertTrue("group $groupId has panels in zone ${meta.zone} matching its meta", panelsInMetaZone.isNotEmpty() || panelsInOtherZone.isEmpty())
        }
    }
}
