package com.xiwei.sujian.ui.compose.workbench

import android.content.pm.ActivityInfo
import android.util.Log
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.v2.createAndroidComposeRule
import androidx.compose.ui.unit.dp
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.xiwei.sujian.ui.OrientationTestActivity
import com.xiwei.sujian.ui.compose.workbench.component.SujianWorkbench
import com.xiwei.sujian.ui.compose.workbench.model.DockZone
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchAction
import com.xiwei.sujian.ui.compose.workbench.model.WorkbenchPanelId
import com.xiwei.sujian.ui.compose.workbench.state.LayoutStorageKey
import com.xiwei.sujian.ui.compose.workbench.state.WindowWidthBucket
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchLayoutRepository
import com.xiwei.sujian.ui.compose.workbench.state.WorkbenchReducer
import kotlinx.coroutines.runBlocking
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
    val composeTestRule = createAndroidComposeRule<OrientationTestActivity>()

    private val defaultState = WorkbenchReducer.computeDefaultLayout()
    private lateinit var repository: WorkbenchLayoutRepository
    private lateinit var testKey: LayoutStorageKey

    @Before
    fun setUp() {
        val context = composeTestRule.activity.applicationContext
        repository = WorkbenchLayoutRepository(context)
        val uniqueId = "test-orient-${System.nanoTime()}"
        testKey = LayoutStorageKey(
            deviceId = uniqueId,
            orientation = "landscape",
            windowWidthBucket = WindowWidthBucket.Large,
            windowMode = "standard",
        )
    }

    @Test
    fun activityOrientation_landscapeToPortrait_clampsFloatingPanelBounds() {
        val activity = composeTestRule.activity
        var layoutState by mutableStateOf(defaultState)
        var reportedWidth by mutableStateOf(0f)
        var reportedHeight by mutableStateOf(0f)

        val originalOrientation = activity.requestedOrientation
        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE

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

        composeTestRule.waitUntil(10_000L) {
            reportedWidth > 0f && reportedHeight > 0f && reportedWidth > reportedHeight
        }

        val landscapeWidth = reportedWidth
        val landscapeHeight = reportedHeight
        Log.i("OrientationTest", "Landscape measured: width=$landscapeWidth height=$landscapeHeight")
        assertTrue("Landscape: width should exceed height, got width=$landscapeWidth height=$landscapeHeight", landscapeWidth > landscapeHeight)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.FloatPanelAt(WorkbenchPanelId.AiAssistant, 900f, 600f))
        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ResizeFloatingPanel(WorkbenchPanelId.AiAssistant, 500f, 400f))
        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(landscapeWidth, landscapeHeight))
        composeTestRule.waitForIdle()

        val landscapePanel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Landscape: X within bounds", landscapePanel.floatingX <= landscapeWidth)
        assertTrue("Landscape: Width clamped", landscapePanel.floatingWidthDp <= landscapeWidth)

        activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_PORTRAIT

        composeTestRule.waitUntil(10_000L) {
            reportedWidth > 0f && reportedHeight > 0f && reportedHeight > reportedWidth
        }

        val portraitWidth = reportedWidth
        val portraitHeight = reportedHeight
        Log.i("OrientationTest", "Portrait measured: width=$portraitWidth height=$portraitHeight")
        assertTrue("Portrait: height should exceed width, got width=$portraitWidth height=$portraitHeight", portraitHeight > portraitWidth)

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ClampFloatingPanels(portraitWidth, portraitHeight))
        composeTestRule.waitForIdle()

        val portraitPanel = layoutState.panels[WorkbenchPanelId.AiAssistant]!!
        assertTrue("Portrait: X within bounds", portraitPanel.floatingX <= portraitWidth)
        assertTrue("Portrait: Width clamped", portraitPanel.floatingWidthDp <= portraitWidth)

        activity.requestedOrientation = originalOrientation
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

        val context = composeTestRule.activity.applicationContext
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
    fun windowResize_narrowToWide_onWindowSizeChanged_reportsWidthAndHeightChange() {
        var layoutState by mutableStateOf(defaultState)
        var containerWidth by mutableStateOf(600.dp)
        var containerHeight by mutableStateOf(400.dp)
        var reportedWidth by mutableStateOf(0f)
        var reportedHeight by mutableStateOf(0f)

        composeTestRule.setContent {
            Box(Modifier.requiredSize(containerWidth, containerHeight)) {
                SujianWorkbench(
                    layoutState = layoutState,
                    onAction = { layoutState = WorkbenchReducer.reduce(layoutState, it) },
                    onWindowSizeChanged = { w, h -> reportedWidth = w; reportedHeight = h },
                    modifier = Modifier.fillMaxSize(),
                    editorContent = { Box(Modifier.size(100.dp)) },
                    panelContent = { _ -> Box(Modifier.size(50.dp)) },
                )
            }
        }

        composeTestRule.waitForIdle()

        layoutState = WorkbenchReducer.reduce(layoutState, WorkbenchAction.ExpandPanel(WorkbenchPanelId.AiAssistant))
        composeTestRule.waitForIdle()

        val narrowReportedWidth = reportedWidth
        val narrowReportedHeight = reportedHeight
        assertTrue("Narrow container should report width > 0", narrowReportedWidth > 0f)
        assertTrue("Narrow container should report height > 0", narrowReportedHeight > 0f)

        containerWidth = 1400.dp
        containerHeight = 900.dp
        composeTestRule.waitForIdle()

        val wideReportedWidth = reportedWidth
        val wideReportedHeight = reportedHeight
        assertTrue("Wide container should report larger width: wide=$wideReportedWidth > narrow=$narrowReportedWidth", wideReportedWidth > narrowReportedWidth)
        assertTrue("Wide container should report larger height: wide=$wideReportedHeight > narrow=$narrowReportedHeight", wideReportedHeight > narrowReportedHeight)

        assertTrue("onWindowSizeChanged must report narrow width ≈600, got $narrowReportedWidth", kotlin.math.abs(narrowReportedWidth - 600f) < 1f)
        assertTrue("onWindowSizeChanged must report narrow height ≈400, got $narrowReportedHeight", kotlin.math.abs(narrowReportedHeight - 400f) < 1f)
        assertTrue("onWindowSizeChanged must report wide width ≈1400, got $wideReportedWidth", kotlin.math.abs(wideReportedWidth - 1400f) < 1f)
        assertTrue("onWindowSizeChanged must report wide height ≈900, got $wideReportedHeight", kotlin.math.abs(wideReportedHeight - 900f) < 1f)
    }
}
