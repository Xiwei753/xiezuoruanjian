package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import org.json.JSONArray
import org.json.JSONObject

// manifest JSON 字段 key 常量（#651 评论 5592465805：消除 StringLiteralDuplication）。
private const val KEY_SCHEMA_VERSION = "schemaVersion"
private const val KEY_REVISION = "revision"
private const val KEY_UPDATED_AT = "updatedAt"
private const val KEY_PROJECTS = "projects"
private const val KEY_VOLUMES = "volumes"
private const val KEY_CHAPTERS = "chapters"
private const val KEY_TARGET_PROJECT_ID = "targetProjectId"
private const val KEY_ID = "id"
private const val KEY_TITLE = "title"
private const val KEY_ORDER = "order"
private const val KEY_CONTENT_FILE = "contentFile"
private const val KEY_CONTENT_HASH = "contentHash"

/**
 * FrozenManifestPlan — 冻结的全局 manifest 计划。
 *
 * #649 评论 5575950895 问题 5：旧 `buildFrozenManifestPlan` 对所有非目标作品的章节都写
 * `contentFile = ""`, `contentHash = ""`，`frozenPlanToManifestJson` 对非目标作品又原样
 * 输出这两个空值，生成的全局 manifest 中非目标作品章节 contentFile/contentHash 为空，
 * 与真实镜像状态不一致。而且 `if (snapshotResult !is BridgeResult.Success) continue`
 * 会在读取任意作品失败时把整个作品从下一版 manifest 静默删掉。
 *
 * 新实现：冻结计划以**上一次已提交 manifest**为基线，只替换本事务目标项目：
 * - 非目标项目：从 `committedManifest` 取原样 metadata + contentFile + contentHash，
 *   不重新从当前 Core 全量状态拼，不写空占位，不静默 continue 丢作品。
 * - 目标项目：复用 `publishProject()` 开始时那一份 `targetSnapshot` + `targetDesiredEntries`，
 *   不再次 `getProjectWorkspaceSnapshot(targetProjectId)`，避免正文 staging 完后 Core 变成 R2
 *   时得到"R2 metadata + R1 正文"的混合状态。
 * - 任何必要数据无法读取时返回 null（不开始正文 swap），而非 continue 静默删作品。
 *
 * @property schemaVersion manifest schema 版本（当前为 1）。
 * @property revision 全局 revision（毫秒时间戳）。
 * @property updatedAt ISO-8601 UTC 时间。
 * @property targetProjectId 本次事务的目标项目 ID。
 * @property projects 所有项目的冻结计划，targetProjectId 的那条在恢复时会被
 *   `promotedEntries` 的真实 URI/hash 覆盖。
 */
data class FrozenManifestPlan(
    val schemaVersion: Int,
    val revision: Long,
    val updatedAt: String,
    val targetProjectId: String,
    val projects: List<FrozenManifestProject>,
)

/**
 * 单个项目在 frozen plan 中的快照。
 *
 * @property id 项目 ID。
 * @property title 项目标题。
 * @property order 项目顺序。
 * @property revision 项目 revision（毫秒时间戳）。
 * @property updatedAt ISO-8601 UTC 时间。
 * @property volumes 卷列表。
 */
data class FrozenManifestProject(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val volumes: List<FrozenManifestVolume>,
)

/**
 * 单个卷在 frozen plan 中的快照。
 */
data class FrozenManifestVolume(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val chapters: List<FrozenManifestChapter>,
)

/**
 * 单个章节在 frozen plan 中的快照。
 *
 * 注意：对非目标项目，contentFile 和 contentHash 来自已提交 manifest 的真实值
 * （不再是空字符串占位）；对目标项目，contentFile/contentHash 在冻结时还是占位值
 * （空字符串），恢复时由 `promotedEntries` 的真实 URI/hash 替换。
 */
data class FrozenManifestChapter(
    val id: String,
    val title: String,
    val order: Int,
    val revision: Long,
    val updatedAt: String,
    val contentFile: String,
    val contentHash: String,
)

// ── 已提交 manifest → frozen plan 映射 helper（保留原样 metadata + contentFile + contentHash）──

