package com.xiwei.sujian.feature.editor.session
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.project.data.model.ChapterVersion

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
        expectedVersion: ChapterVersion? = null,
    ): BridgeResult<ChapterSaveReceipt>

    suspend fun clearChapterContent(
        projectId: String,
        volumeId: String,
        chapterId: String,
        expectedVersion: ChapterVersion? = null,
    ): BridgeResult<ChapterSaveReceipt>
}

/**
 * 可选的显式证明批次保存能力。保存流程在端口支持时携带冻结的 event IDs，
 * 不支持时仍走旧版本 CAS 入口（Core 只做正文摘要校验）。
 */
interface ChapterProofBatchSavePort {
    suspend fun saveChapterContentWithProofBatch(
        projectId: String,
        volumeId: String,
        chapterId: String,
        content: String,
        expectedVersion: ChapterVersion?,
        proofSnapshotRevision: Long,
        proofEventIds: List<String>,
    ): BridgeResult<ChapterSaveReceipt>
}
