package com.xiwei.sujian.app.presentation

import androidx.compose.material3.adaptive.Posture
import androidx.compose.material3.adaptive.WindowAdaptiveInfo
import androidx.window.core.layout.WindowSizeClass
import androidx.window.core.layout.WindowWidthSizeClass
import com.xiwei.sujian.core.platform.api.FoldOrientation
import com.xiwei.sujian.core.platform.api.FoldPosture
import com.xiwei.sujian.core.platform.api.OcclusionType
import com.xiwei.sujian.core.platform.api.PointerKind
import com.xiwei.sujian.core.platform.window.AospFoldFeatureInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.writer_core.PointerClassDto
import uniffi.writer_core.WorkspacePaneModeDto

/**
 * #610：AndroidAdaptiveLayoutPolicy 的纯函数部分 —
 * Android 平台把窗口能力判断好（WindowSizeClass/折叠/指针/键盘），
 * 再交给 Core presentation contract；断点与 dp 不在 Core。
 */
class AndroidAdaptiveLayoutPolicyTest {
    private fun adaptiveInfo(width: WindowWidthSizeClass): WindowAdaptiveInfo =
        WindowAdaptiveInfo(
            windowSizeClass = windowSizeClassFor(width),
            windowPosture = Posture(),
        )

    /** 按 Material3 断点构造 WindowSizeClass（Android 平台决策）。 */
    private fun windowSizeClassFor(width: WindowWidthSizeClass): WindowSizeClass =
        when (width) {
            WindowWidthSizeClass.COMPACT -> WindowSizeClass.compute(360f, 800f)
            WindowWidthSizeClass.MEDIUM -> WindowSizeClass.compute(700f, 800f)
            else -> WindowSizeClass.compute(1000f, 800f)
        }

    // ---- 窗口能力 → Core WindowCapabilities ----

    @Test
    fun `compact width maps to single pane`() {
        val caps =
            buildCapabilities(
                windowAdaptiveInfo = adaptiveInfo(WindowWidthSizeClass.COMPACT),
                foldingFeatures = emptyList(),
                activePointerKind = PointerKind.Touch,
                keyboardVisible = false,
            )
        assertEquals(1, caps.availablePaneCount.toInt())
        assertFalse(caps.hasSeparatingFold)
    }

    @Test
    fun `medium width maps to two panes`() {
        val caps =
            buildCapabilities(
                windowAdaptiveInfo = adaptiveInfo(WindowWidthSizeClass.MEDIUM),
                foldingFeatures = emptyList(),
                activePointerKind = PointerKind.Touch,
                keyboardVisible = false,
            )
        assertEquals(2, caps.availablePaneCount.toInt())
    }

    @Test
    fun `expanded width maps to three panes`() {
        val caps =
            buildCapabilities(
                windowAdaptiveInfo = adaptiveInfo(WindowWidthSizeClass.EXPANDED),
                foldingFeatures = emptyList(),
                activePointerKind = PointerKind.Touch,
                keyboardVisible = false,
            )
        assertEquals(3, caps.availablePaneCount.toInt())
    }

    @Test
    fun `separating fold is forwarded to Core`() {
        val caps =
            buildCapabilities(
                windowAdaptiveInfo = adaptiveInfo(WindowWidthSizeClass.EXPANDED),
                foldingFeatures =
                    listOf(
                        AospFoldFeatureInfo(
                            state = FoldPosture.HalfOpened,
                            orientation = FoldOrientation.Vertical,
                            isSeparating = true,
                            occlusionType = OcclusionType.None,
                            boundsLeft = 500,
                            boundsTop = 0,
                            boundsRight = 520,
                            boundsBottom = 800,
                        ),
                    ),
                activePointerKind = PointerKind.Touch,
                keyboardVisible = false,
            )
        assertTrue(caps.hasSeparatingFold)
    }

    @Test
    fun `pointer kinds map to pointer class`() {
        assertEquals(
            PointerClassDto.MOUSE,
            buildCapabilities(
                adaptiveInfo(WindowWidthSizeClass.COMPACT),
                emptyList(),
                PointerKind.Mouse,
                false,
            ).pointerClass,
        )
        assertEquals(
            PointerClassDto.MOUSE,
            buildCapabilities(
                adaptiveInfo(WindowWidthSizeClass.COMPACT),
                emptyList(),
                PointerKind.Trackpad,
                false,
            ).pointerClass,
        )
        assertEquals(
            PointerClassDto.STYLUS,
            buildCapabilities(
                adaptiveInfo(WindowWidthSizeClass.COMPACT),
                emptyList(),
                PointerKind.Stylus,
                false,
            ).pointerClass,
        )
        assertEquals(
            PointerClassDto.TOUCH,
            buildCapabilities(
                adaptiveInfo(WindowWidthSizeClass.COMPACT),
                emptyList(),
                PointerKind.Touch,
                false,
            ).pointerClass,
        )
    }

    @Test
    fun `keyboard visibility is forwarded to Core`() {
        assertTrue(
            buildCapabilities(
                adaptiveInfo(WindowWidthSizeClass.COMPACT),
                emptyList(),
                PointerKind.Touch,
                keyboardVisible = true,
            ).keyboardVisible,
        )
    }

    // ---- Core WorkspacePaneMode → Material3 partitions ----

    @Test
    fun `workspace pane mode maps to max horizontal partitions`() {
        assertEquals(1, maxHorizontalPartitionsFor(WorkspacePaneModeDto.SINGLE_PANE))
        assertEquals(2, maxHorizontalPartitionsFor(WorkspacePaneModeDto.LIST_DETAIL))
        assertEquals(3, maxHorizontalPartitionsFor(WorkspacePaneModeDto.THREE_PANE))
    }
}
