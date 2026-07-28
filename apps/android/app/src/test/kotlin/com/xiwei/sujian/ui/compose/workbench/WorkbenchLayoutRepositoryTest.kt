package com.xiwei.sujian.ui.compose.workbench

import android.content.Context
import androidx.test.core.app.ApplicationProvider
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
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
        val result = repository.loadLayout(testKey)
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
