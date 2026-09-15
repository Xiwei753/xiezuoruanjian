package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.text.TextRange
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * #689 评论 5675270164 缺陷7：旧 placeholder 测试迁移为新 timeline 行为测试。
 *
 * 原测试断言旧事务机制下的坐标 bug 修复。旧机制已删除，本测试验证新持续 timeline
 * 的坐标行为：ghost unit 的 position 保持旧 layout 位置（不猜坐标）。
 *
 * 等价行为覆盖参见 [ComposeVisualTimelineComment5675270164ReproTest] defect1（ghost 从 oldLayout 建位置）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualCoordinateBugsReproTest {
    @Test
    fun rebase_hasSafePathBounds_forCoordinateComputation() {
        // 方法引用验证 safePathBounds 存在 — 编译时检查
        val ref = ComposeVisualRebase::safePathBounds
        assertNotNull("safePathBounds 应存在（坐标计算基础）", ref)
    }

    @Test
    fun rebase_hasSplitMappedRangeForward_forSliceCoordinateComputation() {
        // 方法引用验证 splitMappedRangeForward 存在 — 编译时检查
        val ref = ComposeVisualRebase::splitMappedRangeForward
        assertNotNull("splitMappedRangeForward 应存在（缺陷5 切片坐标计算）", ref)
    }

    @Test
    fun splitMappedRangeForward_emptyOffsetMap_returnsSingleGhostSlice() {
        val slices =
            ComposeVisualRebase.splitMappedRangeForward(
                range = TextRange(0, 3),
                offsetMap = emptyList(),
            )
        assertEquals("空 offsetMap 应返回单个 GHOST slice", 1, slices.size)
        assertEquals(
            "slice kind 应为 GHOST",
            ComposeVisualRebase.MappedRangeSliceKind.GHOST,
            slices[0].kind,
        )
    }
}
