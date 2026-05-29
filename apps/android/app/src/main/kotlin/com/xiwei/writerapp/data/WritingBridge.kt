package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ChapterOpenResult
import com.xiwei.writerapp.model.ChapterSaveReceipt

class WritingBridge internal constructor(private val nativeBridge: NativeCoreBridge) {
    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterOpenResult> =
        nativeBridge.openChapter(projectId, volumeId, chapterId).toBridgeResult()
    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<ChapterSaveReceipt> =
        nativeBridge.saveChapterContentReceipt(projectId, volumeId, chapterId, content).toBridgeResult()
    fun clearChapterContent(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterSaveReceipt> =
        nativeBridge.clearChapterContentReceipt(projectId, volumeId, chapterId).toBridgeResult()
    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> =
        nativeBridge.updateChapterNote(projectId, volumeId, chapterId, note).toBridgeResult()
    fun recordWritingEvent(deviceId: String, projectId: String, volumeId: String, chapterId: String, source: String, insertedChars: Int, deletedChars: Int, pastedChars: Int, aiInsertedChars: Int, sessionId: String): Boolean =
        nativeBridge.recordWritingEvent(deviceId, projectId, volumeId, chapterId, source, insertedChars, deletedChars, pastedChars, aiInsertedChars, sessionId)
    fun calculateWordCount(text: String): Int = nativeBridge.calculateWordCount(text)
    fun processWritingEvent(deviceId: String, platform: String, projectId: String, volumeId: String, chapterId: String, oldText: String, newText: String, sessionId: String): Boolean =
        nativeBridge.processWritingEvent(deviceId, platform, projectId, volumeId, chapterId, oldText, newText, sessionId)
}
