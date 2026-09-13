package com.xiwei.sujian.core.interop.diagnostics

import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer
import uniffi.writer_core.DiagnosticOriginDto

/**
 * 统一脱敏诊断事件来源 — Issue #670 评论 5651060802。
 *
 * 替代旧的 `core.diagnostics.DiagnosticsEvents`，底层通过
 * [DiagnosticsInterop.recordEvent] 转发到 Rust `recordDiagnosticEvent`，
 * 不再自己定义日志后端或调用 `DiagnosticsLogger.i`。
 *
 * 每个事件只记录事件类型、长度、字节范围、标识、状态码与耗时；
 * 严禁记录正文、preedit 内容、token 和密钥（RingBuffer 与 Rust redact 双重脱敏兜底）。
 *
 * RingBuffer 由生产代码持续写入，导出时同时包含内存事件快照与滚动日志文件。
 *
 * ## 事件来源（origin）— Issue #670 评论 5651060802 第 7 节
 *
 * `origin` 由真正知道触发源的调用点传入，后端不猜：
 * - **User**：用户主动操作（点击、选择、输入、手动触发同步）
 * - **System**：系统回调/环境变化（生命周期、网络变化、系统主题变化、崩溃）
 * - **App**：应用自身生命周期/内部状态（设置保存、主题解析、自动同步）
 */
object DiagnosticsEventsInterop {
    private const val TAG = "SujianDiag"

    fun record(
        origin: DiagnosticOriginDto,
        eventType: String,
        vararg fields: Pair<String, Any?>,
    ) {
        val event =
            linkedMapOf<String, Any?>("event" to eventType).apply {
                for ((k, v) in fields) {
                    if (v != null) put(k, v)
                }
                put("ts", System.currentTimeMillis())
            }
        EditorEventRingBuffer.record(event)
        // 转发到 Rust 统一诊断后端（脱敏 + JSONL 持久化）。
        val dtos =
            fields.mapNotNull { (k, v) ->
                if (v == null) null else uniffi.writer_core.DiagnosticFieldDto(k, v.toString())
            }
        DiagnosticsInterop.recordEvent(
            origin,
            eventType,
            TAG,
            null,
            dtos,
        )
    }

    // ── 应用生命周期（System：系统回调）──────────────────────────

    fun appLifecycle(state: String) = record(DiagnosticOriginDto.SYSTEM, "app.lifecycle", "state" to state)

    fun activityLifecycle(state: String) = record(DiagnosticOriginDto.SYSTEM, "app.activity", "state" to state)

    // ── 构建身份 / 进程启动（App：应用自身）──────────────────────

    fun appBuild(
        versionName: String,
        versionCode: Int,
        gitCommitSha: String,
        flavor: String,
        buildType: String,
        applicationId: String,
    ) = record(
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
    ) = record(
        DiagnosticOriginDto.APP,
        "app.process_start",
        "versionCode" to versionCode,
        "gitCommitSha" to gitCommitSha,
        "flavor" to flavor,
        "buildType" to buildType,
        "processStartMs" to processStartMs,
    )

    // ── 一级导航（User：用户点击导航）────────────────────────────

    fun navigation(destination: String) = record(DiagnosticOriginDto.USER, "nav.destination", "destination" to destination)

    fun navBack(handled: Boolean) = record(DiagnosticOriginDto.USER, "nav.back", "handled" to handled)

    fun workspaceBack(target: String) = record(DiagnosticOriginDto.USER, "nav.workspace_back", "target" to target)

    fun predictiveBack(
        target: String,
        phase: String,
    ) = record(DiagnosticOriginDto.USER, "nav.predictive_back", "target" to target, "phase" to phase)

    // ── 作品/卷/章节（User：用户选择工作区）──────────────────────

    fun workspaceSelection(
        kind: String,
        id: String,
    ) = record(DiagnosticOriginDto.USER, "workspace.select", "kind" to kind, "id" to id)

    fun workspaceClear(kind: String) = record(DiagnosticOriginDto.USER, "workspace.clear", "kind" to kind)

