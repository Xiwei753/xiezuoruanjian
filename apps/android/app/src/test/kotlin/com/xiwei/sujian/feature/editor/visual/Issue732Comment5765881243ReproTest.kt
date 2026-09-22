package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
import com.xiwei.sujian.feature.editor.layout.ComposeLayoutSnapshot
import com.xiwei.sujian.feature.editor.motion.EditorMotionPolicy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Issue #732 评论 5765881243 复现测试 —
 *
 * 上一轮评论 5764716281 已修复存活 unit（target != null）分支：coordinated 模式下
 * [ComposeVisualTimeline.sample] 时 `coordinatedSpatialClip=true && motionSample!=null &&
 * !motionSample.unitClipFractions.containsKey(unit.key)` 则立即 `presentedKeys.remove(unit.key); continue` 释放。
 *
 * 但 deleted ghost 分支（target == null）漏了同样的检查。导致 coordinated 模式下，如果当前
 * motionSample 存在但某个旧 deleted ghost 已经不在 unitClipFractions 里：draw 层不画它，但 Timeline
 * 仍把它留在 units 里，下一笔 patch 时 [ComposeVisualTimeline.activeEditUnits] 会把它作为 DeletedGhost
 * 放进 deletedDescriptors，ComposeEditMotion.redirectTo()/forEdit() 会再次为它创建 deleted channel，
 * 从 1→0 重新进入动画。已失去 caret/motion 所有权的旧删除字仍会被重新拉起来吞一次。
 *
 * 修复后（#732 评论 5765881243）：deleted ghost 分支最开头加上与存活 unit 同一条规则——
 * coordinatedSpatialClip=true 且 motionSample!=null 且 key 不在 unitClipFractions 时，
 * `presentedKeys.remove(unit.key)` 后直接 `continue`，不放进 sampledUnits/remainingUnits，
 * 不等 alpha/reveal timer。coordinated 模式下当前 ComposeEditMotion.Sample 没有接管的文字 unit
 * （包括 Inserted 和 DeletedGhost），Timeline 都不得继续持有。
 *
 * 测试基础设施：Robolectric + createComposeRule 用于构造真实 TextLayoutResult。
 * ComposeVisualTimeline 是纯数据状态机，可直接实例化。
 */
