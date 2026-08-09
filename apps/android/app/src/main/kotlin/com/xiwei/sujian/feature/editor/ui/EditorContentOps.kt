package com.xiwei.sujian.feature.editor.ui

// ! # 编辑器内容编辑操作（从 EditorViewModel 拆分）
// !
// ! 正文变更处理、写作统计上报、字数计算。

import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

fun EditorViewModel.onContentChanged(newContent: String) {
    val currentState = _uiState.value
    if (currentState.loading) return
    if (isLoadingChapter) return
    if (inputFrozen) return

    _uiState.value =
        currentState.copy(
            content = newContent,
            saveStatus = SaveStatus.Unsaved,
        )

    // #595 七：空正文不能靠字符串猜测"是否要保存" — 用户把非空正文编辑为空
    // 是类型化 ClearDocument 语义（随后 autosave/requestSave 走 Clear 落盘，
    // 经过 Core 空覆盖保护）；非空编辑则撤销清空意图。
    contentDirty = true
    if (newContent.isEmpty()) {
        if (currentState.content.isNotEmpty()) {
            contentExplicitlyCleared = true
        }
    } else {
        contentExplicitlyCleared = false
    }
    scheduleAutoSave(newContent)
    scheduleStatsUpdate(newContent)

    if (previousText != newContent) {
        reportWritingEvent(previousText, newContent)
        previousText = newContent
    }
}

fun EditorViewModel.reportWritingEvent(
    oldText: String,
    newText: String,
) {
    val session = currentSession ?: return

    val nowMs = System.currentTimeMillis()
    if (statsLastEventMs == 0L || (nowMs - statsLastEventMs) > 5 * 60 * 1000) {
        statsSessionId = java.util.UUID.randomUUID().toString()
    }
    val durationSeconds =
        if (statsLastEventMs > 0L) {
            ((nowMs - statsLastEventMs) / 1000).toUInt()
        } else {
            0u
        }
    statsLastEventMs = nowMs

    statsRepository.processWritingEvent(
        statsDeviceId, "android", session.projectId, session.volumeId, session.chapterId,
        oldText, newText, durationSeconds, statsSessionId,
    )
}

fun EditorViewModel.scheduleStatsUpdate(content: String) {
    editorScope.launch {
        delay(500)
        updateStats(content)
    }
}

fun EditorViewModel.updateStats(content: String) {
    val currentWordCount = calculateWordCount(content)
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

fun EditorViewModel.calculateWordCount(text: String): Int {
    return chapterRepository.calculateWordCount(text)
}
