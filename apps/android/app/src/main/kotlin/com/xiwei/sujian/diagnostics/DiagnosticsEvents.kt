package com.xiwei.sujian.diagnostics

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

    // ── 设置 ─────────────────────────────────────────────────────

    fun settingsSaved(
        field: String,
        result: String,
    ) = record("settings.save", "field" to field, "result" to result)

    // ── 同步 ─────────────────────────────────────────────────────

    fun syncEvent(
        action: String,
        status: String,
        detail: String? = null,
    ) = record("sync.event", "action" to action, "status" to status, "detail" to detail)
}
