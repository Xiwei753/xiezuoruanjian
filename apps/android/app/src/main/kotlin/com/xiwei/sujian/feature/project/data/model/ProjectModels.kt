package com.xiwei.sujian.feature.project.data.model

data class Project(
    val id: String,
    val title: String,
    val createdAt: String,
    val updatedAt: String,
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
