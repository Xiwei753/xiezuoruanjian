package com.xiwei.sujian.feature.editor.presentation

// ! # 编辑器内容编辑操作（从 EditorViewModel 拆分）
// !
// ! #624 评论9：热路径不传整章 String — onEditorApplied 接轻量 EditorAppliedEvent，
// ! 保存调度/统计/字数全部增量处理。完整正文只在冷路径（save/snapshot）经 lease.text 取。
// !
// ! #624 评论10 第5项：onEditorApplied 状态机门控 — 只有 contentChanged=true 才进
// ! 持久化状态机（置 Unsaved/dirty/scheduleAutoSave/wordCount/统计）；
// ! 纯 selection/cursor-only（contentChanged=false）不进持久化状态机。

import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * #624 评论9/10：轻量编辑应用入口 — 替代旧 onContentChanged(newContent: String)。
 *
 * #624 评论10 第5项：状态机门控 —
 * - **contentChanged=true**：置 Unsaved、scheduleAutoSave()、增量 wordCount、记写作统计、
 *   scheduleStatsRefresh（dirty 由会话层 applyLocalEdit 写入 session store，见评论12 第2项）；
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
    // #624 评论12 第2项：dirty 唯一真值在 session store（applyLocalEdit 经
    // EditorSessionEditOps 写入 localDirty）— ViewModel 不再维护第二份 contentDirty。
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
 * #843：增量写作统计上报 — 直接把 cause + insertedChars/deletedChars 交给统计 Repository，
 * Core 的 record_editor_change_stats 内部做 cause → EventSource 映射。
 * Android 不再自己拼 source/session_id/device_id/duration。
 */
fun EditorViewModel.recordWritingEventIncremental(event: EditorAppliedEvent) {
    val session = currentSession ?: return
    statsRepository.recordEditorChangeStats(
        session.projectId,
        session.volumeId,
        session.chapterId,
        event.cause,
        event.contentDelta.insertedChars,
        event.contentDelta.deletedChars,
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

/** 冷路径字数计算（load/external-apply 时用整章 String）。
 * #624 评论13 第4项：suspend — main-safe 责任在 Repository（经注入的 IO dispatcher），
 * 调用方（loadChapter/applyExternalContentToUi）直接 await，不再套 launch(IO)。 */
suspend fun EditorViewModel.calculateWordCount(text: String): Int {
    return chapterRepository.calculateWordCount(text)
}