    fun chapterLoad(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
    ) = record(
        DiagnosticOriginDto.APP,
        "chapter.load",
        "projectId" to projectId,
        "chapterId" to chapterId,
        "bytes" to byteLength,
        "result" to result,
    )

    fun chapterSave(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
        elapsedMs: Long,
    ) = record(
        DiagnosticOriginDto.APP,
        "chapter.save",
        "projectId" to projectId,
        "chapterId" to chapterId,
        "bytes" to byteLength,
        "result" to result,
        "elapsedMs" to elapsedMs,
    )

    // ── 焦点 / InputConnection（User：用户输入）──────────────────

    fun editorFocus(focused: Boolean) = record(DiagnosticOriginDto.USER, "editor.focus", "focused" to focused)

    fun inputConnection(
        created: Boolean,
        sessionBound: Boolean,
    ) = record(DiagnosticOriginDto.USER, "editor.input_connection", "created" to created, "sessionBound" to sessionBound)

    // ── IME composition 生命周期（User：用户输入）────────────────

    fun compositionBegin(
        byteStart: Int,
        byteEndExclusive: Int,
    ) = record(DiagnosticOriginDto.USER, "editor.composition.begin", "start" to byteStart, "end" to byteEndExclusive)

    fun compositionUpdate(
        preeditBytes: Int,
        cursorOffset: Int,
    ) = record(DiagnosticOriginDto.USER, "editor.composition.update", "preeditBytes" to preeditBytes, "cursorOffset" to cursorOffset)

    fun compositionCommit(
        byteStart: Int,
        byteEndExclusive: Int,
        committedBytes: Int,
    ) = record(
        DiagnosticOriginDto.USER,
        "editor.composition.commit",
        "start" to byteStart,
        "end" to byteEndExclusive,
        "committedBytes" to committedBytes,
    )

    fun compositionCancel(
        byteStart: Int,
        byteEndExclusive: Int,
        preeditBytes: Int,
    ) = record(
        DiagnosticOriginDto.USER,
        "editor.composition.cancel",
        "start" to byteStart,
        "end" to byteEndExclusive,
        "preeditBytes" to preeditBytes,
    )

    // ── 编辑事务（User：用户输入）────────────────────────────────

    fun editTransaction(
        operationKind: String,
        oldStart: Int,
        oldEndExclusive: Int,
        newStart: Int,
        newEndExclusive: Int,
        revision: Long,
        sessionId: String,
        result: String,
    ) = record(
        DiagnosticOriginDto.USER,
        "editor.transaction",
        "kind" to operationKind,
        "oldStart" to oldStart,
        "oldEnd" to oldEndExclusive,
        "newStart" to newStart,
        "newEnd" to newEndExclusive,
        "revision" to revision,
        "session" to sessionId,
        "result" to result,
    )

    fun sessionLifecycle(
        sessionId: String,
        action: String,
    ) = record(DiagnosticOriginDto.APP, "editor.session", "session" to sessionId, "action" to action)

    // ── 动画事务（App：应用内部）────────────────────────────────

    fun animationStart(
        transactionId: Long,
        kind: String,
        durationMs: Long,
    ) = record(DiagnosticOriginDto.APP, "editor.animation.start", "transaction" to transactionId, "kind" to kind, "durationMs" to durationMs)

    fun animationRebase(
        transactionId: Long,
        newTransactionId: Long,
    ) = record(DiagnosticOriginDto.APP, "editor.animation.rebase", "transaction" to transactionId, "newTransaction" to newTransactionId)

    fun animationCancel(transactionId: Long) = record(DiagnosticOriginDto.APP, "editor.animation.cancel", "transaction" to transactionId)

    fun animationComplete(
        transactionId: Long,
        elapsedMs: Long,
    ) = record(DiagnosticOriginDto.APP, "editor.animation.complete", "transaction" to transactionId, "elapsedMs" to elapsedMs)

    fun animationPolicy(policy: String) = record(DiagnosticOriginDto.APP, "editor.animation.policy", "policy" to policy)

    // ── 编辑器运动/排版策略（App：应用内部）──────────────────────

