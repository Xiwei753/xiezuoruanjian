package com.xiwei.sujian.feature.editor.visual

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
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto

/**
 * #684 评论 5664636035 复现测试 — 三个坐标 bug：
 *
 * Bug 1：ComposeVisualFrameCoordinator 中 oldRanges/newRanges 仍然把中间事务坐标直接 flatMap。
 *   chain 里的第 2、3 笔 range 属于 T1/T2 中间正文，不属于屏幕事务的 T0 oldLayout / Tn newLayout。
 *   直接 flatMap 会把中间事务坐标当成屏幕坐标，导致画错位置。
 *
 * Bug 2：无旧动画时光标起点仍拿"最后一笔 intent 的 old cursor"去查 T0 布局。
 *   consumed.layout 是 T0，但 lastIntent.cursor.oldEndUtf16 是 T(n-1) 坐标。
 *
 * Bug 3：ComposeVisualRebase 中空 offsetMap.entries 是合法"零存活映射"，不能当成没有 map。
 *   Core 的 offset map 里，"没有 entry"可以表示这次编辑后没有任何旧文字存活。
 *
 * 三个测试在当前代码下都应失败（证明 bug 存在）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualCoordinateBugsReproTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * Bug 1 复现：两笔连续 Delete chain，oldRanges 不应等于 chain.flatMap { it.oldRanges }。
     *
     * 场景：T0=abcde，Delete c: T0->T1=abde oldRange=[2,3)，Delete d: T1->T2=abe oldRange=[2,3)。
     * 直接 flatMap 得到 [2,3),[2,3)，拿去旧布局 T0 画时两次都指向 c，d 根本没有被表示。
     *
     * 正确行为：屏幕事务的 old changed ranges 应该直接从 T0→Tn composed offset map 的补集算。
     * T0=abcde → T2=abe：composed map 中 "ab" IDENTITY [0,0,2], "e" SHIFTED [4,2,1]。
     * old changed ranges = [0,5) 中没有被 composed map old 区间覆盖的部分 = [2,4)（"cd" 被删除）。
     *
     * 当前代码：mergedOldRanges = [2,3) + [2,3) = [2,3),[2,3)（错坐标系产物）。
     */
    @Test
    fun multiIntentChain_oldRanges_shouldNotBeFlatMap_bug1() {
        val layouts = captureLayouts("abcde", "abde", "abe")
        val oldLayout = layouts[0]
        val newLayout = layouts[2]

        val state = ComposeEditorVisualState(targetId = "test-target-bug1")

        // 1. 基线 layout "abcde" 到达。
        state.onAuthoritativeLayout(oldLayout, TextRange(3, 3), 0)

        // 2. intent1: T0="abcde" -> T1="abde"（删除 old[2,3)="c"）
        val intent1 =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY), // "ab"
                                VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED), // "de" 前移
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)), // 删除的 "c"
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 3, newStart = 2, newEnd = 2),
                expectedOldText = "abcde",
                expectedNewText = "abde",
            )
        state.onVisualIntent(
            intent1,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 3. intent2: T1="abde" -> T2="abe"（删除 T1[2,3)="d"）
        val intent2 =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY), // "ab"
                                VisualOffsetMapEntry(3, 2, 1, VisualOffsetMapKind.SHIFTED), // "e" 前移
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)), // 删除的 "d"（T1 坐标）
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 3, newStart = 2, newEnd = 2),
                expectedOldText = "abde",
                expectedNewText = "abe",
            )
        state.onVisualIntent(
            intent2,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 4. 新 layout "abe" 到达 → 生成事务（chain=[intent1, intent2]）。
        state.onAuthoritativeLayout(newLayout, TextRange(2, 2), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("事务应生成", transaction)

        // 当前代码的错误产物：mergedOldRanges = chain.flatMap { it.oldRanges } = [2,3) + [2,3) = [2,3),[2,3)
        val currentOldRanges = transaction?.oldRanges ?: emptyList()
        val flatMappedRanges = listOf(intent1, intent2).flatMap { it.oldRanges }

        // 核心断言：transaction.oldRanges 不应等于 chain.flatMap { it.oldRanges }。
        // 当前代码直接 flatMap，得到 [2,3),[2,3)（两个都指向 T0 的 "c"，"d" 没被表示）。
        // 正确行为：从 T0→Tn composed offset map 的补集算，应得到 [2,4)（"cd" 被删除）。
        assertFalse(
            "transaction.oldRanges 不应等于 chain.flatMap { it.oldRanges }（中间事务坐标不能直接当屏幕坐标）\n" +
                "当前 oldRanges=$currentOldRanges，flatMap 产物=$flatMappedRanges\n" +
                "#684 评论 5664636035 Bug1：chain 里的第 2 笔 range 属于 T1 中间正文，不属于屏幕事务的 T0 oldLayout",
            currentOldRanges == flatMappedRanges && flatMappedRanges.size > 1,
        )
    }

    /**
     * Bug 3 复现：空 offsetMap.entries 是合法"零存活映射"，不能当成没有 map。
     *
     * 场景：整段删除 "abc" -> ""，offsetMap.entries = emptyList()（没有任何旧文字存活）。
     *
     * 正确行为：composeOffsetMapChain 应返回 emptyList()（零存活映射），而非 null。
     * 当前代码：if (chain.any { it.offsetMap == null || it.offsetMap.entries.isEmpty() }) return null
     *   把空 entries 当成没有 map，回退到 legacy 路径。
     *
     * 同时 materializeStartFrame 里 nextOffsetMap != null && nextOffsetMap.isNotEmpty()
     * 导致空 map 不走 splitRebasedSliceThroughOffsetMap，走 else 分支原样保留。
     */
    @Test
    fun emptyOffsetMapEntries_isValidZeroSurvivalMap_bug3() {
        // 构造一个 intent，offsetMap.entries = emptyList()（整段删除）。
        val intent =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()), // 零存活映射
                oldRanges = listOf(TextRange(0, 3)), // 删除的 "abc"
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "",
            )

        val composed = ComposeVisualRebase.composeOffsetMapChain(listOf(intent))

        // 核心断言 1：composed 应非 null（空 entries 是合法"零存活映射"，不是没有 map）。
        assertNotNull(
            "composeOffsetMapChain 对空 entries 应返回 emptyList() 而非 null\n" +
                "#684 评论 5664636035 Bug3：空 offsetMap.entries 是合法\"零存活映射\"（整段删除/整段替换），" +
                "不能当成没有 map。当前代码 if (chain.any { it.offsetMap == null || it.offsetMap.entries.isEmpty() }) return null 把空 entries 当成没有 map",
            composed,
        )

        // 核心断言 2：composed 应为空列表（零存活）。
        assertTrue(
            "composeOffsetMapChain 对空 entries 应返回 emptyList()（零存活映射）\n" +
                "实际 composed=$composed",
            composed?.isEmpty() == true,
        )
    }

    /**
     * Bug 3 补充：materializeStartFrame 里空 map 也必须走 splitRebasedSliceThroughOffsetMap。
     *
     * 当前代码：nextOffsetMap != null && nextOffsetMap.isNotEmpty() 导致空 map 走 else 分支，
     * 原样保留 slice（不切分），但正确行为是空 map 也走 split（所有 slice 都 fading）。
     *
     * 用一个有 surviving slice 的 startFrame 场景验证：空 map 时所有 slice 应变成 fading。
     */
    @Test
    fun materializeStartFrame_emptyMap_shouldStillSplit_bug3() {
        val layouts = captureLayouts("", "abcdefgh", "")
        val state = ComposeEditorVisualState(targetId = "test-target-bug3-split")

        // === 生成事务 A（Insert "abcdefgh"）===
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        val intentA =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 8)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 8),
                expectedOldText = "",
                expectedNewText = "abcdefgh",
            )
        state.onVisualIntent(
            intentA,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(8, 8), 0)
        val txA = state.activeTransaction.value
        assertNotNull("事务 A 应生成", txA)

        // A 跑到 progress=0.5（surviving slice alpha=0.5，物化 B 时 startFrame 非 null）。
        state.reportProgress(state.activeTransaction.value?.id ?: 0L, 0.5f)

        // === 生成事务 B（整段删除 "abcdefgh" -> ""，offsetMap.entries = emptyList()）===
        val intentB =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()), // 零存活映射
                oldRanges = listOf(TextRange(0, 8)), // 删除的 "abcdefgh"
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 8, newStart = 0, newEnd = 0),
                expectedOldText = "abcdefgh",
                expectedNewText = "",
            )
        state.onVisualIntent(
            intentB,
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )

        // 新 layout "" 到达 → 生成事务 B。
        state.onAuthoritativeLayout(layouts[2], TextRange(0, 0), 0)

        val txB = state.activeTransaction.value
        assertNotNull("事务 B 应生成", txB)

        val startFrame = txB?.startFrame
        assertNotNull(
            "B.startFrame 应非 null（从 A 在 progress=0.5 物化）",
            startFrame,
        )

        val slices = startFrame?.slices ?: emptyList()

        // 核心断言：空 map 时所有 slice 应变成 fading（targetRange = null）。
        // 当前代码 nextOffsetMap != null && nextOffsetMap.isNotEmpty() 为 false（空 map），
        // 走 else 分支原样保留 slice（targetRange 仍非 null），这是错的。
        // 正确行为：空 map 也走 splitRebasedSliceThroughOffsetMap，所有 slice 都 fading。
        val allFading = slices.all { it.targetRange == null }
        assertTrue(
            "空 map 时 startFrame 所有 slice 应变成 fading（targetRange=null），实际 slices targetRanges=${slices.map { it.targetRange }}\n" +
                "#684 评论 5664636035 Bug3：materializeStartFrame 里 nextOffsetMap != null && nextOffsetMap.isNotEmpty() " +
                "导致空 map 走 else 分支原样保留 slice，但正确行为是空 map 也走 split（所有 slice 都 fading）",
            slices.isNotEmpty() && allFading,
        )
    }

    /**
     * 用 [rememberTextMeasurer] 在 Compose 测试环境里构造真实 [TextLayoutResult]。
     * 一次 setContent 构造多份 layout，供测试里多代 rebase 使用。
     * maxWidth=1000 避免折行，让 bounds 计算确定；硬换行 `\n` 一定产生多行布局。
     */
    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> =
        captureLayoutsWithWidth(texts, maxWidth = 1000)

    private fun captureLayoutsWithWidth(
        texts: Array<out String>,
        maxWidth: Int,
        fontSizeSp: Float = 14f,
    ): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = androidx.compose.ui.text.rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        style = TextStyle(fontSize = fontSizeSp.sp),
                        constraints = Constraints(maxWidth = maxWidth),
                    ),
                )
            }
        }
        return results
    }
}
