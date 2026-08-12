package com.xiwei.sujian.feature.project.data
import android.content.Context
import com.xiwei.sujian.R
import com.xiwei.sujian.core.interop.app.AppServiceBridge
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.MessageKeyMapper
import com.xiwei.sujian.core.interop.common.RepositoryException
import com.xiwei.sujian.core.interop.project.WritingBridge
import com.xiwei.sujian.feature.editor.session.ChapterContentSavePort
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.sync.data.SyncFailureKind

/**
 * ChapterRepository — 章节内容仓库层。
 *
 * 从 [com.xiwei.sujian.feature.project.data.ProjectRepository] 拆出，
 * 专门负责章节正文的读取、保存、清空、备注更新与字数计算。
 * 实现 [ChapterContentSavePort] — EditorViewModel 的保存流程只依赖此端口。
 */
class ChapterRepository(
    private val context: Context,
    private val appBridge: AppServiceBridge,
) : ChapterContentSavePort {
    private val writingBridge = WritingBridge(appBridge)

    private fun BridgeResult.Error.localizedMessage(): String {
        return MessageKeyMapper.resolveMessage(context, envelope.messageKey, envelope.messageArgs, envelope.errorCode)
    }

    fun getChapterContentWithMeta(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): Pair<String, ChapterMeta> {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> Pair(result.data.content, result.data.meta)
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_chapter_content_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun updateChapterNote(
        projectId: String,
        volumeId: String,
        chapterId: String,
        note: String,
    ): Boolean {
        return when (val result = writingBridge.updateChapterNote(projectId, volumeId, chapterId, note)) {
            is BridgeResult.Success -> result.data
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_update_chapter_note_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    fun getChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): String {
        return when (val result = writingBridge.openChapter(projectId, volumeId, chapterId)) {
            is BridgeResult.Success -> result.data.content
            is BridgeResult.Error -> throw RepositoryException(
                context.getString(R.string.repo_get_chapter_content_failed, result.localizedMessage()),
            )
            BridgeResult.NotLoaded -> throw RepositoryException(
                context.getString(R.string.repo_native_not_loaded),
                SyncFailureKind.NativeUnavailable,
            )
        }
    }

    override suspend fun saveChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
    ): BridgeResult<ChapterSaveReceipt> {
        return writingBridge.saveChapterContent(projectId, volumeId, chapterId, content)
    }

    fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterSaveReceipt> {
        return writingBridge.clearChapterContent(projectId, volumeId, chapterId)
    }

    fun calculateWordCount(text: String): Int {
        return writingBridge.calculateWordCount(text)
    }
}
