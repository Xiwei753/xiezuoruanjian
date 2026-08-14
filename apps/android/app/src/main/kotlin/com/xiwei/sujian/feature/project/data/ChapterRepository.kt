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
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * ChapterRepository — 章节内容仓库层。
 *
 * 从 [com.xiwei.sujian.feature.project.data.ProjectRepository] 拆出，
 * 专门负责章节正文的读取、保存、清空、备注更新与字数计算。
 * 实现 [ChapterContentSavePort] — EditorViewModel 的保存流程只依赖此端口。
 *
 * #624 评论12 第3项：Repository 自己负责 main-safe — 保存/清空经注入的
 * [ioDispatcher] 派发（默认 [Dispatchers.IO]），调用方（Compose/Main 协程）
 * 不再各自猜线程；`suspend` 关键字不会自动换线程。
 */
class ChapterRepository(
    private val context: Context,
    private val appBridge: AppServiceBridge,
    private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
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
    ): BridgeResult<ChapterSaveReceipt> =
        withContext(ioDispatcher) {
            writingBridge.saveChapterContent(projectId, volumeId, chapterId, content)
        }

    /**
     * #624 评论12 第3项：清空同样经注入的 IO dispatcher — Repository 自己保证
     * main-safe，调用层不要各自再猜线程。
     */
    suspend fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterSaveReceipt> =
        withContext(ioDispatcher) {
            writingBridge.clearChapterContent(projectId, volumeId, chapterId)
        }

    /**
     * #624 评论13 第4项：字数统计同样必须 main-safe — 正文加载后的
     * calculateWordCount 是整章文本同步跨 UniFFI 的调用，不得跑在调用方
     * （Compose/Main）线程。与 save/clear 一样统一经注入的 IO dispatcher。
     */
    suspend fun calculateWordCount(text: String): Int =
        withContext(ioDispatcher) {
            writingBridge.calculateWordCount(text)
        }
}