/** 把已提交 [MirrorChapter] 原样映射成 [FrozenManifestChapter]。 */
private fun frozenChapterFromCommitted(chapter: MirrorChapter): FrozenManifestChapter =
    FrozenManifestChapter(
        id = chapter.id,
        title = chapter.title,
        order = chapter.order,
        revision = chapter.revision,
        updatedAt = chapter.updatedAt,
        contentFile = chapter.contentFile,
        contentHash = chapter.contentHash,
    )

/** 把已提交 [MirrorVolume] 原样映射成 [FrozenManifestVolume]。 */
private fun frozenVolumeFromCommitted(volume: MirrorVolume): FrozenManifestVolume =
    FrozenManifestVolume(
        id = volume.id,
        title = volume.title,
        order = volume.order,
        revision = volume.revision,
        updatedAt = volume.updatedAt,
        chapters = volume.chapters.map { frozenChapterFromCommitted(it) },
    )

/** 把已提交 [MirrorProject] 原样映射成 [FrozenManifestProject]。 */
private fun frozenProjectFromCommitted(project: MirrorProject): FrozenManifestProject =
    FrozenManifestProject(
        id = project.id,
        title = project.title,
        order = project.order,
        revision = project.revision,
        updatedAt = project.updatedAt,
        volumes = project.volumes.map { frozenVolumeFromCommitted(it) },
    )

// ── JSON 编码 helper（单字段/单实体）──

/** 编码章节公共字段 + 指定的 contentFile/contentHash 到 [JSONObject]。 */
private fun encodeChapterJson(
    chapter: FrozenManifestChapter,
    contentFile: String,
    contentHash: String,
): JSONObject = JSONObject().apply {
    put(KEY_ID, chapter.id)
    put(KEY_TITLE, chapter.title)
    put(KEY_ORDER, chapter.order)
    put(KEY_REVISION, chapter.revision)
    put(KEY_UPDATED_AT, chapter.updatedAt)
    put(KEY_CONTENT_FILE, contentFile)
    put(KEY_CONTENT_HASH, contentHash)
}

/** 编码卷到 [JSONObject]（含 chapters 数组）。 */
private fun encodeVolumeJson(
    volume: FrozenManifestVolume,
    chaptersJson: JSONArray,
): JSONObject = JSONObject().apply {
    put(KEY_ID, volume.id)
    put(KEY_TITLE, volume.title)
    put(KEY_ORDER, volume.order)
    put(KEY_REVISION, volume.revision)
    put(KEY_UPDATED_AT, volume.updatedAt)
    put(KEY_CHAPTERS, chaptersJson)
}

/** 编码项目到 [JSONObject]（含 volumes 数组）。 */
private fun encodeProjectJson(
    project: FrozenManifestProject,
    volumesJson: JSONArray,
): JSONObject = JSONObject().apply {
    put(KEY_ID, project.id)
    put(KEY_TITLE, project.title)
    put(KEY_ORDER, project.order)
    put(KEY_REVISION, project.revision)
    put(KEY_UPDATED_AT, project.updatedAt)
    put(KEY_VOLUMES, volumesJson)
}

/** 编码 manifest 根到 [JSONObject]。 */
private fun encodeManifestRoot(
    schemaVersion: Int,
    revision: Long,
    updatedAt: String,
    projectsJson: JSONArray,
): JSONObject = JSONObject().apply {
    put(KEY_SCHEMA_VERSION, schemaVersion)
    put(KEY_REVISION, revision)
    put(KEY_UPDATED_AT, updatedAt)
    put(KEY_PROJECTS, projectsJson)
}

// ── JSON 解码 helper（单实体）──

/** 从 [JSONObject] 解码章节。 */
private fun decodeChapterFromJson(obj: JSONObject): FrozenManifestChapter =
    FrozenManifestChapter(
        id = obj.getString(KEY_ID),
        title = obj.getString(KEY_TITLE),
        order = obj.getInt(KEY_ORDER),
        revision = obj.getLong(KEY_REVISION),
        updatedAt = obj.getString(KEY_UPDATED_AT),
        contentFile = obj.optString(KEY_CONTENT_FILE, ""),
        contentHash = obj.optString(KEY_CONTENT_HASH, ""),
    )

