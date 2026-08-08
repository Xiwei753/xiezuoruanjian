package com.xiwei.sujian.feature.stats.data.model

data class WritingStatsSummary(
    val range: WritingStatsRange? = null,
    val totalWordCount: Long = 0,
    val totalTimeSeconds: Long = 0,
    val activeDays: Int = 0,
    val totalHumanTypedChars: Long? = null,
    val totalActiveSeconds: Long? = null,
    val totalSessions: Int? = null,
    val daysCount: Int? = null,
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
