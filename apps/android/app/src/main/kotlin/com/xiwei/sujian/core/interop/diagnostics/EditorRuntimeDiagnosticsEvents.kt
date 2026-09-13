package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 编辑器运行时诊断事件 — Issue #671 评论 5653096875。
 *
 * 从 [AppDiagnosticsEvents] 拆出，涵盖动画事务、编辑器运动/排版策略、
 * 视口/重基接线等编辑器运行时事件，单独成一类以控制函数数。
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object EditorRuntimeDiagnosticsEvents {
    // ── 重复字段 key（同一文件内出现 2 次以上）──────────────────────

    private const val KEY_TRANSACTION = "transaction"
    private const val KEY_NEW_TRANSACTION = "newTransaction"
    private const val KEY_ELAPSED_MS = "elapsedMs"

    // ── 动画事务（App：应用内部）────────────────────────────────

    fun animationStart(
        transactionId: Long,
        kind: String,
        durationMs: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.animation.start",
        KEY_TRANSACTION to transactionId,
        "kind" to kind,
        "durationMs" to durationMs,
    )

    fun animationRebase(
        transactionId: Long,
        newTransactionId: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.animation.rebase",
        KEY_TRANSACTION to transactionId,
        KEY_NEW_TRANSACTION to newTransactionId,
    )

    fun animationCancel(transactionId: Long) =
        DiagnosticsEventsInterop.record(
            DiagnosticOriginDto.APP,
            "editor.animation.cancel",
            KEY_TRANSACTION to transactionId,
        )

    fun animationComplete(
        transactionId: Long,
        elapsedMs: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.animation.complete",
        KEY_TRANSACTION to transactionId,
        KEY_ELAPSED_MS to elapsedMs,
    )

    fun animationPolicy(policy: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.APP, "editor.animation.policy", "policy" to policy)

    // ── 编辑器运动/排版策略（App：应用内部）──────────────────────

    fun editorMotionPolicy(
        textEnabled: Boolean,
        textMs: Long,
        cursorEnabled: Boolean,
        cursorMs: Long,
        coordinated: Boolean,
        reduceMotion: Boolean,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.motion_policy",
        "textEnabled" to textEnabled,
        "textMs" to textMs,
        "cursorEnabled" to cursorEnabled,
        "cursorMs" to cursorMs,
        "coordinated" to coordinated,
        "reduceMotion" to reduceMotion,
    )

    fun editorTypography(
        fontSizeSp: Float,
        lineSpacing: Float,
        firstLineIndent: Boolean,
        indentChars: Float,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.typography",
        "fontSizeSp" to fontSizeSp,
        "lineSpacing" to lineSpacing,
        "firstLineIndent" to firstLineIndent,
        "indentChars" to indentChars,
    )

    // ── 视口/重基接线（App：应用内部）────────────────────────────

    fun viewportRetarget(
        transactionId: Long?,
        fromY: Float,
        toY: Float,
        maxY: Float,
        reason: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.viewport_retarget",
        KEY_TRANSACTION to transactionId,
        "fromY" to fromY,
        "toY" to toY,
        "maxY" to maxY,
        "reason" to reason,
    )

    fun animationRebaseState(
        oldTransactionId: Long,
        newTransactionId: Long,
        deleteSlices: Int,
        cursorRemaining: Float,
        minSliceRemaining: Float,
        maxSliceRemaining: Float,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.animation_rebase_state",
        "oldTransaction" to oldTransactionId,
        KEY_NEW_TRANSACTION to newTransactionId,
        "deleteSlices" to deleteSlices,
        "cursorRemaining" to cursorRemaining,
        "minSliceRemaining" to minSliceRemaining,
        "maxSliceRemaining" to maxSliceRemaining,
    )

    fun editorReflowPlan(
        transactionId: Long,
        oldAffectedLines: Int,
        newAffectedLines: Int,
        sameLineMoves: Int,
        crossLineCrossfadePairs: Int,
        suffixBlockShift: Boolean,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.reflow_plan",
        KEY_TRANSACTION to transactionId,
        "oldAffectedLines" to oldAffectedLines,
        "newAffectedLines" to newAffectedLines,
        "sameLineMoves" to sameLineMoves,
        "crossLineCrossfadePairs" to crossLineCrossfadePairs,
        "suffixBlockShift" to suffixBlockShift,
    )
}
