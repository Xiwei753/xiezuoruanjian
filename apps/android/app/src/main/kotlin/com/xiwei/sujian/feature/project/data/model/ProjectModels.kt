package com.xiwei.sujian.feature.project.data.model

data class Project(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
)

/**
 * #625 第二段：作品摘要 — 在 [Project] 4 字段基础上携带字数/卷数/章节数。
 *
 * 由 Rust `list_project_summaries` 一次性返回，避免端侧逐卡跨 FFI 调用
 * `get_project_stats`。字段语义与 [ProjectStats] 一致，但与 [Project] 同生命周期，
 * 用于作品卡片字数显示。
 */
data class ProjectSummary(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
    val totalWordCount: Int,
    val volumeCount: Int,
    val chapterCount: Int,
)

data class ProjectStats(
    val totalWordCount: Int,
    val volumeCount: Int,
    val chapterCount: Int,
)

data class Volume(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
    val order: Int = 0,
)

data class ChapterMeta(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
    val order: Int = 0,
    val wordCount: Int,
    val hash: String,
    val note: String? = null,
)

data class ChapterOpenResult(
    val meta: ChapterMeta,
    val content: String,
)

typealias ChapterContent = ChapterOpenResult

data class ChapterSaveReceipt(
    val chapterRelativePath: String,
    val contentLen: Long,
    val contentHash: String,
    val metaHash: String,
    val updatedAt: String,
    val wordCount: Int,
)

data class RecentEdit(
    val projectId: String,
    val volumeId: String,
    val chapterId: String,
    val timestamp: String,
)

/**
 * #644 评论 5467821839 第7节：卷及其章节 — 一次跨 FFI 返回的卷 + 章节列表。
 */
data class VolumeWithChapters(
    val volume: Volume,
    val chapters: List<ChapterMeta>,
)

/**
 * #644 评论 5467821839 第7节：作品工作区快照 — 一次跨 FFI 返回全部卷 + 章节 + 统计。
 *
 * Android `ProjectViewModel` 不再逐卷调 `listChapters`，而是一次拿到完整快照，
 * 减少 FFI 调用次数和中间状态不一致窗口。
 */
data class ProjectWorkspaceSnapshot(
    val project: Project,
    val stats: ProjectStats,
    val volumes: List<VolumeWithChapters>,
)
