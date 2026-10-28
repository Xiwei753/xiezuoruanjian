package com.xiwei.sujian.feature.stats.data.model

data class WritingStatsSummary(
    val range: WritingStatsRange? = null,
    val totalHumanTypedChars: Long = 0,
    val totalPastedChars: Long = 0,
    val totalDeletedChars: Long = 0,
    val totalAiInsertedChars: Long = 0,
    val totalNetDeltaChars: Long = 0,
    val totalActiveSeconds: Long = 0,
    val totalSessions: Int = 0,
    val daysCount: Int = 0,
)

typealias WritingWritingStatsSummary = WritingStatsSummary

data class WritingStatsRange(
    val startDate: String? = null,
    val endDate: String? = null,
)

data class ProjectWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val projects: List<ProjectWritingStatsItem>? = emptyList(),
)

data class ProjectWritingStatsItem(
    val projectId: String? = null,
    val projectTitle: String? = null,
    val humanTypedChars: Long? = null,
    val pastedChars: Long? = null,
    val deletedChars: Long? = null,
    val aiInsertedChars: Long? = null,
    val netDeltaChars: Long? = null,
    val activeSeconds: Long? = null,
)

data class ChapterWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val chapters: List<ChapterWritingStatsItem>? = emptyList(),
)

data class ChapterWritingStatsItem(
    val chapterId: String? = null,
    val humanTypedChars: Long? = null,
    val pastedChars: Long? = null,
    val deletedChars: Long? = null,
    val aiInsertedChars: Long? = null,
    val netDeltaChars: Long? = null,
    val activeSeconds: Long? = null,
)

data class DeviceWritingStatsSummary(
    val range: WritingStatsRange? = null,
    val devices: List<DeviceWritingStatsItem>? = emptyList(),
)

data class DeviceWritingStatsItem(
    val deviceId: String? = null,
    val platform: String? = null,
    val deviceClass: String? = null,
    val humanTypedChars: Long? = null,
    val pastedChars: Long? = null,
    val deletedChars: Long? = null,
    val aiInsertedChars: Long? = null,
    val netDeltaChars: Long? = null,
    val activeSeconds: Long? = null,
    val sessionsCount: Int? = null,
)

/** 历史操作量：某来源新增/删除/修改的字数与对象操作数。 */
data class ContributionCounts(
    val insertedChars: Long = 0,
    val deletedChars: Long = 0,
    val modifiedChars: Long = 0,
    val operations: Long = 0,
    val createdObjects: Long = 0,
    val updatedObjects: Long = 0,
    val deletedObjects: Long = 0,
    val linkedObjects: Long = 0,
    val unlinkedObjects: Long = 0,
)

/** 某一来源的贡献。`retainedChars` 是当前正文中仍保留的字符数，与历史操作量分开。 */
data class ContributionActorStats(
    val actor: String = "",
    val counts: ContributionCounts = ContributionCounts(),
    val retainedChars: Long = 0,
)

/** 贡献来源统计快照，全部数字由 Core 计算。 */
data class ContributionStats(
    val total: ContributionCounts = ContributionCounts(),
    val actors: List<ContributionActorStats> = emptyList(),
    val retainedTotalChars: Long = 0,
    val historyScope: String? = null,
)

data class WritingSpeedCurve(
    val range: WritingStatsRange? = null,
    val bucketMinutes: Int = 0,
    val buckets: List<WritingSpeedBucket>? = emptyList(),
)

data class WritingSpeedBucket(
    val startMs: Long = 0,
    val endMs: Long = 0,
    val charsTyped: Long = 0,
    val charsPerMinute: Double = 0.0,
)

data class ProjectStatsSummary(
    val projectId: String,
    val wordCount: Long,
)
