package com.xiwei.sujian.ui.compose.workbench

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.xiwei.sujian.designsystem.testing.SujianSemanticIds
import com.xiwei.sujian.ui.compose.workbench.component.SujianWorkbench
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutRepository
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class WorkbenchOrientationInstrumentedTest {

    @get:Rule
    val composeTestRule = createComposeRule()

    private val defaultState = WorkbenchReducer.computeDefaultLayout()
    private lateinit var repository: WorkbenchLayoutRepository
    private lateinit var testKey: LayoutStorageKey

    @Before
    fun setUp() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        repository = WorkbenchLayoutRepository(context)
        testKey = LayoutStorageKey(
            deviceId = "test-orientation",
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Large,
            windowMode = "standard",
        )
    }

    @Test
    fun activityOrientation_landscapeToPortrait_clampsFloatingPanelBounds() {
        var layoutState by mutableStateOf(defaultState)
        var reportedWidth by mutableStateOf(0f)
        var reportedHeight by mutableStateOf(0f)

        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { w, h -> reportedWidth = w; reportedHeight = h },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 900f, 600f))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 500f, 400f))
        composeTestRule.waitForIdle()

        val landscapeWidth = reportedWidth
        val landscapeHeight = reportedHeight
        assertTrue("Should have reported window size", landscapeWidth > 0f && landscapeHeight > 0f)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(landscapeWidth, landscapeHeight))
        composeTestRule.waitForIdle()

        val panel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Landscape: X within bounds", panel.floatingX <= landscapeWidth)
        assertTrue("Landscape: Width clamped", panel.floatingWidthDp <= landscapeWidth)

        val portraitWidth = landscapeHeight
        val portraitHeight = landscapeWidth
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(portraitWidth, portraitHeight))
        composeTestRule.waitForIdle()

        val portraitPanel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Portrait: X within bounds", portraitPanel.floatingX <= portraitWidth)
        assertTrue("Portrait: Width clamped", portraitPanel.floatingWidthDp <= portraitWidth)
    }

    @Test
    fun dataStore_saveAndLoad_roundTrip_preservesLayoutState() {
        var state = defaultState
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.DocumentOutline))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.FloatPanel(WorkbenchPanelId.Search))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.MoveFloatingPanel(WorkbenchPanelId.Search, 200f, 150f))

        runBlocking {
            repository.saveLayout(testKey, state)
        }

        val loaded = runBlocking {
            repository.loadLayout(testKey)
        }

        assertNotNull("Loaded state should not be null", loaded)
        val restored = loaded!!
        assertEquals(DockZone.Floating, restored.panels[WorkbenchPanelId.Search]!!.zone)
        assertEquals(200f, restored.panels[WorkbenchPanelId.Search]!!.floatingX, 0.01f)
        assertEquals(150f, restored.panels[WorkbenchPanelId.Search]!!.floatingY, 0.01f)
        assertEquals(state.panels[WorkbenchPanelId.AiAssistant]!!.visibility, restored.panels[WorkbenchPanelId.AiAssistant]!!.visibility)
        assertEquals(state.panels[WorkbenchPanelId.DocumentOutline]!!.visibility, restored.panels[WorkbenchPanelId.DocumentOutline]!!.visibility)
    }

    @Test
    fun dataStore_saveAndLoad_newRepositoryInstance_preservesLayoutState() {
        var state = defaultState
        state = WorkbenchReducer.reduce(state, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        state = WorkbenchReducer.reduce(state, WorkbenchAction.DockPanelAsNewGroup(WorkbenchPanelId.Search, DockZone.Right, 0))

        runBlocking {
            repository.saveLayout(testKey, state)
        }

        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val newRepository = WorkbenchLayoutRepository(context)

        val loaded = runBlocking {
            newRepository.loadLayout(testKey)
        }

        assertNotNull("Loaded state from new repository should not be null", loaded)
        val restored = loaded!!
        assertEquals(DockZone.Right, restored.panels[WorkbenchPanelId.Search]!!.zone)
        assertEquals(state.dockZoneSizeDp, restored.dockZoneSizeDp)
    }

    @Test
    fun windowResize_narrowToWide_dockZoneSizeAdapts() {
        var layoutState by mutableStateOf(defaultState)

        composeTestRule.setContent {
            SujianWorkbench(
                layoutState = layoutState,
                onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                onWindowSizeChanged = { _, _ -> },
                modifier = Modifier.fillMaxSize(),
                editorContent = { Box(Modifier.size(100.dp)) },
                panelContent = { _ -> Box(Modifier.size(50.dp)) },
            )
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()

        val narrowWidth = 600f
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(narrowWidth, 400f))
        composeTestRule.waitForIdle()

        val rightSizeNarrow = layoutState.dockZoneSizeDp[DockZone.Right]
        assertNotNull("Right zone should have a size in narrow mode", rightSizeNarrow)

        val wideWidth = 1400f
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(wideWidth, 900f))
        composeTestRule.waitForIdle()

        val rightSizeWide = layoutState.dockZoneSizeDp[DockZone.Right]
        assertNotNull("Right zone should have a size in wide mode", rightSizeWide)
        assertTrue("Right zone size should be within valid range", rightSizeWide!! >= WorkbenchReducer.SIDE_PANEL_MIN_DP && rightSizeWide <= WorkbenchReducer.SIDE_PANEL_MAX_DP)
    }
}
