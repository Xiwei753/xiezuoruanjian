package com.xiwei.sujian.editor.v2.pipeline

import android.graphics.Color
import android.text.TextPaint
import com.xiwei.sujian.editor.v2.mirror.DisplayTextMirror
import com.xiwei.sujian.editor.v2.mirror.EditResult
import com.xiwei.sujian.editor.v2.mirror.VisualIntent
import com.xiwei.sujian.editor.v2.mirror.CoordinatedCursor
import com.xiwei.sujian.editor.v2.layout.AndroidLayoutEngine
import com.xiwei.sujian.editor.v2.host.EditorKernelBridge
import com.xiwei.sujian.editor.v2.projection.DisplayTextProjection
import uniffi.writer_core.EditorTransactionCauseDto

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
    data class Edited(val result: EditResult) : PipelineOutput()
    object NeedReload : PipelineOutput()
    object StaleOrInvalid : PipelineOutput()
}

class AndroidEditorPipeline private constructor(
    val editPipeline: EditPipeline,
    private val renderRuntime: AndroidRenderRuntime,
    private val layoutRuntime: AndroidLayoutRuntime,
    private val visualRuntime: AndroidVisualRuntime
) : EditorCommandPort, InputCommandPort {

    override val mirror: DisplayTextMirror get() = editPipeline.mirror
    override var kernelBridge: EditorKernelBridge?
        get() = editPipeline.kernelBridge
        set(value) { editPipeline.setKernelBridge(value) }

    companion object {
        fun create(mirror: DisplayTextMirror, textPaint: TextPaint): AndroidEditorPipeline {
            val editPipeline = EditPipeline(mirror)
            val layoutRuntime = AndroidLayoutRuntime(mirror, textPaint)
            val visualRuntime = AndroidVisualRuntime()
            val renderRuntime = AndroidRenderRuntime()
            return AndroidEditorPipeline(editPipeline, renderRuntime, layoutRuntime, visualRuntime)
        }
    }

    private var autoIndentEnabled: Boolean = false
    private var autoIndentWidthSp: Float = 2f
    private var maxLength: Int = 0

    fun loadText(text: String, cursorUtf8: Int, @Suppress("UNUSED_PARAMETER") applySecret: Boolean = true): LoadTextResult {
        val result = editPipeline.loadText(text, cursorUtf8)
        if (result is LoadTextResult.Loaded) {
            resetAfterLoad()
            layoutRuntime.rebuildDisplayProjection()
        }
        return result
    }

    override fun insertText(byteOffset: Int, text: String, cause: EditorTransactionCauseDto): PipelineOutput {
        if (autoIndentEnabled && text == "\n") {
            val indentPrefix = computeAutoIndentPrefix()
            val result = editPipeline.insertLineBreak(byteOffset, indentPrefix, cause)
                ?: return PipelineOutput.StaleOrInvalid
            return applyEditResult(result)
        }
        val result = editPipeline.insertText(byteOffset, text, cause)
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    override fun deleteRange(byteStart: Int, byteEndExclusive: Int, cause: EditorTransactionCauseDto): PipelineOutput {
        val result = editPipeline.deleteRange(byteStart, byteEndExclusive, cause)
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    override fun replaceRangeTyped(byteStart: Int, byteEndExclusive: Int, replacementText: String, originalText: String, cause: EditorTransactionCauseDto, beforePatch: (() -> Unit)?): PipelineOutput {
        val result = editPipeline.replaceRange(byteStart, byteEndExclusive, replacementText, originalText, cause)
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result, beforePatch)
    }

    override fun setSelectionTyped(anchorByteOffset: Int, headByteOffset: Int): PipelineOutput {
        val result = editPipeline.setSelection(anchorByteOffset, headByteOffset)
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    fun performUndo(): PipelineOutput {
        val result = editPipeline.undo()
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    fun performRedo(): PipelineOutput {
        val result = editPipeline.redo()
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    fun replaceAll(searchStr: String, replaceStr: String): PipelineOutput {
        val result = editPipeline.replaceAll(searchStr, replaceStr)
            ?: return PipelineOutput.StaleOrInvalid
        return applyEditResult(result)
    }

    /**
     * Apply a composition commit with platform-side VisualIntent override.
     *
     * The Rust kernel returns a VisualIntent tailored for the raw edit, but the platform
     * must adjust it for two reasons:
     *
     * 1. **Visual-same suppression**: when the committed text is identical to the old
     *    preedit (e.g. IME confirms the same candidate), the visual text has not changed,
     *    so all text animation is suppressed (SYSTEM_SUPPRESSED) — only the cursor animates.
     *    This avoids a spurious fade-out + fade-in of unchanged text.
     *
     * 2. **Animation mode re-evaluation**: when Rust returns SYSTEM_SUPPRESSED but the
     *    platform detects actual byte-level changes, the animation mode is re-selected
     *    based on byte count (glyph/cluster/run) so the commit still animates properly.
     *    The operationKind is overridden to COMPOSITION_COMMIT to route into the replace
     *    animation path (which supports old→new matching via fingerprint).
     *
     * All byte ranges use half-open intervals [start, end).
     */
    override fun applyCompositionCommit(
        dto: uniffi.writer_core.EditorEditResultDto,
        preeditText: String
    ): PipelineOutput {
        val result = EditResult.fromDto(dto)
        val rustOldAffected = result.visualIntent.oldAffectedByteRanges
        val rustNewAffected = result.visualIntent.newAffectedByteRanges

        val committedText = result.displayPatches.firstOrNull()?.insertedText ?: ""
        val replaceStart = rustOldAffected.firstOrNull()?.first
            ?: rustNewAffected.firstOrNull()?.first
            ?: 0

        val preeditByteLen = preeditText.toByteArray(Charsets.UTF_8).size
        val committedByteLen = committedText.toByteArray(Charsets.UTF_8).size

        val oldAffected = if (preeditByteLen > 0) {
            listOf(Pair(replaceStart, replaceStart + preeditByteLen))
        } else {
            rustOldAffected
        }
        val newAffected = if (committedByteLen > 0) {
            listOf(Pair(replaceStart, replaceStart + committedByteLen))
        } else {
            rustNewAffected
        }

        val isVisualSame = preeditText.isNotEmpty() && preeditText == committedText
        if (isVisualSame) {
            val suppressedIntent = VisualIntent(
                cause = result.visualIntent.cause,
                operationKind = result.visualIntent.operationKind,
                oldAffectedByteRanges = oldAffected,
                newAffectedByteRanges = newAffected,
                animationMode = uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED,
                durationMs = 0L,
                coordinatedCursor = CoordinatedCursor(
                    result.visualIntent.coordinatedCursor.oldByteOffset,
                    result.visualIntent.coordinatedCursor.newByteOffset,
                    false
                )
            )
            return applyEditResultWithIntent(result, suppressedIntent)
        }
        if (result.visualIntent.animationMode == uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED &&
            (oldAffected.isNotEmpty() || newAffected.isNotEmpty())) {
            val byteCount = maxOf(
                newAffected.sumOf { it.second - it.first },
                oldAffected.sumOf { it.second - it.first }
            )
            // Animation mode selection for composition: uses grapheme cluster count (not byte
            // count like Rust's generic heuristic) because the platform knows the exact preedit
            // text and can account for grapheme characteristics. Newlines force LineReflow
            // (multi-line preedit); complex graphemes (combining marks, surrogates) force
            // ClusterAnimation for correct visual matching; short preedit uses GlyphAnimation
            // for per-character fade-in/out; longer preedit uses RunAnimation for efficiency.
            val animationMode = when {
                byteCount == 0 -> uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED
                byteCount <= 24 -> uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION
                byteCount <= 96 -> uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION
                else -> uniffi.writer_core.AnimationModeDto.RUN_ANIMATION
            }
            if (animationMode != uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED) {
                val animatedIntent = VisualIntent(
                    cause = result.visualIntent.cause,
                    operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
                    oldAffectedByteRanges = oldAffected,
                    newAffectedByteRanges = newAffected,
                    animationMode = animationMode,
                    durationMs = 200L,
                    coordinatedCursor = CoordinatedCursor(0, 0, true)
                )
                return applyEditResultWithIntent(result, animatedIntent)
            }
        }
        return applyEditResultWithIntent(result, VisualIntent(
            cause = result.visualIntent.cause,
            operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT,
            oldAffectedByteRanges = oldAffected,
            newAffectedByteRanges = newAffected,
            animationMode = result.visualIntent.animationMode,
            durationMs = result.visualIntent.durationMs,
            coordinatedCursor = result.visualIntent.coordinatedCursor
        ))
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
        beforePatch: (() -> Unit)?
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

        visualRuntime.prepareAndSubmit(
            visualIntent = result.visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                editPipeline.applyEditResult(result)
                layoutRuntime.rebuildDisplayProjection()
            },
            beforePatch = beforePatch
        )

        return PipelineOutput.Edited(result)
    }

    private fun applyEditResultWithIntent(
        result: EditResult,
        visualIntent: VisualIntent,
        beforePatch: (() -> Unit)? = null
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

        visualRuntime.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                editPipeline.applyEditResult(result)
                layoutRuntime.rebuildDisplayProjection()
            },
            beforePatch = beforePatch
        )

        return PipelineOutput.Edited(result)
    }

    /**
     * Apply a composition update using a caller-constructed [VisualIntent].
     *
     * This is the low-level entry point used when the caller (typically the input adapter)
     * has already constructed the appropriate [VisualIntent] — e.g. when Rust provides the
     * visual intent directly. For the common case where the platform constructs its own
     * [VisualIntent] based on old/new preedit text, use [applyCompositionUpdateAnimated]
     * instead, which handles visual-same suppression and animation mode selection.
     */
    fun applyCompositionUpdate(
        visualIntent: VisualIntent,
        mirrorUpdate: (() -> Unit)? = null
    ) {
        visualRuntime.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                mirrorUpdate?.invoke()
                layoutRuntime.rebuildDisplayProjection()
            }
        )
    }

    /**
     * Apply a composition update with platform-constructed VisualIntent.
     *
     * The platform constructs its own VisualIntent rather than using Rust's because:
     * - The platform knows the exact old/new preedit text and can detect visual-same
     *   (old preedit == new preedit) to suppress unnecessary animation.
     * - Animation mode is selected by grapheme cluster count and content characteristics
     *   (newlines → LineReflow, complex graphemes → Cluster, etc.), not by Rust's
     *   generic byte-count heuristic.
     * - oldAffected/newAffected are computed from the preedit byte ranges, ensuring
     *   only the changed preedit region animates.
     *
     * When [isVisualSame] is true, animation is suppressed (SYSTEM_SUPPRESSED) and only
     * the cursor animates — the preedit text has not visually changed. However, the
     * [mirrorUpdate] lambda is still invoked (via [AndroidTextAnimationEngine.prepareAndSubmit])
     * to keep the mirror's composition overlay in sync with the IME state, even when
     * no text animation runs.
     */
    override fun applyCompositionUpdateAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        newPreeditText: String,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)?
    ) {
        val oldPreeditByteLen = oldPreeditText.toByteArray(Charsets.UTF_8).size
        val newPreeditByteLen = newPreeditText.toByteArray(Charsets.UTF_8).size
        val isVisualSame = oldPreeditText.isNotEmpty() && oldPreeditText == newPreeditText
        val oldAffected = buildList {
            if (oldPreeditByteLen > 0 && !isVisualSame) {
                add(Pair(replaceStartUtf8, replaceStartUtf8 + oldPreeditByteLen))
            } else if (oldPreeditByteLen == 0 && replaceStartUtf8 < replaceEndUtf8) {
                add(Pair(replaceStartUtf8, replaceEndUtf8))
            }
        }
        val newAffected = if (newPreeditText.isEmpty() || isVisualSame) emptyList() else listOf(Pair(replaceStartUtf8, replaceStartUtf8 + newPreeditByteLen))
        val combinedText = oldPreeditText + newPreeditText
        // Both old and new preedit text are checked for newline/complex grapheme
        // characteristics because either version could contain them — e.g. an IME
        // candidate that introduces a newline or combining mark. Checking only the
        // new text would miss cases where the old preedit had a newline that is now
        // being removed (which still requires LineReflow for correct reflow animation).
        val clusterCount = maxOf(
            newPreeditText.codePointCount(0, newPreeditText.length),
            oldPreeditText.codePointCount(0, oldPreeditText.length)
        )
        val containsNewline = combinedText.any { it == '\n' || it == '\r' }
        val containsComplexGrapheme = combinedText.any { char ->
            val type = Character.getType(char.code)
            type == Character.SURROGATE.toInt() ||
                type == Character.NON_SPACING_MARK.toInt() ||
                type == Character.COMBINING_SPACING_MARK.toInt() ||
                Character.isHighSurrogate(char) || Character.isLowSurrogate(char)
        }
        val animationMode = when {
            isVisualSame || clusterCount == 0 -> uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED
            containsNewline -> uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION
            containsComplexGrapheme -> uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION
            clusterCount <= 8 -> uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION
            else -> uniffi.writer_core.AnimationModeDto.RUN_ANIMATION
        }
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION,
            operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE,
            oldAffectedByteRanges = oldAffected,
            newAffectedByteRanges = newAffected,
            animationMode = animationMode,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(0, 0, true)
        )
        visualRuntime.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                mirrorUpdate?.invoke()
                layoutRuntime.rebuildDisplayProjection()
            }
        )
    }

    /**
     * Apply a composition cancel with platform-constructed VisualIntent.
     *
     * CompositionCancel is semantically a delete: the preedit text is removed and the
     * cursor returns to the pre-edit position. The VisualIntent uses empty
     * [newAffectedByteRanges] (no new text inserted) and [oldAffectedByteRanges] covering
     * the preedit span, which routes through the planner's Delete path to produce
     * CrossfadeOld/fade-out slices for the cancelled preedit. Retained text after the
     * preedit gets Move slices via [addMoveSlicesForShiftedClustersCrossLine].
     *
     * Animation mode is fixed at CLUSTER_ANIMATION (not glyph or run) because composition
     * cancel typically involves short preedit spans where per-cluster fade-out provides
     * the best visual granularity without the overhead of per-glypheme slices.
     */
    override fun applyCompositionCancelAnimated(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        oldPreeditText: String,
        mirrorUpdate: (() -> Unit)?
    ) {
        val preeditByteLen = oldPreeditText.toByteArray(Charsets.UTF_8).size
        val oldAffected = if (preeditByteLen == 0 && replaceStartUtf8 == replaceEndUtf8) emptyList()
            else if (preeditByteLen > 0) listOf(Pair(replaceStartUtf8, replaceStartUtf8 + preeditByteLen))
            else listOf(Pair(replaceStartUtf8, replaceEndUtf8))
        val visualIntent = VisualIntent(
            cause = uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION,
            operationKind = uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL,
            oldAffectedByteRanges = oldAffected,
            newAffectedByteRanges = emptyList(),
            animationMode = uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION,
            durationMs = 160L,
            coordinatedCursor = CoordinatedCursor(0, 0, true)
        )
        visualRuntime.prepareAndSubmit(
            visualIntent = visualIntent,
            layoutEngine = layoutRuntime.layoutEngine,
            mirrorUpdate = {
                mirrorUpdate?.invoke()
                layoutRuntime.rebuildDisplayProjection()
            }
        )
    }

    /**
     * Request a layout rebuild after composition state changes.
     *
     * Composition overlay changes (preedit text, underline) are applied to the mirror's
     * Spannable but do not automatically trigger a layout rebuild. This method must be
     * called after composition updates so that [AndroidLayoutEngine] produces a new
     * [AndroidLayoutRevision] reflecting the updated composition overlay — without it,
     * the renderer would draw the old composition state.
     */
    override fun onCompositionUpdated() {
        layoutRuntime.requestLayout()
    }

    /**
     * Render one frame. Layer order when animation is active:
     * background → search highlights → selection → static text with holes → animated slices
     * → preedit underline → animated cursor (or static cursor if no cursor transition).
     *
     * Without animation: background → search highlights → selection → static text
     * → preedit underline → static cursor.
     */
    fun drawFrame(canvas: android.graphics.Canvas, searchHighlightsUtf16: List<Pair<Int, Int>>, viewportWidth: Int, viewportHeight: Int, scrollX: Float, scrollY: Float) {
        val frameTimeMs = System.nanoTime() / 1_000_000
        val projection = layoutRuntime.getCurrentProjection()
        val cursorDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
        val selStartDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionStartUtf8())
        val selEndDisplayUtf16 = projection.realUtf8ToDisplayUtf16(mirror.getSelectionEndUtf8())
        val frameState = visualRuntime.tick(
            frameTimeMs,
            layoutRuntime.getLayout(),
            layoutRuntime.getCurrentRevision(),
            searchHighlightsUtf16,
            viewportWidth, viewportHeight,
            scrollX, scrollY,
            cursorVisible, selectionAllowed,
            cursorDisplayUtf16,
            selStartDisplayUtf16,
            selEndDisplayUtf16
        )
        if (frameState != null) {
            renderRuntime.drawFromFrameState(canvas, frameState)
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
    fun getDisplayCursorUtf16(): Int = layoutRuntime.getCurrentProjection().realUtf8ToDisplayUtf16(mirror.getCursorUtf8())
    fun getSelectionStartUtf8(): Int = editPipeline.getSelectionStartUtf8()
    fun getSelectionEndUtf8(): Int = editPipeline.getSelectionEndUtf8()
    fun getSelectionStartUtf16(): Int = editPipeline.getSelectionStartUtf16()
    fun getSelectionEndUtf16(): Int = editPipeline.getSelectionEndUtf16()
    fun getLengthUtf16(): Int = editPipeline.getLengthUtf16()
    fun getCommittedCursorUtf8(): Int = editPipeline.getCommittedCursorUtf8()
    fun getCommittedSelectionStartUtf8(): Int = editPipeline.getCommittedSelectionStartUtf8()
    fun getCommittedSelectionEndUtf8(): Int = editPipeline.getCommittedSelectionEndUtf8()
    fun getCommittedText(): String = editPipeline.getCommittedText()

    fun setAutoIndent(enabled: Boolean, widthSp: Float) {
        autoIndentEnabled = enabled
        autoIndentWidthSp = widthSp
    }

    fun isAutoIndentEnabled(): Boolean = autoIndentEnabled
    fun getAutoIndentWidthSp(): Float = autoIndentWidthSp

    private fun computeAutoIndentPrefix(): String {
        if (!autoIndentEnabled) return ""
        val projection = layoutRuntime.getCurrentProjection()
        val cursorUtf8 = mirror.getCursorUtf8()
        val cursorRealUtf16 = projection.realUtf8ToRealUtf16(cursorUtf8)
        val text = mirror.getText()
        val safeCursorUtf16 = cursorRealUtf16.coerceIn(0, text.length)
        val lineStartUtf16 = if (safeCursorUtf16 > 0) text.lastIndexOf('\n', safeCursorUtf16 - 1) + 1 else 0
        val linePrefix = text.substring(lineStartUtf16, safeCursorUtf16)
        val indent = linePrefix.takeWhile { it == ' ' || it == '\t' }
        return indent
    }

    fun previousGraphemeByteLen(offset: Int): Int {
        val projection = layoutRuntime.getCurrentProjection()
        val realUtf16 = projection.realUtf8ToRealUtf16(offset)
        if (realUtf16 <= 0) return 0
        val iter = android.icu.text.BreakIterator.getCharacterInstance()
        iter.setText(mirror.getText())
        val prev = iter.preceding(realUtf16)
        if (prev == android.icu.text.BreakIterator.DONE) return 0
        val prevUtf8 = projection.realUtf16ToRealUtf8(prev)
        return offset - prevUtf8
    }

    fun nextGraphemeByteLen(offset: Int): Int {
        val projection = layoutRuntime.getCurrentProjection()
        val realUtf16 = projection.realUtf8ToRealUtf16(offset)
        if (realUtf16 >= mirror.getLengthUtf16()) return 0
        val iter = android.icu.text.BreakIterator.getCharacterInstance()
        iter.setText(mirror.getText())
        val next = iter.following(realUtf16)
        if (next == android.icu.text.BreakIterator.DONE) return 0
        val nextUtf8 = projection.realUtf16ToRealUtf8(next)
        return nextUtf8 - offset
    }

    fun getLayoutMaxScrollY(viewHeight: Int): Float {
        val layout = layoutRuntime.getLayout() ?: return 0f
        return (layout.height - viewHeight).coerceAtLeast(0).toFloat()
    }

    fun getLayoutLineForVertical(y: Int): Int {
        val layout = layoutRuntime.getLayout() ?: return 0
        return layout.getLineForVertical(y)
    }

    fun getLayoutOffsetForHorizontal(line: Int, x: Float): Int {
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

    fun setThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int = Color.WHITE, searchHighlightColor: Int = Color.argb(40, 255, 200, 0)) {
        renderRuntime.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor, searchHighlightColor)
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
     * when the AnimatedTextEditorCoordinator switches between EditableTextTargets.
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
        editPipeline.loadFromSnapshot("", 0, 0, 0, 0)
        layoutRuntime.rebuildDisplayProjection()
    }

    // ── InputCommandPort implementation ──

    override fun commitComposition(byteStart: Int, byteEndExclusive: Int, replacementText: String, resultingSelectionAnchor: Int, resultingSelectionHead: Int, compositionSessionId: Long, compositionBaseRevision: Long, compositionGeneration: Long, cause: EditorTransactionCauseDto): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.commitText(byteStart, byteEndExclusive, replacementText, resultingSelectionAnchor, resultingSelectionHead, compositionSessionId, compositionBaseRevision, compositionGeneration, cause, mirror.getRevision())
    }

    override fun deleteSurrounding(beforeByteStart: Int, beforeByteEndExclusive: Int, afterByteStart: Int, afterByteEndExclusive: Int, cause: EditorTransactionCauseDto): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.deleteSurrounding(beforeByteStart, beforeByteEndExclusive, afterByteStart, afterByteEndExclusive, cause, mirror.getRevision())
    }

    override fun beginComposition(replaceStart: Int, replaceEndExclusive: Int): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.beginComposition(replaceStart, replaceEndExclusive, mirror.getRevision())
    }

    override fun updateComposition(compositionSessionId: Long, compositionGeneration: Long, newPreeditText: String, newPreeditCursorOffset: Int): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.updateComposition(compositionSessionId, compositionGeneration, newPreeditText, newPreeditCursorOffset, mirror.getRevision())
    }

    override fun finishComposition(compositionSessionId: Long, compositionGeneration: Long): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.finishComposition(compositionSessionId, compositionGeneration, mirror.getRevision())
    }

    override fun cancelComposition(compositionSessionId: Long, compositionGeneration: Long): uniffi.writer_core.EditorEditResultDto? {
        val bridge = kernelBridge ?: return null
        return bridge.cancelComposition(compositionSessionId, compositionGeneration, mirror.getRevision())
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

    fun applySecretDisplayIfActive() {
        layoutRuntime.rebuildDisplayProjection()
    }

    fun applySecretDisplayIfActiveWithLayout() {
        layoutRuntime.rebuildDisplayProjection()
    }

    fun rebuildDisplayProjection() {
        layoutRuntime.rebuildDisplayProjection()
    }

    fun getCurrentProjection(): DisplayTextProjection = layoutRuntime.getCurrentProjection()

    fun setAnimationPolicy(policy: com.xiwei.sujian.editor.v2.visual.TextAnimationPolicy) {
        visualRuntime.setAnimationPolicy(policy)
    }

    fun releaseAnimationResources() {
        visualRuntime.release()
    }

    fun setRendererThemeColors(textColor: Int, cursorColor: Int, selectionColor: Int, preeditColor: Int, bgColor: Int = Color.WHITE, searchHighlightColor: Int = android.graphics.Color.argb(40, 255, 200, 0)) {
        renderRuntime.setThemeColors(textColor, cursorColor, selectionColor, preeditColor, bgColor, searchHighlightColor)
    }
}
