package com.xiwei.sujian.feature.editor.pipeline

import android.graphics.Color
import android.text.TextPaint
import com.xiwei.sujian.feature.editor.interop.EditorKernelBridge
import com.xiwei.sujian.feature.editor.layout.AndroidLayoutEngine
import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.projection.DisplayTextMirror
import com.xiwei.sujian.feature.editor.projection.DisplayTextProjection
import com.xiwei.sujian.feature.editor.projection.EditResult
import com.xiwei.sujian.feature.editor.projection.OffsetMap
import com.xiwei.sujian.feature.editor.projection.OffsetMapEntry
import com.xiwei.sujian.feature.editor.projection.OffsetMapKind
import com.xiwei.sujian.feature.editor.projection.VisualIntent
import com.xiwei.sujian.feature.editor.visual.AndroidVisualPlanner
import com.xiwei.sujian.feature.editor.visual.RebaseMappingProvider
import com.xiwei.sujian.feature.editor.visual.SliceRole
import com.xiwei.sujian.feature.editor.visual.SliceRoleAndByteRange
import uniffi.writer_core.AnimatedSliceRoleDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.OffsetMapDto
import uniffi.writer_core.OffsetMapEntryDto
import uniffi.writer_core.OffsetMapKindDto
import uniffi.writer_core.RebaseSliceMappingDto

/**
 * Visual pipeline for the Android editing runtime.
 *
 * Ownership split (per #550):
 * - [EditPipeline] owns [DisplayTextMirror] and [EditorKernelBridge] — applies EditResult
 *   to mirror and requests new LayoutRevision.
 * - [AndroidLayoutRuntime] owns [AndroidLayoutEngine], [DisplayTextProjection] and secret
 *   display state — produces LayoutRevision and geometric queries.
 * - [AndroidVisualRuntime] owns [AndroidVisualPlanner], Timeline, [VisualResourceStore]
 *   and active visual transactions.
 * - [AndroidRenderRuntime] owns [AndroidTextRenderer], [AndroidTextAnimationRenderer]
 *   and [EditorFrameComposer] — composes and draws the current frame.
 * - [AndroidInputAdapter] depends only on [DisplayTextMirror] and [EditorCommandPort].
 * - [SujianEditorView] handles focus, InputConnection, viewport, system notifications
 *   and frame requests.
 *
 * Both normal text edits and IME composition update/commit/cancel go through the same
 * [AndroidTextAnimationEngine] — composition animations are not a separate code path.
 *
 * Two-level visual transaction architecture: edits produce a [PreparedVisualTransaction] with
 * (1) per-cluster Bitmap slices for the edit paragraph (Insert/Delete/Move/Crossfade) and
 * (2) [PreparedVisualTransaction.BlockShift] entries for subsequent paragraphs that only shifted
 * vertically. BlockShifts apply a uniform Y translation to the static new-layout text without
 * creating per-line Bitmaps, preventing unbounded memory allocation when editing near the top
 * of a long document.
 */
sealed class PipelineOutput {
    /**
     * #595 四：输出天然携带编辑来源 — 不再使用 View 上的 pendingEditSource
     * 可变侧信道（NeedReload/Stale 输出不再污染下一条命令的来源分类）。
     */
    data class Edited(
        val result: EditResult,
        val source: EditorEditSource = EditorEditSource.NORMAL,
    ) : PipelineOutput()

    object NeedReload : PipelineOutput()

    object StaleOrInvalid : PipelineOutput()
}

