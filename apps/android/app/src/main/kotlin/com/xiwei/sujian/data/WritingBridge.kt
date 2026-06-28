package com.xiwei.sujian.data

import com.xiwei.sujian.model.ChapterContent
import com.xiwei.sujian.model.ChapterSaveReceipt

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

    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, durationSeconds: Int, sessionId: String): BridgeResult<Boolean> {
        return appService.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, durationSeconds, sessionId)
    }

    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, durationSeconds: UInt, sessionId: String): BridgeResult<Boolean> {
        return appService.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, durationSeconds, sessionId)
    }
}
