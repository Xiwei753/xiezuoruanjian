package com.xiwei.sujian.core.interop.project
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt

/**
 * 章节正文保存端口 — EditorViewModel 的保存流程只依赖此端口，
 * 不直接耦合 [ProjectRepository]；测试可用可控假实现驱动完整保存流程
 * （#597：保存期间继续输入 → 晚到回执不得覆盖新输入）。
 */
interface ChapterContentSavePort {
    suspend fun saveChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
    ): BridgeResult<ChapterSaveReceipt>
}
