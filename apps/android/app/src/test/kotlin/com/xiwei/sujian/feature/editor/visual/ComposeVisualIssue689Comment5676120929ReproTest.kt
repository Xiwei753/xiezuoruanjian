package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #689 评论 5676120929 复现+验证测试 — 修复后的断言。
 *
 * 问题1：latestPatch 改为 pendingPatches 队列，快速输入时 patch 不丢。
 * 问题2：ghost 绘制使用 currentPosition，删除过程中不跳。
 * 问题3：offsetMap==null 时用 entriesForIntent fallback，等长替换不错认。
 */
@Suppress("StringLiteralDuplication", "MaxLineLength", "FunctionNaming")
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualIssue689Comment5676120929ReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    // ==================== 问题 1 ====================

    /**
     * 问题1：latestPatch 还是"只保留最后一个值"，快速输入时前一笔 patch 直接丢掉。
     *
     * 场景："" -> "a" -> "ab" -> "abc"，连续生成 A/B/C 三个 patch，
     * 中间一次都不要手工 applyVisualPatchAtFrame()，最后只推进一次 frame。
     *
     * 期望：timeline 必须能看到 a、b、c 三笔都进入持续状态，而不是只剩最后 C。
     */
    @Test
    fun issue1_rapidInput_patchesNotLost() {
        val visualState = ComposeEditorVisualState(targetId = "test")

        val layouts = captureLayouts("", "a", "ab", "abc")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)
        val abLayout = ComposeLayoutSnapshot(layouts[2], TextRange(2, 2), 0)
        val abcLayout = ComposeLayoutSnapshot(layouts[3], TextRange(3, 3), 0)

        // 初始 layout
        visualState.onAuthoritativeLayout(emptyLayout.result, emptyLayout.selection, emptyLayout.scrollY)

        // "" -> "a"
        visualState.onAuthoritativeLayout(aLayout.result, aLayout.selection, aLayout.scrollY)
        visualState.onVisualIntent(
            intent = makeIntent(1L, "", "a", insertedRanges = listOf(TextRange(0, 1))),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // "a" -> "ab"
        visualState.onAuthoritativeLayout(abLayout.result, abLayout.selection, abLayout.scrollY)
        visualState.onVisualIntent(
            intent = makeIntent(2L, "a", "ab", insertedRanges = listOf(TextRange(1, 2))),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // "ab" -> "abc"
        visualState.onAuthoritativeLayout(abcLayout.result, abcLayout.selection, abcLayout.scrollY)
        visualState.onVisualIntent(
            intent = makeIntent(3L, "ab", "abc", insertedRanges = listOf(TextRange(2, 3))),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 验证：有三笔 patch 待消费
        assertTrue("问题1: 应有 3 笔 patch 待消费", visualState.hasPendingPatches())

        // 关键：只推进一次帧，消费所有 pending patch
        val applied = visualState.drainPendingPatchesAtFrame(0L)

        // 验证：三笔 patch 都必须被消费
        // #694 评论第 7 步：同一 VSync 的多笔 patch 应先合成一个屏幕 transition
        // （ComposeVisualPatchBatch.compose），再只 applyPatch() 一次。
        // 旧实现逐笔 applyPatch 返回 3 笔；修复后 batch 合成返回 1 笔。
        assertEquals(
            "问题1: 三笔 patch 应合成一个 batch transition 被消费，实际 applied.size=${applied.size}",
            1,
            applied.size,
        )

        // 验证：没有待消费的 patch 了
        assertTrue("问题1: 消费后没有待消费的 patch", !visualState.hasPendingPatches())

        // 验证：sample 后 timeline 能正常推进（不抛异常）
        val scene = visualState.sampleVisualScene(0L)
        assertNotNull("问题1: sample 必须返回 scene", scene)
    }

    // ==================== 问题 2 ====================

    /**
     * 问题2：ghost 保存了"当前屏幕位置"，但绘制时完全没用这个位置，删除过程中仍然会跳。
     *
     * 场景：先插入一个 unit，在 50% 动画位置删除它，转 ghost 后位置必须等于删除前的当前位置。
     *
     * 期望：ghost 位置 = 删除前的当前位置。
     */
    @Test
    fun issue2_ghostPositionContinuesFromCurrentPosition() {
        val layouts = captureLayouts("", "a")
        val emptyLayout = ComposeLayoutSnapshot(layouts[0], TextRange(0, 0), 0)
        val aLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        // Step 1: 插入 "a"（alpha 0->1，duration=100ms）
        val insertPatch =
            makePatch(
                id = 1L,
                oldLayout = emptyLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
            )
        timeline.applyPatch(insertPatch, frameTimeNanos = 0L)

        // Step 2: 在 50% 位置（50ms 时）推进一次，获取当前位置
        val halfFrameTime = 50L * 1_000_000L
        val halfScene = timeline.sample(halfFrameTime)
        val aUnit = halfScene.units.firstOrNull { it.targetRange == TextRange(0, 1) }
        assertNotNull("问题2: 必须有 a unit", aUnit)

        // 计算 a 在 50% 时的当前位置
        val halfPosition = computeCurrentPosition(aUnit?.position, halfFrameTime)
        assertNotNull("问题2: a 必须有当前位置", halfPosition)

        // Step 3: 在 50% 位置删除 "a"
        val deletePatch =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = emptyLayout,
                offsetMap = null,
                deletedUnits = listOf(TextRange(0, 1)),
            )
        timeline.applyPatch(deletePatch, frameTimeNanos = halfFrameTime)

        // Step 4: 转 ghost 后位置必须等于删除前的当前位置
        val ghostScene = timeline.sample(halfFrameTime)
        val ghostA = ghostScene.units.firstOrNull { it.targetRange == null && it.range == TextRange(0, 1) }
        assertNotNull("问题2: a 必须转成 ghost", ghostA)

        val ghostPosition = computeCurrentPosition(ghostA?.position, halfFrameTime)

        // 验证：ghost 位置必须等于删除前的位置
        val deltaX = (ghostPosition?.x ?: 0f) - (halfPosition?.x ?: 0f)
        val deltaY = (ghostPosition?.y ?: 0f) - (halfPosition?.y ?: 0f)
        assertTrue(
            "问题2: ghost 位置必须等于删除前的当前位置，" +
                "实际 ghostPosition=$ghostPosition, halfPosition=$halfPosition",
            kotlin.math.abs(deltaX) < 1f && kotlin.math.abs(deltaY) < 1f,
        )
    }

    // ==================== 问题 3 ====================

    /**
     * 问题3：offsetMap == null 时只看"range 还没越界"就当它没变，等长替换会把旧 unit 错认成新 unit。
     *
     * 场景：active "a"，随后 replace 成 "b"，offsetMap = null，replaceBounds = (0,1,0,1)。
     *
     * 期望：旧 "a" -> ghost，新 "b" -> inserted；不能出现旧 unit 直接 surviving 到新 [0,1)。
     */
    @Test
    @Suppress("LongMethod")
    fun issue3_nullOffsetMap_replaceDoesNotSurviveOldUnit() {
        val layouts = captureLayouts("a", "b")
        val aLayout = ComposeLayoutSnapshot(layouts[0], TextRange(1, 1), 0)
        val bLayout = ComposeLayoutSnapshot(layouts[1], TextRange(1, 1), 0)

        val timeline = ComposeVisualTimeline()

        // Step 1: 先插入 "a"（alpha 0->1）
        val insertPatch =
            makePatch(
                id = 1L,
                oldLayout = aLayout,
                newLayout = aLayout,
                insertedUnits = listOf(TextRange(0, 1)),
            )
        timeline.applyPatch(insertPatch, frameTimeNanos = 0L)

        // 在动画未完成时验证 unit 存在（50ms）
        val midFrameTime = 50L * 1_000_000L
        val midScene = timeline.sample(midFrameTime)
        assertEquals(
            "问题3: 插入 a 后（动画中）应有 1 个存活 unit",
            1,
            midScene.units.count { it.targetRange != null },
        )

        // Step 2: 创建 replace intent，offsetMap=null，replaceBounds=(0,1,0,1)
        val replaceIntent =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 0L,
                newRevision = 0L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(0, 1)),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Move,
                cursor = null,
                expectedOldText = "a",
                expectedNewText = "b",
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 1, newStart = 0, newEnd = 1),
            )

        // 验证：entriesForIntent 应该生成 fallback entries（空，因为整个文本被替换）
        val entries = ComposeVisualRebase.entriesForIntent(replaceIntent)
        // replaceBounds=(0,1,0,1) 时没有前缀和后缀，entries 为空 — 表示没有存活映射
        assertEquals(
            "问题3: 等长全替换的 fallback entries 应为空（无存活映射）",
            0,
            entries.size,
        )

        // 验证：composeOffsetMapChain 不应该返回 null
        val composed = ComposeVisualRebase.composeOffsetMapChain(listOf(replaceIntent))
        assertNotNull(
            "问题3: composeOffsetMapChain 不应因 offsetMap==null 返回 null，实际=$composed",
            composed,
        )

        // Step 3: 构建包含 intent 的 patch，应用到 timeline
        val replacePatch =
            makePatch(
                id = 2L,
                oldLayout = aLayout,
                newLayout = bLayout,
                offsetMap = null,
                insertedUnits = listOf(TextRange(0, 1)),
                deletedUnits = listOf(TextRange(0, 1)),
                intent = replaceIntent,
            )
        timeline.applyPatch(replacePatch, frameTimeNanos = midFrameTime)

        // Step 4: 验证结果 — 旧 "a" 应该转成 ghost，新 "b" 应该 inserted
        val resultScene = timeline.sample(midFrameTime)
        val ghosts = resultScene.units.filter { it.targetRange == null }
        val surviving = resultScene.units.filter { it.targetRange != null }

        // 旧 "a" 应该是 ghost（targetRange == null）
        val ghostA = ghosts.firstOrNull { it.range == TextRange(0, 1) }
        assertNotNull(
            "问题3: 旧 'a' 必须转成 ghost，实际 ghosts=${ghosts.map { "${it.range}->${it.targetRange}" }}",
            ghostA,
        )

        // 新 "b" 应该是 surviving/inserted（targetRange != null）
        val insertedB = surviving.firstOrNull { it.range == TextRange(0, 1) }
        assertNotNull(
            "问题3: 新 'b' 必须作为 inserted/surviving 存在，" +
                "实际 surviving=${surviving.map { "${it.range}->${it.targetRange}" }}",
            insertedB,
        )

        // 关键：不应该有旧 unit 直接 surviving 到新 [0,1)
        val wrongSurvival =
            surviving.any {
                it.targetRange == TextRange(0, 1) && it.layout == aLayout
            }
        val survivingInfo =
            surviving.map {
                "layout=${it.layout == aLayout},range=${it.range},target=${it.targetRange}"
            }
        assertTrue(
            "问题3: 旧 unit 不应直接 surviving 到新 [0,1)，实际=$survivingInfo",
            !wrongSurvival,
        )
    }

    // ==================== 辅助方法 ====================

    @Suppress("LongParameterList")
    private fun makePatch(
        id: Long,
        oldLayout: ComposeLayoutSnapshot,
        newLayout: ComposeLayoutSnapshot,
        offsetMap: List<VisualOffsetMapEntry>? = null,
        insertedUnits: List<TextRange> = emptyList(),
        deletedUnits: List<TextRange> = emptyList(),
        retainedMoves: List<RetainedMove> = emptyList(),
        durationMs: Long = 100L,
        intent: EditorVisualIntent? = null,
    ): ComposeVisualPatch =
        ComposeVisualPatch(
            id = id,
            coreTransactionIds = listOf(id),
            oldLayout = oldLayout,
            newLayout = newLayout,
            offsetMap = offsetMap,
            insertedUnits = insertedUnits,
            deletedUnits = deletedUnits,
            retainedMoves = retainedMoves,
            originCaretRect = Rect.Zero,
            targetCaretRect = Rect.Zero,
            durationMs = durationMs,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            motionPolicy = EditorMotionPolicy(textDurationMillis = durationMs),
            intent = intent,
        )

    private fun makeIntent(
        coreTransactionId: Long,
        oldText: String,
        newText: String,
        insertedRanges: List<TextRange> = emptyList(),
        deletedRanges: List<TextRange> = emptyList(),
    ): EditorVisualIntent =
        EditorVisualIntent(
            coreTransactionId = coreTransactionId,
            baseRevision = 0L,
            newRevision = 0L,
            animationMode = AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 100L,
            offsetMap = null,
            oldRanges = deletedRanges,
            newRanges = insertedRanges,
            textKind = TextVisualKind.Insert,
            cursor = null,
            expectedOldText = oldText,
            expectedNewText = newText,
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

    /**
     * 计算 TimedOffset 通道在指定帧时间的当前位置。
     * 复制了 ComposeVisualTimeline.currentOffset 的逻辑，用于测试。
     */
    private fun computeCurrentPosition(
        channel: TimedOffset?,
        frameTimeNanos: Long,
    ): Offset? {
        if (channel == null) return null
        if (channel.durationNanos <= 0L) return channel.to
        val elapsed = frameTimeNanos - channel.startedAtNanos
        if (elapsed <= 0L) return channel.from
        if (elapsed >= channel.durationNanos) return channel.to
        val t = elapsed.toFloat() / channel.durationNanos.toFloat()
        return Offset(
            channel.from.x + (channel.to.x - channel.from.x) * t,
            channel.from.y + (channel.to.y - channel.from.y) * t,
        )
    }
}
