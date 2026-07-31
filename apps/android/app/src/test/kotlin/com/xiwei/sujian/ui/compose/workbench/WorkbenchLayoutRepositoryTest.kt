package com.xiwei.sujian.ui.compose.workbench

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.LAYOUT_SNAPSHOT_VERSION
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutRepository
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WorkbenchLayoutRepositoryTest {

    private lateinit var repository: WorkbenchLayoutRepository
    private val testKey = LayoutStorageKey(
        deviceId = "test-device",
        orientation = "portrait",
        windowWidthBucket = WindowWidthBucket.Expanded,
        windowMode = "standard",
    )

    @Before
    fun setUp() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        repository = WorkbenchLayoutRepository(context)
    }

    @Test
    fun loadLayout_returnsNullWhenNotSaved() = runTest {
        val unsavedKey = LayoutStorageKey(
            deviceId = "unsaved-test-device",
            orientation = "portrait",
            windowWidthBucket = WindowWidthBucket.Expanded,
            windowMode = "standard",
        )
        val result = repository.loadLayout(unsavedKey)
        assertNull(result)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesPreset() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.ChapterWriting)
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(WorkbenchPreset.ChapterWriting, loaded!!.preset)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesPanelZone() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutStateWithPanel(
            WorkbenchPanelId.AiAssistant, DockZone.Right, PanelVisibility.Expanded
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(DockZone.Right, loaded!!.panels[WorkbenchPanelId.AiAssistant]?.zone)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesPanelVisibility() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutStateWithPanel(
            WorkbenchPanelId.Search, DockZone.Right, PanelVisibility.Expanded
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(PanelVisibility.Expanded, loaded!!.panels[WorkbenchPanelId.Search]?.visibility)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesPanelSize() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutStateWithPanel(
            WorkbenchPanelId.ChapterNavigator, DockZone.Left, PanelVisibility.Expanded, sizeDp = 350f
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(350f, loaded!!.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp!!, 0.01f)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesFloatingPosition() = runTest {
        val baseState = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom)
        val state = baseState.copy(
            panels = baseState.panels + (WorkbenchPanelId.AiAssistant to baseState.panels[WorkbenchPanelId.AiAssistant]!!.copy(
                zone = DockZone.Floating,
                visibility = PanelVisibility.Expanded,
                floatingX = 120f,
                floatingY = 80f,
                floatingWidthDp = 500f,
                floatingHeightDp = 600f,
            ))
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        val panel = loaded!!.panels[WorkbenchPanelId.AiAssistant]!!
        assertEquals(DockZone.Floating, panel.zone)
        assertEquals(120f, panel.floatingX, 0.01f)
        assertEquals(80f, panel.floatingY, 0.01f)
        assertEquals(500f, panel.floatingWidthDp, 0.01f)
        assertEquals(600f, panel.floatingHeightDp, 0.01f)
    }

    @Test
    fun saveAndLoad_differentKeys_independent() = runTest {
        val key2 = LayoutStorageKey(
            deviceId = "test-device",
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Large,
            windowMode = "standard",
        )
        val state1 = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.FocusWriting)
        val state2 = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.AiWriting)
        repository.saveLayout(testKey, state1)
        repository.saveLayout(key2, state2)
        val loaded1 = repository.loadLayout(testKey)
        val loaded2 = repository.loadLayout(key2)
        assertNotNull(loaded1)
        assertNotNull(loaded2)
        assertEquals(WorkbenchPreset.FocusWriting, loaded1!!.preset)
        assertEquals(WorkbenchPreset.AiWriting, loaded2!!.preset)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesDockZoneSizeDp() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).copy(
            dockZoneSizeDp = mapOf(DockZone.Left to 350f, DockZone.Right to 420f, DockZone.Bottom to 250f)
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(350f, loaded!!.dockZoneSizeDp[DockZone.Left]!!, 0.01f)
        assertEquals(420f, loaded.dockZoneSizeDp[DockZone.Right]!!, 0.01f)
        assertEquals(250f, loaded.dockZoneSizeDp[DockZone.Bottom]!!, 0.01f)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesDockGroupWeights() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).copy(
            dockGroupWeights = mapOf("left-nav" to 2f, "right-tools" to 1.5f)
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(2f, loaded!!.dockGroupWeights["left-nav"]!!, 0.01f)
        assertEquals(1.5f, loaded.dockGroupWeights["right-tools"]!!, 0.01f)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesDockGroupMeta() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).copy(
            dockGroupMeta = mapOf(
                "left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0),
                "right-tools" to DockGroupMeta("right-tools", DockZone.Right, 1),
            )
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(DockZone.Left, loaded!!.dockGroupMeta["left-nav"]!!.zone)
        assertEquals(0, loaded.dockGroupMeta["left-nav"]!!.order)
        assertEquals(DockZone.Right, loaded.dockGroupMeta["right-tools"]!!.zone)
        assertEquals(1, loaded.dockGroupMeta["right-tools"]!!.order)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesNextFloatingZIndex() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).copy(
            nextFloatingZIndex = 7
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(7, loaded!!.nextFloatingZIndex)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesSnapshotVersion() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom)
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(LAYOUT_SNAPSHOT_VERSION, loaded!!.snapshotVersion)
    }

    @Test
    fun saveAndLoad_roundTrip_preservesActiveOverlayPanelId() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutStateWithPanel(
            WorkbenchPanelId.AiAssistant, DockZone.Right, PanelVisibility.Expanded
        ).copy(activeOverlayPanelId = WorkbenchPanelId.AiAssistant)
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        assertEquals(WorkbenchPanelId.AiAssistant, loaded!!.activeOverlayPanelId)
    }

    @Test
    fun saveLayout_clearsStaleOverlayKey() = runTest {
        val stateWithOverlay = WorkbenchReducerTestHelper.createTestLayoutStateWithPanel(
            WorkbenchPanelId.AiAssistant, DockZone.Right, PanelVisibility.Expanded
        ).copy(activeOverlayPanelId = WorkbenchPanelId.AiAssistant)
        repository.saveLayout(testKey, stateWithOverlay)
        val loadedWithOverlay = repository.loadLayout(testKey)
        assertNotNull(loadedWithOverlay)
        assertEquals(WorkbenchPanelId.AiAssistant, loadedWithOverlay!!.activeOverlayPanelId)

        val stateWithoutOverlay = stateWithOverlay.copy(activeOverlayPanelId = null)
        repository.saveLayout(testKey, stateWithoutOverlay)
        val loadedWithoutOverlay = repository.loadLayout(testKey)
        assertNotNull(loadedWithoutOverlay)
        assertNull(loadedWithoutOverlay!!.activeOverlayPanelId)
    }

    @Test
    fun saveLayout_clearsStaleGroupKeys() = runTest {
        val stateWithGroup = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).copy(
            dockGroupWeights = mapOf("group-a" to 1f),
            dockGroupMeta = mapOf("group-a" to DockGroupMeta("group-a", DockZone.Left, 0)),
        )
        repository.saveLayout(testKey, stateWithGroup)
        val loadedWithGroup = repository.loadLayout(testKey)
        assertNotNull(loadedWithGroup)
        assertTrue(loadedWithGroup!!.dockGroupWeights.containsKey("group-a"))

        val stateWithoutGroup = stateWithGroup.copy(
            dockGroupWeights = emptyMap(),
            dockGroupMeta = emptyMap(),
        )
        repository.saveLayout(testKey, stateWithoutGroup)
        val loadedWithoutGroup = repository.loadLayout(testKey)
        assertNotNull(loadedWithoutGroup)
        assertFalse(loadedWithoutGroup!!.dockGroupWeights.containsKey("group-a"))
    }

    @Test
    fun loadLayout_v2_nextFloatingZIndex_defendsAgainstLowSavedValue() = runTest {
        val state = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).copy(
            nextFloatingZIndex = 1,
            panels = WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).panels +
                (WorkbenchPanelId.AiAssistant to WorkbenchReducerTestHelper.createTestLayoutState(WorkbenchPreset.Custom).panels[WorkbenchPanelId.AiAssistant]!!.copy(
                    zone = DockZone.Floating,
                    visibility = PanelVisibility.Expanded,
                    floatingZIndex = 10,
                )),
        )
        repository.saveLayout(testKey, state)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        val loadedVal = loaded!!
        assertTrue("nextFloatingZIndex should be at least maxPanelZ + 1 = 11, got ${loadedVal.nextFloatingZIndex}", loadedVal.nextFloatingZIndex >= 11)
    }

    // --- Item 7 (follow-up): every preset must round-trip exactly from a polluted custom state ---

    private fun pollutedCustomState(): WorkbenchLayoutState {
        var state = WorkbenchReducer.computeDefaultLayout()
        state = WorkbenchReducer.reduce(state, WorkbenchAction.CreateDockGroup("custom-a", DockZone.Left, 2))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.MovePanelToGroup(WorkbenchPanelId.Search, "custom-a"))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.Search))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ResizeDockSplit(DockZone.Left, "custom-a", "left-nav", -1000f, 320f))
        return state
    }

    private fun expectedPresetState(preset: WorkbenchPreset): WorkbenchLayoutState {
        val base = WorkbenchReducer.computeDefaultLayout()
        return when (preset) {
            WorkbenchPreset.FocusWriting -> base
            WorkbenchPreset.ChapterWriting -> base.copy(
                panels = base.panels + (WorkbenchPanelId.ChapterNavigator to base.panels.getValue(WorkbenchPanelId.ChapterNavigator).copy(
                    visibility = PanelVisibility.Expanded, sizeDp = 320f
                )),
                activeTabByGroup = mapOf("left-nav" to WorkbenchPanelId.ChapterNavigator),
                dockZoneSizeDp = mapOf(DockZone.Left to 320f),
                dockGroupWeights = mapOf("left-nav" to 1f),
                dockGroupMeta = mapOf("left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0)),
                preset = WorkbenchPreset.ChapterWriting,
            )
            WorkbenchPreset.AiWriting -> base.copy(
                panels = base.panels + (WorkbenchPanelId.AiAssistant to base.panels.getValue(WorkbenchPanelId.AiAssistant).copy(
                    visibility = PanelVisibility.Expanded, sizeDp = 400f
                )),
                activeTabByGroup = mapOf("right-tools" to WorkbenchPanelId.AiAssistant),
                dockZoneSizeDp = mapOf(DockZone.Right to 400f),
                dockGroupWeights = mapOf("right-tools" to 1f),
                dockGroupMeta = mapOf("right-tools" to DockGroupMeta("right-tools", DockZone.Right, 0)),
                preset = WorkbenchPreset.AiWriting,
            )
            WorkbenchPreset.ResearchWriting -> base.copy(
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
                dockGroupWeights = mapOf("left-nav" to 1f, "research-right" to 1f),
                dockGroupMeta = mapOf(
                    "left-nav" to DockGroupMeta("left-nav", DockZone.Left, 0),
                    "research-right" to DockGroupMeta("research-right", DockZone.Right, 0),
                ),
                preset = WorkbenchPreset.ResearchWriting,
            )
            WorkbenchPreset.Custom -> error("no canonical state for Custom")
        }
    }

    private fun assertPresetRoundTripExact(preset: WorkbenchPreset) = runTest {
        val applied = WorkbenchReducer.reduce(pollutedCustomState(), WorkbenchAction.ApplyPreset(preset))
        repository.saveLayout(testKey, applied)
        val loaded = repository.loadLayout(testKey)
        assertNotNull(loaded)
        val expected = expectedPresetState(preset)
        assertEquals("preset must survive the snapshot round trip", expected.preset, loaded!!.preset)
        assertEquals("panels must survive the snapshot round trip", expected.panels, loaded.panels)
        assertEquals("activeTabByGroup must survive the snapshot round trip", expected.activeTabByGroup, loaded.activeTabByGroup)
        assertEquals("dockZoneSizeDp must survive the snapshot round trip", expected.dockZoneSizeDp, loaded.dockZoneSizeDp)
        assertEquals("dockGroupWeights must survive the snapshot round trip", expected.dockGroupWeights, loaded.dockGroupWeights)
        assertEquals("dockGroupMeta must survive the snapshot round trip", expected.dockGroupMeta, loaded.dockGroupMeta)
    }

    @Test
    fun saveAndLoad_roundTrip_focusWriting_exact() = assertPresetRoundTripExact(WorkbenchPreset.FocusWriting)

    @Test
    fun saveAndLoad_roundTrip_chapterWriting_exact() = assertPresetRoundTripExact(WorkbenchPreset.ChapterWriting)

    @Test
    fun saveAndLoad_roundTrip_aiWriting_exact() = assertPresetRoundTripExact(WorkbenchPreset.AiWriting)

    @Test
    fun saveAndLoad_roundTrip_researchWriting_exact() = assertPresetRoundTripExact(WorkbenchPreset.ResearchWriting)
}

internal object WorkbenchReducerTestHelper {
    fun createTestLayoutState(preset: WorkbenchPreset): WorkbenchLayoutState {
        val panels = WorkbenchPanelId.entries.associateWith { id ->
            WorkbenchPanelState(
                id = id,
                zone = when (id) {
                    WorkbenchPanelId.ProjectNavigator, WorkbenchPanelId.ChapterNavigator -> DockZone.Left
                    else -> DockZone.Right
                },
                visibility = PanelVisibility.Collapsed,
                sizeDp = 320f,
                tabGroupId = "default",
                order = id.ordinal,
            )
        }
        return WorkbenchLayoutState(panels = panels, activeTabByGroup = emptyMap(), preset = preset)
    }

    fun createTestLayoutStateWithPanel(
        targetId: WorkbenchPanelId,
        zone: DockZone,
        visibility: PanelVisibility,
        sizeDp: Float = 320f,
    ): WorkbenchLayoutState {
        val base = createTestLayoutState(WorkbenchPreset.Custom)
        return base.copy(
            panels = base.panels + (targetId to base.panels[targetId]!!.copy(
                zone = zone, visibility = visibility, sizeDp = sizeDp
            ))
        )
    }
}
