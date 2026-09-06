package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * MirrorChangeSink — 镜像变更入口。
 *
 * #649 评论 5559759935 / 5560685734：接收业务变更通知（`chapterChanged`、
 * `projectStructureChanged`、`projectDeleted`、`everythingChanged`），
 * 把变更排队后异步发布到 Download/Sujian 镜像。
 *
 * 与 [com.xiwei.sujian.storage.recovery.RecoveryChangeSink] 区分：
 * - RecoveryChangeSink：恢复完成后刷新 UI/缓存（只读）
 * - MirrorChangeSink：业务变更后写 Download/Sujian（写镜像）
 *
 * ## 设计要点
 * - 异步：Bridge 在保存热路径不能同步写 Download；所有发布都进队列，
 *   由后台协程串行消费。
 * - 合并：短时间内多次 chapterChanged 合并成一次全量发布，
 *   避免频繁 I/O。
 * - 幂等：发布失败可重试；manifest 与正文文件都是幂等写入。
 *
 * ## 使用
 * ```kotlin
 * // 在 ProjectBridge/ChapterBridge 成功后调用：
 * mirrorChangeSink.chapterChanged(projectId, volumeId, chapterId)
 * ```
 */
interface MirrorChangeSink {
    /**
     * 单章正文变更。
     *
     * 触发发布该章正文 + 更新 manifest。
     */
    fun chapterChanged(projectId: String, volumeId: String, chapterId: String)

    /**
     * 项目结构变更（新建/重命名/删除 卷或章节、重新排序）。
     *
     * 触发发布该项目全部正文 + 完整 manifest。
     */
    fun projectStructureChanged(projectId: String)

    /**
     * 项目删除。
     *
     * 触发删除镜像中该项目目录 + 更新 manifest。
     */
    fun projectDeleted(projectId: String)

    /**
     * 全部变更（恢复完成、设置变更等）。
     *
     * 触发全量发布。
     */
    fun everythingChanged()

    /**
     * 关闭并取消待处理任务。
     */
    fun close()
}

/**
 * 默认实现：合并队列 + 异步发布。
 *
 * @param publisher 实际的发布器（注入以便测试）
 * @param mergeWindowMs 合并窗口（毫秒），默认 2000ms。窗口内的多个事件合并成一次发布。
 */
class DefaultMirrorChangeSink(
    private val publisher: ReadableMirrorPublisher,
    private val mergeWindowMs: Long = 2000L,
) : MirrorChangeSink {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mutex = Mutex()
    private var pendingTask: PublishTask? = null
    private var lastPublishTime = 0L

    override fun chapterChanged(projectId: String, volumeId: String, chapterId: String) {
        schedule(PublishTask.ChapterChanged(projectId, volumeId, chapterId))
    }

    override fun projectStructureChanged(projectId: String) {
        schedule(PublishTask.ProjectStructureChanged(projectId))
    }

    override fun projectDeleted(projectId: String) {
        schedule(PublishTask.ProjectDeleted(projectId))
    }

    override fun everythingChanged() {
        schedule(PublishTask.EverythingChanged)
    }

    override fun close() {
        scope.cancel()
    }

    /**
     * 调度发布任务。
     *
     * 合并策略：
     * - EverythingChanged 吞并所有其他事件
     * - ProjectDeleted 吞并同项目的 ProjectStructureChanged/ChapterChanged
     * - ProjectStructureChanged 吞并同项目的 ChapterChanged
     */
    private fun schedule(task: PublishTask) {
        scope.launch {
            mutex.withLock {
                val merged = merge(pendingTask, task)
                pendingTask = merged
                // 如果合并窗口还没过，等一等
                val now = System.currentTimeMillis()
                if (now - lastPublishTime < mergeWindowMs) {
                    return@launch
                }
                // 执行发布
                val taskToPublish = pendingTask
                if (taskToPublish != null) {
                    pendingTask = null
                    lastPublishTime = now
                    publish(taskToPublish)
                }
            }
        }
    }

    /**
     * 合并两个任务。
     */
    private fun merge(existing: PublishTask?, new: PublishTask): PublishTask {
        if (existing == null) return new
        // EverythingChanged 优先
        if (existing is PublishTask.EverythingChanged || new is PublishTask.EverythingChanged) {
            return PublishTask.EverythingChanged
        }
        // ProjectDeleted 优先
        if (existing is PublishTask.ProjectDeleted) return existing
        if (new is PublishTask.ProjectDeleted) return new
        // ProjectStructureChanged 吞并同项目 ChapterChanged
        if (existing is PublishTask.ProjectStructureChanged && new is PublishTask.ChapterChanged) {
            if (new.projectId == existing.projectId) return existing
            return PublishTask.Batch(listOf(existing, new))
        }
        if (new is PublishTask.ProjectStructureChanged && existing is PublishTask.ChapterChanged) {
            if (existing.projectId == new.projectId) return new
            return PublishTask.Batch(listOf(existing, new))
        }
        // 同项目多个 ChapterChanged
        if (existing is PublishTask.ChapterChanged && new is PublishTask.ChapterChanged) {
            return PublishTask.Batch(listOf(existing, new))
        }
        // 其他情况合并成 Batch
        return PublishTask.Batch(listOf(existing, new))
    }

    /**
     * 执行发布。
     */
    private suspend fun publish(task: PublishTask) {
        try {
            when (task) {
                is PublishTask.ChapterChanged ->
                    publisher.publishChapter(task.projectId, task.volumeId, task.chapterId)

                is PublishTask.ProjectStructureChanged ->
                    publisher.publishProject(task.projectId)

                is PublishTask.ProjectDeleted ->
                    publisher.deleteProject(task.projectId)

                is PublishTask.EverythingChanged ->
                    publisher.publishAll()

                is PublishTask.Batch -> {
                    // Batch 按项目分组去重
                    val projectIds = task.tasks.mapNotNullTo(mutableSetOf()) { t ->
                        when (t) {
                            is PublishTask.ChapterChanged -> t.projectId
                            is PublishTask.ProjectStructureChanged -> t.projectId
                            is PublishTask.ProjectDeleted -> t.projectId
                            else -> null
                        }
                    }
                    // 如果有 EverythingChanged，直接全量
                    if (task.tasks.any { it is PublishTask.EverythingChanged }) {
                        publisher.publishAll()
                    } else {
                        // 按项目发布
                        projectIds.forEach { projectId ->
                            publisher.publishProject(projectId)
                        }
                    }
                }
            }
        } catch (e: Exception) {
            DiagnosticsLogger.e("MirrorChangeSink", "Failed to publish: ${e.message}", e)
        }
    }

    /**
     * 发布任务。
     */
    sealed class PublishTask {
        data class ChapterChanged(
            val projectId: String,
            val volumeId: String,
            val chapterId: String,
        ) : PublishTask()

        data class ProjectStructureChanged(val projectId: String) : PublishTask()
        data class ProjectDeleted(val projectId: String) : PublishTask()
        object EverythingChanged : PublishTask()
        data class Batch(val tasks: List<PublishTask>) : PublishTask()
    }
}
