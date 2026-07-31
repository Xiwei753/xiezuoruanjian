package com.xiwei.sujian.ui.compose.workbench

import android.app.Application
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.PanelVisibility
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchLayoutState
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPreset
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutStore
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchViewModel
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
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
    private val testKey = LayoutStorageKey(
        deviceId = "test-device",
        orientation = "portrait",
        windowWidthBucket = WindowWidthBucket.Expanded,
        windowMode = "standard",
    )
    private val newKey = LayoutStorageKey(
        deviceId = "test-device",
        orientation = "landscape",
        windowWidthBucket = WindowWidthBucket.Large,
        windowMode = "standard",
    )

    @Before
    fun setUp() {
        val application = ApplicationProvider.getApplicationContext<Application>()
        viewModel = WorkbenchViewModel(application)
    }

    private fun runViewModelTest(block: suspend TestScope.() -> Unit) = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        try {
            block()
        } finally {
            Dispatchers.resetMain()
        }
    }

    private fun expandPanels(vararg ids: WorkbenchPanelId): WorkbenchLayoutState {
        var state = WorkbenchReducer.computeDefaultLayout()
        for (id in ids) {
            state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(id))
        }
        return state
    }

    private fun List<Pair<LayoutStorageKey, WorkbenchLayoutState>>.savesFor(key: LayoutStorageKey) =
        filter { it.first == key }.map { it.second }

    private fun WorkbenchLayoutState.hasExpanded(id: WorkbenchPanelId) =
        panels[id]?.visibility == PanelVisibility.Expanded

    private class RecordingStore(
        private val layouts: Map<LayoutStorageKey, WorkbenchLayoutState>,
    ) : WorkbenchLayoutStore {

        val saves = mutableListOf<Pair<LayoutStorageKey, WorkbenchLayoutState>>()
        val events = mutableListOf<String>()
        private var gatedKey: LayoutStorageKey? = null
        private var gate: CompletableDeferred<Unit>? = null

        fun gateLoad(key: LayoutStorageKey) {
            gatedKey = key
            gate = CompletableDeferred()
        }

        fun releaseGate() {
            gate?.complete(Unit)
        }

        override suspend fun saveLayout(key: LayoutStorageKey, state: WorkbenchLayoutState) {
            events += "save:${key.toStorageKey()}"
            saves += key to state
        }

        override suspend fun loadLayout(key: LayoutStorageKey): WorkbenchLayoutState? {
            events += "load:${key.toStorageKey()}"
            if (key == gatedKey) {
                gate?.await()
            }
            return layouts[key]
        }
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
    fun onWindowBucketChanged_updatesStorageKey() = runViewModelTest {
        val store = RecordingStore(mapOf(newKey to expandPanels(WorkbenchPanelId.AiAssistant)))
        viewModel.initialize(store, testKey)
        advanceUntilIdle()

        viewModel.dispatch(WorkbenchAction.ApplyPreset(WorkbenchPreset.AiWriting))
        advanceUntilIdle()
        assertTrue(
            "old key must receive the AiWriting layout before the switch",
            store.saves.savesFor(testKey).isNotEmpty() && store.saves.savesFor(testKey).all { it.preset == WorkbenchPreset.AiWriting }
        )

        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        assertEquals(WorkbenchPreset.Custom, viewModel.layoutState.preset)
        assertTrue(store.saves.savesFor(newKey).none { it.preset == WorkbenchPreset.AiWriting })
    }

    @Test
    fun onWindowBucketChanged_newBucketGetsDefaultLayout() = runViewModelTest {
        val store = RecordingStore(emptyMap())
        viewModel.initialize(store, testKey)
        advanceUntilIdle()

        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        for (panel in viewModel.layoutState.panels.values) {
            assertEquals(
                "Panel ${panel.id} should be collapsed after switching to an empty bucket",
                PanelVisibility.Collapsed,
                panel.visibility
            )
        }
        assertTrue("nothing may be written to the new key before its layout is loaded", store.saves.savesFor(newKey).isEmpty())
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
    fun persistLayout_capturesStorageKeySnapshot() = runViewModelTest {
        val store = RecordingStore(emptyMap())
        viewModel.initialize(store, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        advanceUntilIdle()

        val saved = store.saves.savesFor(testKey)
        assertTrue("persist must record an actual repository write", saved.isNotEmpty())
        assertTrue("saved snapshot must contain the expanded ChapterNavigator", saved.last().hasExpanded(WorkbenchPanelId.ChapterNavigator))
    }

    @Test
    fun switchStorageKey_savesOldKeyBeforeLoadingNew() = runViewModelTest {
        val store = RecordingStore(mapOf(newKey to expandPanels(WorkbenchPanelId.AiAssistant)))
        viewModel.initialize(store, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        advanceUntilIdle()

        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        val saveOldIdx = store.events.indexOfFirst { it == "save:${testKey.toStorageKey()}" }
        val loadNewIdx = store.events.indexOfFirst { it == "load:${newKey.toStorageKey()}" }
        assertTrue("old key must be saved before the new key is loaded", saveOldIdx >= 0 && loadNewIdx > saveOldIdx)
        assertTrue("old key saves must carry the old layout", store.saves.savesFor(testKey).all { it.hasExpanded(WorkbenchPanelId.ChapterNavigator) })
        assertTrue("new key layout must come from the store", viewModel.layoutState.hasExpanded(WorkbenchPanelId.AiAssistant))
        assertTrue("new key saves must never carry the old layout", store.saves.savesFor(newKey).all { it.hasExpanded(WorkbenchPanelId.AiAssistant) })
    }

    @Test
    fun persistLayout_atomicKeyStateUnderSwitch() = runViewModelTest {
        val store = RecordingStore(mapOf(newKey to expandPanels(WorkbenchPanelId.AiAssistant)))
        store.gateLoad(newKey)
        viewModel.initialize(store, testKey)
        advanceUntilIdle()

        viewModel.onWindowBucketChanged(newKey)
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        advanceUntilIdle()

        assertTrue("switch is still blocked on load, in-memory layout must be the old one", viewModel.layoutState.hasExpanded(WorkbenchPanelId.ChapterNavigator))
        assertTrue(store.saves.savesFor(newKey).isEmpty())

        store.releaseGate()
        advanceUntilIdle()

        assertTrue(
            "every save to the new key must carry the new layout, never the stale old one",
            store.saves.savesFor(newKey).isNotEmpty() && store.saves.savesFor(newKey).all { it.hasExpanded(WorkbenchPanelId.AiAssistant) }
        )
        assertTrue("old key saves must keep the old layout", store.saves.savesFor(testKey).all { it.hasExpanded(WorkbenchPanelId.ChapterNavigator) })
        assertTrue(viewModel.layoutState.hasExpanded(WorkbenchPanelId.AiAssistant))
    }

    @Test
    fun switchStorageKey_currentKeyAndLayoutStateArePaired() = runViewModelTest {
        val store = RecordingStore(mapOf(newKey to expandPanels(WorkbenchPanelId.AiAssistant)))
        store.gateLoad(newKey)
        viewModel.initialize(store, testKey)
        advanceUntilIdle()
        viewModel.dispatch(WorkbenchAction.ExpandPanel(WorkbenchPanelId.ChapterNavigator))
        viewModel.onWindowBucketChanged(newKey)
        advanceUntilIdle()

        assertTrue(
            "old key must be flushed before the new key load starts",
            store.events.indexOfFirst { it == "save:${testKey.toStorageKey()}" } <
                store.events.indexOfFirst { it == "load:${newKey.toStorageKey()}" }
        )
        assertTrue(store.saves.savesFor(newKey).isEmpty())

        store.releaseGate()
        advanceUntilIdle()

        assertTrue(viewModel.layoutState.hasExpanded(WorkbenchPanelId.AiAssistant))
        assertTrue(store.saves.savesFor(newKey).all { it.hasExpanded(WorkbenchPanelId.AiAssistant) })
    }
}
