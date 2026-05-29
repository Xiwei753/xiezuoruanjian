package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ChapterContent
import com.xiwei.writerapp.model.ChapterSaveReceipt

class WritingBridge(private val appService: AppServiceBridge) {
    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterContent> {
        return appService.openChapter(projectId, volumeId, chapterId)
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<ChapterSaveReceipt> {
        return appService.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterSaveReceipt> {
        return appService.clearChapterContent(projectId, volumeId, chapterId)
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> {
        return appService.updateChapterNote(projectId, volumeId, chapterId, note)
    }

    fun calculateWordCount(text: String): Int {
        return appService.calculateWordCount(text)
    }

    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, sessionId: String): Boolean {
        return when (val result = appService.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId)) {
            is BridgeResult.Success -> result.data
            else -> false
        }
    }

    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, sessionId: String): Boolean {
        return when (val result = appService.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)) {
            is BridgeResult.Success -> result.data
            else -> false
        }
    }
}
