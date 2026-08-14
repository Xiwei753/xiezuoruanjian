package com.xiwei.sujian.feature.editor.ui

// ! # 编辑器内容编辑操作（从 EditorViewModel 拆分）
// !
// ! #624 评论9：热路径不传整章 String — onEditorApplied 接轻量 EditorAppliedEvent，
// ! 保存调度/统计/字数全部增量处理。完整正文只在冷路径（save/snapshot）经 lease.text 取。

import com.xiwei.sujian.feature.editor.platform.EditorEditSource
import com.xiwei.sujian.feature.editor.session.EditorAppliedEvent
import com.xiwei.sujian.feature.editor.session.EditorOperationKind
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * #624 评论9：轻量编辑应用入口 — 替代旧 onContentChanged(newContent: String)。
 *
 * - 不再每键存 content 到 _uiState（content 只在冷路径 load/external-apply 设置）；
 * - contentDirty = true（if event.contentChanged）；
 * - contentExplicitlyCleared 在 save 路径从 lease.text 判定（不在此猜测）；
 * - scheduleAutoSave() 无正文参数（save 时从 lease.text 取）；
 * - 统计：增量 recordWritingEvent + 即时 wordCount += netNonWhitespace + 延迟 speed 刷新。
 */
fun EditorViewModel.onEditorApplied(event: EditorAppliedEvent) {
    val currentState = _uiState.value
    if (currentState.loading) return
    if (isLoadingChapter) return
    if (inputFrozen) return

    // #624 评论9：不再每键存 content — 只更新 saveStatus。
    _uiState.value = currentState.copy(saveStatus = SaveStatus.Unsaved)

    if (event.contentChanged) {
        contentDirty = true
        // #624 评论9：contentExplicitlyCleared 不在此判定（需要正文）—
        // 改由 save 路径（EditorSaveOps）从 lease.text 判定。
    }

    scheduleAutoSave()

    if (event.contentChanged) {
        // #624 评论9：即时增量维护 wordCount — 不再每键全文 calculateWordCount。
        _uiState.value =
            _uiState.value.copy(
                wordCount = (_uiState.value.wordCount + event.contentDelta.netNonWhitespace).coerceAtLeast(0),
            )
        recordWritingEventIncremental(event)
        scheduleStatsRefresh()
    }
}

/**
 * #624 评论9：增量写作统计上报 — 不传前后整章 String，只用 contentDelta 增量。
 * source 字符串从 event.operationKind/source 映射（typing/paste/undo/redo/programmatic）。
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

    // #624 评论9：source 映射 — typing/paste/undo/redo/programmatic/selection。
    val source = writingEventSourceFrom(event)
    // paste 检测：PROGRAMMATIC + REPLACE 且 insertedChars > 1 近似为 paste；
    // 待 Core 在 EditResult 携带 cause 后精确化。
    val pastedChars =
        if (event.operationKind == EditorOperationKind.REPLACE &&
            event.source == EditorEditSource.PROGRAMMATIC &&
            event.contentDelta.insertedChars > 1
        ) {
            event.contentDelta.insertedChars
        } else {
            0
        }
    val aiInsertedChars = 0

    statsRepository.recordWritingEvent(
        statsDeviceId,
        session.projectId,
        session.volumeId,
        session.chapterId,
        source,
        event.contentDelta.insertedChars,
        event.contentDelta.deletedChars,
        pastedChars,
        aiInsertedChars,
        durationSeconds,
        statsSessionId,
    )
}

// #624 评论9：统计 source 字符串常量 — 避免 StringLiteralDuplication。
private const val STATS_SOURCE_TYPING = "typing"
private const val STATS_SOURCE_SELECTION = "selection"
private const val STATS_SOURCE_UNDO = "undo"
private const val STATS_SOURCE_REDO = "redo"
private const val STATS_SOURCE_PROGRAMMATIC = "programmatic"

/** #624 评论9：把 [EditorAppliedEvent] 映射为统计 source 字符串。 */
private fun writingEventSourceFrom(event: EditorAppliedEvent): String =
    when (event.source) {
        EditorEditSource.UNDO -> STATS_SOURCE_UNDO
        EditorEditSource.REDO -> STATS_SOURCE_REDO
        EditorEditSource.PROGRAMMATIC -> STATS_SOURCE_PROGRAMMATIC
        EditorEditSource.NORMAL ->
            if (event.operationKind == EditorOperationKind.SELECTION) {
                STATS_SOURCE_SELECTION
            } else {
                STATS_SOURCE_TYPING
            }
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
