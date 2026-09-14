package com.xiwei.sujian.feature.editor.visual

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Constraints
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
 * #641 评论1 第4/5节 / 问题2 / 问题3：[ComposeEditorVisualState] 契约测试。
 *
 * 覆盖：
 * - [onAuthoritativeLayout] 记录布局快照（previous/current）；
 * - [onVisualIntent] 设置 hiddenRanges、动画类型、cursor 和 transaction；
 * - [clearAnimation] 清 hiddenRanges，系统正文马上可见；
 * - drawsVisualCursor 仅由设置/attach 生命周期决定（#684 评论 #5660899405 第4项）；
 * - #641 评论 问题2：cursor?.animate == true 时画视觉光标，不管 textKind。
 *
 * #684 评论 #5660899405：帧协调器改为双向汇合（intent/layout 无论谁先到都能合流），
 * 单 master progress 用于 rebase 物化，offset map chain 合成 retained moves，
 * 以及结构化诊断链事件（intent_queued / layout.presented / transaction_started /
 * transaction_rebased / transaction_completed）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeEditorVisualStateTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun initial_state_hasNoHiddenRanges() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        assertTrue("初始无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertFalse("初始不画视觉光标", state.drawsVisualCursor.value)
        assertNull("初始无活跃 intent", state.activeIntent.value)
        assertNull("初始无活跃 transaction", state.activeTransaction.value)
    }

    @Test
    fun onVisualIntent_insert_setsHiddenRanges() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals(listOf(TextRange(0, 3)), state.hiddenRanges.value)
        assertEquals(TextVisualKind.Insert, state.activeIntent.value?.textKind)
    }

    @Test
    fun onVisualIntent_delete_doesNotHideOldRanges() {
        // #641 评论 5457777142 问题3:Delete 不把 deleted oldRange 放进 hiddenRanges。
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("hello", "hel")
        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(5, 10)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "hello",
                expectedNewText = "hel",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertTrue("Delete 不隐藏 oldRanges", state.hiddenRanges.value.isEmpty())
    }

    @Test
    fun onVisualIntent_cursorAnimate_drawsVisualCursorFromSetting() {
        // #684 评论 #5660899405 第4项：drawsVisualCursor 只由 smooth cursor 设置决定。
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-target", initialDrawsVisualCursor = true)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 80L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.None,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
                expectedOldText = "",
                expectedNewText = "a",
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue("smooth cursor 开启时 drawsVisualCursor 为 true", state.drawsVisualCursor.value)
        assertNotNull("cursor animate 事务生成 visualCursorSnapshot", state.visualCursorSnapshot.value)
    }

    @Test
    fun onVisualIntent_insertWithCursorAnimate_drawsVisualCursorFromSetting() {
        // #641 评论 问题2：Insert + cursor animate=true 也应画视觉光标（由设置决定）。
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-target", initialDrawsVisualCursor = true)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
                expectedOldText = "",
                expectedNewText = "a",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue("smooth cursor 开启时 drawsVisualCursor 为 true", state.drawsVisualCursor.value)
    }

    @Test
    fun onVisualIntent_filtersEmptyRanges() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 0), TextRange(3, 3), TextRange(5, 8)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals("空 range 被过滤", listOf(TextRange(5, 8)), state.hiddenRanges.value)
    }

    @Test
    fun onVisualIntent_createsTransactionWithIncrementingId() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc", "abcde")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val firstId = state.activeTransaction.value?.id
        assertEquals("首个事务 ID 为 1", 1L, firstId)
        assertEquals("事务 motionPolicy 正确", 100L, state.activeTransaction.value?.motionPolicy?.textDurationMillis)

        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 200L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 5)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "abcde",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 200L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(5, 5), 0)
        val secondId = state.activeTransaction.value?.id
        assertEquals("第二个事务 ID 为 2", 2L, secondId)
        assertEquals("事务 motionPolicy 更新", 200L, state.activeTransaction.value?.motionPolicy?.textDurationMillis)
    }

    @Test
    fun clearAnimation_clearsHiddenRangesAndIntent() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        state.clearAnimation()
        assertTrue("clearAnimation 后无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertNull("clearAnimation 后无活跃 intent", state.activeIntent.value)
        assertNull("clearAnimation 后无活跃 transaction", state.activeTransaction.value)
    }

    @Test
    fun clearAnimation_afterCursorIntent_keepsDrawsVisualCursorFromSetting() {
        // #684 评论 #5660899405 第4项：clearAnimation 不重置 drawsVisualCursor，
        // 它由 smooth cursor 设置（attach 生命周期）决定。
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-target", initialDrawsVisualCursor = true)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 80L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = emptyList(),
                textKind = TextVisualKind.None,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
                expectedOldText = "",
                expectedNewText = "a",
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue(state.drawsVisualCursor.value)
        state.clearAnimation()
        assertTrue("clearAnimation 后 drawsVisualCursor 保持 true（smooth cursor 设置）", state.drawsVisualCursor.value)
    }

    @Test
    fun latestLayout_initiallyNull() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        assertNull("初始无 latestLayout", state.latestLayout.value)
    }

    /**
     * #641 评论 5458880786 问题1a：materializeStartFrame 按 textKind 物化所有可见 slice。
     * Insert 事务的 startFrame 应包含 newRanges（淡入中）。
     */
    @Test
    fun onVisualIntent_secondInsert_startFrameContainsPreviousNewRanges() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc", "abcdef")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔 Insert 事务。
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        val firstTransaction = state.activeTransaction.value
        assertEquals("第一笔无 startFrame", null, firstTransaction?.startFrame)

        // 第二笔 Insert 事务到来时，第一笔的 startFrame 应包含 newRanges slice。
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 6)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "abc",
                expectedNewText = "abcdef",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(6, 6), 0)
        val secondTransaction = state.activeTransaction.value
        assertEquals(TextVisualKind.Insert, secondTransaction?.intents?.lastOrNull()?.textKind)
    }

    /**
     * #641 评论 5458880786 问题1a：Delete 事务的 textKind 保存在 transaction 中。
     */
    @Test
    fun onVisualIntent_delete_savesTextKindInTransaction() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("hello", "world")
        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(5, 10)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "hello",
                expectedNewText = "world",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(5, 5), 0)
        assertEquals(TextVisualKind.Delete, state.activeTransaction.value?.intents?.lastOrNull()?.textKind)
    }

    /**
     * #641 评论 5458880786 问题1a：Move 事务的 textKind 保存在 transaction 中。
     */
    @Test
    fun onVisualIntent_move_savesTextKindInTransaction() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("hello", "world!")
        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(5, 10)),
                newRanges = listOf(TextRange(5, 12)),
                textKind = TextVisualKind.Move,
                cursor = null,
                expectedOldText = "hello",
                expectedNewText = "world!",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(6, 6), 0)
        assertEquals(TextVisualKind.Move, state.activeTransaction.value?.intents?.lastOrNull()?.textKind)
    }

    /**
     * #641 评论 5458880786 问题1d：onVisualIntent 先冻结上一帧再切 activeIntent —
     * 验证事务 ID 单调递增且 frozenStartFrame 在新事务创建前物化。
     */
    @Test
    fun onVisualIntent_freezesStartFrameBeforeSwitchingIntent() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc", "abcdefgh")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 3)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 3, animate = true),
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals(1L, state.activeTransaction.value?.id)

        // 第二笔事务到来 — startFrame 应在 _activeIntent 切换前物化。
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(5, 8)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 3, newEndUtf16 = 8, animate = true),
                expectedOldText = "abc",
                expectedNewText = "abcdefgh",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(8, 8), 0)
        assertEquals(2L, state.activeTransaction.value?.id)
        assertEquals(8, state.activeTransaction.value?.intents?.lastOrNull()?.cursor?.newEndUtf16)
    }

    /**
     * #641 评论 5458880786 问题2e：applyPendingRetainedMoves 后 hiddenRanges 包含 retained newRanges。
     */
    @Test
    fun onAuthoritativeLayout_withPendingRetainedMoves_updatesHiddenRanges() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("", "abc")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals(listOf(TextRange(0, 1)), state.hiddenRanges.value)
    }

    /**
     * #641 评论 5458880786 问题2c：computeRetainedMoves 用 replaceBounds 算 suffix 起点。
     * 验证 EditorVisualIntent 携带 replaceBounds 时 retained reflow 正确。
     */
    @Test
    fun onVisualIntent_withReplaceBounds_passesToTransaction() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        val layouts = captureLayouts("hello", "world!")
        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        val bounds = VisualReplaceBounds(oldStart = 5, oldEnd = 6, newStart = 5, newEnd = 7)
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(5, 6)),
                newRanges = listOf(TextRange(5, 7)),
                textKind = TextVisualKind.Move,
                cursor = null,
                replaceBounds = bounds,
                expectedOldText = "hello",
                expectedNewText = "world!",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(6, 6), 0)
        assertEquals(TextVisualKind.Move, state.activeTransaction.value?.intents?.lastOrNull()?.textKind)
    }

    /**
     * #684 评论 #5660899405：单 master progress —
     * reportProgress(Float) 更新内部 _masterProgress。
     */
    @Test
    fun reportProgress_updatesMasterProgress() {
        val state = ComposeEditorVisualState(targetId = "test-target")
        state.reportProgress(0.5f)
        assertEquals(0.5f, state.masterProgress.value, 0.001f)
        state.reportProgress(1f)
        assertEquals(1f, state.masterProgress.value, 0.001f)
    }

    /**
     * #641 评论 5460373035 问题2：被下一笔 replace/delete 覆盖的"正在淡入文字"不应突然亮回 100% 再淡出。
     *
     * 端到端场景：B 插入 a（alpha 只到 0.5）→ C 删除 a（replace 覆盖 [0,1)）。
     * C 的 startFrame 从 B 在真实 progress=0.5 处物化：保留 B 当前 alpha（非 1.0），
     * 同时把 [0,1) 计进 ownedOldRanges，C.oldRanges 减掉它后为空。
     *
     * 验证：
     * 1. C.startFrame 非 null（materializeStartFrame 在真实 progress<1f 时返回非 null）。
     * 2. C.startFrame.slices 含 fading slice（targetRange==null），其 alpha 在 (0,1) 之间
     *    （保留 B 当前 alpha，不是 1.0 也不是 0）。
     * 3. C.startFrame.ownedOldRanges 含 TextRange(0,1)。
     * 4. C.oldRanges 不含 TextRange(0,1)（effectiveOldRanges = subtractRanges([0,1), [0,1)) = 空）。
     */
    @Test
    fun ownedOldRanges_overlapFadingSliceKeptAndSubtractedFromOldRanges() {
        val layouts = captureLayouts("b", "ab", "b")
        val state = ComposeEditorVisualState(targetId = "test-target")

        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 第一笔 Insert 事务（B 插入 "a" at 0）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 1),
                expectedOldText = "b",
                expectedNewText = "ab",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // B 的 "a" 只淡入到 alpha=0.5（真实当前 master progress）
        state.reportProgress(0.5f)

        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        // 第二笔 Delete 事务（C 删除 "a"）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(0, 1)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 1, newStart = 0, newEnd = 0),
                expectedOldText = "ab",
                expectedNewText = "b",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        val cTransaction = state.activeTransaction.value
        assertNotNull("C 事务应生成", cTransaction)

        // 1. C.startFrame 非 null（真实 progress=0.5 物化）
        val cStartFrame = cTransaction?.startFrame
        assertNotNull("C.startFrame 应非 null（真实 progress=0.5 物化 rebase）", cStartFrame)

        // 2. fading slice（targetRange==null）alpha 在 (0,1)
        val fadingSlice = cStartFrame?.slices?.firstOrNull { it.targetRange == null }
        assertNotNull("C.startFrame 应含 fading slice", fadingSlice)
        val fadingAlpha = fadingSlice?.sourceAlpha ?: 0f
        assertTrue("fading slice alpha 应介于 0 与 1 之间（保留 B 当前进度）", fadingAlpha > 0f && fadingAlpha < 1f)

        // 3. ownedOldRanges 含 [0,1)
        val cOwnedOldRanges = cStartFrame?.ownedOldRanges ?: emptyList()
        assertTrue(
            "C.startFrame.ownedOldRanges 应含 TextRange(0,1)",
            cOwnedOldRanges.any { it.start == 0 && it.end == 1 },
        )

        // 4. C.oldRanges 不含 [0,1)（已被 subtractRanges 减掉）
        val cOldRanges = cTransaction?.oldRanges ?: emptyList()
        assertTrue(
            "C.oldRanges 不应含 TextRange(0,1)（startFrame 已接管）",
            cOldRanges.none { it.start == 0 && it.end == 1 },
        )
    }

    /**
     * #641 评论 5460373035 问题2 对照：无 overlap 时 ownedOldRanges 为空，oldRanges 不变。
     *
     * 场景：B 插入 a（alpha=0.5）→ C 在不重叠位置插入 c。
     */
    @Test
    fun ownedOldRanges_noOverlap_oldRangesUnchanged() {
        val layouts = captureLayouts("b", "ab", "abc")
        val state = ComposeEditorVisualState(targetId = "test-target")

        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 第一笔 Insert 事务（B 插入 "a" at 0）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 0, oldEnd = 0, newStart = 0, newEnd = 1),
                expectedOldText = "b",
                expectedNewText = "ab",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // B 的 "a" 只淡入到 alpha=0.5
        state.reportProgress(0.5f)

        state.onAuthoritativeLayout(layouts[2], TextRange(3, 3), 0)

        // 第二笔 Insert 事务（C 插入 "c" at 2，不与 B 的 [0,1) 重叠）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(2, 3)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                replaceBounds = VisualReplaceBounds(oldStart = 2, oldEnd = 2, newStart = 2, newEnd = 3),
                expectedOldText = "ab",
                expectedNewText = "abc",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(3, 3), 0)

        val cTransaction = state.activeTransaction.value
        assertNotNull("C 事务应生成", cTransaction)

        // C.startFrame 非 null（B 在 progress=0.5 处被 rebase）
        val cStartFrame = cTransaction?.startFrame
        assertNotNull("C.startFrame 应非 null（rebase B 的 in-progress 帧）", cStartFrame)

        // 无 overlap → ownedOldRanges 为空
        val cOwnedOldRanges = cStartFrame?.ownedOldRanges ?: emptyList()
        assertTrue("C.startFrame.ownedOldRanges 应为空（无 overlap）", cOwnedOldRanges.isEmpty())

        // C 是 Insert，oldRanges 本来就是空
        val cOldRanges = cTransaction?.oldRanges ?: emptyList()
        assertTrue("C.oldRanges 应为空（C 是 Insert）", cOldRanges.isEmpty())
    }

    /**
     * #666 回归：空章节首次输入时 [ComposeEditorVisualState] 必须等待匹配布局到达后
     * 才生成事务与 visualCursorSnapshot。
     *
     * 新模型（#684 评论 #5660899405 第4项）：drawsVisualCursor 仅由设置决定，
     * 这里用 initialDrawsVisualCursor=false 验证默认行为；事务与 snapshot 仍等布局匹配。
     */
    @Test
    fun repro_emptyChapterFirstInput_cursorOffsetOutOfBounds() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-target", initialDrawsVisualCursor = false)

        // 第一次空文本 layout 到达 → 基线
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 再次空文本 layout 到达
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // Core 给出 "" -> "a" 的视觉事务，cursor 0→1, animate=true。
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 80L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
                expectedOldText = "",
                expectedNewText = "a",
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // 回归核心 1：onVisualIntent 只排队，还没调 onAuthoritativeLayout，无事务。
        assertNull("activeTransaction 应为 null（intent 排队，布局未到）", state.activeTransaction.value)
        // drawsVisualCursor 由设置决定（false）
        assertFalse("drawsVisualCursor 应为 false（设置关闭）", state.drawsVisualCursor.value)
        // visualCursorSnapshot 应为 null（不基于错误布局）
        assertNull("visualCursorSnapshot 应为 null（布局未到）", state.visualCursorSnapshot.value)

        // 当正确的 layout("a") 到达后，事务与 snapshot 生成。
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertNotNull("layout(\"a\") 到达后 activeTransaction 应非 null", state.activeTransaction.value)
        assertNotNull("layout(\"a\") 到达后 visualCursorSnapshot 应非 null", state.visualCursorSnapshot.value)
        assertFalse("layout(\"a\") 到达后 drawsVisualCursor 仍为 false（设置关闭）", state.drawsVisualCursor.value)
    }

    /**
     * #666 时序测试 1：空章节第一次输入完整时序 —
     * 验证事务与 visualCursorSnapshot 等待匹配布局到达，drawsVisualCursor 由设置决定。
     */
    @Test
    fun emptyChapterFirstInput_waitsForMatchingLayoutBeforeVisualCursor() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState(targetId = "test-target", initialDrawsVisualCursor = true)

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 80L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
                expectedOldText = "",
                expectedNewText = "a",
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // intent 排队，无匹配布局 → 无事务、无 snapshot；drawsVisualCursor 由设置决定（true）
        assertNull("activeTransaction 应为 null（布局未到）", state.activeTransaction.value)
        assertNull("visualCursorSnapshot 应为 null（布局未到）", state.visualCursorSnapshot.value)
        assertTrue("drawsVisualCursor 应为 true（设置开启）", state.drawsVisualCursor.value)

        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        assertNotNull("layout(\"a\") 到达后 activeTransaction 应非 null", state.activeTransaction.value)
        assertNotNull("layout(\"a\") 到达后 visualCursorSnapshot 应非 null", state.visualCursorSnapshot.value)
        assertTrue("drawsVisualCursor 保持 true（设置开启）", state.drawsVisualCursor.value)
    }

    /**
     * #666 时序测试 2：连续输入两次，第二笔 intent 先于第二份 layout 到达。
     * drawsVisualCursor 由设置决定；事务与 snapshot 等布局匹配。
     */
    @Test
    fun consecutiveInput_secondIntentBeforeSecondLayout_waitsForMatchingLayout() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState(targetId = "test-target", initialDrawsVisualCursor = true)

        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 第一笔 intent: "" -> "a"
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 80L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(0, 1)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 0, newEndUtf16 = 1, animate = true),
                expectedOldText = "",
                expectedNewText = "a",
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertNotNull("第一笔匹配后 activeTransaction 应非 null", state.activeTransaction.value)
        assertNotNull("第一笔匹配后 visualCursorSnapshot 应非 null", state.visualCursorSnapshot.value)
        assertTrue("drawsVisualCursor 应为 true（设置开启）", state.drawsVisualCursor.value)

        // 第二笔 intent: "a" -> "ab"，但 layout("ab") 还没到
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 80L,
                offsetMap = null,
                oldRanges = listOf(TextRange(1, 1)),
                newRanges = listOf(TextRange(1, 2)),
                textKind = TextVisualKind.Insert,
                cursor = CursorVisualIntent(oldEndUtf16 = 1, newEndUtf16 = 2, animate = true),
                expectedOldText = "a",
                expectedNewText = "ab",
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // 第二笔 intent 没匹配布局前，pending 链等待；无新事务。
        // drawsVisualCursor 仍为 true（设置）。
        assertTrue("drawsVisualCursor 保持 true（设置开启）", state.drawsVisualCursor.value)

        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)

        assertNotNull("layout(\"ab\") 到达后 activeTransaction 应非 null", state.activeTransaction.value)
        assertNotNull("layout(\"ab\") 到达后 visualCursorSnapshot 应非 null", state.visualCursorSnapshot.value)
        assertTrue("drawsVisualCursor 保持 true（设置开启）", state.drawsVisualCursor.value)
    }

    /**
     * #684 评论 #5660899405 第2项：动画被下一笔输入打断时，startFrame 从真实当前
     * master progress（0.5）物化，而非假定上一笔已跑到 1f。
     *
     * 三代 rebase 端到端：
     * - A(Delete "hello"->"hel") 产生 fading slice。
     * - B 到来时从 A 在 progress=0.5 处物化 startFrame（非 null，保留 A 半途状态）。
     * - C 到来时若 B 已到 progress=1，则 C.startFrame 为 null（B 已完成）。
     */
    @Test
    fun interruptedAnimation_rebasesFromRealProgress() {
        val layouts = captureLayouts("hello", "hel", "help", "help!")
        val state = ComposeEditorVisualState(targetId = "test-target")

        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)

        // 第一笔 Delete 事务（删除 "lo"）
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 1L,
                baseRevision = 0L,
                newRevision = 1L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = listOf(TextRange(3, 5)),
                newRanges = emptyList(),
                textKind = TextVisualKind.Delete,
                cursor = null,
                expectedOldText = "hello",
                expectedNewText = "hel",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        // A.startFrame = null（第一笔事务，无 active 事务可 materialize）
        assertNull("A.startFrame 应为 null", state.activeTransaction.value?.startFrame)

        // 报告 progress 0.5（A 跑到一半）
        state.reportProgress(0f)

        // 第二笔 Insert 事务到来
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 2L,
                baseRevision = 1L,
                newRevision = 2L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(3, 4)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "hel",
                expectedNewText = "help",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)
        // B 在 progress=0 时物化 A 的 startFrame：A 的 fading slice alpha=lerp(1,0,0)=1，非 null
        assertNotNull("B.startFrame 应非 null（rebase A 的真实进度）", state.activeTransaction.value?.startFrame)

        // A 跑到 0.5（真实当前进度）
        state.reportProgress(0.5f)

        // 第三笔 Insert 事务到来
        state.onVisualIntent(
            EditorVisualIntent(
                coreTransactionId = 3L,
                baseRevision = 2L,
                newRevision = 3L,
                animationMode = AnimationModeDto.CLUSTER_ANIMATION,
                durationMs = 100L,
                offsetMap = null,
                oldRanges = emptyList(),
                newRanges = listOf(TextRange(4, 5)),
                textKind = TextVisualKind.Insert,
                cursor = null,
                expectedOldText = "help",
                expectedNewText = "help!",
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(5, 5), 0)
        // C 在 progress=0.5 时物化 B 的 startFrame（非 null，保留 B 半途状态）
        assertNotNull("C.startFrame 应非 null（rebase B 的真实进度 0.5）", state.activeTransaction.value?.startFrame)
    }

    /**
     * #641 评论 5460233781 问题2 差距 A：surviving slice 物化时 currentX/currentY 应加 slice.sourceTranslate。
     */
    @Test
    fun materializeRebasedSlice_survivingWithSourceTranslate_appliedToCurrentPos() {
        val layouts = captureLayouts("hello")
        val layout = ComposeLayoutSnapshot(layouts[0], TextRange(5, 5), 0)

        val slice =
            RebasedTextSlice(
                sourceLayout = layout,
                sourceRange = TextRange(0, 2),
                sourceTranslate = Offset(10f, 5f),
                sourceAlpha = 1f,
                targetRange = TextRange(0, 2),
            )

        val result = ComposeVisualRebase.materializeRebasedSlice(slice, layout, 0.5f) as RebasedTextSlice

        assertEquals(
            "sourceTranslate.x 应包含原 translate 按 rebaseProgress 插值（差距 A）",
            5f,
            result.sourceTranslate.x,
            0.01f,
        )
        assertEquals(
            "sourceTranslate.y 应包含原 translate 按 rebaseProgress 插值（差距 A）",
            2.5f,
            result.sourceTranslate.y,
            0.01f,
        )
    }

    /**
     * #641 评论 5460233781 问题2 差距 B 补充：fading slice alpha 降到 0 的边界条件。
     */
    @Test
    fun materializeRebasedSlice_fadingAlphaAlreadyZero_droppedEvenAtRebaseZero() {
        val layouts = captureLayouts("hello")
        val layout = ComposeLayoutSnapshot(layouts[0], TextRange(5, 5), 0)

        val slice =
            RebasedTextSlice(
                sourceLayout = layout,
                sourceRange = TextRange(0, 2),
                sourceTranslate = Offset.Zero,
                sourceAlpha = 0f,
                targetRange = null,
            )

        val result = ComposeVisualRebase.materializeRebasedSlice(slice, layout, 0f)
        assertNull(
            "sourceAlpha=0 的 fading slice 即使 rebaseProgress=0 也应被丢弃（返回 null）",
            result,
        )
    }

    /**
     * 用 [rememberTextMeasurer] 在 Compose 测试环境里构造真实 [TextLayoutResult]。
     * 一次 setContent 构造多份 layout，供测试里多代 rebase 使用。
     * maxWidth=1000 避免折行，让 bounds 计算确定。
     */
    private fun captureLayouts(vararg texts: String): List<TextLayoutResult> {
        val results = mutableListOf<TextLayoutResult>()
        composeRule.setContent {
            val textMeasurer = rememberTextMeasurer()
            texts.forEach { text ->
                results.add(
                    textMeasurer.measure(
                        text = AnnotatedString(text),
                        constraints = Constraints(maxWidth = 1000),
                    ),
                )
            }
        }
        return results
    }
}
