package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 编辑器用户输入诊断事件 — Issue #671 第 3 部分。
 *
 * 涵盖焦点/InputConnection、IME composition 生命周期、编辑事务、
 * 字段焦点/提交等用户输入事件。
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object EditorDiagnosticsEvents {
    // ── 重复字段 key（同一文件内出现 2 次以上）──────────────────────

    private const val KEY_START = "start"
    private const val KEY_END = "end"
    private const val KEY_PREEDIT_BYTES = "preeditBytes"
    private const val KEY_RESULT = "result"
    private const val KEY_FIELD_TYPE = "fieldType"
    private const val KEY_FOCUSED = "focused"

    // ── 焦点 / InputConnection（User：用户输入）──────────────────

    fun editorFocus(focused: Boolean) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "editor.focus", KEY_FOCUSED to focused)

    fun inputConnection(
        created: Boolean,
        sessionBound: Boolean,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "editor.input_connection",
        "created" to created,
        "sessionBound" to sessionBound,
    )

    // ── IME composition 生命周期（User：用户输入）────────────────

    fun compositionBegin(
        byteStart: Int,
        byteEndExclusive: Int,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "editor.composition.begin",
        KEY_START to byteStart,
        KEY_END to byteEndExclusive,
    )

    fun compositionUpdate(
        preeditBytes: Int,
        cursorOffset: Int,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "editor.composition.update",
        KEY_PREEDIT_BYTES to preeditBytes,
        "cursorOffset" to cursorOffset,
    )

    fun compositionCommit(
        byteStart: Int,
        byteEndExclusive: Int,
        committedBytes: Int,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "editor.composition.commit",
        KEY_START to byteStart,
        KEY_END to byteEndExclusive,
        "committedBytes" to committedBytes,
    )

    fun compositionCancel(
        byteStart: Int,
        byteEndExclusive: Int,
        preeditBytes: Int,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "editor.composition.cancel",
        KEY_START to byteStart,
        KEY_END to byteEndExclusive,
        KEY_PREEDIT_BYTES to preeditBytes,
    )

    // ── 编辑事务（User：用户输入）────────────────────────────────

    fun editTransaction(diag: EditTransactionDiagnostic) =
        DiagnosticsEventsInterop.record(
            DiagnosticOriginDto.USER,
            "editor.transaction",
            "kind" to diag.operationKind,
            "oldStart" to diag.oldRange.start,
            "oldEnd" to diag.oldRange.endExclusive,
            "newStart" to diag.newRange.start,
            "newEnd" to diag.newRange.endExclusive,
            "revision" to diag.revision,
            "session" to diag.sessionId,
            KEY_RESULT to diag.result,
        )

    // ── 字段焦点 / 提交（User：用户输入）─────────────────────────

    fun fieldFocus(
        fieldType: String,
        focused: Boolean,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "field.focus",
        KEY_FIELD_TYPE to fieldType,
        KEY_FOCUSED to focused,
    )

    fun fieldCommit(
        fieldType: String,
        charCount: Int,
        result: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "field.commit",
        KEY_FIELD_TYPE to fieldType,
        "charCount" to charCount,
        KEY_RESULT to result,
    )
}

/**
 * 诊断字节范围 — 编辑事务中 old/new 正文范围。
 *
 * @property start 起始字节偏移（含）。
 * @property endExclusive 结束字节偏移（不含）。
 */
internal data class DiagnosticByteRange(
    val start: Int,
    val endExclusive: Int,
)

/**
 * 编辑事务诊断数据 — Issue #671 第 3 部分。
 *
 * 替代 [EditorDiagnosticsEvents.editTransaction] 的 8 个独立参数，
 * 用 [oldRange]/[newRange] 表达编辑前后字节范围，最终写入 Rust 的字段名
 * 仍保持 `oldStart/oldEnd/newStart/newEnd/revision/session/result`。
 *
 * @property operationKind 操作类型。
 * @property oldRange 编辑前字节范围。
 * @property newRange 编辑后字节范围。
 * @property revision 正文修订号。
 * @property sessionId 编辑会话 id。
 * @property result 事务结果。
 */
internal data class EditTransactionDiagnostic(
    val operationKind: String,
    val oldRange: DiagnosticByteRange,
    val newRange: DiagnosticByteRange,
    val revision: Long,
    val sessionId: String,
    val result: String,
)