class AndroidEditorPipeline private constructor(
    val editPipeline: EditPipeline,
    private val renderRuntime: AndroidRenderRuntime,
    private val layoutRuntime: AndroidLayoutRuntime,
    private val visualRuntime: AndroidVisualRuntime,
) : EditorCommandPort, InputCommandPort {
    override val mirror: DisplayTextMirror get() = editPipeline.mirror
    override var kernelBridge: EditorKernelBridge?
        get() = editPipeline.kernelBridge
        set(value) {
            editPipeline.setKernelBridge(value)
        }

    companion object {
        /** #606: 平台 SliceRole → Core AnimatedSliceRoleDto（纯数据映射，无逻辑）。 */
        private fun SliceRole.toAnimatedSliceRoleDto(): AnimatedSliceRoleDto? =
            when (this) {
                SliceRole.Insert -> AnimatedSliceRoleDto.INSERT
                SliceRole.Delete -> AnimatedSliceRoleDto.DELETE
                SliceRole.Move -> AnimatedSliceRoleDto.MOVE
                SliceRole.CrossfadeOld -> AnimatedSliceRoleDto.CROSSFADE_OLD
                SliceRole.CrossfadeNew -> AnimatedSliceRoleDto.CROSSFADE_NEW
                SliceRole.Static -> null
            }

        /** #606: 投影层 OffsetMap → UniFFI DTO（纯数据映射，无逻辑）。 */
        private fun OffsetMap.toDto(): OffsetMapDto = OffsetMapDto(entries.map { it.toDto() })

        /** #606: 投影层 OffsetMapEntry → UniFFI DTO（纯数据映射，无逻辑）。 */
        private fun OffsetMapEntry.toDto(): OffsetMapEntryDto =
            OffsetMapEntryDto(
                oldByteOffset = oldByteOffset.toUInt(),
                newByteOffset = newByteOffset.toUInt(),
                length = length.toUInt(),
                kind =
                    when (kind) {
                        OffsetMapKind.IDENTITY -> OffsetMapKindDto.IDENTITY
                        OffsetMapKind.SHIFTED -> OffsetMapKindDto.SHIFTED
                    },
            )

        /**
         * #606: 旧→新逻辑 slice 对应关系由 Core 唯一计算。
         *
         * 平台端只提供输入（旧/新 slice 角色与 byte range、本次事务的 OffsetMap），
         * 返回 Core 的 `RebaseSliceMappingDto` 原样消费 — 平台不再维护任何本地副本。
         * Static 角色不在 Core 动画角色集合中（不参与 rebase 匹配），先过滤再调用，
         * 返回后把 Core 索引（过滤后列表的索引）翻译回完整列表索引 — 纯数据管道，
         * 无任何匹配逻辑。
         * bridge 为 null 或调用失败时返回空映射（平台端按无对应关系处理）。
         */
        fun computeRebaseSliceMappings(
            bridge: EditorKernelBridge?,
            oldSlices: List<SliceRoleAndByteRange>,
            newSlices: List<SliceRoleAndByteRange>,
            offsetMap: OffsetMap?,
        ): List<RebaseSliceMappingDto> {
            if (bridge == null) return emptyList()
            val oldNonStaticIndices =
                oldSlices.mapIndexedNotNull { index, s ->
                    if (s.role.toAnimatedSliceRoleDto() == null) null else index
                }
            val newNonStaticIndices =
                newSlices.mapIndexedNotNull { index, s ->
                    if (s.role.toAnimatedSliceRoleDto() == null) null else index
                }
            val coreMappings =
                bridge.computeRebaseSliceMappings(
                    oldSliceRoles = oldNonStaticIndices.map { oldSlices[it].role.toAnimatedSliceRoleDto()!! },
                    oldSliceByteRanges =
                        oldNonStaticIndices.map {
                            uniffi.writer_core.EditorByteRangeDto(
                                start = oldSlices[it].byteStart.toUInt(),
                                endExclusive = oldSlices[it].byteEndExclusive.toUInt(),
                            )
                        },
                    newSliceRoles = newNonStaticIndices.map { newSlices[it].role.toAnimatedSliceRoleDto()!! },
                    newSliceByteRanges =
                        newNonStaticIndices.map {
                            uniffi.writer_core.EditorByteRangeDto(
                                start = newSlices[it].byteStart.toUInt(),
                                endExclusive = newSlices[it].byteEndExclusive.toUInt(),
                            )
                        },
                    offsetMap = offsetMap?.toDto(),
                )
            return coreMappings
                ?.map { dto ->
                    dto.copy(
                        oldSliceIndex = oldNonStaticIndices[dto.oldSliceIndex.toInt()].toUInt(),
                        newSliceIndex = newNonStaticIndices[dto.newSliceIndex.toInt()].toUInt(),
                    )
                } ?: emptyList()
        }

        fun create(
            mirror: DisplayTextMirror,
            textPaint: TextPaint,
            timeSource: com.xiwei.sujian.feature.editor.visual.AnimationTimeSource =
                com.xiwei.sujian.feature.editor.visual.ChoreographerAnimationTimeSource(),
            transactionIdSource: com.xiwei.sujian.feature.editor.visual.TransactionIdSource =
                com.xiwei.sujian.feature.editor.visual.TransactionIdSource(),
        ): AndroidEditorPipeline {
            val editPipeline = EditPipeline(mirror)
            val layoutRuntime = AndroidLayoutRuntime(mirror, textPaint)
            val rebaseMappingProvider =
                RebaseMappingProvider { oldSlices, newSlices, offsetMap ->
                    computeRebaseSliceMappings(
                        editPipeline.kernelBridge,
                        oldSlices,
                        newSlices,
                        offsetMap,
                    )
                }
            val visualRuntime =
                AndroidVisualRuntime(
                    visualPlanner = AndroidVisualPlanner(rebaseMappingProvider = rebaseMappingProvider),
                    timeSource = timeSource,
                    transactionIdSource = transactionIdSource,
                )
            val renderRuntime = AndroidRenderRuntime()
            return AndroidEditorPipeline(editPipeline, renderRuntime, layoutRuntime, visualRuntime)
        }
    }

    private var autoIndentEnabled: Boolean = false
    private var maxLength: Int = 0
    private var typingAnimationDurationMs: Long = 200L

    /**
     * #606: Core-returned visual intent for the most recent composition update/finish/cancel.
     * Set by [updateComposition]/[finishComposition]/[cancelComposition] and consumed by
     * [applyCompositionUpdateAnimated]/[applyCompositionCancelAnimated]. This is the single
     * source of truth for composition visual semantics — the platform no longer reconstructs
     * a VisualIntent from preedit text.
     */
    private var pendingCompositionVisualIntent: VisualIntent? = null

    /**
     * 打字动画时长（生产路径：设置 → Editor Host → 输入事务）。
     * 平台侧构造的 composition 事务（update/commit/cancel）使用该时长，
     * 与 Rust kernel 的 setAnimationDurationMs 保持一致。
     */
    fun setTypingAnimationDurationMs(durationMs: Long) {
        typingAnimationDurationMs = durationMs.coerceAtLeast(1L)
    }

    fun setSmoothCursor(
        enabled: Boolean,
        durationMs: Long,
    ) {
        visualRuntime.setSmoothCursor(enabled, durationMs)
    }

    fun setCoordinatedAnimationEnabled(enabled: Boolean) {
        visualRuntime.setCoordinatedAnimationEnabled(enabled)
    }

    fun setReduceMotion(enabled: Boolean) {
        visualRuntime.setReduceMotion(enabled)
    }

    fun pauseAnimation(frameTimeMs: Long) {
        visualRuntime.pause(frameTimeMs)
    }

    fun resumeAnimation(frameTimeMs: Long) {
        visualRuntime.resume(frameTimeMs)
    }

    fun isAnimationPaused(): Boolean = visualRuntime.isAnimationPaused()

    fun loadText(
        text: String,
        cursorUtf8: Int,
        @Suppress("UNUSED_PARAMETER") applySecret: Boolean = true,
    ): LoadTextResult {
        val result = editPipeline.loadText(text, cursorUtf8)
        if (result is LoadTextResult.Loaded) {
            resetAfterLoad()
            layoutRuntime.rebuildDisplayProjection()
        }
        return result
    }

    /**
     * #592 一：附着既有会话快照 — 只重建本地 mirror/layout，不调用 Rust loadText。
     *
     * 用于窗口重建/重新绑定场景：从 textEditSessionSnapshot 读取的
     * text/revision/cursor/selection 直接装入 Android mirror，Rust revision 不变、
     * Undo/Redo 不清空、composition 不重置。
     * textEditSessionLoadText 只允许用于新正文载入或明确的外部内容重置。
     */
    fun attachSnapshot(
        text: String,
        revision: Long,
        cursorUtf8: Int,
        selStartUtf8: Int,
        selEndUtf8: Int,
    ) {
        editPipeline.loadFromSnapshot(text, cursorUtf8, revision, selStartUtf8, selEndUtf8)
        resetAfterLoad()
        layoutRuntime.rebuildDisplayProjection()
    }

    override fun insertText(
        byteOffset: Int,
        text: String,
        cause: EditorTransactionCauseDto,
    ): PipelineOutput {
        if (text == "\n") {
            val result =
                editPipeline.insertLineBreak(byteOffset, autoIndentEnabled, cause)
                    ?: return PipelineOutput.StaleOrInvalid
            return applyEditResult(result)
        }
        val result =
            editPipeline.insertText(byteOffset, text, cause)
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    /**
     * #624 评论2：唯一换行命令入口 — 软键盘 commitText("\n")、硬件 Enter、
     * 粘贴换行都收敛到这里。无选区时走 Core insertLineBreak（写作区 auto-indent
     * 恒为 false，只产生 `\n`）；有选区时通过 Core 的单一“换行替换”语义完成（一次
     * replace 命令把选区换成 \n），不在平台端先删选区再插入换行。
     */
    override fun insertLineBreak(cause: EditorTransactionCauseDto): PipelineOutput {
        val selStart = editPipeline.getCommittedSelectionStartUtf8()
        val selEnd = editPipeline.getCommittedSelectionEndUtf8()
        if (selStart != selEnd) {
            val originalText = committedSubstring(selStart, selEnd)
            val result =
                editPipeline.replaceRange(selStart, selEnd, "\n", originalText, cause)
                    ?: return PipelineOutput.StaleOrInvalid
            return applyEditResult(result)
        }
        val byteOffset = editPipeline.getCommittedCursorUtf8()
        val result =
            editPipeline.insertLineBreak(byteOffset, autoIndentEnabled, cause)
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    private fun committedSubstring(
        startUtf8: Int,
        endUtf8: Int,
    ): String = editPipeline.committedSliceUtf8(startUtf8, endUtf8)

    override fun deleteRange(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ): PipelineOutput {
        val result =
            editPipeline.deleteRange(byteStart, byteEndExclusive, cause)
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    override fun replaceRangeTyped(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: EditorTransactionCauseDto,
        beforePatch: (() -> Unit)?,
        source: EditorEditSource,
    ): PipelineOutput {
        val result =
            editPipeline.replaceRange(byteStart, byteEndExclusive, replacementText, originalText, cause)
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result, beforePatch, source)
    }

    override fun setSelectionTyped(
        anchorByteOffset: Int,
        headByteOffset: Int,
        source: EditorEditSource,
    ): PipelineOutput {
        val result =
            editPipeline.setSelection(anchorByteOffset, headByteOffset)
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result, source = source)
    }

    fun performUndo(): PipelineOutput {
        val result =
            editPipeline.undo()
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result, source = EditorEditSource.UNDO)
    }

    fun performRedo(): PipelineOutput {
        val result =
            editPipeline.redo()
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result, source = EditorEditSource.REDO)
    }

    fun replaceAll(
        searchStr: String,
        replaceStr: String,
    ): PipelineOutput {
        val result =
            editPipeline.replaceAll(searchStr, replaceStr)
                ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result, source = EditorEditSource.PROGRAMMATIC)
    }

    /**
     * Apply a composition commit by consuming the Core-returned [VisualIntent] directly.
     *
     * #606: Core now classifies composition operations (visual-same suppression, animation
     * mode selection, operationKind, old/new affected ranges) via its shared
     * `classify_composition_visual` function. The platform no longer re-computes these
     * — it just applies the edit result and feeds Core's visualIntent into the animation
     * pipeline.
     *
     * The Core dto is the single source of truth for visual semantics.
     */
    override fun applyCompositionCommit(dto: uniffi.writer_core.EditorEditResultDto): PipelineOutput {
        val result = EditResult.fromDto(dto)
        pendingCompositionVisualIntent = null
        return applyEditResultWithIntent(result, result.visualIntent)
    }

    /**
     * Apply an [EditResult] from the Rust kernel via the animation pipeline.
     *
     * Composition-override invariant: [AndroidTextAnimationEngine.prepareAndSubmit] is the
     * sole entry point for all visual state transitions — normal edits, composition updates,
     * composition commits, and composition cancels all go through the same animation engine.
     * There is no separate "fast path" that bypasses animation for composition or system-
     * suppressed edits, because even suppressed edits must still update the mirror and layout
     * (prepareAndSubmit handles this internally by calling mirrorUpdate + requestLayout when
     * animation is suppressed). Bypassing prepareAndSubmit would leave the display showing
     * stale text.
     */
    override fun applyEditResult(
        result: EditResult,
        beforePatch: (() -> Unit)?,
        source: EditorEditSource,
    ): PipelineOutput {
        if (result.isStale()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.isInvalid() && result.displayPatches.isEmpty()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != result.newRevision) {
            return PipelineOutput.NeedReload
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != mirror.getRevision()) {
            return PipelineOutput.NeedReload
        }

        // #630 评论12 项3: 语义 no-op 出口 — 光标/选区确实完全没变的 CURSOR_ONLY
        // 不创建视觉事务。条件同时满足时直接返回，不 capture snapshot、不 planner、
        // 不 prepareAndSubmit、不 timeline。但仍需同步 mirror 状态。
        if (isCursorOnlyNoOp(result)) {
            editPipeline.applyEditResult(result)
            layoutRuntime.onMirrorContentChanged(result.displayPatches)
            return PipelineOutput.Edited(result, source)
        }

        recordEditTransaction(result)

        visualRuntime.prepareAndSubmit(
            visualIntent = result.visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                editPipeline.applyEditResult(result)
                layoutRuntime.onMirrorContentChanged(result.displayPatches)
            },
            beforePatch = beforePatch,
        )

        return PipelineOutput.Edited(result, source)
    }

    private fun recordEditTransaction(result: EditResult) {
        val oldAffected = result.visualIntent.oldAffectedByteRanges
        val newAffected = result.visualIntent.newAffectedByteRanges
        com.xiwei.sujian.core.diagnostics.DiagnosticsEvents.editTransaction(
            operationKind = result.visualIntent.operationKind.name,
            oldStart = oldAffected.firstOrNull()?.first ?: 0,
            oldEndExclusive = oldAffected.firstOrNull()?.second ?: 0,
            newStart = newAffected.firstOrNull()?.first ?: 0,
            newEndExclusive = newAffected.firstOrNull()?.second ?: 0,
            revision = result.newRevision,
            sessionId = "-",
            result =
                if (result.isApplied()) {
                    "applied"
                } else if (result.isNoChange()) {
                    "no_change"
                } else {
                    "other"
                },
        )
    }

    private fun applyEditResultWithIntent(
        result: EditResult,
        visualIntent: VisualIntent,
        beforePatch: (() -> Unit)? = null,
        source: EditorEditSource = EditorEditSource.NORMAL,
    ): PipelineOutput {
        if (result.isStale()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.isInvalid() && result.displayPatches.isEmpty()) {
            return PipelineOutput.StaleOrInvalid
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != result.newRevision) {
            return PipelineOutput.NeedReload
        }
        if (result.displayPatches.isEmpty() && result.baseRevision != mirror.getRevision()) {
            return PipelineOutput.NeedReload
        }

        // #630 评论12 项3: 语义 no-op 出口（同 applyEditResult）
        if (isCursorOnlyNoOp(result)) {
            return PipelineOutput.Edited(result, source)
        }

        visualRuntime.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                editPipeline.applyEditResult(result)
                layoutRuntime.onMirrorContentChanged(result.displayPatches)
            },
            beforePatch = beforePatch,
        )

        return PipelineOutput.Edited(result, source)
    }

    /**
     * Apply a composition update by consuming the Core-returned [VisualIntent] directly.
     *
     * #606: Core's `classify_composition_visual` now determines visual-same suppression,
     * animation mode (based on grapheme cluster count, newlines, complex graphemes),
     * old/new affected ranges, and operationKind. The platform no longer reconstructs
     * a VisualIntent from old/new preedit text — it feeds Core's result into the
     * animation pipeline.
     *
     * The Core dto is the single source of truth for visual semantics; the platform
     * holds no preedit-text parameters.
     */
    override fun applyCompositionUpdateAnimated(mirrorUpdate: (() -> Unit)?) {
        val visualIntent = pendingCompositionVisualIntent
        if (visualIntent != null) {
            visualRuntime.prepareAndSubmit(
                visualIntent = visualIntent,
                layoutEngine = layoutRuntime.layoutEngine,
                mirrorUpdate = {
                    mirrorUpdate?.invoke()
                    layoutRuntime.onMirrorContentChanged()
                },
            )
        } else {
            // 无视觉意图的兜底路径：没有动画引擎推进布局，这里显式推进一次。
            mirrorUpdate?.invoke()
            layoutRuntime.onMirrorContentChanged()
            layoutRuntime.requestLayout()
        }
    }

    /**
     * Apply a composition cancel by consuming the Core-returned [VisualIntent] directly.
     *
     * #606: Core's `classify_composition_visual` now determines the cancel visual
     * semantics (oldAffected ranges, animation mode, operationKind = COMPOSITION_CANCEL).
     * The platform no longer constructs a VisualIntent locally — it feeds Core's result
     * into the animation pipeline.
     *
     * The Core dto is the single source of truth for visual semantics; the platform
     * holds no preedit-text parameters.
     */
    override fun applyCompositionCancelAnimated(mirrorUpdate: (() -> Unit)?) {
        val visualIntent = pendingCompositionVisualIntent
        if (visualIntent != null) {
            visualRuntime.prepareAndSubmit(
                visualIntent = visualIntent,
                layoutEngine = layoutRuntime.layoutEngine,
                mirrorUpdate = {
                    mirrorUpdate?.invoke()
                    layoutRuntime.onMirrorContentChanged()
                },
            )
        } else {
            // 无视觉意图的兜底路径：没有动画引擎推进布局，这里显式推进一次。
            mirrorUpdate?.invoke()
            layoutRuntime.onMirrorContentChanged()
            layoutRuntime.requestLayout()
        }
    }

    /**
     * Render one frame. Layer order when animation is active:
     * background → search highlights → selection → static text with holes → animated slices
     * → preedit underline → animated cursor (or static cursor if no cursor transition).
     *
     * Without animation: background → search highlights → selection → static text
     * → preedit underline → static cursor.
     */
    fun drawFrame(
        canvas: android.graphics.Canvas,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
    ) {
        val frameTimeNanos = visualRuntime.currentTimeNanos()
        drawFrameWithTime(
            canvas,
            searchHighlightsUtf16,
            viewportWidth,
            viewportHeight,
            scrollX,
            scrollY,
            frameTimeNanos,
        )
    }

    fun drawFrame(
        canvas: android.graphics.Canvas,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
        frameTimeNanos: Long,
    ) {
        drawFrameWithTime(
            canvas,
            searchHighlightsUtf16,
            viewportWidth,
            viewportHeight,
            scrollX,
            scrollY,
            frameTimeNanos,
        )
    }

    private fun drawFrameWithTime(
        canvas: android.graphics.Canvas,
        searchHighlightsUtf16: List<Pair<Int, Int>>,
        viewportWidth: Int,
        viewportHeight: Int,
        scrollX: Float,
        scrollY: Float,
        frameTimeNanos: Long,
    ) {
        val frameTimeMs = frameTimeNanos / 1_000_000
        val projection = layoutRuntime.getCurrentProjection()
        val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
        val selStartDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionStartUtf8())
        val selEndDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionEndUtf8())
        val frameState =
            visualRuntime.tick(
                frameTimeMs,
                layoutRuntime.getLayout(),
                layoutRuntime.getCurrentRevision(),
                searchHighlightsUtf16,
                viewportWidth, viewportHeight,
                scrollX, scrollY,
                cursorVisible, selectionAllowed,
                cursorDisplayUtf16,
                selStartDisplayUtf16,
                selEndDisplayUtf16,
            )
        if (frameState != null) {
            renderRuntime.drawFromFrameState(canvas, frameState)
            if (frameState.completeAfterDraw) {
                visualRuntime.completeAfterDraw(frameTimeMs)
            }
        }
    }

    /** Cancel the active animation transaction and release its snapshots.
     *  Used when the host loses focus or the window is backgrounded — the active
     *  animation is abandoned and the display reverts to the static new-layout text. */
    fun cancelActiveTransaction() {
        visualRuntime.cancel()
    }

    /** Release all animation resources (active transaction + all session-owned Bitmaps).
     *  Called when the host is permanently destroyed — unlike [resetForReuse], this uses
     *  [AndroidTextAnimationEngine.release] which calls [VisualResourceStore.releaseAll]
     *  to ensure no Bitmaps survive after the host is removed from the composition tree. */
    fun releaseAllResources() {
        visualRuntime.release()
    }

    fun hasActiveAnimation(): Boolean = visualRuntime.hasActiveAnimation()

    fun currentTimeNanos(): Long = visualRuntime.currentTimeNanos()

    /**
     * Advance timeline state (first-visible-frame anchor, completion) for a dispatched
     * frame timestamp without drawing. Called from the host's frame callback so that
     * animation state is deterministic at dispatch time; the draw path re-applies the
     * same transitions idempotently.
     */
    fun onFrameTick(frameTimeMs: Long) {
        visualRuntime.onFrameTick(frameTimeMs)
    }

    fun updateLayout(width: Float) {
        layoutRuntime.setWidth(width)
        layoutRuntime.requestLayout()
    }

    /** Reset animation state after loading text from the kernel.
     *  Uses [AndroidTextAnimationEngine.cancel] (not [resetForSession]) because loadText
     *  replaces the mirror content atomically — the old session's Bitmaps are no longer
     *  visually relevant, but the resource store may still hold snapshots from completed
     *  transactions that will be garbage-collected naturally. [cancel] releases only the
     *  active transaction's snapshots; [releaseAllResources] is reserved for host destruction
     *  where no future rendering will occur. */
    fun resetAfterLoad() {
        visualRuntime.cancel()
        pendingCompositionVisualIntent = null
    }

    override fun reloadFromKernel(): Boolean {
        if (!editPipeline.reloadFromKernel()) return false
        cancelActiveTransaction()
        releaseAllResources()
        layoutRuntime.rebuildDisplayProjection()
        return true
    }

    override fun getText(): String = editPipeline.getText()

    override fun getRevision(): Long = editPipeline.getRevision()

    override fun getCursorUtf8(): Int = editPipeline.getCursorUtf8()

    fun getCursorUtf16(): Int = editPipeline.getCursorUtf16()

    fun getDisplayCursorUtf16(): Int =
        layoutRuntime.getCurrentProjection().realUtf8ToDisplayUtf16(
            mirror.getCursorUtf8(),
        )

    fun getSelectionStartUtf8(): Int = editPipeline.getSelectionStartUtf8()

    fun getSelectionEndUtf8(): Int = editPipeline.getSelectionEndUtf8()

    fun getSelectionStartUtf16(): Int = editPipeline.getSelectionStartUtf16()

    fun getSelectionEndUtf16(): Int = editPipeline.getSelectionEndUtf16()

    fun getLengthUtf16(): Int = editPipeline.getLengthUtf16()

    fun getCommittedCursorUtf8(): Int = editPipeline.getCommittedCursorUtf8()

    fun getCommittedSelectionStartUtf8(): Int = editPipeline.getCommittedSelectionStartUtf8()

    fun getCommittedSelectionEndUtf8(): Int = editPipeline.getCommittedSelectionEndUtf8()

    fun getCommittedText(): String = editPipeline.getCommittedText()

    /** #624 评论7：已提交文本 UTF-8 字节长度 — O(1) 转发。 */
    fun getCommittedTextLengthUtf8(): Int = editPipeline.getCommittedTextLengthUtf8()

    /** #624 评论7：已提交文本 UTF-8 字节区间局部读取。 */
    fun committedSliceUtf8(
        startUtf8: Int,
        endUtf8: Int,
    ): String = editPipeline.committedSliceUtf8(startUtf8, endUtf8)

    /**
     * Core insertLineBreak 的 auto-indent 策略（继承当前行前导空白 — 代码编辑器式
     * 语义）。#624 评论3：写作软件的首行缩进不往正文塞空格，由 [setFirstLineIndent]
     * 以显示层 span 实现；写作区不再调用本开关（恒为 false，DocumentBody 的
     * insertLineBreak 只产生 `\n`）。本入口保留给未来真正需要 INDENT_ON_ENTER 的
     * profile，不与首行缩进显示样式共用 boolean。
     */
    fun setAutoIndent(enabled: Boolean) {
        autoIndentEnabled = enabled
    }

    /**
     * #624 评论3：首行缩进显示样式（开关 + 字符宽度）透传给 layout runtime —
     * 由 ParagraphStyleProjection 以 span 施加在显示层，不改正文字符串。
     */
    fun setFirstLineIndent(
        enabled: Boolean,
        widthChars: Float,
    ) {
        layoutRuntime.setFirstLineIndent(enabled, widthChars)
    }

    fun isAutoIndentEnabled(): Boolean = autoIndentEnabled

    /**
     * #606: Returns the byte length of the grapheme cluster immediately before [offset].
     *
     * Used by Backspace/Delete key handlers ([SujianEditorView.onKeyDown] KEYCODE_DEL) to
     * determine which grapheme cluster to delete — a body-edit semantic. Calls Core's
     * Unicode cluster logic (unicode_segmentation) via [EditorKernelBridge] — single
     * source of truth for grapheme boundary semantics. The platform no longer uses ICU
     * BreakIterator for this calculation.
     */
    fun previousGraphemeByteLen(offset: Int): Int {
        val bridge = kernelBridge ?: return 0
        val boundary = bridge.previousGraphemeBoundary(offset)
        return offset - boundary
    }

    /**
     * #606: Returns the byte length of the grapheme cluster immediately after [offset].
     *
     * Used by Forward-Delete key handlers ([SujianEditorView.onKeyDown] KEYCODE_FORWARD_DEL)
     * to determine which grapheme cluster to delete — a body-edit semantic. Calls Core's
     * Unicode cluster logic (unicode_segmentation) via [EditorKernelBridge] — single
     * source of truth for grapheme boundary semantics.
     */
    fun nextGraphemeByteLen(offset: Int): Int {
        val bridge = kernelBridge ?: return 0
        val boundary = bridge.nextGraphemeBoundary(offset)
        return boundary - offset
    }

    fun getLayoutMaxScrollY(viewHeight: Int): Float {
        val layout = layoutRuntime.getLayout() ?: return 0f
        return (layout.height - viewHeight).coerceAtLeast(0).toFloat()
    }

    /** 当前排版实例（只读查询）— 测试与宿主可验证 DynamicLayout 复用契约（#624 评论7）。 */
    fun getLayout(): android.text.Layout? = layoutRuntime.getLayout()

    fun getLayoutLineForVertical(y: Int): Int {
        val layout = layoutRuntime.getLayout() ?: return 0
        return layout.getLineForVertical(y)
    }

    fun getLayoutOffsetForHorizontal(
        line: Int,
        x: Float,
    ): Int {
        val layout = layoutRuntime.getLayout() ?: return 0
        return layout.getOffsetForHorizontal(line, x)
    }

    fun getLayoutLineTop(line: Int): Int {
        val layout = layoutRuntime.getLayout() ?: return 0
        return layout.getLineTop(line)
    }

    fun getLayoutLineBottom(line: Int): Int {
        val layout = layoutRuntime.getLayout() ?: return 0
        return layout.getLineBottom(line)
    }

    fun getLayoutLineForOffset(offsetUtf16: Int): Int {
        val layout = layoutRuntime.getLayout() ?: return 0
        return layout.getLineForOffset(offsetUtf16)
    }

    fun getLayoutPrimaryHorizontal(offsetUtf16: Int): Float {
        val layout = layoutRuntime.getLayout() ?: return 0f
        return layout.getPrimaryHorizontal(offsetUtf16)
    }

    fun setLineSpacingMultiplier(multiplier: Float) {
        layoutRuntime.setLineSpacingMultiplier(multiplier)
    }

    fun setThemeColors(
        textColor: Int,
        cursorColor: Int,
        selectionColor: Int,
        preeditColor: Int,
        bgColor: Int = Color.WHITE,
        searchHighlightColor: Int = 0,
    ) {
        renderRuntime.setThemeColors(
            textColor,
            cursorColor,
            selectionColor,
            preeditColor,
            bgColor,
            searchHighlightColor,
        )
    }

    fun utf16ToUtf8(offsetUtf16: Int): Int {
        val projection = layoutRuntime.getCurrentProjection()
        return projection.displayUtf16ToRealUtf8(offsetUtf16)
    }

    fun utf8ToUtf16(offsetUtf8: Int): Int {
        val projection = layoutRuntime.getCurrentProjection()
        return projection.realUtf8ToDisplayUtf16(offsetUtf8)
    }

    fun getSpannable(): android.text.SpannableStringBuilder = editPipeline.getSpannable()

    sealed class LoadTextResult {
        data class Loaded(val result: EditResult) : LoadTextResult()

        object Failed : LoadTextResult()
    }

    /**
     * Reset the pipeline for reuse by a different editing target (session rebind).
     *
     * Clears target-specific transient state (active animation, mirror content, layout)
     * but preserves the pipeline infrastructure (LayoutEngine, VisualPlanner, Renderer,
     * ResourceStore, InputAdapter) so the shared host can be rebound without recreating
     * the full pipeline. Per #541, this corresponds to the resetForReuse lifecycle step
     * when the EditorWindowHost switches between EditableTextTargets.
     *
     * Uses [AndroidTextAnimationEngine.cancel] (not [resetForSession]) because the
     * ResourceStore is shared across targets — [cancel] releases only the active
     * transaction's snapshots, while [resetForSession] would release ALL snapshots
     * including those from completed transactions that may still be referenced by
     * the renderer. The mirror is immediately reloaded with the new target's content,
     * so stale Bitmaps from the old target are harmless (they will be replaced by new
     * captures on the next edit).
     */
    fun resetForReuse() {
        visualRuntime.cancel()
        pendingCompositionVisualIntent = null
        editPipeline.loadFromSnapshot("", 0, 0, 0, 0)
        layoutRuntime.rebuildDisplayProjection()
    }

    // ── InputCommandPort implementation ──

    override fun commitComposition(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        compositionSessionId: Long,
        compositionBaseRevision: Long,
        compositionGeneration: Long,
        cause: EditorTransactionCauseDto,
    ): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.commitText(
            byteStart, byteEndExclusive, replacementText, resultingSelectionAnchor,
            resultingSelectionHead, compositionSessionId, compositionBaseRevision,
            compositionGeneration, cause, mirror.getRevision(),
        )
    }

    override fun deleteSurrounding(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: EditorTransactionCauseDto,
    ): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.deleteSurrounding(
            beforeByteStart,
            beforeByteEndExclusive,
            afterByteStart,
            afterByteEndExclusive,
            cause,
            mirror.getRevision(),
        )
    }

    override fun beginComposition(
        replaceStart: Int,
        replaceEndExclusive: Int,
    ): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.beginComposition(replaceStart, replaceEndExclusive, mirror.getRevision())
    }

    override fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int,
    ): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        val dto =
            bridge.updateComposition(
                compositionSessionId,
                compositionGeneration,
                newPreeditText,
                newPreeditCursorOffset,
                mirror.getRevision(),
            ) ?: return null
        pendingCompositionVisualIntent = EditResult.fromDto(dto).visualIntent
        return dto
    }

    override fun finishComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
    ): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.finishComposition(compositionSessionId, compositionGeneration, mirror.getRevision())
        if (dto != null) {
            pendingCompositionVisualIntent = EditResult.fromDto(dto).visualIntent
        }
        return dto
    }

    override fun cancelComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
    ): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        val dto = bridge.cancelComposition(compositionSessionId, compositionGeneration, mirror.getRevision())
        if (dto != null) {
            pendingCompositionVisualIntent = EditResult.fromDto(dto).visualIntent
        }
        return dto
    }

    private var cursorVisible: Boolean = true
    private var selectionAllowed: Boolean = true
    private var copyAllowed: Boolean = true
    private var pasteAllowed: Boolean = true

    fun setCursorVisible(visible: Boolean) {
        cursorVisible = visible
    }

    fun isCursorVisible(): Boolean = cursorVisible

    fun setSelectionAllowed(allowed: Boolean) {
        selectionAllowed = allowed
    }

    fun isSelectionAllowed(): Boolean = selectionAllowed

    fun setCopyAllowed(allowed: Boolean) {
        copyAllowed = allowed
    }

    fun isCopyAllowed(): Boolean = copyAllowed

    fun setPasteAllowed(allowed: Boolean) {
        pasteAllowed = allowed
    }

    fun isPasteAllowed(): Boolean = pasteAllowed

    fun setMaxLength(max: Int) {
        maxLength = max
    }

    fun getMaxLength(): Int = maxLength

    fun setSecretDisplayMode(enabled: Boolean) {
        layoutRuntime.setSecretDisplayMode(enabled)
    }

    fun isSecretDisplayMode(): Boolean = layoutRuntime.isSecretDisplayMode()

    fun getCurrentProjection(): DisplayTextProjection = layoutRuntime.getCurrentProjection()

    /**
     * #630 评论12 项3: 语义 no-op 检测 — 光标/选区确实完全没变的 CURSOR_ONLY。
     * 提取为独立方法以通过 detekt ComplexCondition 阈值。
     */
    private fun isCursorOnlyNoOp(result: EditResult): Boolean {
        return result.displayPatches.isEmpty() &&
            result.baseRevision == result.newRevision &&
            result.oldSelectionStart == result.newSelectionStart &&
            result.oldSelectionEnd == result.newSelectionEnd &&
            result.visualIntent.isCursorOnly()
    }

    fun setAnimationPolicy(policy: com.xiwei.sujian.feature.editor.visual.TextAnimationPolicy) {
        visualRuntime.setAnimationPolicy(policy)
    }

    fun releaseAnimationResources() {
        visualRuntime.release()
    }

    fun setRendererThemeColors(
        textColor: Int,
        cursorColor: Int,
        selectionColor: Int,
        preeditColor: Int,
        bgColor: Int = Color.WHITE,
        searchHighlightColor: Int = 0,
    ) {
        renderRuntime.setThemeColors(
            textColor,
            cursorColor,
            selectionColor,
            preeditColor,
            bgColor,
            searchHighlightColor,
        )
    }
}
