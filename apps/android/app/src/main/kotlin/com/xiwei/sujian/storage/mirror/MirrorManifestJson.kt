package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import java.time.Instant

/** 把 [MirrorManifest] 序列化为 JSON 字符串。 */
internal fun mirrorManifestToJson(manifest: MirrorManifest): String {
    val sb = StringBuilder()
    sb.appendLine("{")
    sb.appendLine("  \"schemaVersion\": ${manifest.schemaVersion},")
    sb.appendLine("  \"revision\": ${manifest.revision},")
    sb.appendLine("  \"updatedAt\": \"${manifest.updatedAt}\",")
    sb.appendLine("  \"projects\": [")
    for ((i, project) in manifest.projects.withIndex()) {
        sb.append(projectToJson(project, "    "))
        if (i < manifest.projects.lastIndex) sb.appendLine(",") else sb.appendLine()
    }
    sb.appendLine("  ]")
    sb.append("}")
    return sb.toString()
}

/**
 * 把 ProjectWorkspaceSnapshot 转成 MirrorProject。
 *
 * contentFile/contentHash 从 entries 取（overrides 优先，由调用方传入）。
 */
internal fun ProjectWorkspaceSnapshot.toMirrorProject(entries: Map<ChapterKey, ChapterMirrorEntry>): MirrorProject {
    return MirrorProject(
        id = this.project.id,
        title = this.project.title,
        order = 0,
        revision = this.project.updatedAt.toEpochMillis(),
        updatedAt = this.project.updatedAt,
        volumes = this.volumes.map { it.toMirrorVolume(this.project.id, entries) },
    )
}

internal fun VolumeWithChapters.toMirrorVolume(
    projectId: String,
    entries: Map<ChapterKey, ChapterMirrorEntry>,
): MirrorVolume {
    return MirrorVolume(
        id = this.volume.id,
        title = this.volume.title,
        order = this.volume.order,
        revision = this.volume.updatedAt.toEpochMillis(),
        updatedAt = this.volume.updatedAt,
        chapters =
            this.chapters.map { chapter ->
                val key = ChapterKey(projectId, this.volume.id, chapter.id)
                val entry = entries[key]
                MirrorChapter(
                    id = chapter.id,
                    title = chapter.title,
                    order = chapter.order,
                    revision = chapter.updatedAt.toEpochMillis(),
                    updatedAt = chapter.updatedAt,
                    contentFile = entry?.relativePath ?: "",
                    contentHash = entry?.contentHash ?: "",
                )
            },
    )
}

/** 把 ISO-8601 字符串转成毫秒级时间戳。 */
internal fun String.toEpochMillis(): Long {
    return try {
        Instant.parse(this).toEpochMilli()
    } catch (e: Exception) {
        System.currentTimeMillis()
    }
}

private fun projectToJson(
    project: MirrorProject,
    indent: String,
): String {
    val sb = StringBuilder()
    sb.appendJsonOpen(indent)
    sb.appendLine("$indent  \"id\": \"${project.id}\",")
    sb.appendLine("$indent  \"title\": \"${escapeJson(project.title)}\",")
    sb.appendLine("$indent  \"order\": ${project.order},")
    sb.appendLine("$indent  \"revision\": ${project.revision},")
    sb.appendLine("$indent  \"updatedAt\": \"${project.updatedAt}\",")
    sb.appendLine("$indent  \"volumes\": [")
    for ((i, volume) in project.volumes.withIndex()) {
        sb.append(volumeToJson(volume, "$indent    "))
        if (i < project.volumes.lastIndex) sb.appendLine(",") else sb.appendLine()
    }
    sb.appendLine("$indent  ]")
    sb.appendJsonClose(indent)
    return sb.toString()
}

private fun volumeToJson(
    volume: MirrorVolume,
    indent: String,
): String {
    val sb = StringBuilder()
    sb.appendJsonOpen(indent)
    sb.appendLine("$indent  \"id\": \"${volume.id}\",")
    sb.appendLine("$indent  \"title\": \"${escapeJson(volume.title)}\",")
    sb.appendLine("$indent  \"order\": ${volume.order},")
    sb.appendLine("$indent  \"revision\": ${volume.revision},")
    sb.appendLine("$indent  \"updatedAt\": \"${volume.updatedAt}\",")
    sb.appendLine("$indent  \"chapters\": [")
    for ((i, chapter) in volume.chapters.withIndex()) {
        sb.append(chapterToJson(chapter, "$indent    "))
        if (i < volume.chapters.lastIndex) sb.appendLine(",") else sb.appendLine()
    }
    sb.appendLine("$indent  ]")
    sb.appendJsonClose(indent)
    return sb.toString()
}

private fun chapterToJson(
    chapter: MirrorChapter,
    indent: String,
): String {
    val sb = StringBuilder()
    sb.appendJsonOpen(indent)
    sb.appendLine("$indent  \"id\": \"${chapter.id}\",")
    sb.appendLine("$indent  \"title\": \"${escapeJson(chapter.title)}\",")
    sb.appendLine("$indent  \"order\": ${chapter.order},")
    sb.appendLine("$indent  \"revision\": ${chapter.revision},")
    sb.appendLine("$indent  \"updatedAt\": \"${chapter.updatedAt}\",")
    sb.appendLine("$indent  \"contentFile\": \"${escapeJson(chapter.contentFile)}\",")
    sb.appendLine("$indent  \"contentHash\": \"${chapter.contentHash}\"")
    sb.appendJsonClose(indent)
    return sb.toString()
}

private fun StringBuilder.appendJsonOpen(indent: String) = appendLine("$indent{")

private fun StringBuilder.appendJsonClose(indent: String) = append("$indent}")

private fun escapeJson(s: String): String =
    s.replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