/** 从 [JSONObject] 解码卷。 */
private fun decodeVolumeFromJson(obj: JSONObject): FrozenManifestVolume {
    val chaptersArray = obj.getJSONArray(KEY_CHAPTERS)
    val chapters = (0 until chaptersArray.length()).map { i ->
        decodeChapterFromJson(chaptersArray.getJSONObject(i))
    }
    return FrozenManifestVolume(
        id = obj.getString(KEY_ID),
        title = obj.getString(KEY_TITLE),
        order = obj.getInt(KEY_ORDER),
        revision = obj.getLong(KEY_REVISION),
        updatedAt = obj.getString(KEY_UPDATED_AT),
        chapters = chapters,
    )
}

/** 从 [JSONObject] 解码项目。 */
private fun decodeProjectFromJson(obj: JSONObject): FrozenManifestProject {
    val volumesArray = obj.getJSONArray(KEY_VOLUMES)
    val volumes = (0 until volumesArray.length()).map { j ->
        decodeVolumeFromJson(volumesArray.getJSONObject(j))
    }
    return FrozenManifestProject(
        id = obj.getString(KEY_ID),
        title = obj.getString(KEY_TITLE),
        order = obj.getInt(KEY_ORDER),
        revision = obj.getLong(KEY_REVISION),
        updatedAt = obj.getString(KEY_UPDATED_AT),
        volumes = volumes,
    )
}

/**
 * 构建 frozen manifest plan。
 *
 * #649 评论 5575950895 问题 5：以已提交 manifest 为基线，只替换本事务目标项目。
 *
 * - 非目标项目来自 [committedManifest]，保留原样 metadata + contentFile + contentHash。
 * - 目标项目来自当前事务已经冻结的 [targetSnapshot] + [targetDesiredEntries]，
 *   不再次 `getProjectWorkspaceSnapshot(targetProjectId)`，避免正文 staging 完后
 *   Core 变成 R2 时得到"R2 metadata + R1 正文"的混合状态。
 * - 任何必要数据无法读取时返回 null（不开始正文 swap），而非 continue 静默删作品。
 *
 * @param committedManifest 上一次已提交的全局 manifest（null 表示首次发布，无基线）。
 * @param targetProjectId 本次事务的目标项目 ID。
 * @param targetSnapshot publishProject 开始时那一份目标项目快照（与建立正文 plan 同一份）。
 * @param targetDesiredEntries 目标项目的 desired entries（promote 后填 URI）。
 * @return frozen plan；任何步骤失败返回 null。
 */
fun buildFrozenManifestPlan(
    committedManifest: MirrorManifest?,
    targetProjectId: String,
    targetSnapshot: ProjectWorkspaceSnapshot,
    targetDesiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
): FrozenManifestPlan? {
    val now = java.time.Instant.now()
    val updatedAt = java.time.format.DateTimeFormatter.ISO_INSTANT.format(now)
    val revision = now.toEpochMilli()

    val frozenProjects = mutableListOf<FrozenManifestProject>()

    // 1. 非目标项目：从 committedManifest 取原样 metadata + contentFile + contentHash
    if (committedManifest != null) {
        for (project in committedManifest.projects) {
            if (project.id == targetProjectId) continue
            frozenProjects.add(frozenProjectFromCommitted(project))
        }
    }

    // 2. 目标项目：复用 targetSnapshot + targetDesiredEntries，不再次 getProjectWorkspaceSnapshot
    frozenProjects.add(buildTargetFrozenProject(targetProjectId, targetSnapshot, targetDesiredEntries))

    return FrozenManifestPlan(
        schemaVersion = 1,
        revision = revision,
        updatedAt = updatedAt,
        targetProjectId = targetProjectId,
        projects = frozenProjects,
    )
}