    fun editorMotionPolicy(
        textEnabled: Boolean,
        textMs: Long,
        cursorEnabled: Boolean,
        cursorMs: Long,
        coordinated: Boolean,
        reduceMotion: Boolean,
    ) = record(
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
    ) = record(
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
    ) = record(
        DiagnosticOriginDto.APP,
        "editor.viewport_retarget",
        "transaction" to transactionId,
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
    ) = record(
        DiagnosticOriginDto.APP,
        "editor.animation_rebase_state",
        "oldTransaction" to oldTransactionId,
        "newTransaction" to newTransactionId,
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
    ) = record(
        DiagnosticOriginDto.APP,
        "editor.reflow_plan",
        "transaction" to transactionId,
        "oldAffectedLines" to oldAffectedLines,
        "newAffectedLines" to newAffectedLines,
        "sameLineMoves" to sameLineMoves,
        "crossLineCrossfadePairs" to crossLineCrossfadePairs,
        "suffixBlockShift" to suffixBlockShift,
    )

    // ── 设置（App：设置保存；User：用户展开/折叠分区）────────────

    fun settingsSaved(
        field: String,
        result: String,
    ) = record(DiagnosticOriginDto.APP, "settings.save", "field" to field, "result" to result)

    fun settingsSection(
        section: String,
        expanded: Boolean,
    ) = record(DiagnosticOriginDto.USER, "settings.section", "section" to section, "expanded" to expanded)

    // ── 主题链（Issue #670 评论 5651060802 第 7 节）──────────────
    //
    // 用户选择外观模式 → origin=User, event=theme.appearance_select
    // 设置保存           → origin=App,  event=settings.save（见 settingsSaved）
    // 主题解析           → origin=App,  event=theme.resolve
    // 系统主题变化       → origin=System, event=theme.system_color_scheme

    /** 用户选择外观模式（点击 dark/light/system）。origin=User。 */
    fun themeAppearanceSelect(requested: String) =
        record(DiagnosticOriginDto.USER, "theme.appearance_select", "requested" to requested)

    /** 系统主题变化回调。origin=System。 */
    fun themeSystemColorScheme(isDark: Boolean) =
        record(DiagnosticOriginDto.SYSTEM, "theme.system_color_scheme", "isDark" to isDark)

    fun themeResolve(
        appearanceMode: String,
        colorSource: String,
        isDark: Boolean,
        selectedBuiltin: String?,
        selectedPalette: String?,
        sdk: Int,
    ) = record(
        DiagnosticOriginDto.APP,
        "theme.resolve",
        "appearanceMode" to appearanceMode,
        "colorSource" to colorSource,
        "isDark" to isDark,
        "selectedBuiltin" to selectedBuiltin,
        "selectedPalette" to selectedPalette,
        "sdk" to sdk,
    )

    // ── 字段焦点 / 提交（User：用户输入）─────────────────────────

    fun fieldFocus(
        fieldType: String,
        focused: Boolean,
    ) = record(DiagnosticOriginDto.USER, "field.focus", "fieldType" to fieldType, "focused" to focused)

    fun fieldCommit(
        fieldType: String,
        charCount: Int,
        result: String,
    ) = record(DiagnosticOriginDto.USER, "field.commit", "fieldType" to fieldType, "charCount" to charCount, "result" to result)

    // ── 一级导航切换（User：用户点击一级导航）────────────────────

    fun navTopLevelSwitch(
        from: String,
        to: String,
    ) = record(DiagnosticOriginDto.USER, "nav.top_level_switch", "from" to from, "to" to to)

    // ── 同步 ─────────────────────────────────────────────────────
    //
    // origin 由调用点传入：
    // - 用户触发同步（Manual / SettingsPage / Import）→ User
    // - 定时自动同步（Auto / ForegroundService / scheduler）→ App
    fun syncEvent(
        origin: DiagnosticOriginDto,
        action: String,
        status: String,
        detail: String? = null,
    ) = record(origin, "sync.event", "action" to action, "status" to status, "detail" to detail)
}
