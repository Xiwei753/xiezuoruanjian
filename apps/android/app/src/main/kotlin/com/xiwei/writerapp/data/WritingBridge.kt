package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.ChapterContent

class WritingBridge(private val appService: AppServiceBridge) {
    fun openChapter(projectId: String, volumeId: String, chapterId: String): BridgeResult<ChapterContent> {
        return appService.openChapter(projectId, volumeId, chapterId)
    }

    fun saveChapterContent(projectId: String, volumeId: String, chapterId: String, content: String): BridgeResult<Boolean> {
        return appService.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun updateChapterNote(projectId: String, volumeId: String, chapterId: String, note: String): BridgeResult<Boolean> {
        return appService.updateChapterNote(projectId, volumeId, chapterId, note)
    }

    fun calculateWordCount(text: String): Long {
        return appService.calculateWordCount(text)
    }
}