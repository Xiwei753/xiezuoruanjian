package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.project.toModel
import com.xiwei.sujian.feature.project.data.model.ChapterOpenResult
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot

/**
 * MirrorSnapshotSource — 镜像发布器读取 Core 数据的唯一入口。
 *
 * #649 评论 5560971132 修复 1：[ReadableMirrorPublisher] 原先直接依赖
 * [com.xiwei.sujian.core.interop.project.ProjectBridge] 与
 * [com.xiwei.sujian.core.interop.app.AppServiceBridge]，而 `AppServiceBridge`
 * 又通过 `MirrorChangeSink` 反向依赖 Publisher（循环依赖）。
 *
 * 抽出此接口后：
 * - Publisher 只依赖 [MirrorSnapshotSource]（只读快照），不再持有 Bridge。
 * - 生产实现 [CoreMirrorSnapshotSource] 直接调 `WriterAppServiceHolder.service`，
 *   不经过 `AppServiceBridge`，切断循环。
 * - 测试可用 fake 实现注入确定性数据。
 *
 * ## 架构约束
 * - 位于 `:app` 的 `storage/mirror` 包，依赖 `core/interop` 边界（合法）。
 * - 不持有 Compose/UI 状态，不写正文，只读 Core 快照。
 */
interface MirrorSnapshotSource {
    /** 列出全部作品。 */
    fun listProjects(): BridgeResult<List<Project>>

    /** 获取作品工作区快照（卷 + 章节 + 统计）。 */
    fun getProjectWorkspaceSnapshot(projectId: String): BridgeResult<ProjectWorkspaceSnapshot>

    /** 打开章节正文。 */
    fun openChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterOpenResult>
}

/**
 * 生产实现：直接通过 [WriterAppServiceHolder] 调 Core，不经过 AppServiceBridge。
 *
 * 复用 `core.interop.project` 包内的 `toModel()` 扩展（同 `:app` 模块 internal 可见）。
 */
class CoreMirrorSnapshotSource(
    private val holder: WriterAppServiceHolder,
) : MirrorSnapshotSource {
    override fun listProjects(): BridgeResult<List<Project>> =
        holder.wrapResult {
            holder.service.listProjects().map { it.toModel() }
        }

    override fun getProjectWorkspaceSnapshot(projectId: String): BridgeResult<ProjectWorkspaceSnapshot> =
        holder.wrapResult {
            holder.service.getProjectWorkspaceSnapshot(projectId).toModel()
        }

    override fun openChapter(
        projectId: String,
        volumeId: String,
        chapterId: String,
    ): BridgeResult<ChapterOpenResult> =
        holder.wrapResult {
            holder.service.openChapter(projectId, volumeId, chapterId).toModel()
        }
}
