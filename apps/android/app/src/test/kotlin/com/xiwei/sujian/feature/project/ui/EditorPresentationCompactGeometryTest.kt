package com.xiwei.sujian.feature.project.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsNotDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #640：窄屏 compact host body geometry 测量不变性测试。
 *
 * Issue 640 核心要求：预热 ready 必须在最终 Editor chrome 的真实 bounds。
 * compact host 在 hidden 和 visible 两种状态必须使用完全相同的 measured body bounds，
 * 仅 top-bar 的 place 随 presentationVisible 切换。
 *
 * 本测调用生产 [CompactEditorMeasureLayout]，验证：
 * - body 的 boundsInRoot 在 visible=true/false 完全相同（不随 presentationVisible 改变）；
 * - top-bar 在 hidden 时 not placed（不显示），visible 时 placed（显示）；
 * - 无 bottom primary NavigationBar、无 ChapterTree Scaffold innerPadding 参与。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w400dp h1000dp")
class EditorPresentationCompactGeometryTest {
    @get:Rule
    val composeRule = createComposeRule()

    private val rootWidthDp = 400
    private val rootHeightDp = 1000
    private val topBarHeightDp = 80

    private fun setContentWithState(): MutableState<Boolean> {
        val visibleState = mutableStateOf(false)
        composeRule.setContent {
            Box(modifier = Modifier.size(rootWidthDp.dp, rootHeightDp.dp)) {
                CompactEditorMeasureLayout(
                    presentationVisible = visibleState.value,
                    topBar = {
                        Box(
                            modifier =
                                Modifier
                                    .fillMaxWidth()
                                    .height(topBarHeightDp.dp)
                                    .testTag(TOP_BAR_TAG),
                        )
                    },
                    body = {
                        Box(
                            modifier = Modifier.fillMaxSize().testTag(BODY_TAG),
                        )
                    },
                )
            }
        }
        return visibleState
    }

    @Test
    fun bodyGeometry_hiddenAndVisible_identicalBounds() {
        val visibleState = setContentWithState()

        // hidden
        visibleState.value = false
        composeRule.waitForIdle()
        val hiddenBody = composeRule.onNodeWithTag(BODY_TAG).fetchSemanticsNode().boundsInRoot

        // visible — 同一组合，仅切换 presentationVisible，不重建
        visibleState.value = true
        composeRule.waitForIdle()
        val visibleBody = composeRule.onNodeWithTag(BODY_TAG).fetchSemanticsNode().boundsInRoot

        assertEquals(
            "body top 必须不随 presentationVisible 改变（预热 ready bounds = 最终 bounds）",
            hiddenBody.top,
            visibleBody.top,
            0.5f,
        )
        assertEquals(
            "body height 必须不随 presentationVisible 改变（预热 ready bounds = 最终 bounds）",
            hiddenBody.height,
            visibleBody.height,
            0.5f,
        )
        assertEquals(
            "body top 必须等于 top-bar 实际测量高度",
            topBarHeightDp.toFloat(),
            visibleBody.top,
            0.5f,
        )
        assertEquals(
            "body height 必须为 root 去掉 top-bar 高度",
            (rootHeightDp - topBarHeightDp).toFloat(),
            visibleBody.height,
            0.5f,
        )
    }

    @Test
    fun topBar_hidden_notPlaced() {
        val visibleState = setContentWithState()
        visibleState.value = false
        composeRule.waitForIdle()
        // top-bar 始终测量但 hidden 时不 place — 不显示。
        composeRule.onNodeWithTag(TOP_BAR_TAG).assertIsNotDisplayed()
    }

    @Test
    fun topBar_visible_placedAtRootTop() {
        val visibleState = setContentWithState()
        visibleState.value = true
        composeRule.waitForIdle()
        composeRule.onNodeWithTag(TOP_BAR_TAG).assertIsDisplayed()
        val topBar = composeRule.onNodeWithTag(TOP_BAR_TAG).fetchSemanticsNode().boundsInRoot
        assertEquals("top-bar visible 时必须 place 在 root 顶部", 0f, topBar.top, 0.5f)
        assertEquals(
            "top-bar 高度必须为其实际测量高度",
            topBarHeightDp.toFloat(),
            topBar.height,
            0.5f,
        )
    }

    @Test
    fun body_hidden_isPlacedAndMeasured() {
        val visibleState = setContentWithState()
        visibleState.value = false
        composeRule.waitForIdle()
        val body = composeRule.onNodeWithTag(BODY_TAG).fetchSemanticsNode().boundsInRoot
        assertEquals(
            "hidden 时 body 仍必须 place（View INVISIBLE 到 VISIBLE，不重建）",
            topBarHeightDp.toFloat(),
            body.top,
            0.5f,
        )
        assertEquals(
            "hidden 时 body height 必须已是 root 去掉 top-bar（预热真实 bounds）",
            (rootHeightDp - topBarHeightDp).toFloat(),
            body.height,
            0.5f,
        )
    }

    private companion object {
        const val TOP_BAR_TAG = "compact_topbar"
        const val BODY_TAG = "compact_body"
    }
}
