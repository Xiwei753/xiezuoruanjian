// #644 评论 5467821839 第7节：project/volume/chapter DTO mapper 独立文件，从 BridgeMappers.kt 抽出。
package com.xiwei.sujian.core.interop.common
import com.xiwei.sujian.feature.project.data.model.ChapterMeta
import com.xiwei.sujian.feature.project.data.model.ChapterOpenResult
import com.xiwei.sujian.feature.project.data.model.ChapterSaveReceipt
import com.xiwei.sujian.feature.project.data.model.Project
import com.xiwei.sujian.feature.project.data.model.ProjectStats
import com.xiwei.sujian.feature.project.data.model.ProjectSummary
import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.RecentEdit
import com.xiwei.sujian.feature.project.data.model.Volume
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import uniffi.writer_core.ChapterContentDto
import uniffi.writer_core.ChapterMetaDto
import uniffi.writer_core.ChapterSaveReceiptDto
import uniffi.writer_core.ProjectDto
import uniffi.writer_core.ProjectStatsDto
import uniffi.writer_core.ProjectSummaryDto
import uniffi.writer_core.ProjectWorkspaceSnapshotDto
import uniffi.writer_core.RecentEditDto
import uniffi.writer_core.VolumeDto
import uniffi.writer_core.VolumeWithChaptersDto

internal fun ProjectDto.toModel() = Project(id, title, createdAt, updatedAt)

/**
 * #625 第二段：[uniffi.writer_core.ProjectSummaryDto] → [ProjectSummary]。
 * 字段映射与 [ProjectDto.toModel] + [ProjectStatsDto.toModel] 同语义，
 * 但合并为单一数据类，避免作品卡片字数显示时跨 FFI 二次查询。
 */
internal fun ProjectSummaryDto.toModel() =
    ProjectSummary(
        id = id,
        title = title,
        createdAt = createdAt,
        updatedAt = updatedAt,
        totalWordCount = totalWordCount.toInt(),
        volumeCount = volumeCount.toInt(),
        chapterCount = chapterCount.toInt(),
    )

internal fun RecentEditDto.toModel() = RecentEdit(projectId, volumeId, chapterId, timestamp)

internal fun ProjectStatsDto.toModel() =
    ProjectStats(
        totalWordCount = totalWordCount.toInt(),
        volumeCount = volumeCount.toInt(),
        chapterCount = chapterCount.toInt(),
    )

internal fun VolumeDto.toModel() =
    Volume(
        id = id,
        title = title,
        createdAt = createdAt,
        updatedAt = updatedAt,
        order = order,
    )

internal fun ChapterMetaDto.toModel() =
    ChapterMeta(
        id = id,
        title = title,
        createdAt = createdAt,
        updatedAt = updatedAt,
        order = order,
        wordCount = wordCount.toInt(),
        hash = hash,
        note = note,
    )

internal fun VolumeWithChaptersDto.toModel() =
    VolumeWithChapters(
        volume = volume.toModel(),
        chapters = chapters.map { it.toModel() },
    )

internal fun ProjectWorkspaceSnapshotDto.toModel() =
    ProjectWorkspaceSnapshot(
        project = project.toModel(),
        stats = stats.toModel(),
        volumes = volumes.map { it.toModel() },
    )

internal fun ChapterContentDto.toModel() = ChapterOpenResult(meta.toModel(), content)

internal fun ChapterSaveReceiptDto.toModel() =
    ChapterSaveReceipt(
        chapterRelativePath = chapterRelativePath,
        contentLen = contentLen.toLong(),
        contentHash = contentHash,
        metaHash = metaHash,
        updatedAt = updatedAt,
        wordCount = wordCount.toInt(),
    )