@Suppress("MaxLineLength", "StringLiteralDuplication", "LongMethod", "FunctionNaming")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class Issue732Comment5765881243ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 测试 1：motion sample 缺 ghost key → ghost 立即释放 ====================

    /**
     * 测试 1：coordinated 模式下 motion sample 存在但缺 ghost key → sample 后 ghost 立即释放。
     *
     * 场景：章节打开后 timeline 为空，用户第一次 Backspace 删除 "a"（oldLayout="a", newLayout=""）。
     * applyPatch 从 oldLayout + deletedUnits 建 deleted ghost（alpha 1→0，defect1 模式）。
     * 此时 ghost 在 timeline.units 里。
     *
     * 构造 motionSampleMissingKey：unitClipFractions=emptyMap()，不包含该 ghost 的 key
     * （模拟当前 ComposeEditMotion.Sample 已不接管该 ghost —— 例如 rapid redirect 后旧 motion 通道已结束）。
     *
     * 修复前：sample 在 deleted ghost 分支只看 alphaFinished/motionFractionReachedTarget，
     * 0ms 时 alpha 未结束（1→0 duration=100ms），ghost 留在 units 里，
     * activeEditUnits() 的 deletedDescriptors 非空，下一笔 patch 会重新为它创建 deleted channel。
     *
     * 修复后：sample 在 deleted ghost 分支最开头检查到 coordinatedSpatialClip && motionSample!=null &&
     * !containsKey，立即 presentedKeys.remove + continue，ghost 从 units 释放。
     */
    @Test
    fun motionSampleMissingGhostKey_sampleReleasesGhostFromSceneAndActiveEditUnits() {
        val layouts = captureLayouts("a", "")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        val timeline = ComposeVisualTimeline()
        // 章节打开后 timeline 为空，用户第一次 Backspace 删除 "a"（defect1 模式）
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                deletedUnits = listOf(TextRange(0, 1)),
            )
        // coordinated=true → coordinatedSpatialClip=true（policy.textAnimationEnabledForEdit && coordinated）
        val policy = EditorMotionPolicy(textDurationMillis = 100L, coordinated = true, reduceMotion = false)
        timeline.applyPatch(patch, frameTimeNanos = 0L, motionPolicy = policy)

        // applyPatch 后 ghost 在 units 里，用 activeEditUnits() 拿 deleted ghost 的 key
        // （不经过 sample，units 还在；sample(null) 会触发硬问题2 修复清掉所有 units）
        val (_, deletedDescriptorsBefore) = timeline.activeEditUnits()
        assertTrue(
            "前置: applyPatch 后 deleted ghost 应在 units 里，deletedDescriptors 非空——" +
                "从 oldLayout + deletedUnits 建 ghost（alpha 1→0）\n" +
                "deletedDescriptors.size=${deletedDescriptorsBefore.size}",
            deletedDescriptorsBefore.isNotEmpty(),
        )
        val ghostKey = deletedDescriptorsBefore.first().key

        // 构造 motionSampleMissingKey：unitClipFractions 不包含 ghostKey
        // （模拟当前 ComposeEditMotion.Sample 已不接管该 ghost）
        val motionSampleMissingKey =
            ComposeEditMotion.Sample(
                caretRect = Rect.Zero,
                unitClipFractions = emptyMap(),
                finished = false,
            )

        // sample 触发修复：deleted ghost 缺 motion key 时立即释放
        // frameTimeNanos=0L：alpha 1→0 duration=100ms 未结束，唯一能释放 ghost 的是修复的 motion key 检查
        val scene = timeline.sample(frameTimeNanos = 0L, motionSample = motionSampleMissingKey)

        // 证据 1: scene.units 里没有该 ghost（targetRange==null 的 unit 为空）
        val ghostsAfter = scene.units.filter { it.targetRange == null }
        assertTrue(
            "修复后: motionSample 缺 ghost key 时，sample 后 scene.units 不含 deleted ghost——" +
                "ghosts.size=${ghostsAfter.size}（应为 0）\n" +
                "场景：coordinated 模式下当前 ComposeEditMotion.Sample 没有接管的 deleted ghost 立即释放，" +
                "不放进 sampledUnits/remainingUnits，不等 alpha/reveal timer",
            ghostsAfter.isEmpty(),
        )

        // 证据 2: scene.unitClipFractions 不包含该 ghost key
        assertFalse(
            "修复后: motionSample 缺 ghost key 时，scene.unitClipFractions 不含该 ghost key——" +
                "overlay 不画该 ghost，unitClipFractions.keys=${scene.unitClipFractions.keys}",
            scene.unitClipFractions.containsKey(ghostKey),
        )

        // 证据 3: timeline.activeEditUnits() 的 deletedDescriptors 为空（ghost 已从 units 释放）
        val (_, deletedDescriptorsAfter) = timeline.activeEditUnits()
        assertTrue(
            "修复后: motionSample 缺 ghost key 时，ghost 已从 units 释放，" +
                "activeEditUnits() 的 deletedDescriptors 为空——" +
                "下一笔 patch 不会重新为它创建 deleted channel，" +
                "ComposeEditMotion.redirectTo()/forEdit() 不会再次从 1→0 重新进入动画\n" +
                "deletedDescriptors.size=${deletedDescriptorsAfter.size}（应为 0）",
            deletedDescriptorsAfter.isEmpty(),
        )

        // 证据 4: scene.units 整体为空（既无存活 unit 也无 ghost）
        assertTrue(
            "修复后: motionSample 缺 ghost key 时，scene.units 整体为空——" +
                "既无存活 unit 也无 deleted ghost，overlay 停帧\n" +
                "scene.units.size=${scene.units.size}（应为 0）",
            scene.units.isEmpty(),
        )
    }

    // ==================== 测试 2（对照）：motion sample 包含 ghost key → ghost 保留 ====================

    /**
     * 测试 2（对照）：coordinated 模式下 motion sample 包含 ghost key 时 ghost 保留。
     *
     * 同样场景，但 motionSample.unitClipFractions 包含该 ghost 的 key（motion 还在接管）。
     * sample 后 ghost 应保留在 scene.units 和 activeEditUnits() 的 deletedDescriptors 里。
     *
     * 这证明修复是精确的：只在 motion 丢弃 key 时释放，motion 仍接管时保留。
     */
    @Test
    fun motionSampleContainsGhostKey_sampleRetainsGhost() {
        val layouts = captureLayouts("a", "")
        val oldLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val newLayout = ComposeLayoutSnapshot(layouts[1], TextRange(0, 0), 0)

        val timeline = ComposeVisualTimeline()
        val patch =
            makePatch(
                id = 1L,
                oldLayout = oldLayout,
                newLayout = newLayout,
                deletedUnits = listOf(TextRange(0, 1)),
            )
        val policy = EditorMotionPolicy(textDurationMillis = 100L, coordinated = true, reduceMotion = false)
        timeline.applyPatch(patch, frameTimeNanos = 0L, motionPolicy = policy)

        val (_, deletedDescriptorsBefore) = timeline.activeEditUnits()
        assertTrue(
            "前置: applyPatch 后 deleted ghost 应在 units 里，deletedDescriptors 非空",
            deletedDescriptorsBefore.isNotEmpty(),
        )
        val ghostKey = deletedDescriptorsBefore.first().key

        // motionSample.unitClipFractions 包含 ghostKey（motion 还在接管该 ghost，fraction=0.5）
        val motionSampleWithKey =
            ComposeEditMotion.Sample(
                caretRect = Rect.Zero,
                unitClipFractions = mapOf(ghostKey to 0.5f),
                finished = false,
            )

        // sample：motion 仍接管该 ghost，不应提前释放
        val scene = timeline.sample(frameTimeNanos = 0L, motionSample = motionSampleWithKey)

        // 对照 1: ghost 保留在 scene.units 里
        val ghostsAfter = scene.units.filter { it.targetRange == null }
        assertTrue(
            "对照: motionSample 包含 ghost key 时，sample 后 ghost 保留在 scene.units——" +
                "motion 仍接管（fraction=0.5），不能提前释放\n" +
                "ghosts.size=${ghostsAfter.size}（应 >= 1）",
            ghostsAfter.isNotEmpty(),
        )

        // 对照 2: ghost 保留在 activeEditUnits() 的 deletedDescriptors 里
        val (_, deletedDescriptorsAfter) = timeline.activeEditUnits()
        assertTrue(
            "对照: motionSample 包含 ghost key 时，ghost 保留在 activeEditUnits() 的 deletedDescriptors——" +
                "motion 仍接管，下一笔 patch 可继续为它调度 deleted channel\n" +
                "deletedDescriptors.size=${deletedDescriptorsAfter.size}（应 >= 1）",
            deletedDescriptorsAfter.isNotEmpty(),
        )

        // 对照 3: 保留的 ghost key 一致
        assertEquals(
            "对照: 保留的 ghost key 应与 applyPatch 时一致",
            ghostKey,
            deletedDescriptorsAfter.first().key,
        )
    }

    // ==================== 辅助方法 ====================

    private companion object {
        /** 1 ms = 1_000_000 ns。 */
        const val NANOS_PER_MS: Long = 1_000_000L
    }

    /**
     * 构造 ComposeVisualPatch。
     *
     * originCaretRect/targetCaretRect 用 Rect.Zero — 本测试验证 timeline/motion 机制，
     * 不验证 caret 几何。offsetMap 默认 null（删除场景不需要 offset 映射）。
     */
    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>? = null,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
        durationMs: Long = 100L,
    ): ComposeVisualPatch =
        ComposeVisualPatch(
            id = id,
            coreTransactionIds = listOf(id),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = emptyList(),
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = durationMs,
            animationMode = AnimationMode.CLUSTER_ANIMATION,
        )

    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = 14f.sp),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
