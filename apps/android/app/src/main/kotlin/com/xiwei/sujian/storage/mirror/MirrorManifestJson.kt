package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import com.xiwei.sujian.feature.project.data.model.VolumeWithChapters
import org.json.JSONException
import org.json.JSONObject
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

// ════════════════════════════════════════════════════════════════════════════
// #649 评论 5576949398 问题 1：严格 manifest 解析 codec
// ════════════════════════════════════════════════════════════════════════════

/**
 * 严格解析 manifest JSON 字符串为 [MirrorManifest]。
 *
 * #649 评论 5576949398 问题 1：旧 [ReadableMirrorPublisher.parseMirrorManifestFromJson] 和
 * [ReadableMirrorRestorer.parseManifest] 各自维护一套宽松解析，把字段缺失/类型错误
 * 静默补成空字符串或默认值，导致损坏的 manifest 被当成合法对象继续进入事务，
 * 把"状态损坏"误判为"首次发布"。
 *
 * 本函数用严格 [JSONObject.getInt] / [JSONObject.getString] / [JSONObject.getJSONArray]
 * 读取所有必填字段，任一字段缺失或类型错误时抛 [JSONException]；
 * schemaVersion != 1 时抛 [IllegalArgumentException]；
 * project/volume/chapter 的 id、chapter 的 contentFile/contentHash 用 [JSONObject.getString]
 * 读取并校验非空（空 id/contentFile/contentHash 视为损坏）。
 *
 * #649 评论 5577831998 问题 3：还必须校验 ID 唯一性：
 * - 两个 project 不能使用同一个 projectId
 * - 同一 project 下两个 volume 不能使用同一个 volumeId
 * - 同一 volume 下两个 chapter 不能使用同一个 chapterId
 * 重复 ID 会互相覆盖 Restorer 的正文预读缓存 key，导致部分恢复；
 * 唯一性必须在任何 Core 写入之前由共享 strict codec 拦掉。
 *
 * @param json manifest JSON 字符串
 * @return 解析后的 [MirrorManifest]
 * @throws JSONException 字段缺失或类型错误
 * @throws IllegalArgumentException schemaVersion 不支持、字段值为空或 ID 重复
 */
internal fun mirrorManifestFromJsonStrict(json: String): MirrorManifest {
    val root = JSONObject(json)
    // schemaVersion 必须存在且为 int，不再 optInt(_, 1) 把缺失字段补成 1
    val schemaVersion = root.getInt(SCHEMA_VERSION_KEY)
    if (schemaVersion != 1) {
        throw IllegalArgumentException("Unsupported manifest schemaVersion: $schemaVersion (expected 1)")
    }
    // revision / updatedAt 严格读取（revision 用 getLong，updatedAt 用 getString）
    val revision = root.getLong(REVISION_KEY)
    val updatedAt = root.getString(UPDATED_AT_KEY)
    // projects 必须是数组
    val projectsArray = root.getJSONArray(PROJECTS_KEY)
    val projects = mutableListOf<MirrorProject>()
    val projectIds = mutableSetOf<String>()
    for (i in 0 until projectsArray.length()) {
        val project = parseProjectStrict(projectsArray.getJSONObject(i))
        require(projectIds.add(project.id)) {
            "Duplicate project id: ${project.id}"
        }
        projects.add(project)
    }
    return MirrorManifest(
        schemaVersion = schemaVersion,
        revision = revision,
        updatedAt = updatedAt,
        projects = projects,
    )
}

