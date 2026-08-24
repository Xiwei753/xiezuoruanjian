package com.xiwei.sujian.core.diagnostics

import com.xiwei.sujian.feature.editor.diagnostics.EditorEventRingBuffer

/**
 * 统一脱敏诊断事件来源（生产路径持续写入）。
 *
 * 每个事件只记录事件类型、长度、字节范围、标识、状态码与耗时；
 * 严禁记录正文、preedit 内容、token 和密钥（[EditorEventRingBuffer] 与
 * [DiagnosticsLogger] 双重脱敏兜底）。
 *
 * RingBuffer 由生产代码持续写入，导出时同时包含内存事件快照与滚动日志文件。
 */
object DiagnosticsEvents {
    private const val TAG = "SujianDiag"

    fun record(
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
        DiagnosticsLogger.i(TAG, "$eventType ${fields.joinToString(" ") { "${it.first}=${it.second}" }}")
    }

    // ── 应用生命周期 ──────────────────────────────────────────────

    fun appLifecycle(state: String) = record("app.lifecycle", "state" to state)

    fun activityLifecycle(state: String) = record("app.activity", "state" to state)

    // ── 构建身份 / 进程启动（#623 评论 3）─────────────────────────

    fun appBuild(
        versionName: String,
        versionCode: Int,
        gitCommitSha: String,
        flavor: String,
        buildType: String,
        applicationId: String,
    ) = record(
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
        "app.process_start",
        "versionCode" to versionCode,
        "gitCommitSha" to gitCommitSha,
        "flavor" to flavor,
        "buildType" to buildType,
        "processStartMs" to processStartMs,
    )

    // ── 一级导航 ─────────────────────────────────────────────────

    fun navigation(destination: String) = record("nav.destination", "destination" to destination)

    fun navBack(handled: Boolean) = record("nav.back", "handled" to handled)

    fun workspaceBack(target: String) = record("nav.workspace_back", "target" to target)

    /**
     * 工作区内部可预见返回手势：start / progress / cancel / complete。
     * 只记录阶段与目标窗格，不记录手势进度数值（避免拖动期间刷屏淹没 RingBuffer）。
     */
    fun predictiveBack(
        target: String,
        phase: String,
    ) = record("nav.predictive_back", "target" to target, "phase" to phase)

    // ── 作品/卷/章节 ─────────────────────────────────────────────

    fun workspaceSelection(
        kind: String,
        id: String,
    ) = record("workspace.select", "kind" to kind, "id" to id)

    fun workspaceClear(kind: String) = record("workspace.clear", "kind" to kind)

    fun chapterLoad(
        projectId: String,
        chapterId: String,
        byteLength: Int,
        result: String,
    ) = record(
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
        "chapter.save",
        "projectId" to projectId,
        "chapterId" to chapterId,
        "bytes" to byteLength,
        "result" to result,
        "elapsedMs" to elapsedMs,
    )

    // ── 焦点 / InputConnection ───────────────────────────────────

    fun editorFocus(focused: Boolean) = record("editor.focus", "focused" to focused)

    fun inputConnection(
        created: Boolean,
        sessionBound: Boolean,
    ) = record("editor.input_connection", "created" to created, "sessionBound" to sessionBound)

    // ── IME composition 生命周期 ─────────────────────────────────

    fun compositionBegin(
        byteStart: Int,
        byteEndExclusive: Int,
    ) = record("editor.composition.begin", "start" to byteStart, "end" to byteEndExclusive)

    fun compositionUpdate(
        preeditBytes: Int,
        cursorOffset: Int,
    ) = record("editor.composition.update", "preeditBytes" to preeditBytes, "cursorOffset" to cursorOffset)

    fun compositionCommit(
        byteStart: Int,
        byteEndExclusive: Int,
        committedBytes: Int,
    ) = record(
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
        "editor.composition.cancel",
        "start" to byteStart,
        "end" to byteEndExclusive,
        "preeditBytes" to preeditBytes,
    )

