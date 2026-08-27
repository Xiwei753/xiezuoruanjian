package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fitInside
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.HorizontalRuler
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.RectRulers
import androidx.compose.ui.layout.VerticalRuler
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import com.xiwei.sujian.app.presentation.layout.resolveSafeWorkbenchViewport
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.WindowOcclusionDto
import uniffi.writer_core.WindowViewportDto

/**
 * #640 评论 5443789509：wide Workbench 坐标系闭合修复回归测试。
 *
 * 修复把稳定系统 UI（systemBars + displayCutout）的 safe viewport 放到 Workbench planner
 * 输入前，Rust 看到的 (0,0) 是稳定安全工作区左上角，plan 返回后再 `offsetBy` 平移回物理窗口
 * 坐标；IME 留在 Editor 层（EditorPane 用 `WindowInsetsRulers.Ime.current`）动态处理。
 *
 * 本测覆盖两件事：
 * 1. **safe viewport frame 坐标平移**（纯函数 [resolveSafeWorkbenchViewport]，不依赖 Compose rule）：
 *    - raw viewport 800×1000 + insets (10,24,10,48) → safe viewport 780×928，origin=(10,24)；
 *    - raw viewport 含 occlusion (100,200,300,400,separating=true) → 平移后 occlusion
 *      (90,176,290,376,separating=true)；
 *    - insets 超过 viewport 时 width/height 钳到 0（coerceAtLeast(0f)）。
 * 2. **Editor IME ruler 收缩**（Compose rule）：构造固定 root 800×1000 + 自定义 [RectRulers]
 *    注入 bottom=700（模拟 IME 300dp 占底部），`Box.fillMaxSize().fitInside(rectRulers)` 后
 *    inner content `boundsInRoot.bottom ≤ 700`。锁住 Editor 用 `WindowInsetsRulers.Ime.current`
 *    动态收进键盘上方，键盘出现只改变 Editor 实际可用高度，不重算 Rust workbench plan。
 *
 * Robolectric density=1（mdpi），1dp=1px，ruler value 用 px（=dp）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w800dp h1000dp")
class EditorPresentationWideInsetBoundsTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ── 组1：safe viewport frame 坐标平移（纯函数，不依赖 Compose rule） ──

    @Test
    fun resolveSafeWorkbenchViewport_translatesViewportAndOrigin() {
        // raw viewport 800×1000，insets left=10, top=24, right=10, bottom=48
        // → safe viewport width=780, height=928, origin=(10,24)。
        val raw =
            WindowViewportDto(
                widthDp = 800f,
                heightDp = 1000f,
                occlusions = emptyList(),
            )
        val frame =
            resolveSafeWorkbenchViewport(
                rawViewport = raw,
                leftDp = 10f,
                topDp = 24f,
                rightDp = 10f,
                bottomDp = 48f,
            )
        assertEquals("originXDp 必须等于 left inset", 10f, frame.originXDp, 0.001f)
        assertEquals("originYDp 必须等于 top inset", 24f, frame.originYDp, 0.001f)
        assertEquals(
            "safe viewport width = raw - left - right = 780",
            780f,
            frame.viewport.widthDp,
            0.001f,
        )
        assertEquals(
            "safe viewport height = raw - top - bottom = 928",
            928f,
            frame.viewport.heightDp,
            0.001f,
        )
    }

    @Test
    fun resolveSafeWorkbenchViewport_translatesOcclusionsIntoSafeCoordinates() {
        // raw viewport 含 occlusion (100,200,300,400,separating=true)，
        // insets left=10, top=24 → 平移后 occlusion (90,176,290,376,separating=true)。
        val raw =
            WindowViewportDto(
                widthDp = 800f,
                heightDp = 1000f,
                occlusions =
                    listOf(
                        WindowOcclusionDto(
                            leftDp = 100f,
                            topDp = 200f,
                            rightDp = 300f,
                            bottomDp = 400f,
                            separating = true,
                        ),
                    ),
            )
        val frame =
            resolveSafeWorkbenchViewport(
                rawViewport = raw,
                leftDp = 10f,
                topDp = 24f,
                rightDp = 10f,
                bottomDp = 48f,
            )
        assertEquals("平移后 occlusion 数量不变", 1, frame.viewport.occlusions.size)
        val o = frame.viewport.occlusions.first()
        assertEquals("occlusion leftDp = raw.left - left = 90", 90f, o.leftDp, 0.001f)
        assertEquals("occlusion topDp = raw.top - top = 176", 176f, o.topDp, 0.001f)
        assertEquals("occlusion rightDp = raw.right - left = 290", 290f, o.rightDp, 0.001f)
        assertEquals("occlusion bottomDp = raw.bottom - top = 376", 376f, o.bottomDp, 0.001f)
        assertTrue("occlusion separating 标志保留", o.separating)
    }

    @Test
    fun resolveSafeWorkbenchViewport_clampsNegativeDimensionsToZero() {
        // insets 超过 viewport：left=500, right=500 → width = 800-500-500 = -200 → 钳到 0。
        // top=600, bottom=600 → height = 1000-600-600 = -200 → 钳到 0。
        val raw =
            WindowViewportDto(
                widthDp = 800f,
                heightDp = 1000f,
                occlusions = emptyList(),
            )
        val frame =
            resolveSafeWorkbenchViewport(
                rawViewport = raw,
                leftDp = 500f,
                topDp = 600f,
                rightDp = 500f,
                bottomDp = 600f,
            )
        assertEquals("insets 超过 viewport 时 width 钳到 0", 0f, frame.viewport.widthDp, 0.001f)
        assertEquals("insets 超过 viewport 时 height 钳到 0", 0f, frame.viewport.heightDp, 0.001f)
    }

    @Test
    fun resolveSafeWorkbenchViewport_emptyInsetsReturnsIdentityFrame() {
        // insets 全 0 → safe viewport = raw viewport，origin=(0,0)。
        val raw =
            WindowViewportDto(
                widthDp = 800f,
                heightDp = 1000f,
                occlusions = emptyList(),
            )
        val frame =
            resolveSafeWorkbenchViewport(
                rawViewport = raw,
                leftDp = 0f,
                topDp = 0f,
                rightDp = 0f,
                bottomDp = 0f,
            )
        assertEquals(0f, frame.originXDp, 0.001f)
        assertEquals(0f, frame.originYDp, 0.001f)
        assertEquals(800f, frame.viewport.widthDp, 0.001f)
        assertEquals(1000f, frame.viewport.heightDp, 0.001f)
    }

    // ── 组2：Editor IME ruler 收缩（Compose rule） ──

    private val rootWidthDp = 800
    private val rootHeightDp = 1000
    private val imeHeightDp = 300
    private val safeBottomDp = rootHeightDp - imeHeightDp // 700

    @Test
    fun fitInside_imeRuler_shrinksEditorContentAboveKeyboard() {
        // #640 评论 5443789509：Editor 用 WindowInsetsRulers.Ime.current 动态收进键盘上方。
        // 构造固定 root 800×1000 + 自定义 RectRulers 注入 bottom=700（模拟 IME 300dp 占底部），
        // Box.fillMaxSize().fitInside(rectRulers) 后 inner content boundsInRoot.bottom ≤ 700。
        val rectRulers = TestRectRulers()
        composeRule.setContent {
            Box(modifier = Modifier.size(rootWidthDp.dp, rootHeightDp.dp)) {
                Layout(
                    content = {
                        Box(
                            modifier =
                                Modifier
                                    .fillMaxSize()
                                    .fitInside(rectRulers)
                                    .testTag(INNER_TAG),
                        )
                    },
                    measurePolicy = { measurables, constraints ->
                        val placeable = measurables.first().measure(constraints)
                        layout(
                            constraints.maxWidth,
                            constraints.maxHeight,
                            rulers = {
                                // IME ruler region: left=0, top=0, right=rootWidth, bottom=safeBottom
                                // （IME 300dp 占底部，Editor 内容收进 bottom=700 上方）。
                                rectRulers.left.provides(0f)
                                rectRulers.top.provides(0f)
                                rectRulers.right.provides(rootWidthDp.toFloat())
                                rectRulers.bottom.provides(safeBottomDp.toFloat())
                            },
                        ) {
                            placeable.place(0, 0)
                        }
                    },
                )
            }
        }
        composeRule.waitForIdle()
        val inner = composeRule.onNodeWithTag(INNER_TAG).fetchSemanticsNode().boundsInRoot
        assertTrue(
            "fitInside(Ime) 后 inner content top 必须在 IME region 顶部（≈0），实际=$inner",
            inner.top <= 1f,
        )
        assertTrue(
            "fitInside(Ime) 后 inner content bottom 必须收进键盘上方（≤ $safeBottomDp），实际 bottom=${inner.bottom}",
            inner.bottom <= safeBottomDp.toFloat() + 1f,
        )
        assertTrue(
            "fitInside(Ime) 后 inner content height 必须收缩到 IME region 高度（≤ $safeBottomDp），实际 height=${inner.height}",
            inner.height <= safeBottomDp.toFloat() + 1f,
        )
    }

    /**
     * 测试用 [RectRulers] 实现 — 注入已知 left/top/right/bottom ruler，模拟 IME region。
     *
     * 生产用 [androidx.compose.ui.layout.WindowInsetsRulers.Ime.current]（ruler 值由 WindowInsets
     * 系统提供，键盘出现时 bottom 收进键盘上方）；本测用自定义实现可控注入 IME region，
     * 不依赖 Robolectric WindowInsets 模拟。
     */
    private class TestRectRulers(
        override val left: VerticalRuler = VerticalRuler(),
        override val top: HorizontalRuler = HorizontalRuler(),
        override val right: VerticalRuler = VerticalRuler(),
        override val bottom: HorizontalRuler = HorizontalRuler(),
    ) : RectRulers

    private companion object {
        const val INNER_TAG = "wide_inner_content"
    }
}
