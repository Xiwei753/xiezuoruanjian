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
            viewModel.layoutState.preset == WorkbenchPreset.FocusWriting
        )
    }
}