/** 严格解析单个 project。 */
private fun parseProjectStrict(obj: JSONObject): MirrorProject {
    val id = obj.getString(ID_KEY)
    require(id.isEmpty().not()) {
        "Project id is empty"
    }
    requireUuid(id, "project")
    val title = obj.getString(TITLE_KEY)
    val order = obj.getInt(ORDER_KEY)
    val revision = obj.getLong(REVISION_KEY)
    val updatedAt = obj.getString(UPDATED_AT_KEY)
    val volumesArray = obj.getJSONArray(VOLUMES_KEY)
    val volumes = mutableListOf<MirrorVolume>()
    val volumeIds = mutableSetOf<String>()
    for (i in 0 until volumesArray.length()) {
        val volume = parseVolumeStrict(volumesArray.getJSONObject(i))
        require(volumeIds.add(volume.id)) {
            "Duplicate volume id in project $id: ${volume.id}"
        }
        volumes.add(volume)
    }
    return MirrorProject(
        id = id,
        title = title,
        order = order,
        revision = revision,
        updatedAt = updatedAt,
        volumes = volumes,
    )
}

/** 严格解析单个 volume。 */
private fun parseVolumeStrict(obj: JSONObject): MirrorVolume {
    val id = obj.getString(ID_KEY)
    require(id.isEmpty().not()) {
        "Volume id is empty"
    }
    requireUuid(id, "volume")
    val title = obj.getString(TITLE_KEY)
    val order = obj.getInt(ORDER_KEY)
    val revision = obj.getLong(REVISION_KEY)
    val updatedAt = obj.getString(UPDATED_AT_KEY)
    val chaptersArray = obj.getJSONArray(CHAPTERS_KEY)
    val chapters = mutableListOf<MirrorChapter>()
    val chapterIds = mutableSetOf<String>()
    for (i in 0 until chaptersArray.length()) {
        val chapter = parseChapterStrict(chaptersArray.getJSONObject(i))
        require(chapterIds.add(chapter.id)) {
            "Duplicate chapter id in volume $id: ${chapter.id}"
        }
        chapters.add(chapter)
    }
    return MirrorVolume(
        id = id,
        title = title,
        order = order,
        revision = revision,
        updatedAt = updatedAt,
        chapters = chapters,
    )
}

/** 严格解析单个 chapter。contentFile/contentHash 用 getString 读取并校验非空。 */
private fun parseChapterStrict(obj: JSONObject): MirrorChapter {
    val id = obj.getString(ID_KEY)
    require(id.isEmpty().not()) {
        "Chapter id is empty"
    }
    requireUuid(id, "chapter")
    val title = obj.getString(TITLE_KEY)
    val order = obj.getInt(ORDER_KEY)
    val revision = obj.getLong(REVISION_KEY)
    val updatedAt = obj.getString(UPDATED_AT_KEY)
    val contentFile = obj.getString(CONTENT_FILE_KEY)
    if (contentFile.isEmpty()) {
        throw IllegalArgumentException("Chapter contentFile is empty for chapter $id")
    }
    val contentHash = obj.getString(CONTENT_HASH_KEY)
    if (contentHash.isEmpty()) {
        throw IllegalArgumentException("Chapter contentHash is empty for chapter $id")
    }
    return MirrorChapter(
        id = id,
        title = title,
        order = order,
        revision = revision,
        updatedAt = updatedAt,
        contentFile = contentFile,
        contentHash = contentHash,
    )
}

// #649 评论 5578053805 问题 3：UUID 格式预检 — 在 manifest strict parser 阶段就拦截非 UUID ID，
// 不等恢复循环跑到第 N 个项目才由 Core 发现 ID 非法。
private fun requireUuid(id: String, field: String) {
    try {
        java.util.UUID.fromString(id)
    } catch (e: IllegalArgumentException) {
        throw IllegalArgumentException("Invalid $field UUID: $id", e)
    }
}

// 严格解析用的 JSON key 常量（与 [MirrorManifest] schema 对齐）
private const val SCHEMA_VERSION_KEY = "schemaVersion"
private const val REVISION_KEY = "revision"
private const val UPDATED_AT_KEY = "updatedAt"
private const val PROJECTS_KEY = "projects"
private const val VOLUMES_KEY = "volumes"
private const val CHAPTERS_KEY = "chapters"
private const val ID_KEY = "id"
private const val TITLE_KEY = "title"
private const val ORDER_KEY = "order"
private const val CONTENT_FILE_KEY = "contentFile"
private const val CONTENT_HASH_KEY = "contentHash"
