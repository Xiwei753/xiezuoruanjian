package com.xiwei.sujian.feature.editor.projection

import android.text.SpannableStringBuilder
import android.text.style.UnderlineSpan
import uniffi.writer_core.AnimationModeDto
import uniffi.writer_core.CoordinatedCursorDto
import uniffi.writer_core.DisplayPatchDto
import uniffi.writer_core.EditorEditResultDto
import uniffi.writer_core.EditorOperationKindDto
import uniffi.writer_core.EditorTransactionCauseDto
import uniffi.writer_core.EditorVisualIntentDto
import uniffi.writer_core.OffsetMapDto
import uniffi.writer_core.OffsetMapEntryDto
import uniffi.writer_core.OffsetMapKindDto

/**
 * A single incremental text patch from the Rust kernel.
 * Byte ranges are half-open: [replaceByteStart, replaceByteEndExclusive).
 */
data class DisplayPatch(
    val baseRevision: Long,
    val newRevision: Long,
    val replaceByteStart: Int,
    val replaceByteEndExclusive: Int,
    val insertedText: String,
    val resultingSelectionStart: Int,
    val resultingSelectionEnd: Int,
) {
    companion object {
        fun fromDto(dto: DisplayPatchDto): DisplayPatch =
            DisplayPatch(
                baseRevision = dto.baseRevision.toLong(),
                newRevision = dto.newRevision.toLong(),
                replaceByteStart = dto.replaceByteStart.toInt(),
                replaceByteEndExclusive = dto.replaceByteEndExclusive.toInt(),
                insertedText = dto.insertedText,
                resultingSelectionStart = dto.resultingSelectionStart.toInt(),
                resultingSelectionEnd = dto.resultingSelectionEnd.toInt(),
            )

        fun fromDtoList(dtos: List<DisplayPatchDto>): List<DisplayPatch> = dtos.map { fromDto(it) }
    }
}

