package com.xiwei.sujian.feature.editor.interop

/**
 * Bridge contract between the platform pipeline and the Rust EditorKernel.
 *
 * Per #541: this interface will become session-scoped (TextEditSessionBridge) so that
 * each bound EditableTextTarget carries its own session ID. All commands must implicitly
 * or explicitly carry the current session ID; the bridge must not use a global singleton.
 *
 * Byte offset convention: all offsets are UTF-8 byte offsets using half-open intervals
 * [start, endExclusive). The bridge is responsible for converting to the Rust FFI
 * unsigned integer types (UInt/ULong) at the boundary.
 */
interface EditorKernelBridge {
    fun insert(
        byteOffset: Int,
        text: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun delete(
        byteStart: Int,
        byteEndExclusive: Int,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun replace(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        originalText: String,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun setSelection(
        anchorByteOffset: Int,
        headByteOffset: Int,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun undo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?

    fun redo(expectedRevision: Long): uniffi.writer_core.EditorEditResultDto?

    fun loadText(
        text: String,
        cursorUtf8: Int,
    ): uniffi.writer_core.EditorEditResultDto?

    fun commitText(
        byteStart: Int,
        byteEndExclusive: Int,
        replacementText: String,
        resultingSelectionAnchor: Int,
        resultingSelectionHead: Int,
        compositionSessionId: Long,
        compositionBaseRevision: Long,
        compositionGeneration: Long,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun deleteSurrounding(
        beforeByteStart: Int,
        beforeByteEndExclusive: Int,
        afterByteStart: Int,
        afterByteEndExclusive: Int,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun beginComposition(
        replaceStart: Int,
        replaceEndExclusive: Int,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun updateComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        newPreeditText: String,
        newPreeditCursorOffset: Int,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun finishComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun cancelComposition(
        compositionSessionId: Long,
        compositionGeneration: Long,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun setAnimationEnabled(enabled: Boolean)

    fun setAnimationDurationMs(durationMs: Long)

    fun replaceAll(
        search: String,
        replacement: String,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun insertLineBreak(
        byteOffset: Int,
        autoIndentEnabled: Boolean,
        cause: uniffi.writer_core.EditorTransactionCauseDto,
        expectedRevision: Long,
    ): uniffi.writer_core.EditorEditResultDto?

    fun sessionSnapshot(): uniffi.writer_core.EditorSessionSnapshotDto?

    /**
     * #606: Returns the UTF-8 byte offset of the grapheme cluster boundary strictly before [byteOffset].
     *
     * Calls Core's Unicode cluster logic (unicode_segmentation) — single source of truth
     * for grapheme boundary semantics. Used by Backspace/Delete key handlers.
     */
    fun previousGraphemeBoundary(byteOffset: Int): Int

    /**
     * #606: Returns the UTF-8 byte offset of the grapheme cluster boundary strictly after [byteOffset].
     *
     * Calls Core's Unicode cluster logic (unicode_segmentation) — single source of truth
     * for grapheme boundary semantics. Used by Forward-Delete key handlers.
     */
    fun nextGraphemeBoundary(byteOffset: Int): Int
}
