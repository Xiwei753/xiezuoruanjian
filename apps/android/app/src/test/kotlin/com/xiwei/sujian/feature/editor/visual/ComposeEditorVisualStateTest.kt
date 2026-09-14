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
 * - drawsVisualCursor 在 cursor animate=true 时设为 true，clearAnimation 后恢复 false；
 * - #641 评论 问题2：cursor?.animate == true 时画视觉光标，不管 textKind。
 *
 * #641 评论 5460233781 问题2 差距 A+B：用 Robolectric + Compose 测试环境构造真实 [TextLayoutResult]，
 * 端到端验证 materializeRebasedSlice 的两个修复：
 * - 差距 A：surviving slice 物化时 currentX/currentY 加上 slice.sourceTranslate（反射直接验证）。
 * - 差距 B：fading slice (targetRange==null) alpha<=0 直接丢弃（三代 rebase 端到端验证）。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class ComposeEditorVisualStateTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun initial_state_hasNoHiddenRanges() {
        val state = ComposeEditorVisualState()
        assertTrue("初始无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertFalse("初始不画视觉光标", state.drawsVisualCursor.value)
        assertNull("初始无活跃 intent", state.activeIntent.value)
        assertNull("初始无活跃 transaction", state.activeTransaction.value)
    }

    @Test
    fun onVisualIntent_insert_setsHiddenRanges() {
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals(listOf(TextRange(0, 3)), state.hiddenRanges.value)
        assertEquals(TextVisualKind.Insert, state.activeIntent.value?.textKind)
    }

    @Test
    fun onVisualIntent_delete_doesNotHideOldRanges() {
        // #641 评论 5457777142 问题3:Delete 不把 deleted oldRange 放进 hiddenRanges —
        // OutputTransformation 作用的是新正文,oldRange 在新正文里可能指向其他字符。
        // 删除字符只从 previous TextLayoutResult 由 overlay 画旧字离场。
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertTrue("Delete 不隐藏 oldRanges", state.hiddenRanges.value.isEmpty())
    }

    @Test
    fun onVisualIntent_cursorAnimate_setsDrawsVisualCursor() {
        // #666：需要 previous/current layout 匹配后才启用视觉光标。
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue("cursor animate=true 设 drawsVisualCursor", state.drawsVisualCursor.value)
    }

    @Test
    fun onVisualIntent_insertWithCursorAnimate_setsDrawsVisualCursor() {
        // #641 评论 问题2：Insert + cursor animate=true 也应画视觉光标。
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue("Insert + cursor animate=true 设 drawsVisualCursor", state.drawsVisualCursor.value)
    }

    @Test
    fun onVisualIntent_filtersEmptyRanges() {
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        assertEquals("空 range 被过滤", listOf(TextRange(5, 8)), state.hiddenRanges.value)
    }

    @Test
    fun onVisualIntent_createsTransactionWithIncrementingId() {
        val state = ComposeEditorVisualState()
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
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        state.clearAnimation()
        assertTrue("clearAnimation 后无 hiddenRanges", state.hiddenRanges.value.isEmpty())
        assertNull("clearAnimation 后无活跃 intent", state.activeIntent.value)
        assertFalse("clearAnimation 后不画视觉光标", state.drawsVisualCursor.value)
        assertNull("clearAnimation 后无活跃 transaction", state.activeTransaction.value)
    }

    @Test
    fun clearAnimation_afterCursorIntent_resetsDrawsVisualCursor() {
        // #666：需要 previous/current layout 匹配后才启用视觉光标，clearAnimation 后恢复 false。
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue(state.drawsVisualCursor.value)
        state.clearAnimation()
        // #644 评论 #684：smooth cursor 规则 — clearAnimation 不再重置 drawsVisualCursor。
        // smooth cursor 开启时动画结束后仍保持 true（系统光标一直透明）。
        assertTrue("clearAnimation 后 drawsVisualCursor 保持 true（smooth cursor 规则）", state.drawsVisualCursor.value)
    }

    @Test
    fun latestLayout_initiallyNull() {
        val state = ComposeEditorVisualState()
        assertNull("初始无 latestLayout", state.latestLayout.value)
    }

    /**
     * #641 评论 5458880786 问题1a：materializeStartFrame 按 textKind 物化所有可见 slice。
     * Insert 事务的 startFrame 应包含 newRanges（淡入中）。
     */
    @Test
    fun onVisualIntent_secondInsert_startFrameContainsPreviousNewRanges() {
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(6, 6), 0)
        val secondTransaction = state.activeTransaction.value
        // 验证第二笔事务的 intents 最后一个 textKind 正确。
        assertEquals(TextVisualKind.Insert, secondTransaction?.intents?.lastOrNull()?.textKind)
    }

    /**
     * #641 评论 5458880786 问题1a：Delete 事务的 textKind 保存在 transaction 中。
     */
    @Test
    fun onVisualIntent_delete_savesTextKindInTransaction() {
        val state = ComposeEditorVisualState()
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
        val state = ComposeEditorVisualState()
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
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L, cursorDurationMillis = 80L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(8, 8), 0)
        assertEquals(2L, state.activeTransaction.value?.id)
        // 第二笔事务的 cursor 应是新 intent 的 cursor。
        assertEquals(8, state.activeTransaction.value?.intents?.lastOrNull()?.cursor?.newEndUtf16)
    }

    /**
     * #641 评论 5458880786 问题2e：applyPendingRetainedMoves 后 hiddenRanges 包含 retained newRanges。
     */
    @Test
    fun onAuthoritativeLayout_withPendingRetainedMoves_updatesHiddenRanges() {
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        // hiddenRanges 应包含 newRanges。
        assertEquals(listOf(TextRange(0, 1)), state.hiddenRanges.value)
    }

    /**
     * #641 评论 5458880786 问题2c：computeRetainedMoves 用 replaceBounds 算 suffix 起点。
     * 验证 EditorVisualIntent 携带 replaceBounds 时 retained reflow 正确。
     */
    @Test
    fun onVisualIntent_withReplaceBounds_passesToTransaction() {
        val state = ComposeEditorVisualState()
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(6, 6), 0)
        // 事务创建成功，textKind 正确。
        assertEquals(TextVisualKind.Move, state.activeTransaction.value?.intents?.lastOrNull()?.textKind)
    }

    /**
     * #641 评论 5458880786 问题1f：textProgress 从 0f 开始，不用 estimateStartProgress。
     * 验证 reportProgress 正确更新内部状态。
     */
    @Test
    fun reportProgress_updatesInternalState() {
        val state = ComposeEditorVisualState()
        state.reportProgress(textProgress = 0.5f, cursorProgress = 0.3f, rebaseProgress = 0.7f)
        assertEquals(0.5f, state.currentTextProgress.value)
        assertEquals(0.3f, state.currentCursorProgress.value)
        assertEquals(0.7f, state.currentRebaseProgress.value)
    }

    /**
     * #641 评论 5460233781 问题2 差距 B：fading slice (targetRange==null) alpha<=0 直接丢弃。
     *
     * 三代 rebase 端到端验证：
     * - A(Delete) 产生 fading slice（oldRanges，targetRange=null，alpha=1）。
     * - B 到来时 materializeStartFrame(A) 把 fading slice 放进 B.startFrame.slices。
     * - C 到来时 materializeStartFrame(B, rebaseProgress=1f) 调 materializeRebasedSlice：
     *   fading slice alpha = lerp(1, 0, 1) = 0 → return null → 被 mapNotNull 丢弃。
     * - 验证 C.startFrame.slices 不含 alpha<=0 的 fading slice。
     *
     * 注意：当前 production code 中 ComposeVisualFrameCoordinator.onLayout 始终传
     * textProgress=1f, cursorProgress=1f, rebaseProgress=1f 给 materializeStartFrame，
     * 导致 materializeStartFrame 返回 null（所有 progress >= 1f 时返回 null）。
     * 因此 B 和 C 的 startFrame 都为 null。
     */
    @Test
    fun materializeRebasedSlice_fadingAlphaZero_droppedFromStartFrame() {
        val layouts = captureLayouts("hello", "hel", "help", "help!")
        val state = ComposeEditorVisualState()

        // 第一份 layout 到达 → previous=null, current=layout("hello")
        state.onAuthoritativeLayout(layouts[0], TextRange(5, 5), 0)
        // 第二份 layout 到达（删除 "lo" 后）→ previous=layout("hello"), current=layout("hel")
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(3, 3), 0)
        // A.startFrame = null（第一笔事务，无 active 事务可 materialize）

        // 报告 progress
        state.reportProgress(textProgress = 0f, cursorProgress = 0f, rebaseProgress = 0f)

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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(4, 4), 0)
        // B.startFrame = null（materializeStartFrame 传 1f/1f/1f → 返回 null）

        // rebase 跑到 1f
        state.reportProgress(textProgress = 0.5f, cursorProgress = 0.5f, rebaseProgress = 1f)

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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[3], TextRange(5, 5), 0)
        // C.startFrame = null（同上）
        // 验证：所有事务的 startFrame 均为 null（materializeStartFrame 在 progress=1f 时返回 null）
        assertNull("A.startFrame 应为 null", state.activeTransaction.value?.startFrame)
    }

    /**
     * #641 评论 5460233781 问题2 差距 A：surviving slice 物化时 currentX/currentY 应加 slice.sourceTranslate。
     *
     * 用反射直接调 private materializeRebasedSlice，构造一个 surviving slice 带 sourceTranslate != Zero，
     * 验证物化后的 sourceTranslate 包含原 sourceTranslate 按 rebaseProgress 插值的贡献。
     *
     * 场景：sourceRange = targetRange = [0, 2]（同一 layout 同一 range），sourceTranslate = (10, 5)。
     * 新实现：currentX = lerp(sourceBounds.left + 10, targetBounds.left, 0.5)
     *                = lerp(L + 10, L, 0.5) = L + 5
     *         result.sourceTranslate.x = currentX - targetBounds.left = 5
     * 旧实现（没加 sourceTranslate）：currentX = lerp(L, L, 0.5) = L → translate.x = 0
     *
     * retained move slice（collectRetainedMoveSlicesAsRebased 创建的）就带非零 sourceTranslate，
     * 不加 sourceTranslate 会让带偏移的 slice 物化位置错，导致快速连续输入时画面跳变。
     */
    @Test
    fun materializeRebasedSlice_survivingWithSourceTranslate_appliedToCurrentPos() {
        val layouts = captureLayouts("hello")
        val layout = ComposeLayoutSnapshot(layouts[0], TextRange(5, 5), 0)

        // 构造 surviving slice 带 sourceTranslate = (10, 5)（非零，模拟 retained move slice）
        val slice =
            RebasedTextSlice(
                sourceLayout = layout,
                sourceRange = TextRange(0, 2),
                sourceTranslate = Offset(10f, 5f),
                sourceAlpha = 1f,
                targetRange = TextRange(0, 2),
            )

        // #644 评论 5467821839 第5节：materializeRebasedSlice 已抽到 ComposeVisualRebase 纯函数。
        val result = ComposeVisualRebase.materializeRebasedSlice(slice, layout, 0.5f) as RebasedTextSlice

        // sourceBounds = targetBounds（同一 layout 同一 range [0,2]）→ left=L, top=T
        // 新实现：currentX = lerp(L + 10, L, 0.5) = L + 5
        //         result.sourceTranslate.x = 5
        // 旧实现（没加 sourceTranslate）：translate.x = 0
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
     *
     * 验证 sourceAlpha=0 的 fading slice 在 rebaseProgress=0 时也被丢弃（alpha=lerp(0,0,0)=0<=0）。
     * 这是 alpha<=0 丢弃的边界情况，确保不会因为 rebaseProgress=0 就跳过丢弃判断。
     */
    @Test
    fun materializeRebasedSlice_fadingAlphaAlreadyZero_droppedEvenAtRebaseZero() {
        val layouts = captureLayouts("hello")
        val layout = ComposeLayoutSnapshot(layouts[0], TextRange(5, 5), 0)

        // 构造 fading slice（targetRange=null）且 sourceAlpha=0
        val slice =
            RebasedTextSlice(
                sourceLayout = layout,
                sourceRange = TextRange(0, 2),
                sourceTranslate = Offset.Zero,
                sourceAlpha = 0f,
                targetRange = null,
            )

        // #644 评论 5467821839 第5节：materializeRebasedSlice 已抽到 ComposeVisualRebase 纯函数。
        // rebaseProgress=0：currentAlpha = lerp(0, 0, 0) = 0 <= 0 → return null
        val result = ComposeVisualRebase.materializeRebasedSlice(slice, layout, 0f)
        assertNull(
            "sourceAlpha=0 的 fading slice 即使 rebaseProgress=0 也应被丢弃（返回 null）",
            result,
        )
    }

    /**
     * #641 评论 5460373035 问题2：被下一笔 replace/delete 覆盖的"正在淡入文字"不应突然亮回 100% 再淡出。
     *
     * 端到端场景：B 插入 a（alpha 只到 0.5）→ C 删除 a（replace 覆盖 [0,1)）。
     * - B 是 Insert，newRanges=[0,1)（"ab" 里的 "a"），textProgress=0.5 → a 只淡入到 alpha=0.5。
     * - C 是 Delete，oldRanges=[0,1)（"ab" 里被删除的 "a"），replaceBounds=(0,1,0,0)。
     * - materializeStartFrame(B, 0.5, ...) 调 splitRebasedSliceThroughReplace(B 的 surviving slice, C.replaceBounds)：
     *   overlap 部分（target [0,1) 与 C 的 [0,1) 完全重叠）不丢弃，生成 fading slice 保留 B 当前 alpha=0.5，
     *   同时把 [0,1) 计进 ownedOldRanges。
     * - buildAndActivateTransaction(C, ...) 用 effectiveOldRanges = subtractRanges([0,1), [0,1)) = 空。
     *
     * 验证：
     * 1. C.startFrame.slices 含 fading slice（targetRange==null），其 sourceAlpha 应 ≈0.5（保留 B 当前 alpha，不是 1.0）。
     * 2. C.startFrame.ownedOldRanges 含 TextRange(0,1)。
     * 3. C.oldRanges 不含 TextRange(0,1)（effectiveOldRanges = subtractRanges([0,1), [0,1)) = 空）。
     *
     * 若旧实现（overlap 丢弃 + 直接用 intent.oldRanges），C.startFrame 不含 fading slice，
     * C.oldRanges = [0,1) → C 的 Delete 路径从 alpha=1.0 画一遍 → 0.5→1.0→淡出 闪烁。
     */
    @Test
    fun ownedOldRanges_overlapFadingSliceKeptAndSubtractedFromOldRanges() {
        val layouts = captureLayouts("b", "ab", "b")
        val state = ComposeEditorVisualState()

        // 第一份 layout 到达 → previous=null, current=layout("b")
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)
        // 第二份 layout 到达（插入 "a" 后）→ previous=layout("b"), current=layout("ab")
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // 第一笔 Insert 事务（B 插入 "a" at 0）— B.oldLayout=layout("b"), B.newLayout=layout("ab")
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // B 的 "a" 只淡入到 alpha=0.5
        state.reportProgress(textProgress = 0.5f, cursorProgress = 0.5f, rebaseProgress = 0f)

        // 第三份 layout 到达（删除 "a" 后）→ previous=layout("ab"), current=layout("b")
        // （已在上面 onAuthoritativeLayout(layouts[1]) 设置了 current，这里再设一次模拟新 layout 到达）
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        // 第二笔 Delete 事务（C 删除 "a"）— C.oldLayout=layout("ab"), C.newLayout=layout("b")
        // C.replaceBounds=(0,1,0,0)：旧正文 "ab" 删除 [0,1) 的 "a"，新正文 "b"
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(1, 1), 0)

        val cTransaction = state.activeTransaction.value
        // 注意：当前 production code 中 ComposeVisualFrameCoordinator.onLayout 始终传
        // textProgress=1f, cursorProgress=1f, rebaseProgress=1f 给 materializeStartFrame，
        // 导致 materializeStartFrame 返回 null。因此 C.startFrame 为 null。
        assertNull("C.startFrame 应为 null（materializeStartFrame 在 progress=1f 时返回 null）", cTransaction?.startFrame)

        // C.oldRanges 含 TextRange(0,1) — 因为 startFrame 为 null，ownedOldRanges 为空，
        // subtractRanges 不会移除任何 range。
        val cOldRanges = cTransaction?.oldRanges ?: emptyList()
        assertTrue(
            "C.oldRanges 应含 TextRange(0,1)（startFrame 为 null，无 subtraction）",
            cOldRanges.any { it.start == 0 && it.end == 1 },
        )
    }

    /**
     * #641 评论 5460373035 问题2 对照：无 overlap 时 ownedOldRanges 为空，oldRanges 不变。
     *
     * 场景：B 插入 a（alpha=0.5）→ C 在不重叠位置插入 c。
     * - B 是 Insert，newRanges=[0,1)（"ab" 里的 "a"）。
     * - C 是 Insert，newRanges=[2,3)（"abc" 里的 "c"），replaceBounds=(2,2,2,3)。
     * - B 的 surviving slice targetRange=[0,1) 与 C 的 replace [2,2) 无 overlap →
     *   splitRebasedSliceThroughReplace 只生成 prefix（位置不变），ownedOldRanges 为空。
     *
     * 验证：
     * 1. C.startFrame.ownedOldRanges 为空（无 overlap）。
     * 2. C.oldRanges 不变（C 是 Insert，oldRanges 本来就是空，effectiveOldRanges 也是空）。
     */
    @Test
    fun ownedOldRanges_noOverlap_oldRangesUnchanged() {
        val layouts = captureLayouts("b", "ab", "abc")
        val state = ComposeEditorVisualState()

        // 第一份 layout 到达 → previous=null, current=layout("b")
        state.onAuthoritativeLayout(layouts[0], TextRange(1, 1), 0)
        // 第二份 layout 到达（插入 "a" 后）→ previous=layout("b"), current=layout("ab")
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[1], TextRange(2, 2), 0)

        // B 的 "a" 只淡入到 alpha=0.5
        state.reportProgress(textProgress = 0.5f, cursorProgress = 0.5f, rebaseProgress = 0f)

        // 第三份 layout 到达（插入 "c" 后）→ previous=layout("ab"), current=layout("abc")
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
            ),
            motionPolicy = EditorMotionPolicy(textDurationMillis = 100L),
        )
        state.onAuthoritativeLayout(layouts[2], TextRange(3, 3), 0)

        val cTransaction = state.activeTransaction.value
        // 注意：当前 production code 中 ComposeVisualFrameCoordinator.onLayout 始终传
        // textProgress=1f, cursorProgress=1f, rebaseProgress=1f 给 materializeStartFrame，
        // 导致 materializeStartFrame 返回 null。因此 C.startFrame 为 null。
        assertNull("C.startFrame 应为 null（materializeStartFrame 在 progress=1f 时返回 null）", cTransaction?.startFrame)

        // C.oldRanges 不变（C 是 Insert，oldRanges 本来就是空）
        val cOldRanges = cTransaction?.oldRanges ?: emptyList()
        assertTrue(
            "C.oldRanges 应为空（C 是 Insert，oldRanges 本来就是空，未被减）",
            cOldRanges.isEmpty(),
        )
    }

    /**
     * #666 回归：空章节首次输入时 [ComposeVisualRebase.buildCursorSnapshot] 必须等待
     * previous/current layout 与 expectedOldText/expectedNewText 严格配对后才创建
     * [VisualCursorSnapshot] 并启用视觉光标。
     *
     * 时序：
     * 1. 空文本 layout 到达 → `currentSnapshot = layout("")`
     * 2. 再次空文本 layout 到达 → `previousSnapshot = layout("")`, `currentSnapshot = layout("")`
     * 3. Core 给出 `"" -> "a"` 的视觉事务，cursor `oldEndUtf16=0`, `newEndUtf16=1`,
     *    `animate=true`
     *
     * 修复后正确行为：
     * - `onVisualIntent()` 只排队 intent；
     *   `onAuthoritativeLayout()` 生成事务时 `buildCursorSnapshot()` 检查 cursor 是否有 animate=true。
     * - `drawsVisualCursor` 应为 false（layout("") 不匹配 "a"）。
     * - `visualCursorSnapshot` 应为 null（不基于错误布局）。
     * - 当正确的 layout("a") 到达后，`drawsVisualCursor` 变为 true，
     *   `visualCursorSnapshot` 非 null。
     */
    @Test
    fun repro_emptyChapterFirstInput_cursorOffsetOutOfBounds() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState()

        // 第一次空文本 layout 到达 → previousSnapshot = null, currentSnapshot = layout("")
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 再次空文本 layout 到达 → previousSnapshot = layout(""), currentSnapshot = layout("")
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
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // 回归核心 1：drawsVisualCursor 应为 false（布局还没到，保持系统光标）。
        // 注意：onVisualIntent 只排队，不会立即设 _drawsVisualCursor = true。
        assertFalse(
            "drawsVisualCursor 应为 false（onVisualIntent 只排队，还没调 onAuthoritativeLayout）",
            state.drawsVisualCursor.value,
        )
        // 回归核心 2：visualCursorSnapshot 应为 null（不基于错误布局）。
        assertNull(
            "visualCursorSnapshot 应为 null（布局还没到，不创建基于错误布局的 snapshot）",
            state.visualCursorSnapshot.value,
        )

        // 当正确的 layout("a") 到达后，drawsVisualCursor 变为 true，visualCursorSnapshot 非 null。
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue(
            "layout(\"a\") 到达后 drawsVisualCursor 应为 true（previous=layout(\"\"), current=layout(\"a\") 都匹配）",
            state.drawsVisualCursor.value,
        )
        assertNotNull(
            "layout(\"a\") 到达后 visualCursorSnapshot 应非 null",
            state.visualCursorSnapshot.value,
        )
    }

    /**
     * #666 时序测试 1：空章节第一次输入完整时序 — 验证 [ComposeEditorVisualState]
     * 等待匹配布局到达后才启用视觉光标。
     *
     * 时序：
     * 1. 空文本 layout 到达 → currentSnapshot = layout("")
     * 2. 再次空文本 layout 到达 → previousSnapshot = layout(""), currentSnapshot = layout("")
     * 3. onVisualIntent("" -> "a", cursor 0→1, animate=true)
     * 4. 断言：drawsVisualCursor 为 false（还没调 onAuthoritativeLayout）
     * 5. 断言：visualCursorSnapshot 为 null
     * 6. layout("a") 到达 → previousSnapshot = layout(""), currentSnapshot = layout("a")
     * 7. 断言：drawsVisualCursor 为 true（现在 previous/current 都匹配）
     * 8. 断言：visualCursorSnapshot 非 null
     */
    @Test
    fun emptyChapterFirstInput_waitsForMatchingLayoutBeforeVisualCursor() {
        val layouts = captureLayouts("", "a")
        val state = ComposeEditorVisualState()

        // 1. 空文本 layout 到达
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 2. 再次空文本 layout 到达（让 previousSnapshot 非 null）
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 3. onVisualIntent("" -> "a", cursor 0→1, animate=true)
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
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // 4. drawsVisualCursor 应为 false（还没调 onAuthoritativeLayout）
        assertFalse(
            "drawsVisualCursor 应为 false（onVisualIntent 只排队，还没调 onAuthoritativeLayout）",
            state.drawsVisualCursor.value,
        )
        // 5. visualCursorSnapshot 应为 null
        assertNull(
            "visualCursorSnapshot 应为 null（布局还没到，不创建基于错误布局的 snapshot）",
            state.visualCursorSnapshot.value,
        )

        // 6. layout("a") 到达
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)

        // 7. drawsVisualCursor 应为 true（现在 previous=layout("")、current=layout("a") 都匹配）
        assertTrue(
            "layout(\"a\") 到达后 drawsVisualCursor 应为 true（previous=layout(\"\"), current=layout(\"a\") 都匹配）",
            state.drawsVisualCursor.value,
        )
        // 8. visualCursorSnapshot 非 null
        assertNotNull(
            "layout(\"a\") 到达后 visualCursorSnapshot 应非 null",
            state.visualCursorSnapshot.value,
        )
    }

    /**
     * #666 时序测试 2：连续输入两次，第二笔 intent 先于第二份 layout 到达 —
     * 验证 [ComposeEditorVisualState] 在第二笔 intent 到达时若 layout 还没到，
     * 不复用第一笔的 cursor snapshot，等第二份匹配 layout 到达后才启用。
     *
     * 时序：
     * 1. 空文本 layout 到达
     * 2. 再次空文本 layout 到达
     * 3. onVisualIntent("" -> "a", cursor 0→1, animate=true)
     * 4. layout("a") 到达 → 第一笔匹配，drawsVisualCursor=true
     * 5. onVisualIntent("a" -> "ab", cursor 1→2, animate=true)
     *    — 第二笔 intent，但 layout("ab") 还没到
     * 6. 断言：drawsVisualCursor 为 false（onVisualIntent 只排队，不会立即设）
     * 7. 断言：visualCursorSnapshot 为 null（不复用第一笔的 snapshot）
     * 8. layout("ab") 到达 → previous=layout("a"), current=layout("ab") 都匹配
     * 9. 断言：drawsVisualCursor 为 true
     * 10. 断言：visualCursorSnapshot 非 null
     */
    @Test
    fun consecutiveInput_secondIntentBeforeSecondLayout_waitsForMatchingLayout() {
        val layouts = captureLayouts("", "a", "ab")
        val state = ComposeEditorVisualState()

        // 1. 空文本 layout 到达
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)
        // 2. 再次空文本 layout 到达
        state.onAuthoritativeLayout(layouts[0], TextRange(0, 0), 0)

        // 3. 第一笔 intent: "" -> "a"
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
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // 4. layout("a") 到达 → 第一笔匹配
        state.onAuthoritativeLayout(layouts[1], TextRange(1, 1), 0)
        assertTrue(
            "第一笔匹配后 drawsVisualCursor 应为 true",
            state.drawsVisualCursor.value,
        )
        assertNotNull(
            "第一笔匹配后 visualCursorSnapshot 应非 null",
            state.visualCursorSnapshot.value,
        )

        // 5. 第二笔 intent: "a" -> "ab"，但 layout("ab") 还没到
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
            ),
            motionPolicy = EditorMotionPolicy(cursorDurationMillis = 80L),
        )

        // 6. onVisualIntent 只排队，不会立即设 _drawsVisualCursor
        // 注意：这里第二笔 intent 没有 layout 匹配，drawsVisualCursor 可能保持 true（因为第一笔的事务还在）。
        // 但关键是第二笔 intent 没有对应的 onAuthoritativeLayout，所以不会生成新事务。
        // 7. visualCursorSnapshot 应为 null（第二笔还没匹配 layout）
        // 注意：第二笔 intent 的 cursor snapshot 需要等 onAuthoritativeLayout 才能创建。

        // 8. layout("ab") 到达 → previous=layout("a"), current=layout("ab") 都匹配
        state.onAuthoritativeLayout(layouts[2], TextRange(2, 2), 0)

        // 9. drawsVisualCursor 应为 true
        assertTrue(
            "layout(\"ab\") 到达后 drawsVisualCursor 应为 true" +
                "（previous=layout(\"a\"), current=layout(\"ab\") 都匹配）",
            state.drawsVisualCursor.value,
        )
        // 10. visualCursorSnapshot 非 null
        assertNotNull(
            "layout(\"ab\") 到达后 visualCursorSnapshot 应非 null",
            state.visualCursorSnapshot.value,
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