data class VisualIntent(
    val cause: EditorTransactionCauseDto,
    /**
     * Semantic category of the edit, used by the animation planner to select the correct
     * slice-generation path. Composition operations are separate kinds (not just REPLACE)
     * because the preedit text is a virtual overlay on the committed buffer — the planner
     * must treat COMPOSITION_CANCEL as a Delete (preedit text fades out, retained text
     * Moves back) and COMPOSITION_COMMIT/UPDATE as a Replace (old preedit fades out,
     * new text fades in, retained text Moves). Using REPLACE for all three would lose
     * the Delete semantics of cancel and the virtual-overlay semantics of update/commit.
     */
    val operationKind: EditorOperationKindDto,
    /** Byte ranges in the old document affected by this edit. Half-open: [start, end).
     *  For pure Insert, this list is empty (no old bytes were affected). */
    val oldAffectedByteRanges: List<Pair<Int, Int>>,
    /** Byte ranges in the new document affected by this edit. Half-open: [start, end).
     *  For pure Delete, this list is empty (no new bytes were created). */
    val newAffectedByteRanges: List<Pair<Int, Int>>,
    val animationMode: AnimationModeDto,
    val durationMs: Long,
    val coordinatedCursor: CoordinatedCursor,
    /**
     * #606: Core-returned old→new byte offset map for this edit.
     *
     * When non-null, [AffectedLayoutPlanner] consumes this map directly instead of
     * reconstructing offset mappings from old/new affected ranges. When null (e.g.
     * cursor-only operations, or Core determines the edit has no structural mapping),
     * the planner falls back to identity mapping. This is the single source of truth
     * for old→new offset translation semantics.
     */
    val offsetMap: OffsetMap? = null,
) {
    companion object {
        fun fromDto(dto: EditorVisualIntentDto): VisualIntent =
            VisualIntent(
                cause = dto.cause,
                operationKind = dto.operationKind,
                oldAffectedByteRanges =
                    dto.oldAffectedByteRanges.map {
                        Pair(
                            it.start.toInt(),
                            it.endExclusive.toInt(),
                        )
                    },
                newAffectedByteRanges =
                    dto.newAffectedByteRanges.map {
                        Pair(
                            it.start.toInt(),
                            it.endExclusive.toInt(),
                        )
                    },
                animationMode = dto.animationMode,
                durationMs = dto.durationMs.toLong(),
                coordinatedCursor = CoordinatedCursor.fromDto(dto.coordinatedCursor),
                offsetMap = dto.offsetMap?.let { OffsetMap.fromDto(it) },
            )
    }

    fun isInsert(): Boolean = operationKind == EditorOperationKindDto.INSERT

    fun isDelete(): Boolean = operationKind == EditorOperationKindDto.DELETE

    fun isReplace(): Boolean = operationKind == EditorOperationKindDto.REPLACE

    fun isCompositionUpdate(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_UPDATE

    fun isCompositionCommit(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_COMMIT

    fun isCompositionCancel(): Boolean = operationKind == EditorOperationKindDto.COMPOSITION_CANCEL

    fun isCursorOnly(): Boolean = operationKind == EditorOperationKindDto.CURSOR_ONLY

    /**
     * #606: Rendering-role helpers — dispatch the planner into the correct slice-generation
     * path based on Core's [operationKind]. These are NOT local re-classification; they
     * consume Core's classification to select the Android rendering path:
     * - INSERT → insert slice path (reveal new clusters)
     * - DELETE, COMPOSITION_CANCEL → delete slice path (swallow old clusters)
     * - REPLACE, COMPOSITION_COMMIT, COMPOSITION_UPDATE → replace slice path (match old↔new)
     */
    fun isInsertRenderRole(): Boolean = isInsert()

    fun isDeleteRenderRole(): Boolean = isDelete() || isCompositionCancel()

    fun isReplaceRenderRole(): Boolean = isReplace() || isCompositionCommit() || isCompositionUpdate()

    /**
     * Whether this edit removes or replaces existing text (vs pure insert/cursor-only).
     * Used by [AffectedLayoutPlanner] to include the next paragraph in affected lines
     * (delete/replace can cause the next paragraph to shift up). Consumes Core's
     * [operationKind] — not a local re-classification.
     */
    fun isDeleteOrReplaceRenderRole(): Boolean =
        isDelete() || isReplace() || isCompositionCancel() || isCompositionCommit() ||
            isCompositionUpdate()
}

/**
 * #606: Core-returned old→new byte offset map.
 *
 * Each [OffsetMapEntry] covers a contiguous range of old byte offsets [oldByteOffset,
 * oldByteOffset + length) that maps to new byte offsets [newByteOffset, newByteOffset +
 * length). Offsets within an entry map linearly: old → newByteOffset + (old - oldByteOffset).
 * Offsets not covered by any entry are in the changed region and have no mapping (null).
 */
data class OffsetMap(
    val entries: List<OffsetMapEntry>,
) {
    companion object {
        fun fromDto(dto: OffsetMapDto): OffsetMap =
            OffsetMap(
                entries = dto.entries.map { OffsetMapEntry.fromDto(it) },
            )
    }
}

data class OffsetMapEntry(
    val oldByteOffset: Int,
    val newByteOffset: Int,
    val length: Int,
    val kind: OffsetMapKind,
) {
    companion object {
        fun fromDto(dto: OffsetMapEntryDto): OffsetMapEntry =
            OffsetMapEntry(
                oldByteOffset = dto.oldByteOffset.toInt(),
                newByteOffset = dto.newByteOffset.toInt(),
                length = dto.length.toInt(),
                kind = OffsetMapKind.fromDto(dto.kind),
            )
    }
}

enum class OffsetMapKind {
    IDENTITY,
    SHIFTED,
    ;

    companion object {
        fun fromDto(dto: OffsetMapKindDto): OffsetMapKind =
            when (dto) {
                OffsetMapKindDto.IDENTITY -> IDENTITY
                OffsetMapKindDto.SHIFTED -> SHIFTED
            }
    }
}

data class CoordinatedCursor(
    val oldByteOffset: Int,
    val newByteOffset: Int,
    val shouldAnimate: Boolean,
) {
    companion object {
        fun fromDto(dto: CoordinatedCursorDto): CoordinatedCursor =
            CoordinatedCursor(
                oldByteOffset = dto.oldByteOffset.toInt(),
                newByteOffset = dto.newByteOffset.toInt(),
                shouldAnimate = dto.shouldAnimate,
            )
    }
}

data class EditResult(
    val outcome: uniffi.writer_core.EditorEditOutcomeDto,
    val transactionId: Long,
    val baseRevision: Long,
    val newRevision: Long,
    val displayPatches: List<DisplayPatch>,
    val oldSelectionStart: Int,
    val oldSelectionEnd: Int,
    val newSelectionStart: Int,
    val newSelectionEnd: Int,
    val visualIntent: VisualIntent,
) {
    companion object {
        fun fromDto(dto: EditorEditResultDto): EditResult =
            EditResult(
                outcome = dto.outcome,
                transactionId = dto.transactionId.toLong(),
                baseRevision = dto.baseRevision.toLong(),
                newRevision = dto.newRevision.toLong(),
                displayPatches = DisplayPatch.fromDtoList(dto.displayPatches),
                oldSelectionStart = dto.oldSelectionStart.toInt(),
                oldSelectionEnd = dto.oldSelectionEnd.toInt(),
                newSelectionStart = dto.newSelectionStart.toInt(),
                newSelectionEnd = dto.newSelectionEnd.toInt(),
                visualIntent = VisualIntent.fromDto(dto.visualIntent),
            )
    }

    fun isApplied(): Boolean =
        outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED ||
            outcome == uniffi.writer_core.EditorEditOutcomeDto.APPLIED_WITH_ADJUSTED_SELECTION

    fun isStale(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.STALE_REVISION

    fun isInvalid(): Boolean =
        outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_OFFSET ||
            outcome == uniffi.writer_core.EditorEditOutcomeDto.INVALID_RANGE

    fun isNoChange(): Boolean = outcome == uniffi.writer_core.EditorEditOutcomeDto.NO_CHANGE
}

/**
 * Platform-side text buffer that mirrors the Rust EditorKernel's committed text state.
 *
 * Composition overlay model: when an IME composition is active, the preedit text is
 * overlaid on top of the committed text in the SpannableStringBuilder (with an
 * UnderlineSpan). The original text under the preedit range is saved in
 * [compositionOriginalText] and restored when the composition is cleared or committed.
 *
 * Design intent: the overlay model exists because the Rust EditorKernel operates on
 * committed text only — it never sees the preedit. The platform must maintain the
 * committed-text view for the kernel (via [getCommittedText]/[getCommittedCursorUtf8])
 * while simultaneously presenting the preedit to the IME and layout engine. Directly
 * modifying the buffer with preedit text and then reverting on cancel would require
 * the kernel to undo a non-existent edit; the overlay avoids this by keeping the
 * committed buffer untouched and layering the preedit on top.
 *
 * "Committed" accessors ([getCommittedCursorUtf8], [getCommittedText], etc.) return
 * values as if the active composition did not exist — they reflect the state that the
 * Rust kernel sees, which operates on committed text only. The IME sees the full buffer
 * including the preedit overlay.
 *
 * Thread constraint: this class is not thread-safe; all access must be on the UI thread.
 */
class DisplayTextMirror {
    private val buffer = SpannableStringBuilder()

    /**
     * #624 评论4: 增量 UTF-8/UTF-16 偏移索引 — 由本 mirror 唯一持有，随 patch/装载
     * 增量更新，避免每键整章偏移索引重建。identity projections
     * 长期引用本索引，不再每键 mirror.getText() / buildRealBoundaries()。
     */
    private val textOffsetIndex = TextOffsetIndex()
    private var currentRevision: Long = 0
    private var cursorUtf8: Int = 0
    private var cursorUtf16: Int = 0
    private var compositionStartUtf16: Int = -1
    private var compositionEndUtf16: Int = -1
    private var selectionAnchorUtf8: Int = 0
    private var selectionHeadUtf8: Int = 0
    private var selectionAnchorUtf16: Int = 0
    private var selectionHeadUtf16: Int = 0

    /** Committed-text UTF-8 byte offset where the composition replacement starts.
     *  In committed-text coordinates (not virtual/preedit coordinates), matching the
     *  Rust CompositionSession convention. The kernel only knows committed text, so
     *  all composition range parameters sent to the kernel must use these coordinates.
     *  The virtual preedit range in the buffer ([compositionStartUtf16]/[compositionEndUtf16])
     *  is in full-buffer coordinates (including the overlay) and must NOT be sent to the kernel. */
    private var compositionReplaceStartUtf8: Int = 0

    /** Committed-text UTF-8 byte offset where the composition replacement ends (exclusive).
     *  Same coordinate convention as [compositionReplaceStartUtf8] — committed-text space. */
    private var compositionReplaceEndUtf8: Int = 0
    private var compositionOriginalText: String = ""
    private var hasActiveComposition: Boolean = false

    /**
     * 已提交缓冲区的 UTF-8 字节长度 — 随 patch/装载增量维护，避免热路径上
     * `getText().toByteArray(Charsets.UTF_8).size` 的整章拷贝（#624 评论3：
     * 每键热路径不得做整章级复制）。
     */
    private var committedTextLengthUtf8: Int = 0

    /** 活动 composition 覆盖层相对已提交文本的 UTF-8 字节增量（无覆盖时为 0）。 */
    private var compositionOverlayDeltaUtf8: Int = 0

    fun getText(): String = buffer.toString()

    /**
     * 当前缓冲区的 UTF-8 字节长度（含活动 composition 覆盖层，与 [getText] 一致）。
     * O(1) 读取 — 由 [applyPatches]/[loadFromSnapshot]/[updateComposition] 增量维护。
     */
    fun getTextLengthUtf8(): Int = committedTextLengthUtf8 + compositionOverlayDeltaUtf8

    fun getCursorUtf8(): Int = cursorUtf8

    fun getCursorUtf16(): Int = cursorUtf16

    fun getRevision(): Long = currentRevision

    fun getSpannable(): SpannableStringBuilder = buffer

    /** #624 评论4: 暴露增量偏移索引供 identity projection 长期引用。 */
    fun getTextOffsetIndex(): TextOffsetIndex = textOffsetIndex

    fun getLengthUtf16(): Int = buffer.length

    fun getSelectionStartUtf16(): Int = minOf(selectionAnchorUtf16, selectionHeadUtf16)

    fun getSelectionEndUtf16(): Int = maxOf(selectionAnchorUtf16, selectionHeadUtf16)

    fun getSelectionStartUtf8(): Int = minOf(selectionAnchorUtf8, selectionHeadUtf8)

    fun getSelectionEndUtf8(): Int = maxOf(selectionAnchorUtf8, selectionHeadUtf8)

    fun getSelectionAnchorUtf8(): Int = selectionAnchorUtf8

    fun getSelectionHeadUtf8(): Int = selectionHeadUtf8

    fun getSelectionAnchorUtf16(): Int = selectionAnchorUtf16

    fun getSelectionHeadUtf16(): Int = selectionHeadUtf16

    fun hasComposition(): Boolean = hasActiveComposition

    /** Cursor position in the committed text (excluding active composition overlay).
     *  When a composition is active, returns the start of the composition range —
     *  the Rust kernel's cursor is at the composition boundary, not inside the preedit. */
    fun getCommittedCursorUtf8(): Int {
        if (!hasActiveComposition) return cursorUtf8
        return compositionReplaceStartUtf8
    }

    fun getCommittedSelectionStartUtf8(): Int {
        if (!hasActiveComposition) return getSelectionStartUtf8()
        return compositionReplaceStartUtf8
    }

    fun getCommittedSelectionEndUtf8(): Int {
        if (!hasActiveComposition) return getSelectionEndUtf8()
        return compositionReplaceStartUtf8
    }

    /** Full text as seen by the Rust kernel (committed text only, excluding preedit overlay).
     *  Reconstructs the text by replacing the preedit range with [compositionOriginalText]. */
    fun getCommittedText(): String {
        if (!hasActiveComposition) return buffer.toString()
        val startUtf16 = textOffsetIndex.utf8ToUtf16(compositionReplaceStartUtf8)
        val endUtf16 = compositionStartUtf16
        return buffer.substring(0, startUtf16) +
            compositionOriginalText +
            buffer.substring(compositionEndUtf16.coerceAtMost(buffer.length))
    }

    /** 已提交文本的 UTF-8 字节长度（不含活动 composition 覆盖层）。O(1) 读取。 */
    fun getCommittedTextLengthUtf8(): Int = committedTextLengthUtf8

    /**
     * #624 评论7：已提交文本的 UTF-8 字节区间局部读取 — 只复制请求区间，不重建整篇。
     *
     * 无 composition 时直接用 [textOffsetIndex] 把两端映射到 UTF-16，对 buffer 做局部
     * subSequence。composition 时按 committed 坐标把区间拆成覆盖区前 /
     * [compositionOriginalText] / 覆盖区后三段局部拼接，禁止调用 [getCommittedText]。
     */
    fun committedSliceUtf8(startUtf8: Int, endUtf8: Int): String {
        val committedLen = committedTextLengthUtf8
        val safeStart = startUtf8.coerceIn(0, committedLen)
        val safeEnd = endUtf8.coerceIn(safeStart, committedLen)
        if (safeStart >= safeEnd) return ""

        if (!hasActiveComposition) {
            val start16 = textOffsetIndex.utf8ToUtf16(safeStart)
            val end16 = textOffsetIndex.utf8ToUtf16(safeEnd)
            return buffer.subSequence(start16, end16).toString()
        }

        // composition 时 committed text = buffer[0, compReplaceStartUtf16) + compositionOriginalText + buffer[compEndUtf16, len)
        val compStartUtf8 = compositionReplaceStartUtf8
        val compEndUtf8 = compositionReplaceEndUtf8
        val compReplaceStartUtf16 = textOffsetIndex.utf8ToUtf16(compStartUtf8)
        val compEndUtf16 = compositionEndUtf16.coerceAtMost(buffer.length)
        val origText = compositionOriginalText

        val sb = StringBuilder()
        // 覆盖区前
        val preEnd = minOf(safeEnd, compStartUtf8)
        if (safeStart < preEnd) {
            val s16 = textOffsetIndex.utf8ToUtf16(safeStart)
            val e16 = textOffsetIndex.utf8ToUtf16(preEnd)
            sb.append(buffer.subSequence(s16, e16))
        }
        // compositionOriginalText 段
        val origStart = maxOf(safeStart, compStartUtf8)
        val origEnd = minOf(safeEnd, compEndUtf8)
        if (origStart < origEnd) {
            val localStart = origStart - compStartUtf8
            val localEnd = origEnd - compStartUtf8
            sb.append(utf8SliceByBytes(origText, localStart, localEnd))
        }
        // 覆盖区后
        val postStart = maxOf(safeStart, compEndUtf8)
        if (postStart < safeEnd) {
            // committed UTF-8 c (c >= compEndUtf8) → buffer UTF-16:
            // 后段内 UTF-16 = index.utf8ToUtf16(c) - compReplaceStartUtf16 - origText.length
            // bufferUtf16 = compEndUtf16 + 后段内UTF16
            val committedUtf16Start = textOffsetIndex.utf8ToUtf16(postStart)
            val committedUtf16End = textOffsetIndex.utf8ToUtf16(safeEnd)
            val s16 = compEndUtf16 + (committedUtf16Start - compReplaceStartUtf16 - origText.length)
            val e16 = compEndUtf16 + (committedUtf16End - compReplaceStartUtf16 - origText.length)
            sb.append(buffer.subSequence(s16.coerceIn(0, buffer.length), e16.coerceIn(0, buffer.length)))
        }
        return sb.toString()
    }

    fun getCommittedLengthUtf16(): Int {
        if (!hasActiveComposition) return buffer.length
        val startUtf16 = textOffsetIndex.utf8ToUtf16(compositionReplaceStartUtf8)
        return startUtf16 +
            compositionOriginalText.length +
            (buffer.length - compositionEndUtf16.coerceAtMost(buffer.length))
    }

    fun applyEditResult(result: EditResult) {
        val hadComposition = hasActiveComposition
        // Overlay removal invariant: patches are generated by the Rust kernel against
        // committed text, so the buffer must be in committed-text state before applying them.
        // If the overlay were left in place, the patch's UTF-8→UTF-16 offset mapping would
        // be wrong because the buffer contains virtual preedit text that the kernel doesn't
        // know about — the kernel's byte offsets map to committed text, not the overlaid text.
        if (hadComposition) {
            removeCompositionOverlay()
        }
        applyPatches(result.displayPatches)
        updateSelectionFromResult(result)
    }

    private fun updateSelectionFromResult(result: EditResult) {
        val normStart = minOf(result.newSelectionStart, result.newSelectionEnd)
        val normEnd = maxOf(result.newSelectionStart, result.newSelectionEnd)
        cursorUtf8 = normEnd
        cursorUtf16 = textOffsetIndex.utf8ToUtf16(normEnd)
        selectionAnchorUtf8 = normStart
        selectionHeadUtf8 = normEnd
        selectionAnchorUtf16 = textOffsetIndex.utf8ToUtf16(normStart)
        selectionHeadUtf16 = textOffsetIndex.utf8ToUtf16(normEnd)
    }

    /**
     * Apply a sequence of display patches from the Rust kernel.
     *
     * Revision continuity invariant: each patch's [baseRevision] must equal the mirror's
     * current [currentRevision]. A mismatch means patches were generated against an
     * outdated revision and the mirror must be reloaded from the kernel snapshot instead.
     *
     * After each patch, the UTF-8→UTF-16 index map is rebuilt because the buffer
     * content has changed — subsequent patches in the same batch must use updated offsets.
     */
    fun applyPatches(patches: List<DisplayPatch>) {
        if (patches.isEmpty()) return

        for (patch in patches) {
            if (patch.baseRevision != currentRevision) {
                throw IllegalStateException(
                    "DisplayTextMirror revision discontinuity: expected baseRevision=$currentRevision, " +
                        "got ${patch.baseRevision}. " +
                        "Must reload from EditorSession.",
                )
            }

            val normReplaceStart = minOf(patch.replaceByteStart, patch.replaceByteEndExclusive)
            val normReplaceEnd = maxOf(patch.replaceByteStart, patch.replaceByteEndExclusive)

            val replaceStartUtf16 = textOffsetIndex.utf8ToUtf16(normReplaceStart)
            val replaceEndUtf16 = textOffsetIndex.utf8ToUtf16(normReplaceEnd)

            buffer.replace(replaceStartUtf16, replaceEndUtf16, patch.insertedText as CharSequence)
            // 增量维护 UTF-8 字节长度：被替换区间字节数（patch 字节坐标）换成
            // 插入文本的字节数 — 不扫描整章。
            committedTextLengthUtf8 += utf8LengthOf(patch.insertedText) - (normReplaceEnd - normReplaceStart)
            // #624 评论4: 增量更新偏移索引 — 只扫描受影响段落，不整章重建。
            textOffsetIndex.onBufferReplaced(replaceStartUtf16, replaceEndUtf16, patch.insertedText, buffer)

            currentRevision = patch.newRevision
        }
    }

    fun applyDtoPatches(patches: List<DisplayPatchDto>) {
        applyPatches(DisplayPatch.fromDtoList(patches))
    }

    fun restoreCompositionBeforePatch() {
        removeCompositionOverlay()
    }

    /**
     * Update or begin a composition overlay.
     *
     * Composition lifecycle: begin (first updateComposition) → update (subsequent calls) →
     * commit (applyCompositionCommit) or cancel (clearComposition). The original text under
     * the preedit range is saved in [compositionOriginalText] and restored on commit/cancel
     * before the actual text replacement is applied.
     *
     * Overlay invariant: [removeCompositionOverlay] is always called first to restore the
     * committed text, ensuring the buffer is in a consistent committed-text state before
     * the new preedit is overlaid. Without this, consecutive updateComposition calls would
     * treat the previous preedit as committed text, corrupting [compositionOriginalText].
     */
    fun updateComposition(
        replaceStartUtf8: Int,
        replaceEndUtf8: Int,
        preeditText: String,
    ) {
        // 先把缓冲区恢复到 committed 状态，再用 textOffsetIndex 查询 committed 坐标 —
        // index 必须基于 committed 文本映射（replaceStart/End 是 committed 坐标）。
        // removeCompositionOverlay 已把 index 恢复到 committed 状态。若在覆盖层仍
        // 在缓冲区时查询，多字节区间会映射到覆盖层文本的错误 UTF-16 位置，连续
        // composition 更新会把替换变成插入（#624 评论3 回归测试暴露）。
        // #624 评论4: buffer.replace 叠加 preedit 后不调用 onBufferReplaced —
        // index 始终反映 committed 文本，不含 overlay。
        removeCompositionOverlay()

        compositionReplaceStartUtf8 = replaceStartUtf8
        compositionReplaceEndUtf8 = replaceEndUtf8
        val insertStartUtf16 = textOffsetIndex.utf8ToUtf16(replaceStartUtf8)
        val insertEndUtf16 = textOffsetIndex.utf8ToUtf16(replaceEndUtf8)

        if (insertStartUtf16 < insertEndUtf16) {
            compositionOriginalText = buffer.substring(insertStartUtf16, insertEndUtf16)
            buffer.replace(insertStartUtf16, insertEndUtf16, preeditText as CharSequence)
        } else {
            compositionOriginalText = ""
            buffer.insert(insertStartUtf16, preeditText as CharSequence)
        }
        // 覆盖层字节增量 = 新 preedit 字节数 - 被覆盖原文本节数（removeCompositionOverlay
        // 已把上一轮覆盖层的增量清零）。
        compositionOverlayDeltaUtf8 =
            utf8LengthOf(preeditText) - utf8LengthOf(compositionOriginalText)

        compositionStartUtf16 = insertStartUtf16
        compositionEndUtf16 = insertStartUtf16 + preeditText.length
        hasActiveComposition = true

        buffer.setSpan(
            UnderlineSpan(),
            compositionStartUtf16,
            compositionEndUtf16,
            // SPAN_EXCLUSIVE_EXCLUSIVE: the span does not expand when text is inserted at
            // its boundaries. This is correct for the preedit underline because the IME
            // controls the exact range — adjacent insertions should not extend the underline.
            SpannableStringBuilder.SPAN_EXCLUSIVE_EXCLUSIVE,
        )
    }

    fun clearComposition() {
        removeCompositionOverlay()
    }

    /**
     * Remove the composition overlay and restore the original committed text.
     * After removal, [compositionStartUtf16] is set to -1 (sentinel meaning "no active
     * composition") since 0 is a valid UTF-16 offset.
     *
     * Must be called before any edit that modifies the committed buffer (patches, composition
     * update, composition commit, composition cancel) to ensure the buffer reflects committed
     * text only. If the overlay were left in place, the patch would be applied to the virtual
     * text (including preedit), producing incorrect UTF-8→UTF-16 offset mappings and
     * corrupting the committed-text state that the kernel expects.
     */
    private fun removeCompositionOverlay() {
        if (!hasActiveComposition) return
        if (compositionStartUtf16 >= 0 && compositionEndUtf16 > compositionStartUtf16) {
            clearCompositionSpans()
            buffer.replace(compositionStartUtf16, compositionEndUtf16, compositionOriginalText)
            // #624 评论4: index 始终反映 committed 文本（updateComposition 叠加 overlay
            // 时不调用 onBufferReplaced），removeCompositionOverlay 恢复 buffer 到
            // committed 文本后 index 已经正确 — 不需要 onBufferReplaced。
        }
        compositionStartUtf16 = -1
        compositionEndUtf16 = -1
        hasActiveComposition = false
        compositionOriginalText = ""
        compositionOverlayDeltaUtf8 = 0
    }

    fun getCompositionRangeUtf16(): Pair<Int, Int>? {
        if (compositionStartUtf16 < 0) return null
        return Pair(compositionStartUtf16, compositionEndUtf16)
    }

    private fun clearCompositionSpans() {
        val spans = buffer.getSpans(0, buffer.length, UnderlineSpan::class.java)
        for (span in spans) {
            buffer.removeSpan(span)
        }
    }

    fun loadFromSnapshot(
        text: String,
        cursorUtf8: Int,
        revision: Long,
        selectionAnchorUtf8: Int = cursorUtf8,
        selectionHeadUtf8: Int = cursorUtf8,
    ) {
        buffer.clear()
        buffer.append(text)
        committedTextLengthUtf8 = utf8LengthOf(text)
        compositionOverlayDeltaUtf8 = 0
        // #624 评论4: 全量重建偏移索引 — 加载/快照路径扫描整篇一次。
        textOffsetIndex.rebuildFromText(text)
        this.cursorUtf8 = cursorUtf8
        this.currentRevision = revision
        this.compositionStartUtf16 = -1
        this.compositionEndUtf16 = -1
        this.hasActiveComposition = false
        this.compositionOriginalText = ""
        this.selectionAnchorUtf8 = selectionAnchorUtf8
        this.selectionHeadUtf8 = selectionHeadUtf8
        this.cursorUtf16 = textOffsetIndex.utf8ToUtf16(cursorUtf8)
        this.selectionAnchorUtf16 = textOffsetIndex.utf8ToUtf16(selectionAnchorUtf8)
        this.selectionHeadUtf16 = textOffsetIndex.utf8ToUtf16(selectionHeadUtf8)
    }

    fun loadText(
        text: String,
        cursorUtf8: Int,
    ) {
        loadFromSnapshot(text, cursorUtf8, 0)
    }

    fun setSelectionInternal(
        anchorUtf8: Int,
        headUtf8: Int,
    ) {
        selectionAnchorUtf8 = anchorUtf8
        selectionHeadUtf8 = headUtf8
        selectionAnchorUtf16 = textOffsetIndex.utf8ToUtf16(anchorUtf8)
        selectionHeadUtf16 = textOffsetIndex.utf8ToUtf16(headUtf8)
        cursorUtf8 = headUtf8
        cursorUtf16 = selectionHeadUtf16
    }

    /** 字符串的 UTF-8 字节长度 — 无分配的单遍扫描（热路径避免整章 toByteArray）。 */
    private fun utf8LengthOf(s: String): Int {
        var len = 0
        var i = 0
        while (i < s.length) {
            val codePoint = s.codePointAt(i)
            len += utf8ByteLengthCp(codePoint)
            i += Character.charCount(codePoint)
        }
        return len
    }

    /** 字符串按 UTF-8 字节区间切片，snap 到 codepoint 边界。 */
    private fun utf8SliceByBytes(text: String, startByte: Int, endByte: Int): String {
        if (startByte >= endByte) return ""
        var bytePos = 0
        var i = 0
        while (i < text.length && bytePos < startByte) {
            val cp = text.codePointAt(i)
            val cpLen = utf8ByteLengthCp(cp)
            if (bytePos + cpLen > startByte) break
            bytePos += cpLen
            i += Character.charCount(cp)
        }
        val startIdx = i
        while (i < text.length && bytePos < endByte) {
            val cp = text.codePointAt(i)
            val cpLen = utf8ByteLengthCp(cp)
            if (bytePos + cpLen > endByte) break
            bytePos += cpLen
            i += Character.charCount(cp)
        }
        return text.substring(startIdx, i)
    }

    private fun utf8ByteLengthCp(codePoint: Int): Int = when {
        codePoint <= 0x7F -> 1
        codePoint <= 0x7FF -> 2
        codePoint <= 0xFFFF -> 3
        else -> 4
    }
}
