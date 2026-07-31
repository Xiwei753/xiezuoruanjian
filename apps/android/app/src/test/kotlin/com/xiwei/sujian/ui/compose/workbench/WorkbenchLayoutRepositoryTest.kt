package com.xiwei.sujian.ui.compose.workbench

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.ui.compose.workbench.model.DockGroupMeta
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.LAYOUT_SNAPSHOT_VERSION
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutRepository
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
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
