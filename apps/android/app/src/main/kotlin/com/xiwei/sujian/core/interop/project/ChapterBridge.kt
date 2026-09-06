package com.xiwei.sujian.core.interop.project
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.storage.mirror.MirrorChangeSink
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ChapterOpenResult
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt

/**
 * 章节 领域 Bridge。
 *
 * 从 AppServiceBridge 拆出，负责章节相关操作。
 *
 * #649 评论 5560685734：成功后通知 [MirrorChangeSink] 触发镜像发布，
 * 不能在保存热路径同步写 Download。
 */
class ChapterBridge internal constructor(
    private val holder: WriterAppServiceHolder,
    private val mirrorChangeSink: MirrorChangeSink? = null,
) {
    companion object {
        private const val TAG = "ChapterBridge"
    }

    fun listChapters(
        projectId: String,
        volumeId: String,
    ): BridgeResult<List<ChapterMeta>> =
        holder.wrapResult {
            holder.service.listChapters(projectId, volumeId).map { it.toModel() }
        }

    fun createChapter(
        projectId: String,
        volumeId: String,
        title: String,
    ): BridgeResult<ChapterMeta> =
        holder.wrapResult {
            holder.service.createChapter(projectId, volumeId, title).toModel()
        }.also { result ->
            if (result is BridgeResult.Success) {
                mirrorChangeSink?.projectStructureChanged(projectId)
            }
        }

    fun renameChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
        newTitle: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.renameChapter(projectId, volumeId, chapterId, newTitle)
        }.also { result ->
            if (result is BridgeResult.Success && result.data) {
                mirrorChangeSink?.chapterChanged(projectId, volumeId, chapterId)
            }
        }

    fun deleteChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.deleteChapter(projectId, volumeId, chapterId)
        }.also { result ->
            if (result is BridgeResult.Success && result.data) {
                mirrorChangeSink?.projectStructureChanged(projectId)
            }
        }

    fun reorderChapters(
        projectId: String,
        volumeId: String,
        orderedIds: List<String>,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.reorderChapters(projectId, volumeId, orderedIds)
        }.also { result ->
            if (result is BridgeResult.Success && result.data) {
                mirrorChangeSink?.projectStructureChanged(projectId)
            }
        }

    fun openChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterOpenResult> =
        holder.wrapResult {
            holder.service.openChapter(projectId, volumeId, chapterId).toModel()
        }

    fun saveChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
    ): BridgeResult<ChapterSaveReceipt> =
        holder.wrapResult {
            holder.service.saveChapterContent(projectId, volumeId, chapterId, content).toModel()
        }.also { result ->
            if (result is BridgeResult.Success) {
                mirrorChangeSink?.chapterChanged(projectId, volumeId, chapterId)
            }
        }

    fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterSaveReceipt> =
        holder.wrapResult {
            holder.service.clearChapterContent(projectId, volumeId, chapterId).toModel()
        }.also { result ->
            if (result is BridgeResult.Success) {
                mirrorChangeSink?.chapterChanged(projectId, volumeId, chapterId)
            }
        }

    fun updateChapterNote(
        projectId: String,
        volumeId: String,
        chapterId: String,
        note: String,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.updateChapterNote(projectId, volumeId, chapterId, note)
        }

    fun calculateWordCount(text: String): Int {
        return try {
            holder.service.calculateWordCount(text).toInt()
        } catch (e: UnsatisfiedLinkError) {
            DiagnosticsLogger.e(TAG, "Native library is not loaded", e)
            text.length
        }
    }
}