    // ── 编辑事务（范围 / revision / session / 结果）──────────────

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
    ) = record("editor.session", "session" to sessionId, "action" to action)

    // ── 动画事务 ─────────────────────────────────────────────────

    fun animationStart(
        transactionId: Long,
        kind: String,
        durationMs: Long,
    ) = record("editor.animation.start", "transaction" to transactionId, "kind" to kind, "durationMs" to durationMs)

    fun animationRebase(
        transactionId: Long,
        newTransactionId: Long,
    ) = record("editor.animation.rebase", "transaction" to transactionId, "newTransaction" to newTransactionId)

    fun animationCancel(transactionId: Long) = record("editor.animation.cancel", "transaction" to transactionId)

    fun animationComplete(
        transactionId: Long,
        elapsedMs: Long,
    ) = record("editor.animation.complete", "transaction" to transactionId, "elapsedMs" to elapsedMs)

    fun animationPolicy(policy: String) = record("editor.animation.policy", "policy" to policy)

    // ── 编辑器运动/排版策略（#637 评论 5386066978 项5）──────────

    /**
     * 编辑器运动策略 — 记录 text/cursor/协同/降级与各自时长，不记录正文或 glyph。
     * 只在 effective policy 变化时记录一次，不要每帧刷。
     * 注意：字段名 textEnabled/cursorEnabled 避免与 SENSITIVE_KEYS 中 "text" 冲突。
     */
    fun editorMotionPolicy(
        textEnabled: Boolean,
        textMs: Long,
        cursorEnabled: Boolean,
        cursorMs: Long,
        coordinated: Boolean,
        reduceMotion: Boolean,
    ) = record(
        "editor.motion_policy",
        "textEnabled" to textEnabled,
        "textMs" to textMs,
        "cursorEnabled" to cursorEnabled,
        "cursorMs" to cursorMs,
        "coordinated" to coordinated,
        "reduceMotion" to reduceMotion,
    )

    /**
     * 编辑器排版设置 — 记录字号/行距/首行缩进开关与字符宽度，不记录正文内容。
     * 只在值变化时记录一次。
     */
    fun editorTypography(
        fontSizeSp: Float,
        lineSpacing: Float,
        firstLineIndent: Boolean,
        indentChars: Float,
    ) = record(
        "editor.typography",
        "fontSizeSp" to fontSizeSp,
        "lineSpacing" to lineSpacing,
        "firstLineIndent" to firstLineIndent,
        "indentChars" to indentChars,
    )

    // ── 视口/重基接线（#638 结构化持久诊断）─────────────────────

    /**
     * 视口重定向 — 记录 transaction/起止 Y/最大 Y/原因，不记录正文或 glyph。
     * 供 viewport/rebase 接线调用方使用。
     */
    fun viewportRetarget(
        transactionId: Long,
        fromY: Float,
        toY: Float,
        maxY: Float,
        reason: String,
    ) = record(
        "editor.viewport_retarget",
        "transaction" to transactionId,
        "fromY" to fromY,
        "toY" to toY,
        "maxY" to maxY,
        "reason" to reason,
    )

    /**
     * 动画重基状态 — 记录新旧 transaction/删除 slice 数/光标剩余与 slice 范围。
     * 不记录正文、glyph 或 preedit 内容。
     */
    fun animationRebaseState(
        oldTransactionId: Long,
        newTransactionId: Long,
        deleteSlices: Int,
        cursorRemaining: Int,
        minSliceRemaining: Int,
        maxSliceRemaining: Int,
    ) = record(
        "editor.animation_rebase_state",
        "oldTransaction" to oldTransactionId,
        "newTransaction" to newTransactionId,
        "deleteSlices" to deleteSlices,
        "cursorRemaining" to cursorRemaining,
        "minSliceRemaining" to minSliceRemaining,
        "maxSliceRemaining" to maxSliceRemaining,
    )

    // ── 设置 ─────────────────────────────────────────────────────

    fun settingsSaved(
        field: String,
        result: String,
    ) = record("settings.save", "field" to field, "result" to result)

    /**
     * 设置页分组展开/收起（Issue #612 五）。只记录分类标识与展开状态，
     * 不记录分类下的具体设置值。
     */
    fun settingsSection(
        section: String,
        expanded: Boolean,
    ) = record("settings.section", "section" to section, "expanded" to expanded)

    // ── 主题解析（Issue #612 五）─────────────────────────────────

    /**
     * 主题解析结果。只记录模式、来源、是否暗色、选中的内置主题/调色板标识与 SDK，
     * 不记录颜色具体数值。
     */
    fun themeResolve(
        appearanceMode: String,
        colorSource: String,
        isDark: Boolean,
        selectedBuiltin: String?,
        selectedPalette: String?,
        sdk: Int,
    ) = record(
        "theme.resolve",
        "appearanceMode" to appearanceMode,
        "colorSource" to colorSource,
        "isDark" to isDark,
        "selectedBuiltin" to selectedBuiltin,
        "selectedPalette" to selectedPalette,
        "sdk" to sdk,
    )

    // ── 字段焦点 / 提交（Issue #612 五）──────────────────────────

    /**
     * 字段获得/失去焦点。只记录字段类型与焦点状态，不记录字段值。
     */
    fun fieldFocus(
        fieldType: String,
        focused: Boolean,
    ) = record("field.focus", "fieldType" to fieldType, "focused" to focused)

    /**
     * 字段提交结果。只记录字段类型、字符数与结果状态，不记录字段实际内容。
     */
    fun fieldCommit(
        fieldType: String,
        charCount: Int,
        result: String,
    ) = record("field.commit", "fieldType" to fieldType, "charCount" to charCount, "result" to result)

    // ── 一级导航切换（Issue #612 五）─────────────────────────────

    /**
     * 一级导航切换。记录 from/to 标识，不记录导航参数或作品标题。
     */
    fun navTopLevelSwitch(
        from: String,
        to: String,
    ) = record("nav.top_level_switch", "from" to from, "to" to to)

    // ── 同步 ─────────────────────────────────────────────────────

    fun syncEvent(
        action: String,
        status: String,
        detail: String? = null,
    ) = record("sync.event", "action" to action, "status" to status, "detail" to detail)
}
