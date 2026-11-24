package com.xiwei.sujian.core.interop.project
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterContent
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.project.data.model.ChapterVersion

class WritingBridge(private val appService: AppServiceBridge) {
    fun openChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterContent> {
        return appService.openChapter(projectId, volumeId, chapterId)
    }

    fun saveChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
        expectedVersion: ChapterVersion? = null,
    ): BridgeResult<ChapterSaveReceipt> {
        return appService.saveChapterContent(projectId, volumeId, chapterId, content, expectedVersion)
    }

    fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        expectedVersion: ChapterVersion? = null,
    ): BridgeResult<ChapterSaveReceipt> {
        return appService.clearChapterContent(projectId, volumeId, chapterId, expectedVersion)
    }

    fun updateChapterNote(
        projectId: String,
        volumeId: String,
        chapterId: String,
        note: String,
    ): BridgeResult<Boolean> {
        return appService.updateChapterNote(projectId, volumeId, chapterId, note)
    }

    fun calculateWordCount(text: String): Int {
        return appService.calculateWordCount(text)
    }
}
