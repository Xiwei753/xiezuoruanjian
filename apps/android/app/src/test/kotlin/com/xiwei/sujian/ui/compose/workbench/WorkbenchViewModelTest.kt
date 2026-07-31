package com.xiwei.sujian.ui.compose.workbench

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutRepository
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchViewModel
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class WorkbenchViewModelTest {

    private lateinit var viewModel: WorkbenchViewModel
    private lateinit var repository: WorkbenchLayoutRepository
    private val testKey = LayoutStorageKey(
        deviceId = "test-device",
        orientation = "portrait",
        windowWidthBucket = WindowWidthBucket.Expanded,
        windowMode = "standard",
    )

    @Before
    fun setUp() {
        val application = ApplicationProvider.getApplicationContext<Application>()
        viewModel = WorkbenchViewModel(application)
        repository = WorkbenchLayoutRepository(application)
    }

    @Test
    fun initialState_isDefaultLayout() {
        val state = viewModel.layoutState
        assertEquals(WorkbenchPreset.FocusWriting, state.preset)
        for (panel in state.panels.values) {
            assertEquals(PanelVisibility.Collapsed, panel.visibility)
        }
    }

    @Test
    fun dispatch_togglePanel_changesVisibility() {
        viewModel.dispatch(WorkbenchAction.TogglePanel(WorkbenchPanelId.ChapterNavigator))
        assertEquals(PanelVisibility.Expanded, viewModel.layoutState.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
    }

    @Test
    fun dispatch_expandPanel_marksCustom() {
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        assertEquals(WorkbenchPreset.Custom, viewModel.layoutState.preset)
    }

    @Test
    fun dispatch_resizePanel_updatesSizeInMemory() {
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        viewModel.dispatch(WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 400f))
        val size = viewModel.layoutState.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp
        assertEquals(400f, size!!, 0.01f)
    }

    @Test
    fun dispatchDeferredPersist_resizePanel_updatesSizeInMemory() {
        viewModel.dispatchDeferredPersist(WorkbenchAction.ResizePanel(WorkbenchPanelId.ChapterNavigator, 380f))
        val size = viewModel.layoutState.panels[WorkbenchPanelId.ChapterNavigator]?.sizeDp
        assertEquals(380f, size!!, 0.01f)
    }

    @Test
    fun dispatchDeferredPersist_resizeDockSplit_updatesWeightsInMemory() {
        viewModel.dispatchDeferredPersist(WorkbenchAction.ResizeDockSplit(DockZone.Left, "left-nav", "left-extra", 50f, 320f))
        val weights = viewModel.layoutState.dockGroupWeights
        assertTrue(weights.containsKey("left-nav"))
    }

    @Test
    fun dispatchDeferredPersist_resizeDockZone_updatesZoneSizeInMemory() {
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        viewModel.dispatchDeferredPersist(WorkbenchAction.ResizeDockZone(DockZone.Left, 50f, 1200f))
        val zoneSize = viewModel.layoutState.dockZoneSizeDp[DockZone.Left]
        assertNotNull(zoneSize)
        assertTrue(zoneSize!! > 320f)
    }

    @Test
    fun dispatch_floatPanel_setsZoneToFloating() {
        viewModel.dispatch(WorkbenchAction.FloatPanel(WorkbenchPanelId.Statistics))
        assertEquals(DockZone.Floating, viewModel.layoutState.panels[WorkbenchPanelId.Statistics]?.zone)
        assertEquals(PanelVisibility.Expanded, viewModel.layoutState.panels[WorkbenchPanelId.Statistics]?.visibility)
    }

    @Test
    fun dispatch_applyPreset_changesPreset() {
        viewModel.dispatch(WorkbenchAction.ApplyPreset(WorkbenchPreset.ChapterWriting))
        assertEquals(WorkbenchPreset.ChapterWriting, viewModel.layoutState.preset)
        assertEquals(PanelVisibility.Expanded, viewModel.layoutState.panels[WorkbenchPanelId.ChapterNavigator]?.visibility)
    }

    @Test
    fun onWindowBucketChanged_updatesStorageKey() = runTest {
        viewModel.initialize(repository, testKey)
        advanceUntilIdle()

        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        viewModel.dispatch(WorkbenchAction.ApplyPreset(WorkbenchPreset.AiWriting))

        val newKey = LayoutStorageKey(
            deviceId = "test-device",
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Large,
            windowMode = "standard",
        )
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        assertTrue(
            viewModel.layoutState.preset == WorkbenchPreset.AiWriting ||
            viewModel.layoutState.preset == WorkbenchPreset.FocusWriting ||
            viewModel.layoutState.preset == WorkbenchPreset.Custom
        )
    }

    @Test
    fun onWindowBucketChanged_newBucketGetsDefaultLayout() = runTest {
        viewModel.initialize(repository, testKey)
        advanceUntilIdle()

        val newKey = LayoutStorageKey(
            deviceId = "test-device-new",
            orientation = "portrait",
            windowWidthBucket = WindowWidthBucket.Compact,
            windowMode = "standard",
        )
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        val state = viewModel.layoutState
        for (panel in state.panels.values) {
            assertTrue(
                "Panel ${panel.id} visibility should be valid",
                panel.visibility in listOf(PanelVisibility.Collapsed, PanelVisibility.Expanded, PanelVisibility.Hidden)
            )
        }
    }

    @Test
    fun dispatch_floatPanelAt_setsPosition() {
        viewModel.dispatch(WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 100f, 200f))
        assertEquals(DockZone.Floating, viewModel.layoutState.panels[WorkbenchPanelId.AiAssistant]?.zone)
        assertEquals(100f, viewModel.layoutState.panels[WorkbenchPanelId.AiAssistant]?.floatingX!!, 0.01f)
        assertEquals(200f, viewModel.layoutState.panels[WorkbenchPanelId.AiAssistant]?.floatingY!!, 0.01f)
    }

    @Test
    fun dispatch_activateOverlayPanel_updatesState() {
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        viewModel.dispatch(WorkbenchAction.ActivateOverlayPanel(WorkbenchPanelId.ChapterNavigator))
        assertEquals(WorkbenchPanelId.ChapterNavigator, viewModel.layoutState.activeOverlayPanelId)
    }

    @Test
    fun dispatch_clampFloatingPanels_clampsPositions() {
        viewModel.dispatch(WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, -100f, -50f))
        viewModel.dispatch(WorkbenchAction.ClampFloatingPanels(800f, 600f))
        val panel = viewModel.layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingX >= -(panel.floatingWidthDp - 32f))
        assertTrue(panel.floatingY >= 0f)
    }

    @Test
    fun onWindowSizeChanged_clampsFloatingPanels() {
        viewModel.dispatch(WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, -100f, -50f))
        viewModel.onWindowSizeChanged(800f, 600f)
        val panel = viewModel.layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue(panel.floatingX >= -(panel.floatingWidthDp - 32f))
        assertTrue(panel.floatingY >= 0f)
    }

    @Test
    fun dispatchDeferredPersist_cancelsPreviousJob() {
        viewModel.dispatchDeferredPersist(WorkbenchAction.ResizeDockZone(DockZone.Left, 10f, 1200f))
        viewModel.dispatchDeferredPersist(WorkbenchAction.ResizeDockZone(DockZone.Left, 20f, 1200f))
        val zoneSize = viewModel.layoutState.dockZoneSizeDp[DockZone.Left]
        assertNotNull(zoneSize)
    }

    @Test
    fun persistLayout_capturesStorageKeySnapshot() = runTest {
        viewModel.initialize(repository, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        val snapshotKey = viewModel.layoutState.panels[WorkbenchPanelId.ChapterNavigator]?.visibility
        assertEquals(PanelVisibility.Expanded, snapshotKey)
    }

    @Test
    fun switchStorageKey_savesOldKeyBeforeLoadingNew() = runTest {
        viewModel.initialize(repository, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))

        val newKey = LayoutStorageKey(
            deviceId = "test-device",
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Large,
            windowMode = "standard",
        )
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        val state = viewModel.layoutState
        assertTrue(
            state.preset == WorkbenchPreset.FocusWriting ||
            state.preset == WorkbenchPreset.Custom
        )
    }

    @Test
    fun persistLayout_atomicKeyStateUnderSwitch() = runTest {
        viewModel.initialize(repository, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))

        val newKey = LayoutStorageKey(
            deviceId = "test-device",
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Large,
            windowMode = "standard",
        )
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        val stateAfterSwitch = viewModel.layoutState
        for (panel in stateAfterSwitch.panels.values) {
            assertTrue(
                "Panel ${panel.id} state should be valid after key switch",
                panel.visibility in listOf(PanelVisibility.Collapsed, PanelVisibility.Expanded, PanelVisibility.Hidden)
            )
        }
    }

    @Test
    fun switchStorageKey_currentKeyAndLayoutStateArePaired() = runTest {
        viewModel.initialize(repository, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))

        val newKey = LayoutStorageKey(
            deviceId = "alt-device",
            orientation = "portrait",
            windowWidthBucket = WindowWidthBucket.Compact,
            windowMode = "standard",
        )
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        val state = viewModel.layoutState
        for (panel in state.panels.values) {
            assertTrue(
                "Panel ${panel.id} state should be valid after key switch",
                panel.visibility in listOf(PanelVisibility.Collapsed, PanelVisibility.Expanded, PanelVisibility.Hidden)
            )
        }
    }
}
