package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.sp
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
 * #684 评论 5664636035 Type 4 Patch Adversarial — 验证补丁确实改变了三个 bug 的代码路径。
 *
 * 这些测试不是复现测试（复现测试在 ComposeVisualCoordinateBugsReproTest），
 * 而是对补丁新引入逻辑的对抗式验证：
 *
 * 1. Bug1: changedRangesFromComposedMap / complementRanges 正确性 — 三笔 chain、边界。
 * 2. Bug2: firstCursor/lastCursor 在三笔 chain 中的正确性 — cursorStartRect 用第一笔，cursorEndRect 用最后一笔。
 * 3. Bug3: 空 entries compose 传播 — 空 + 非空 = 空（整段删除后插入，T0→Tn 没有任何存活文字）。
 * 4. textKind 按最终净变化决定 — Insert+Delete chain 的 transactionTextKind 应为 Move（oldChanged 和 newChanged 都非空）。
 *
 * 所有测试在补丁应用后应 PASS。如果测试被削弱（例如断言改为 assertFalse），这些对抗测试会暴露。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeVisualPatchAdversarialTest {
    @get:Rule
    val composeRule = createComposeRule()

    /**
     * Bug1 对抗：三笔连续 Delete chain 的 composed offset map 补集正确性。
     *
     * 场景：T0=abcde → T1=abde（删 c）→ T2=abe（删 d）→ T3=ae（删 b）。
     * composed map: "a" IDENTITY [0,0,1], "e" SHIFTED [4,1,1]。
     * old changed ranges = [0,5) 中没有被 composed map old 区间 ([0,1)+[4,5)) 覆盖的部分 = [1,4)（"bcd" 被删除）。
     * new changed ranges = [0,2) 中没有被 composed map new 区间 ([0,1)+[1,2)) 覆盖的部分 = emptyList()。
     *
     * 这验证 changedRangesFromComposedMap 和 complementRanges 在三笔 chain 下正确工作，
     * 而不是只在两笔 chain（复现测试）下正确。
     */
    @Test
    fun bug1_threeIntentChain_composedComplement_correct() {
        // 直接测试 changedRangesFromComposedMap（public 函数）。
        // composed map: "a" IDENTITY [0,0,1], "e" SHIFTED [4,1,1]
        val composedMap =
            listOf(
                VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY), // "a"
                VisualOffsetMapEntry(4, 1, 1, VisualOffsetMapKind.SHIFTED), // "e"
            )
        val oldLength = 5 // T0="abcde"
        val newLength = 2 // T3="ae"
        val frameChangedRanges =
            ComposeVisualRebase.changedRangesFromComposedMap(composedMap, oldLength, newLength)

        // old changed ranges = [1,4)（"bcd" 被删除）
        assertEquals(
            "三笔 chain old changed ranges 应为 [1,4)（bcd 被删除）",
            listOf(TextRange(1, 4)),
            frameChangedRanges.oldRanges,
        )
        // new changed ranges = emptyList()（"ae" 都存活，没有新增）
        assertEquals(
            "三笔 chain new changed ranges 应为 emptyList()（ae 都存活）",
            emptyList<TextRange>(),
            frameChangedRanges.newRanges,
        )
    }

    /**
     * Bug1 对抗：complementRanges 边界 — totalLength=0、covered 为空、covered 覆盖全部。
     *
     * 这验证 complementRanges 的边界处理正确，不是只在正常情况正确。
     */
    @Test
    fun bug1_complementRanges_boundaryCases_correct() {
        // 边界 1：totalLength=0 → emptyList()
        val emptyOld =
            ComposeVisualRebase.changedRangesFromComposedMap(
                listOf(VisualOffsetMapEntry(0, 0, 0, VisualOffsetMapKind.IDENTITY)),
                0,
                0,
            )
        assertEquals(
            "totalLength=0 时 oldRanges 应为 emptyList()",
            emptyList<TextRange>(),
            emptyOld.oldRanges,
        )

        // 边界 2：covered 为空（map 为空列表）→ [0, totalLength)
        val fullDeleted =
            ComposeVisualRebase.changedRangesFromComposedMap(
                emptyList(),
                3,
                0,
            )
        assertEquals(
            "map 为空时 oldRanges 应为 [0,3)（整段删除）",
            listOf(TextRange(0, 3)),
            fullDeleted.oldRanges,
        )
        assertEquals(
            "map 为空且 newLength=0 时 newRanges 应为 emptyList()",
            emptyList<TextRange>(),
            fullDeleted.newRanges,
        )

        // 边界 3：covered 覆盖全部 → emptyList()
        val allSurvive =
            ComposeVisualRebase.changedRangesFromComposedMap(
                listOf(VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY)),
                3,
                3,
            )
        assertEquals(
            "全存活时 oldRanges 应为 emptyList()",
            emptyList<TextRange>(),
            allSurvive.oldRanges,
        )
        assertEquals(
            "全存活时 newRanges 应为 emptyList()",
            emptyList<TextRange>(),
            allSurvive.newRanges,
        )
    }

    /**
     * Bug1 对抗：complementRanges 重叠区间处理。
     *
     * 如果 composed map 的 old 区间有重叠（不应发生但要防御），complementRanges 应正确处理。
     * 这里用非重叠但相邻的区间验证。
     */
    @Test
    fun bug1_complementRanges_adjacentRanges_correct() {
        // composed map: [0,2) + [2,4) 覆盖 [0,4)，totalLength=5 → oldRanges=[4,5)
        val composedMap =
            listOf(
                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                VisualOffsetMapEntry(2, 2, 2, VisualOffsetMapKind.IDENTITY),
            )
        val result = ComposeVisualRebase.changedRangesFromComposedMap(composedMap, 5, 5)
        assertEquals(
            "相邻区间覆盖 [0,4) 时 oldRanges 应为 [4,5)",
            listOf(TextRange(4, 5)),
            result.oldRanges,
        )
    }

    /**
     * Bug2 对抗：三笔连续 Backspace chain 的 firstCursor/lastCursor 正确性。
     *
     * 场景：T0 cursor=5 → T1=4 → T2=3 → T3=2（Backspace 三次）。
     * firstCursor.oldEndUtf16=5（T0 坐标），lastCursor.newEndUtf16=2（T3 坐标）。
     * cursorStartRect 应对应 T0 layout offset=5，cursorEndRect 应对应 T3 layout offset=2。
     *
     * 这验证 firstCursor/lastCursor 在三笔 chain 下正确工作，
     * 而不是只在两笔 chain（复现测试）下正确。
     */
    @Test
    fun bug2_threeIntentChain_firstAndLastCursor_correct() {
        val layouts = captureLayouts("abcde", "abcd", "abc", "ab")
        val oldLayout = layouts[0] // T0 = "abcde"
        val newLayout = layouts[3] // T3 = "ab"

        val state = ComposeEditorVisualState(targetId = "test-target-bug2-three")

        // 1. 基线 layout "abcde" 到达，cursor 在末尾 (offset=5)。
        state.onAuthoritativeLayout(oldLayout, TextRange(5, 5), 0)

        // 2. intent1: T0="abcde" -> T1="abcd"（Backspace 删除 "e"，cursor 5->4）
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
                                VisualOffsetMapEntry(0, 0, 4, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = listOf(TextRange(4, 5)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = CursorVisualIntent(oldEndUtf16 = 5, newEndUtf16 = 4, animate = true),
                replaceBounds = VisualReplaceBounds(oldStart = 4, oldEnd = 5, newStart = 4, newEnd = 4),
                expectedOldText = "abcde",
                expectedNewText = "abcd",
            )
        state.onVisualIntent(intent1, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 3. intent2: T1="abcd" -> T2="abc"（Backspace 删除 "d"，cursor 4->3）
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
                                VisualOffsetMapEntry(0, 0, 3, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = listOf(TextRange(3, 4)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = CursorVisualIntent(oldEndUtf16 = 4, newEndUtf16 = 3, animate = true),
                replaceBounds = VisualReplaceBounds(oldStart = 3, oldEnd = 4, newStart = 3, newEnd = 3),
                expectedOldText = "abcd",
                expectedNewText = "abc",
            )
        state.onVisualIntent(intent2, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 4. intent3: T2="abc" -> T3="ab"（Backspace 删除 "c"，cursor 3->2）
        val intent3 =
            EditorVisualIntent(
                coreTransactionId = 3L,
                baseRevision = 2L,
                newRevision = 3L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap =
                    VisualOffsetMap(
                        entries =
                            listOf(
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = CursorVisualIntent(oldEndUtf16 = 3, newEndUtf16 = 2, animate = true),
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 3, newStart = 2, newEnd = 2),
                expectedOldText = "abc",
                expectedNewText = "ab",
            )
        state.onVisualIntent(intent3, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 5. 新 layout "ab" 到达 → 生成事务（chain=[intent1, intent2, intent3]）。
        state.onAuthoritativeLayout(newLayout, TextRange(2, 2), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("事务应生成", transaction)

        // cursorStartRect 应对应 T0 的 firstCursor.oldEndUtf16=5
        val cursorStartRect = transaction?.cursorStartRect
        assertNotNull("cursorStartRect 应非 null", cursorStartRect)
        val correctStartRect = oldLayout.getCursorRect(5) // 第一笔 old cursor = 5
        val startRect = cursorStartRect!!
        val matchesCorrectStart =
            kotlin.math.abs(startRect.left - correctStartRect.left) < 1f &&
                kotlin.math.abs(startRect.top - correctStartRect.top) < 1f
        assertTrue(
            "三笔 chain cursorStartRect 应对应 T0 的 firstCursor=5\n" +
                "实际=$startRect, 正确(offset=5)=$correctStartRect",
            matchesCorrectStart,
        )

        // cursorEndRect 应对应 T3 的 lastCursor.newEndUtf16=2
        val cursorEndRect = transaction?.cursorEndRect
        assertNotNull("cursorEndRect 应非 null", cursorEndRect)
        val correctEndRect = newLayout.getCursorRect(2) // 最后一笔 new cursor = 2
        val endRect = cursorEndRect!!
        val matchesCorrectEnd =
            kotlin.math.abs(endRect.left - correctEndRect.left) < 1f &&
                kotlin.math.abs(endRect.top - correctEndRect.top) < 1f
        assertTrue(
            "三笔 chain cursorEndRect 应对应 T3 的 lastCursor=2\n" +
                "实际=$endRect, 正确(offset=2)=$correctEndRect",
            matchesCorrectEnd,
        )
    }

    /**
     * Bug3 对抗：空 entries + 非空 entries compose 传播。
     *
     * 场景：intent1 空 entries（整段删除 "abc"→""），intent2 非空 entries（插入 ""→"xy"）。
     * composed 应为空列表（T0→Tn 没有任何存活文字：abc 全删，xy 全新插入）。
     *
     * 这验证 composeOffsetMapChain 在空+非空 chain 下正确传播空，
     * 而不是只在单笔空 entries（复现测试）下正确。
     */
    @Test
    fun bug3_emptyThenNonEmpty_composePropagatesEmpty() {
        // intent1: "abc" -> ""（空 entries，零存活）
        val intent1 =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()),
                oldRanges = listOf(TextRange(0, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "",
            )

        // intent2: "" -> "xy"（非空 entries，但 acc 已空，compose 后仍空）
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
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 2)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "xy",
            )

        val composed = ComposeVisualRebase.composeOffsetMapChain(listOf(intent1, intent2))
        assertNotNull(
            "空+非空 chain composeOffsetMapChain 应非 null（空 entries 是合法零存活映射）",
            composed,
        )
        assertTrue(
            "空+非空 chain composed 应为 emptyList()（T0→Tn 没有任何存活文字：abc 全删，xy 全新插入）\n" +
                "实际 composed=$composed",
            composed?.isEmpty() == true,
        )
    }

    /**
     * Bug3 对抗：非空 entries + 空 entries compose 传播。
     *
     * 场景：intent1 非空 entries（"abc"→"ab" 删 c），intent2 空 entries（"ab"→"" 整段删）。
     * composed 应为空列表（T0→Tn 没有任何存活文字）。
     */
    @Test
    fun bug3_nonEmptyThenEmpty_composePropagatesEmpty() {
        // intent1: "abc" -> "ab"（删 c，非空 entries）
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
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "ab",
            )

        // intent2: "ab" -> ""（空 entries，零存活）
        val intent2 =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()),
                oldRanges = listOf(TextRange(0, 2)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "ab",
                expectedNewText = "",
            )

        val composed = ComposeVisualRebase.composeOffsetMapChain(listOf(intent1, intent2))
        assertNotNull(
            "非空+空 chain composeOffsetMapChain 应非 null",
            composed,
        )
        assertTrue(
            "非空+空 chain composed 应为 emptyList()（T0→Tn 没有任何存活文字）\n" +
                "实际 composed=$composed",
            composed?.isEmpty() == true,
        )
    }

    /**
     * textKind 对抗：Insert + Delete chain 的 transactionTextKind 应为 Move。
     *
     * 场景：intent1 Insert（oldRanges=[], newRanges=[0,2]，T0="" → T1="xy"），
     * intent2 Delete（oldRanges=[0,2], newRanges=[]，T1="xy" → T2=""）。
     * mergedOldRanges 和 mergedNewRanges 都非空（从 composed map 补集算），
     * transactionTextKind 应为 Move（else 分支）。
     *
     * 这验证 textKind 按最终净变化决定，而不是从最后一笔 intent.textKind 读。
     * 最后一笔 intent2.textKind=Delete，但屏幕事务的净变化是 T0="" → T2=""（无变化），
     * 所以 transactionTextKind 应为 None（oldChanged 和 newChanged 都空）。
     *
     * 修正：T0="" → T2=""，composed map 为空（无存活），oldLength=0, newLength=0。
     * changedRangesFromComposedMap(empty, 0, 0) = FrameChangedRanges(emptyList(), emptyList())。
     * transactionTextKind = None（都空）。
     */
    @Test
    fun textKind_insertThenDelete_netNone() {
        val layouts = captureLayouts("", "xy", "")
        val oldLayout = layouts[0] // T0 = ""
        val newLayout = layouts[2] // T2 = ""

        val state = ComposeEditorVisualState(targetId = "test-target-textkind-insert-delete")

        // 1. 基线 layout "" 到达。
        state.onAuthoritativeLayout(oldLayout, TextRange(0, 0), 0)

        // 2. intent1: T0="" -> T1="xy"（Insert）
        val intent1 =
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 2)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 2),
                expectedOldText = "",
                expectedNewText = "xy",
            )
        state.onVisualIntent(intent1, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 3. intent2: T1="xy" -> T2=""（Delete）
        val intent2 =
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = VisualOffsetMap(entries = emptyList()),
                oldRanges = listOf(TextRange(0, 2)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 2, newStart = 0, newEnd = 0),
                expectedOldText = "xy",
                expectedNewText = "",
            )
        state.onVisualIntent(intent2, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 4. 新 layout "" 到达 → 生成事务。
        state.onAuthoritativeLayout(newLayout, TextRange(0, 0), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("事务应生成", transaction)

        // T0="" → T2=""，净变化为空，transactionTextKind 应为 None。
        // 最后一笔 intent2.textKind=Delete，但屏幕事务按最终净变化决定应为 None。
        // 这验证 textKind 从 transaction.textKind 读，而非从最后一笔 intent.textKind 读。
        val transactionTextKind = transaction?.textKind
        assertEquals(
            "Insert+Delete chain 净变化为空，transactionTextKind 应为 None\n" +
                "实际 transactionTextKind=$transactionTextKind\n" +
                "#684 评论 5664636035 Bug1：textKind 按最终净变化决定，不从最后一笔 intent 读",
            TextVisualKind.None,
            transactionTextKind,
        )
    }

    /**
     * textKind 对抗：Delete + Insert chain 的 transactionTextKind 应为 Move。
     *
     * 场景：intent1 Delete（T0="ab" → T1="a"，删 b），intent2 Insert（T1="a" → T2="ac"，插入 c）。
     * T0="ab" → T2="ac"，composed map: "a" IDENTITY [0,0,1]。
     * old changed ranges = [0,2) 中没被 [0,1) 覆盖的部分 = [1,2)（"b" 被删除）。
     * new changed ranges = [0,2) 中没被 [0,1) 覆盖的部分 = [1,2)（"c" 新增）。
     * mergedOldRanges=[1,2), mergedNewRanges=[1,2) 都非空 → transactionTextKind=Move。
     *
     * 最后一笔 intent2.textKind=Insert，但屏幕事务净变化是 Replace（删 b 插 c）→ Move。
     */
    @Test
    fun textKind_deleteThenInsert_netMove() {
        val layouts = captureLayouts("ab", "a", "ac")
        val oldLayout = layouts[0] // T0 = "ab"
        val newLayout = layouts[2] // T2 = "ac"

        val state = ComposeEditorVisualState(targetId = "test-target-textkind-delete-insert")

        // 1. 基线 layout "ab" 到达。
        state.onAuthoritativeLayout(oldLayout, TextRange(2, 2), 0)

        // 2. intent1: T0="ab" -> T1="a"（Delete "b"）
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
                                VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = listOf(TextRange(1, 2)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 1, oldEnd = 2, newStart = 1, newEnd = 1),
                expectedOldText = "ab",
                expectedNewText = "a",
            )
        state.onVisualIntent(intent1, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 3. intent2: T1="a" -> T2="ac"（Insert "c"）
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
                                VisualOffsetMapEntry(0, 0, 1, VisualOffsetMapKind.IDENTITY),
                            ),
                    ),
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(1, 2)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 1, oldEnd = 1, newStart = 1, newEnd = 2),
                expectedOldText = "a",
                expectedNewText = "ac",
            )
        state.onVisualIntent(intent2, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        // 4. 新 layout "ac" 到达 → 生成事务。
        state.onAuthoritativeLayout(newLayout, TextRange(2, 2), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("事务应生成", transaction)

        // T0="ab" → T2="ac"，净变化是 Replace（删 b 插 c）→ Move。
        // 最后一笔 intent2.textKind=Insert，但屏幕事务按最终净变化决定应为 Move。
        val transactionTextKind = transaction?.textKind
        assertEquals(
            "Delete+Insert chain 净变化是 Replace，transactionTextKind 应为 Move\n" +
                "实际 transactionTextKind=$transactionTextKind\n" +
                "#684 评论 5664636035 Bug1：textKind 按最终净变化决定，不从最后一笔 intent 读",
            TextVisualKind.Move,
            transactionTextKind,
        )
    }

    /**
     * Bug1 对抗：验证 transaction.oldRanges 确实从 composed map 补集算，而非 chain.flatMap。
     *
     * 这是端到端验证：构造两笔 Delete chain，检查 transaction.oldRanges 等于补集结果，
     * 而非 flatMap 结果。如果补丁被削弱（回退到 flatMap），此测试会 FAIL。
     */
    @Test
    fun bug1_transactionOldRanges_equalsComplementNotFlatMap() {
        val layouts = captureLayouts("abcde", "abde", "abe")
        val oldLayout = layouts[0]
        val newLayout = layouts[2]

        val state = ComposeEditorVisualState(targetId = "test-target-bug1-e2e")

        state.onAuthoritativeLayout(oldLayout, TextRange(3, 3), 0)

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
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                                VisualOffsetMapEntry(3, 2, 2, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 3, newStart = 2, newEnd = 2),
                expectedOldText = "abcde",
                expectedNewText = "abde",
            )
        state.onVisualIntent(intent1, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

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
                                VisualOffsetMapEntry(0, 0, 2, VisualOffsetMapKind.IDENTITY),
                                VisualOffsetMapEntry(3, 2, 1, VisualOffsetMapKind.SHIFTED),
                            ),
                    ),
                oldRanges = listOf(TextRange(2, 3)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 3, newStart = 2, newEnd = 2),
                expectedOldText = "abde",
                expectedNewText = "abe",
            )
        state.onVisualIntent(intent2, motionPolicy = EditorMotionPolicy(textDurationMillis = 100L))

        state.onAuthoritativeLayout(newLayout, TextRange(2, 2), 0)

        val transaction = state.activeTransaction.value
        assertNotNull("事务应生成", transaction)

        val actualOldRanges = transaction?.oldRanges ?: emptyList()
        // 正确行为：从 composed map 补集算，T0="abcde" → T2="abe"，
        // composed map: "ab" IDENTITY [0,0,2], "e" SHIFTED [4,2,1]。
        // old changed ranges = [0,5) 中没被 [0,2)+[4,5) 覆盖的部分 = [2,4)（"cd" 被删除）。
        val expectedComplementRanges = listOf(TextRange(2, 4))
        // 错误行为：chain.flatMap { it.oldRanges } = [2,3) + [2,3)
        val flatMapRanges = listOf(intent1, intent2).flatMap { it.oldRanges }

        assertEquals(
            "transaction.oldRanges 应等于 composed map 补集 [2,4)（cd 被删除），而非 chain.flatMap [2,3),[2,3)\n" +
                "实际 oldRanges=$actualOldRanges\n" +
                "补集期望=$expectedComplementRanges\n" +
                "flatMap 错误=$flatMapRanges\n" +
                "#684 评论 5664636035 Bug1：补丁应从 composed offset map 补集算",
            expectedComplementRanges,
            actualOldRanges,
        )
        assertFalse(
            "transaction.oldRanges 不应等于 chain.flatMap { it.oldRanges }（补丁未被削弱）",
            actualOldRanges == flatMapRanges,
        )
    }

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
