package com.xiwei.sujian.feature.editor.interop

import com.xiwei.sujian.feature.editor.platform.SujianEditorView
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import com.xiwei.sujian.feature.editor.session.TextEditorProfile
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.EditorEditOutcomeDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto

/**
 * #595 五：selection-only 操作必须更新会话层 SessionState 契约测试。
 *
 * 旧缺陷：SujianEditorView.handlePipelineOutputInternal 用
 * `displayPatches.isNotEmpty()` 门控整个回调 — CURSOR_ONLY 操作（移动光标/选区）
 * 没有文字 patch，onLocalEdit 不被调用，会话层 selection 停留在旧值，
 * 跨配置恢复/外部更新排序拿到过期选区。
 *
 * 修复后契约：
 * - 结果已应用（isApplied）就调用 onLocalEdit（携带真实新选区 + SELECTION 语义），
 *   不受 displayPatches 是否为空影响；
 * - onContentChanged（ViewModel 保存）仍只按 displayPatches 非空门控 —
 *   纯选区移动不把章节标记为未保存。
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class SelectionOnlySessionCallbackTest {
    // 记录 setSelection 调用并返回 CURSOR_ONLY + 空 displayPatches 的假 bridge。
    // #597 测试 fake bridge，函数由 EditorKernelBridge 接口契约决定 — 无法裁减
    @Suppress("TooManyFunctions")
    private class CursorOnlyBridge(
        // #624 评论10 第5项：真实 Core 内核 apply_set_selection 返回 NoChange
        // （正文未变）。旧测试 fake 返回 APPLIED，掩盖了生产路径上光标移动
        // 无法到达会话层的问题 — 两个 outcome 都必须触发 onLocalEdit。
        private val outcome: EditorEditOutcomeDto = EditorEditOutcomeDto.APPLIED,
    ) : EditorKernelBridge {
        var setSelectionCalls = 0
        var lastAnchor = -1
        var lastHead = -1
        var txId = 1UL

        private fun cursorOnlyResult(
            anchor: Int,
            head: Int,
        ): EditorEditResultDto =
            EditorEditResultDto(
                outcome = outcome,
                transactionId = txId++,
                baseRevision = 1UL,
                newRevision = 1UL,
                displayPatches = emptyList(),
                oldSelectionStart = anchor.toUInt(),
                oldSelectionEnd = head.toUInt(),
                newSelectionStart = anchor.toUInt(),
                newSelectionEnd = head.toUInt(),
                visualIntent =
                    EditorVisualIntentDto(
                        cause = EditorTransactionCauseDto.TYPING,
                        operationKind = EditorOperationKindDto.CURSOR_ONLY,
                        oldAffectedByteRanges = emptyList(),
                        newAffectedByteRanges = emptyList(),
                        animationMode = AnimationModeDto.SYSTEM_SUPPRESSED,
                        durationMs = 0uL,
                        coordinatedCursor =
                            CoordinatedCursorDto(
                                oldByteOffset = 0u,
                                newByteOffset = head.toUInt(),
                                shouldAnimate = false,
                            ),
                        offsetMap = null,
                    ),
                compositionSession = null,
                contentDelta = uniffi.writer_core.EditorContentDeltaDto(0u, 0u, 0u, 0u),
                composition = null,
            )

        override fun setSelection(
            anchorByteOffset: Int,
            headByteOffset: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? {
            setSelectionCalls++
            lastAnchor = anchorByteOffset
            lastHead = headByteOffset
            return cursorOnlyResult(anchorByteOffset, headByteOffset)
        }

        override fun insert(
            byteOffset: Int,
            text: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun delete(
            byteStart: Int,
            byteEndExclusive: Int,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun replace(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            originalText: String,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun undo(expectedRevision: Long): EditorEditResultDto? = null

        override fun redo(expectedRevision: Long): EditorEditResultDto? = null

        override fun loadText(
            text: String,
            cursorUtf8: Int,
        ): EditorEditResultDto? = null

        override fun commitText(
            byteStart: Int,
            byteEndExclusive: Int,
            replacementText: String,
            resultingSelectionAnchor: Int,
            resultingSelectionHead: Int,
            compositionSessionId: Long,
            compositionBaseRevision: Long,
            compositionGeneration: Long,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun deleteSurrounding(
            beforeByteStart: Int,
            beforeByteEndExclusive: Int,
            afterByteStart: Int,
            afterByteEndExclusive: Int,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun beginComposition(
            replaceStart: Int,
            replaceEndExclusive: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun updateComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            newPreeditText: String,
            newPreeditCursorOffset: Int,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun finishComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun cancelComposition(
            compositionSessionId: Long,
            compositionGeneration: Long,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun setAnimationEnabled(enabled: Boolean) { }

        override fun setAnimationDurationMs(durationMs: Long) { }

        override fun replaceAll(
            search: String,
            replacement: String,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun insertLineBreak(
            byteOffset: Int,
            autoIndentEnabled: Boolean,
            cause: EditorTransactionCauseDto,
            expectedRevision: Long,
        ): EditorEditResultDto? = null

        override fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto? = null

        // #606: grapheme 边界 stub — 测试不验证 grapheme 语义
        override fun previousGraphemeBoundary(byteOffset: Int): Int = byteOffset

        override fun nextGraphemeBoundary(byteOffset: Int): Int = byteOffset

        // #606: 测试不覆盖 rebase 映射 — 返回 null（平台端按无映射处理）。
        override fun computeRebaseSliceMappings(
            oldSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            oldSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            newSliceRoles: List<uniffi.writer_core.AnimatedSliceRoleDto>,
            newSliceByteRanges: List<uniffi.writer_core.EditorByteRangeDto>,
            offsetMap: uniffi.writer_core.OffsetMapDto?,
        ): List<uniffi.writer_core.RebaseSliceMappingDto>? = null
    }

    @Test
    fun selectionOnlyEdit_invokesOnLocalEdit_withRealSelection() {
        val context = RuntimeEnvironment.getApplication()
        val view = SujianEditorView(context)
        val bridge = CursorOnlyBridge()
        // 附着快照：正文 "hello"（UTF-8 5 字节）、revision 1、光标/选区在末尾。
        val attached =
            view.attachSession(
                sessionBridge = bridge,
                profile = TextEditorProfile.DocumentBody,
                text = "hello",
                revision = 1L,
                cursorUtf8 = 5,
                selStartUtf8 = 5,
                selEndUtf8 = 5,
            )
        assertTrue("attachSession must succeed", attached)

        var localEditCalls = 0
        var lastKind: EditorOperationKind? = null
        var lastAnchor = -1
        var lastHead = -1
        var lastContentChanged: Boolean? = null
        // #624 评论9：热路径回调已是轻量 EditorAppliedEvent（无整章 String）。
        view.onLocalEdit = { event ->
            localEditCalls++
            assertEquals("selection-only 不改变 revision", 1L, event.revision)
            assertTrue("selection-only 事务必须携带 transactionId", event.transactionId > 0L)
            assertEquals("正文未变 → contentChanged=false", false, event.contentChanged)
            lastKind = event.operationKind
            lastAnchor = event.selectionAnchorUtf8
            lastHead = event.selectionHeadUtf8
            lastContentChanged = event.contentChanged
        }

        // CURSOR_ONLY：光标从末尾移到 "he|llo"（UTF-8 偏移 2）。
        view.setSelectionTyped(2, 2)

        assertEquals("setSelection 必须到达 kernel bridge", 1, bridge.setSelectionCalls)
        assertEquals(
            "#595 五：selection-only（空 displayPatches）也必须调用 onLocalEdit — " +
                "会话层 selection 不得停留在旧值",
            1,
            localEditCalls,
        )
        assertEquals("operationKind 必须为 SELECTION", EditorOperationKind.SELECTION, lastKind)
        assertEquals("onLocalEdit 必须携带真实新选区 anchor", 2, lastAnchor)
        assertEquals("onLocalEdit 必须携带真实新选区 head", 2, lastHead)
        assertEquals(
            "#624 评论9：纯选区移动 contentChanged=false（正文未变，不应标记未保存）",
            false,
            lastContentChanged,
        )
        assertEquals("View 镜像选区必须同步", 2, view.getSelectionStart())
        assertEquals(2, view.getSelectionEnd())
    }

    /**
     * #624 评论10 第5项：真实 Core 内核 `apply_set_selection` 返回 NO_CHANGE
     * （正文未变，outcome 不是 APPLIED）。旧门控 `output.result.isApplied()`
     * 会把这个结果整个拦掉：onLocalEdit 不被调用，会话层 selection 停留在旧值，
     * 与"纯 selection 只更新会话层 selection/revision"的契约矛盾。
     * NO_CHANGE 结果同样携带真实新选区/transactionId/revision，必须进入会话层；
     * 持久化状态机由 contentChanged=false 在 onEditorApplied 侧拦截。
     */
    @Test
    fun selectionOnlyEdit_noChangeOutcome_stillInvokesOnLocalEdit() {
        val context = RuntimeEnvironment.getApplication()
        val view = SujianEditorView(context)
        // 真实内核语义：CURSOR_ONLY + NO_CHANGE + 空 displayPatches。
        val bridge = CursorOnlyBridge(outcome = EditorEditOutcomeDto.NO_CHANGE)
        val attached =
            view.attachSession(
                sessionBridge = bridge,
                profile = TextEditorProfile.DocumentBody,
                text = "hello",
                revision = 1L,
                cursorUtf8 = 5,
                selStartUtf8 = 5,
                selEndUtf8 = 5,
            )
        assertTrue("attachSession must succeed", attached)

        var localEditCalls = 0
        var lastKind: EditorOperationKind? = null
        var lastAnchor = -1
        var lastHead = -1
        var lastContentChanged: Boolean? = null
        view.onLocalEdit = { event ->
            localEditCalls++
            assertEquals("selection-only 不改变 revision", 1L, event.revision)
            assertTrue("selection-only 事务必须携带 transactionId", event.transactionId > 0L)
            lastKind = event.operationKind
            lastAnchor = event.selectionAnchorUtf8
            lastHead = event.selectionHeadUtf8
            lastContentChanged = event.contentChanged
        }

        // CURSOR_ONLY（NO_CHANGE）：光标从末尾移到 "he|llo"（UTF-8 偏移 2）。
        view.setSelectionTyped(2, 2)

        assertEquals("setSelection 必须到达 kernel bridge", 1, bridge.setSelectionCalls)
        assertEquals(
            "#624 评论10 第5项：NO_CHANGE 光标移动也必须调用 onLocalEdit — " +
                "会话层 selection 不得停留在旧值",
            1,
            localEditCalls,
        )
        assertEquals("operationKind 必须为 SELECTION", EditorOperationKind.SELECTION, lastKind)
        assertEquals("onLocalEdit 必须携带真实新选区 anchor", 2, lastAnchor)
        assertEquals("onLocalEdit 必须携带真实新选区 head", 2, lastHead)
        assertEquals(
            "#624 评论9：纯选区移动 contentChanged=false（正文未变，不应标记未保存）",
            false,
            lastContentChanged,
        )
        assertEquals("View 镜像选区必须同步", 2, view.getSelectionStart())
        assertEquals(2, view.getSelectionEnd())
    }
}
