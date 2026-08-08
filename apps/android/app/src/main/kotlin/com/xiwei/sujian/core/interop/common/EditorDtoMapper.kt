package com.xiwei.sujian.core.interop.common

import com.xiwei.sujian.feature.editor.model.AnimationMode
import com.xiwei.sujian.feature.editor.model.EditorOperationKind
import com.xiwei.sujian.feature.editor.model.EditorTransactionCause

/**
 * UniFFI 编辑器 DTO → app 层编辑器枚举映射器。
 *
 * 所有映射逻辑集中在此对象，editor 子模块只使用 [com.xiwei.sujian.model] 下的类型。
 * pipeline 层调用这些方法完成转换，editor 子模块不直接接触 UniFFI 绑定。
 */
object EditorDtoMapper {
    fun fromDto(dto: uniffi.writer_core.EditorTransactionCauseDto): EditorTransactionCause =
        when (dto) {
            uniffi.writer_core.EditorTransactionCauseDto.TYPING -> EditorTransactionCause.TYPING
            uniffi.writer_core.EditorTransactionCauseDto.DELETE -> EditorTransactionCause.DELETE
            uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION -> EditorTransactionCause.IME_COMPOSITION
            uniffi.writer_core.EditorTransactionCauseDto.TYPING_COMMIT -> EditorTransactionCause.TYPING_COMMIT
            uniffi.writer_core.EditorTransactionCauseDto.PASTE -> EditorTransactionCause.PASTE
            uniffi.writer_core.EditorTransactionCauseDto.UNDO -> EditorTransactionCause.UNDO
            uniffi.writer_core.EditorTransactionCauseDto.REDO -> EditorTransactionCause.REDO
            uniffi.writer_core.EditorTransactionCauseDto.LOAD -> EditorTransactionCause.LOAD
            uniffi.writer_core.EditorTransactionCauseDto.FORMAT -> EditorTransactionCause.FORMAT
            uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC -> EditorTransactionCause.PROGRAMMATIC
        }

    fun toDto(cause: EditorTransactionCause): uniffi.writer_core.EditorTransactionCauseDto =
        when (cause) {
            EditorTransactionCause.TYPING -> uniffi.writer_core.EditorTransactionCauseDto.TYPING
            EditorTransactionCause.DELETE -> uniffi.writer_core.EditorTransactionCauseDto.DELETE
            EditorTransactionCause.IME_COMPOSITION -> uniffi.writer_core.EditorTransactionCauseDto.IME_COMPOSITION
            EditorTransactionCause.TYPING_COMMIT -> uniffi.writer_core.EditorTransactionCauseDto.TYPING_COMMIT
            EditorTransactionCause.PASTE -> uniffi.writer_core.EditorTransactionCauseDto.PASTE
            EditorTransactionCause.UNDO -> uniffi.writer_core.EditorTransactionCauseDto.UNDO
            EditorTransactionCause.REDO -> uniffi.writer_core.EditorTransactionCauseDto.REDO
            EditorTransactionCause.LOAD -> uniffi.writer_core.EditorTransactionCauseDto.LOAD
            EditorTransactionCause.FORMAT -> uniffi.writer_core.EditorTransactionCauseDto.FORMAT
            EditorTransactionCause.PROGRAMMATIC -> uniffi.writer_core.EditorTransactionCauseDto.PROGRAMMATIC
        }

    fun fromDto(dto: uniffi.writer_core.EditorOperationKindDto): EditorOperationKind =
        when (dto) {
            uniffi.writer_core.EditorOperationKindDto.INSERT -> EditorOperationKind.INSERT
            uniffi.writer_core.EditorOperationKindDto.DELETE -> EditorOperationKind.DELETE
            uniffi.writer_core.EditorOperationKindDto.REPLACE -> EditorOperationKind.REPLACE
            uniffi.writer_core.EditorOperationKindDto.CURSOR_ONLY -> EditorOperationKind.CURSOR_ONLY
            uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE -> EditorOperationKind.COMPOSITION_UPDATE
            uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT -> EditorOperationKind.COMPOSITION_COMMIT
            uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL -> EditorOperationKind.COMPOSITION_CANCEL
            uniffi.writer_core.EditorOperationKindDto.LOAD -> EditorOperationKind.LOAD
            uniffi.writer_core.EditorOperationKindDto.FORMAT -> EditorOperationKind.FORMAT
        }

    fun toDto(kind: EditorOperationKind): uniffi.writer_core.EditorOperationKindDto =
        when (kind) {
            EditorOperationKind.INSERT -> uniffi.writer_core.EditorOperationKindDto.INSERT
            EditorOperationKind.DELETE -> uniffi.writer_core.EditorOperationKindDto.DELETE
            EditorOperationKind.REPLACE -> uniffi.writer_core.EditorOperationKindDto.REPLACE
            EditorOperationKind.CURSOR_ONLY -> uniffi.writer_core.EditorOperationKindDto.CURSOR_ONLY
            EditorOperationKind.COMPOSITION_UPDATE -> uniffi.writer_core.EditorOperationKindDto.COMPOSITION_UPDATE
            EditorOperationKind.COMPOSITION_COMMIT -> uniffi.writer_core.EditorOperationKindDto.COMPOSITION_COMMIT
            EditorOperationKind.COMPOSITION_CANCEL -> uniffi.writer_core.EditorOperationKindDto.COMPOSITION_CANCEL
            EditorOperationKind.LOAD -> uniffi.writer_core.EditorOperationKindDto.LOAD
            EditorOperationKind.FORMAT -> uniffi.writer_core.EditorOperationKindDto.FORMAT
        }

    fun fromDto(dto: uniffi.writer_core.AnimationModeDto): AnimationMode =
        when (dto) {
            uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION -> AnimationMode.GLYPH_ANIMATION
            uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION -> AnimationMode.CLUSTER_ANIMATION
            uniffi.writer_core.AnimationModeDto.RUN_ANIMATION -> AnimationMode.RUN_ANIMATION
            uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION -> AnimationMode.LINE_REFLOW_ANIMATION
            uniffi.writer_core.AnimationModeDto.SNAPSHOT_ANIMATION -> AnimationMode.SNAPSHOT_ANIMATION
            uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED -> AnimationMode.SYSTEM_SUPPRESSED
        }

    fun toDto(mode: AnimationMode): uniffi.writer_core.AnimationModeDto =
        when (mode) {
            AnimationMode.GLYPH_ANIMATION -> uniffi.writer_core.AnimationModeDto.GLYPH_ANIMATION
            AnimationMode.CLUSTER_ANIMATION -> uniffi.writer_core.AnimationModeDto.CLUSTER_ANIMATION
            AnimationMode.RUN_ANIMATION -> uniffi.writer_core.AnimationModeDto.RUN_ANIMATION
            AnimationMode.LINE_REFLOW_ANIMATION -> uniffi.writer_core.AnimationModeDto.LINE_REFLOW_ANIMATION
            AnimationMode.SNAPSHOT_ANIMATION -> uniffi.writer_core.AnimationModeDto.SNAPSHOT_ANIMATION
            AnimationMode.SYSTEM_SUPPRESSED -> uniffi.writer_core.AnimationModeDto.SYSTEM_SUPPRESSED
        }
}
