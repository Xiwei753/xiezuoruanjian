package com.xiwei.sujian.core.interop.diagnostics

import uniffi.writer_core.DiagnosticOriginDto

/**
 * 应用自身生命周期/内部状态诊断事件 — Issue #671 第 3 部分。
 *
 * 涵盖构建身份、进程启动、作品/章节加载保存、编辑器会话生命周期、
 * 动画事务、编辑器运动/排版策略、视口/重基接线、主题解析，
 * 以及一级导航、工作区选择、主题外观选择等应用级用户操作事件。
 *
 * 所有事件通过 [DiagnosticsEventsInterop.record] 转发到 Rust 统一诊断后端。
 */
object AppDiagnosticsEvents {
    // ── 重复字段 key（同一文件内出现 2 次以上）──────────────────────

    private const val KEY_PROJECT_ID = "projectId"
    private const val KEY_CHAPTER_ID = "chapterId"
    private const val KEY_BYTES = "bytes"
    private const val KEY_RESULT = "result"
    private const val KEY_ELAPSED_MS = "elapsedMs"
    private const val KEY_TRANSACTION = "transaction"
    private const val KEY_NEW_TRANSACTION = "newTransaction"
    private const val KEY_TARGET = "target"

    // ── 构建身份 / 进程启动（App：应用自身）──────────────────────

    fun appBuild(
        versionName: String,
        versionCode: Int,
        gitCommitSha: String,
        flavor: String,
        buildType: String,
        applicationId: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "app.build",
        "versionName" to versionName,
        "versionCode" to versionCode,
        "gitCommitSha" to gitCommitSha,
        "flavor" to flavor,
        "buildType" to buildType,
        "applicationId" to applicationId,
    )

    fun appProcessStart(
        versionCode: Int,
        gitCommitSha: String,
        flavor: String,
        buildType: String,
        processStartMs: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "app.process_start",
        "versionCode" to versionCode,
        "gitCommitSha" to gitCommitSha,
        "flavor" to flavor,
        "buildType" to buildType,
        "processStartMs" to processStartMs,
    )

    // ── 作品/卷/章节（App：应用加载保存）──────────────────────────

    fun chapterLoad(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "chapter.load",
        KEY_PROJECT_ID to projectId,
        KEY_CHAPTER_ID to chapterId,
        KEY_BYTES to byteLength,
        KEY_RESULT to result,
    )

    fun chapterSave(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
        elapsedMs: Long,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "chapter.save",
        KEY_PROJECT_ID to projectId,
        KEY_CHAPTER_ID to chapterId,
        KEY_BYTES to byteLength,
        KEY_RESULT to result,
        KEY_ELAPSED_MS to elapsedMs,
    )

    // ── 编辑器会话生命周期（App：应用内部）────────────────────────

    fun sessionLifecycle(
        sessionId: String,
        action: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "editor.session",
        "session" to sessionId,
        "action" to action,
    )

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

    // ── 主题解析（App：应用内部）──────────────────────────────────

    fun themeResolve(
        appearanceMode: String,
        colorSource: String,
        isDark: Boolean,
        selectedBuiltin: String?,
        selectedPalette: String?,
        sdk: Int,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.APP,
        "theme.resolve",
        "appearanceMode" to appearanceMode,
        "colorSource" to colorSource,
        "isDark" to isDark,
        "selectedBuiltin" to selectedBuiltin,
        "selectedPalette" to selectedPalette,
        "sdk" to sdk,
    )

    // ── 一级导航（User：用户点击导航）────────────────────────────

    fun navigation(destination: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "nav.destination", "destination" to destination)

    fun navBack(handled: Boolean) =
        DiagnosticsEventsInterop.record(
            DiagnosticOriginDto.USER,
            "nav.back",
            "handled" to handled,
        )

    fun workspaceBack(target: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "nav.workspace_back", KEY_TARGET to target)

    fun predictiveBack(
        target: String,
        phase: String,
    ) = DiagnosticsEventsInterop.record(
        DiagnosticOriginDto.USER,
        "nav.predictive_back",
        KEY_TARGET to target,
        "phase" to phase,
    )

    fun navTopLevelSwitch(
        from: String,
        to: String,
    ) = DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "nav.top_level_switch", "from" to from, "to" to to)

    // ── 作品/卷/章节选择（User：用户选择工作区）──────────────────

    fun workspaceSelection(
        kind: String,
        id: String,
    ) = DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "workspace.select", "kind" to kind, "id" to id)

    fun workspaceClear(kind: String) =
        DiagnosticsEventsInterop.record(
            DiagnosticOriginDto.USER,
            "workspace.clear",
            "kind" to kind,
        )

    // ── 主题外观选择（User：用户选择外观模式）──────────────────────

    /** 用户选择外观模式（点击 dark/light/system）。origin=User。 */
    fun themeAppearanceSelect(requested: String) =
        DiagnosticsEventsInterop.record(DiagnosticOriginDto.USER, "theme.appearance_select", "requested" to requested)
}
