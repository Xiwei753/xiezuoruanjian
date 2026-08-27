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
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #640 评论 5442422507：wide slot `fitInside(WindowInsetsRulers.SafeDrawing.current)` 真实 bounds 收缩测试。
 *
 * 根因：wide host 不套 compact inset padding（Rust plan 按整窗尺寸算 slot bounds，host 先
 * windowInsetsPadding 缩小会让 measureAndPlaceWorkbench 把子项撑回原尺寸、伸进 IME/system bar）。
 * wide slot 内容改用 `fitInside(SafeDrawing.current)` 在 slot 内部把内容 reposition 到 safe region
 * （systemBars + displayCutout + IME），用绝对窗口位置绕过 ancestor consumption。
 *
 * 本测不"自己证明自己"：构造固定尺寸 root（模拟 Rust Editor rect）+ 自定义 [RectRulers] 注入已知
 * safe region（bottom 收进 IME 上方），断言 `fitInside` 真实改变内容 boundsInRoot。对照不加 fitInside
 * 的内容 bounds 不收缩。锁住 fitInside 真实收缩内容到 safe region，防止 Rust rect 绝对坐标把内容撑进 IME。
 *
 * - root: 800dp x 1000dp（模拟 Rust Editor rect，整窗坐标）；
 * - safe region: left=0, top=0, right=800, bottom=700（IME 300dp 占底部）；
 * - fitInside 后 inner content boundsInRoot.bottom ≤ 700（收进 IME 上方）；
 * - 对照（不加 fitInside）boundsInRoot.bottom ≈ 1000（未收进，撑满 root）。
 *
 * Robolectric density=1（mdpi），1dp=1px，ruler value 用 px（=dp）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w800dp h1000dp")
class EditorPresentationWideInsetBoundsTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val rootWidthDp = 800
    private val rootHeightDp = 1000
    private val imeHeightDp = 300
    private val safeBottomDp = rootHeightDp - imeHeightDp // 700

    @Test
    fun fitInside_shrinksContentBoundsIntoSafeRegion() {
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
                                // safe region: left=0, top=0, right=rootWidth, bottom=safeBottom
                                // Ruler.provides(value) 是 RulerScope 上的扩展函数，绑定 ruler 到 layout coordinates。
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
            "fitInside 后 inner content top 必须在 safe region 顶部（≈0），实际=$inner",
            inner.top <= 1f,
        )
        assertTrue(
            "fitInside 后 inner content bottom 必须收进 IME 上方（≤ $safeBottomDp），实际 bottom=${inner.bottom}",
            inner.bottom <= safeBottomDp.toFloat() + 1f,
        )
        assertTrue(
            "fitInside 后 inner content height 必须收缩到 safe region 高度（≤ $safeBottomDp），实际 height=${inner.height}",
            inner.height <= safeBottomDp.toFloat() + 1f,
        )
    }

    @Test
    fun withoutFitInside_contentFillsRootAndExtendsIntoImeRegion() {
        // 对照：不加 fitInside，内容 fillMaxSize 撑满 root，bottom ≈ rootHeight（未收进 IME 上方）。
        composeRule.setContent {
            Box(modifier = Modifier.size(rootWidthDp.dp, rootHeightDp.dp)) {
                Box(
                    modifier =
                        Modifier
                            .fillMaxSize()
                            .testTag(INNER_TAG),
                )
            }
        }
        composeRule.waitForIdle()
        val inner = composeRule.onNodeWithTag(INNER_TAG).fetchSemanticsNode().boundsInRoot
        assertEquals(
            "不加 fitInside 时 inner content bottom 必须撑满 root（≈ $rootHeightDp），未收进 safe region",
            rootHeightDp.toFloat(),
            inner.bottom,
            1f,
        )
        assertEquals(
            "不加 fitInside 时 inner content height 必须撑满 root",
            rootHeightDp.toFloat(),
            inner.height,
            1f,
        )
    }

    /**
     * 测试用 [RectRulers] 实现 — 注入已知 left/top/right/bottom ruler，模拟 SafeDrawing region。
     *
     * 生产用 [WindowInsetsRulers.SafeDrawing.current]（ruler 值由 WindowInsets 系统提供）；
     * 本测用自定义实现可控注入 safe region，不依赖 Robolectric WindowInsets 模拟。
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
