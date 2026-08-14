package com.xiwei.sujian.feature.editor.ui

// ! # 编辑器内容编辑操作（从 EditorViewModel 拆分）
// !
// ! #624 评论9：热路径不传整章 String — onEditorApplied 接轻量 EditorAppliedEvent，
// ! 保存调度/统计/字数全部增量处理。完整正文只在冷路径（save/snapshot）经 lease.text 取。
// !
// ! #624 评论10 第5项：onEditorApplied 状态机门控 — 只有 contentChanged=true 才进
// ! 持久化状态机（置 Unsaved/dirty/scheduleAutoSave/wordCount/统计）；
// ! 纯 selection/cursor-only（contentChanged=false）不进持久化状态机。

import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.statsCountsFor
import com.xiwei.sujian.feature.editor.session.writingEventSourceFrom
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * #624 评论9/10：轻量编辑应用入口 — 替代旧 onContentChanged(newContent: String)。
 *
 * #624 评论10 第5项：状态机门控 —
 * - **contentChanged=true**：置 Unsaved、contentDirty=true、scheduleAutoSave()、
 *   增量 wordCount、记写作统计、scheduleStatsRefresh；
 * - **contentChanged=false**（纯 selection/cursor-only）：不进持久化状态机 —
 *   不置 Unsaved、不置 dirty、不 scheduleAutoSave、不改 wordCount、不记统计。
 *   会话层 selection/revision 由 EditorWindowHost 的 onLocalEdit/onExternalEdit
 *   回调经 sessionCoordinator.applyLocalEdit 独立更新，不经过此方法。
 */
fun EditorViewModel.onEditorApplied(event: EditorAppliedEvent) {
    val currentState = _uiState.value
    if (currentState.loading) return
    if (isLoadingChapter) return
    if (inputFrozen) return

    if (!event.contentChanged) {
        // #624 评论10 第5项：纯 selection/cursor-only 不进持久化状态机 —
        // 不置 Unsaved、不置 dirty、不 scheduleAutoSave、不改 wordCount、不记统计。
        // 会话层 selection 已由 sessionCoordinator.applyLocalEdit 独立更新。
        return
    }

    // #624 评论9：不再每键存 content — 只更新 saveStatus。
    _uiState.value = currentState.copy(saveStatus = SaveStatus.Unsaved)
    contentDirty = true
    // #624 评论11 第2项：Save/Clear 决策统一在保存路径按 contentDirty + 有效
    // lease.text 判定（EditorSaveOps）— 不在此维护布尔侧信道。
    scheduleAutoSave()

    // #624 评论9：即时增量维护 wordCount — 不再每键全文 calculateWordCount。
    _uiState.value =
        _uiState.value.copy(
            wordCount = (_uiState.value.wordCount + event.contentDelta.netNonWhitespace).coerceAtLeast(0),
        )
    recordWritingEventIncremental(event)
    scheduleStatsRefresh()
}

/**
 * #624 评论9：增量写作统计上报 — 不传前后整章 String，只用 contentDelta 增量。
 * #624 评论10 第5项：source 字符串从 event.cause 明确映射（typing/pasted/deleted/
 * undo/redo/programmatic），不再靠 source/operationKind 猜。
 * #624 评论11 第4项：各分类计数由 [statsCountsFor]（session 层 mapper）决定，
 * 不在调用参数里临时拼；Paste 不再把净增字符算两遍。
 * #624 评论11 第3项：只 enqueue 到进程级 stats writer actor，不在输入主线程
 * 同步跨 UniFFI 写盘。
 */
fun EditorViewModel.recordWritingEventIncremental(event: EditorAppliedEvent) {
    val session = currentSession ?: return

    val nowMs = System.currentTimeMillis()
    if (statsLastEventMs == 0L || (nowMs - statsLastEventMs) > 5 * 60 * 1000) {
        statsSessionId = java.util.UUID.randomUUID().toString()
    }
    val durationSeconds =
        if (statsLastEventMs > 0L) {
            ((nowMs - statsLastEventMs) / 1000).toInt()
        } else {
            0
        }
    statsLastEventMs = nowMs

    // #624 评论10 第5项：source 按 Core cause 明确分类。
    val source = writingEventSourceFrom(event.cause)
    // #624 评论11 第4项：各分类计数收成 mapper — 不在调用参数里临时拼。
    val counts = statsCountsFor(event.cause, event.contentDelta)
    val aiInsertedChars = 0

    statsRepository.recordWritingEvent(
        statsDeviceId,
        session.projectId,
        session.volumeId,
        session.chapterId,
        source,
        counts.insertedChars,
        counts.deletedChars,
        counts.pastedChars,
        aiInsertedChars,
        durationSeconds,
        statsSessionId,
    )
}

/**
 * #624 评论9：延迟刷新 speed（可取消 Job）— wordCount 已即时增量维护，
 * delay(500) 后只重算 speed（不重算 wordCount，不取整章 String）。
 */
fun EditorViewModel.scheduleStatsRefresh() {
    statsRefreshJob?.cancel()
    statsRefreshJob =
        editorScope.launch {
            delay(500)
            updateStats()
        }
}

/** #624 评论9：updateStats 不再重算 wordCount — 用 _uiState.wordCount 即时增量值。 */
fun EditorViewModel.updateStats() {
    val currentWordCount = _uiState.value.wordCount
    val sessionAdded = currentWordCount - initialWordCount
    val elapsedMinutes = (System.currentTimeMillis() - sessionStartTime) / 60000.0
    val speed =
        if (elapsedMinutes > 0 && sessionAdded > 0) {
            (sessionAdded / elapsedMinutes).toInt()
        } else {
            0
        }
    _uiState.value =
        _uiState.value.copy(
            wordCount = currentWordCount,
            sessionAdded = sessionAdded,
            speed = speed,
        )
}

/** 冷路径字数计算（load/external-apply 时用整章 String）。 */
fun EditorViewModel.calculateWordCount(text: String): Int {
    return chapterRepository.calculateWordCount(text)
}
