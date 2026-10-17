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
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #640 评论 5443789509：#641 wide Workbench 稳定系统 UI / IME 坐标回归测试。
 *
 * 布局已由 Android 本端 Compose 决定，外层 Scaffold 用 `WindowInsets.safeDrawing`
 * 处理稳定系统 UI；IME 留在 Editor 层动态处理。本测覆盖 Editor IME ruler 收缩：
 * 构造固定 root 800×1000 + 自定义 [RectRulers] 注入 bottom=700（模拟 IME 300dp 占底部），
 * `Box.fillMaxSize().fitInside(rectRulers)` 后 inner content `boundsInRoot.bottom ≤ 700`。
 * 锁住 Editor 用 `WindowInsetsRulers.Ime.current` 动态收进键盘上方，键盘出现只改变
 * Editor 实际可用高度，不重算工作区布局。
 *
 * Robolectric density=1（mdpi），1dp=1px，ruler value 用 px（=dp）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w800dp h1000dp")
class EditorPresentationWideInsetBoundsTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ── Editor IME ruler 收缩（Compose rule） ──

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