/** 用 targetSnapshot + targetDesiredEntries 构建目标项目的 [FrozenManifestProject]。 */
private fun buildTargetFrozenProject(
    targetProjectId: String,
    targetSnapshot: ProjectWorkspaceSnapshot,
    targetDesiredEntries: Map<ChapterKey, ChapterMirrorEntry>,
): FrozenManifestProject {
    val targetVolumes = targetSnapshot.volumes.map { vol ->
        FrozenManifestVolume(
            id = vol.volume.id,
            title = vol.volume.title,
            order = vol.volume.order,
            revision = vol.volume.updatedAt.toEpochMillis(),
            updatedAt = vol.volume.updatedAt,
            chapters = vol.chapters.map { ch ->
                val key = ChapterKey(targetProjectId, vol.volume.id, ch.id)
                val entry = targetDesiredEntries[key]
                FrozenManifestChapter(
                    id = ch.id,
                    title = ch.title,
                    order = ch.order,
                    revision = ch.updatedAt.toEpochMillis(),
                    updatedAt = ch.updatedAt,
                    // 目标项目：用 targetDesiredEntries 的真实 URI/hash（promote 后填）；
                    // 若 desiredEntries 缺失则用空占位，恢复时由 promotedEntries 替换。
                    contentFile = entry?.relativePath ?: "",
                    contentHash = entry?.contentHash ?: "",
                )
            },
        )
    }
    return FrozenManifestProject(
        id = targetProjectId,
        title = targetSnapshot.project.title,
        order = 0,
        revision = targetSnapshot.project.updatedAt.toEpochMillis(),
        updatedAt = targetSnapshot.project.updatedAt,
        volumes = targetVolumes,
    )
}

/**
 * 从 frozen plan 生成 MirrorManifest JSON，targetProjectId 的章节用 promotedEntries 替换。
 *
 * 输出与 [mirrorManifestToJson] 一致的 schema：`{schemaVersion, revision, updatedAt, projects: [...]}`。
 *
 * #649 评论 5575950895 问题 5：非目标项目章节原样输出冻结的真实 contentFile/contentHash
 * （来自已提交 manifest），不再是空字符串。
 *
 * @param plan 冻结的 manifest 计划。
 * @param promotedEntries 本次事务已 promote 的章节 entries（key 为 ChapterKey）。
 * @return manifest JSON 字符串；如果目标项目有章节在 promotedEntries 中缺失 URI/hash，返回 null。
 */
fun frozenPlanToManifestJson(
    plan: FrozenManifestPlan,
    promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
): String? {
    val projectsJson = JSONArray()
    for (frozenProject in plan.projects) {
        val volumesJson = encodeProjectVolumesForManifest(plan, frozenProject, promotedEntries)
            ?: return null
        projectsJson.put(encodeProjectJson(frozenProject, volumesJson))
    }
    return encodeManifestRoot(plan.schemaVersion, plan.revision, plan.updatedAt, projectsJson).toString()
}

/** 编码单个项目的所有卷为 manifest JSON 数组；目标项目章节缺失 promotedEntries 时返回 null。 */
private fun encodeProjectVolumesForManifest(
    plan: FrozenManifestPlan,
    frozenProject: FrozenManifestProject,
    promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
): JSONArray? {
    val volumesJson = JSONArray()
    for (frozenVolume in frozenProject.volumes) {
        val chaptersJson = encodeVolumeChaptersForManifest(plan, frozenProject, frozenVolume, promotedEntries)
            ?: return null
        volumesJson.put(encodeVolumeJson(frozenVolume, chaptersJson))
    }
    return volumesJson
}

/** 编码单卷所有章节为 manifest JSON 数组；目标项目章节缺失 promotedEntries 时返回 null。 */
private fun encodeVolumeChaptersForManifest(
    plan: FrozenManifestPlan,
    frozenProject: FrozenManifestProject,
    frozenVolume: FrozenManifestVolume,
    promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
): JSONArray? {
    val chaptersJson = JSONArray()
    for (frozenChapter in frozenVolume.chapters) {
        val chapterKey = ChapterKey(frozenProject.id, frozenVolume.id, frozenChapter.id)
        val chapterJson = encodeChapterForManifest(plan, frozenProject, frozenChapter, chapterKey, promotedEntries)
            ?: return null
        chaptersJson.put(chapterJson)
    }
    return chaptersJson
}

/** 编码单章节为 manifest JSON：目标项目用 promotedEntries 真实值，非目标项目用冻结值。 */
private fun encodeChapterForManifest(
    plan: FrozenManifestPlan,
    frozenProject: FrozenManifestProject,
    frozenChapter: FrozenManifestChapter,
    chapterKey: ChapterKey,
    promotedEntries: Map<ChapterKey, ChapterMirrorEntry>,
): JSONObject? =
    if (frozenProject.id == plan.targetProjectId) {
        // 目标项目：用 promotedEntries 的真实 URI/hash
        val entry = promotedEntries[chapterKey] ?: return null
        encodeChapterJson(frozenChapter, entry.relativePath, entry.contentHash)
    } else {
        // #649 评论 5575950895 问题 5：非目标项目原样输出冻结的真实 contentFile/contentHash
        encodeChapterJson(frozenChapter, frozenChapter.contentFile, frozenChapter.contentHash)
    }

/**
 * 序列化 FrozenManifestPlan 为 JSON 字符串。
 */
fun frozenManifestPlanToJson(plan: FrozenManifestPlan): String {
    val projectsArray = JSONArray()
    for (project in plan.projects) {
        val volumesArray = JSONArray()
        for (volume in project.volumes) {
            val chaptersArray = JSONArray()
            for (chapter in volume.chapters) {
                chaptersArray.put(encodeChapterJson(chapter, chapter.contentFile, chapter.contentHash))
            }
            volumesArray.put(encodeVolumeJson(volume, chaptersArray))
        }
        projectsArray.put(encodeProjectJson(project, volumesArray))
    }
    // 保持原 key 顺序：schemaVersion, revision, updatedAt, targetProjectId, projects
    return JSONObject().apply {
        put(KEY_SCHEMA_VERSION, plan.schemaVersion)
        put(KEY_REVISION, plan.revision)
        put(KEY_UPDATED_AT, plan.updatedAt)
        put(KEY_TARGET_PROJECT_ID, plan.targetProjectId)
        put(KEY_PROJECTS, projectsArray)
    }.toString()
}

/**
 * 反序列化 FrozenManifestPlan 从 JSON 字符串。
 */
fun frozenManifestPlanFromJson(json: String): FrozenManifestPlan? =
    try {
        val root = JSONObject(json)
        val projectsArray = root.getJSONArray(KEY_PROJECTS)
        val projects = (0 until projectsArray.length()).map { i ->
            decodeProjectFromJson(projectsArray.getJSONObject(i))
        }
        FrozenManifestPlan(
            schemaVersion = root.getInt(KEY_SCHEMA_VERSION),
            revision = root.getLong(KEY_REVISION),
            updatedAt = root.getString(KEY_UPDATED_AT),
            targetProjectId = root.getString(KEY_TARGET_PROJECT_ID),
            projects = projects,
        )
    } catch (_: Exception) {
        null
    }

/**
 * 为删除项目构建 frozen manifest plan。
 *
 * 以 private committed manifest 为基线，只把被删项目剔掉，其他作品全部原样保留。
 * 不需要再读其他项目 Core。
 *
 * @param committedManifest 上一次已提交的全局 manifest
 * @param deletedProjectId 被删除的项目 ID
 * @return frozen plan；失败返回 null
 */
fun buildFrozenDeleteManifestPlan(
    committedManifest: MirrorManifest,
    deletedProjectId: String,
): FrozenManifestPlan {
    val now = java.time.Instant.now()
    val updatedAt = java.time.format.DateTimeFormatter.ISO_INSTANT.format(now)
    val revision = now.toEpochMilli()

    val frozenProjects = mutableListOf<FrozenManifestProject>()

    // 非被删项目：从 committedManifest 取原样 metadata + contentFile + contentHash
    for (project in committedManifest.projects) {
        if (project.id == deletedProjectId) continue
        frozenProjects.add(frozenProjectFromCommitted(project))
    }

    return FrozenManifestPlan(
        schemaVersion = 1,
        revision = revision,
        updatedAt = updatedAt,
        targetProjectId = deletedProjectId,
        projects = frozenProjects,
    )
}
