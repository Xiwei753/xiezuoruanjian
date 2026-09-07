package com.xiwei.sujian.storage.mirror

import com.xiwei.sujian.feature.project.data.model.ProjectWorkspaceSnapshot
import org.json.JSONArray
import org.json.JSONObject

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
            frozenProjects.add(
                FrozenManifestProject(
                    id = project.id,
                    title = project.title,
                    order = project.order,
                    revision = project.revision,
                    updatedAt = project.updatedAt,
                    volumes = project.volumes.map { vol ->
                        FrozenManifestVolume(
                            id = vol.id,
                            title = vol.title,
                            order = vol.order,
                            revision = vol.revision,
                            updatedAt = vol.updatedAt,
                            chapters = vol.chapters.map { ch ->
                                FrozenManifestChapter(
                                    id = ch.id,
                                    title = ch.title,
                                    order = ch.order,
                                    revision = ch.revision,
                                    updatedAt = ch.updatedAt,
                                    // #649 评论 5575950895 问题 5：保留已提交 manifest 的真实值，
                                    // 不再写空字符串占位。
                                    contentFile = ch.contentFile,
                                    contentHash = ch.contentHash,
                                )
                            },
                        )
                    },
                ),
            )
        }
    }

    // 2. 目标项目：复用 targetSnapshot + targetDesiredEntries，不再次 getProjectWorkspaceSnapshot
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
    frozenProjects.add(
        FrozenManifestProject(
            id = targetProjectId,
            title = targetSnapshot.project.title,
            order = 0,
            revision = targetSnapshot.project.updatedAt.toEpochMillis(),
            updatedAt = targetSnapshot.project.updatedAt,
            volumes = targetVolumes,
        ),
    )

    return FrozenManifestPlan(
        schemaVersion = 1,
        revision = revision,
        updatedAt = updatedAt,
        targetProjectId = targetProjectId,
        projects = frozenProjects,
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
        val volumesJson = JSONArray()

        for (frozenVolume in frozenProject.volumes) {
            val chaptersJson = JSONArray()

            for (frozenChapter in frozenVolume.chapters) {
                val chapterKey = ChapterKey(frozenProject.id, frozenVolume.id, frozenChapter.id)

                if (frozenProject.id == plan.targetProjectId) {
                    // 目标项目：用 promotedEntries 的真实 URI/hash
                    val entry = promotedEntries[chapterKey]
                    if (entry == null) {
                        // 章节在 frozen plan 中存在但没有 promotedEntries → 不完整，停止
                        return null
                    }
                    chaptersJson.put(JSONObject().apply {
                        put("id", frozenChapter.id)
                        put("title", frozenChapter.title)
                        put("order", frozenChapter.order)
                        put("revision", frozenChapter.revision)
                        put("updatedAt", frozenChapter.updatedAt)
                        put("contentFile", entry.relativePath)
                        put("contentHash", entry.contentHash)
                    })
                } else {
                    // #649 评论 5575950895 问题 5：非目标项目原样输出冻结的真实 contentFile/contentHash
                    // （来自已提交 manifest），不再输出空字符串占位。
                    chaptersJson.put(JSONObject().apply {
                        put("id", frozenChapter.id)
                        put("title", frozenChapter.title)
                        put("order", frozenChapter.order)
                        put("revision", frozenChapter.revision)
                        put("updatedAt", frozenChapter.updatedAt)
                        put("contentFile", frozenChapter.contentFile)
                        put("contentHash", frozenChapter.contentHash)
                    })
                }
            }

            volumesJson.put(JSONObject().apply {
                put("id", frozenVolume.id)
                put("title", frozenVolume.title)
                put("order", frozenVolume.order)
                put("revision", frozenVolume.revision)
                put("updatedAt", frozenVolume.updatedAt)
                put("chapters", chaptersJson)
            })
        }

        projectsJson.put(JSONObject().apply {
            put("id", frozenProject.id)
            put("title", frozenProject.title)
            put("order", frozenProject.order)
            put("revision", frozenProject.revision)
            put("updatedAt", frozenProject.updatedAt)
            put("volumes", volumesJson)
        })
    }

    return JSONObject().apply {
        put("schemaVersion", plan.schemaVersion)
        put("revision", plan.revision)
        put("updatedAt", plan.updatedAt)
        put("projects", projectsJson)
    }.toString()
}

/**
 * 序列化 FrozenManifestPlan 为 JSON 字符串。
 */
fun frozenManifestPlanToJson(plan: FrozenManifestPlan): String {
    val root = JSONObject()
    root.put("schemaVersion", plan.schemaVersion)
    root.put("revision", plan.revision)
    root.put("updatedAt", plan.updatedAt)
    root.put("targetProjectId", plan.targetProjectId)

    val projectsArray = JSONArray()
    for (project in plan.projects) {
        val volumesArray = JSONArray()
        for (volume in project.volumes) {
            val chaptersArray = JSONArray()
            for (chapter in volume.chapters) {
                chaptersArray.put(JSONObject().apply {
                    put("id", chapter.id)
                    put("title", chapter.title)
                    put("order", chapter.order)
                    put("revision", chapter.revision)
                    put("updatedAt", chapter.updatedAt)
                    put("contentFile", chapter.contentFile)
                    put("contentHash", chapter.contentHash)
                })
            }
            volumesArray.put(JSONObject().apply {
                put("id", volume.id)
                put("title", volume.title)
                put("order", volume.order)
                put("revision", volume.revision)
                put("updatedAt", volume.updatedAt)
                put("chapters", chaptersArray)
            })
        }
        projectsArray.put(JSONObject().apply {
            put("id", project.id)
            put("title", project.title)
            put("order", project.order)
            put("revision", project.revision)
            put("updatedAt", project.updatedAt)
            put("volumes", volumesArray)
        })
    }
    root.put("projects", projectsArray)
    return root.toString()
}

/**
 * 反序列化 FrozenManifestPlan 从 JSON 字符串。
 */
fun frozenManifestPlanFromJson(json: String): FrozenManifestPlan? {
    return try {
        val root = JSONObject(json)
        val schemaVersion = root.getInt("schemaVersion")
        val revision = root.getLong("revision")
        val updatedAt = root.getString("updatedAt")
        val targetProjectId = root.getString("targetProjectId")

        val projectsArray = root.getJSONArray("projects")
        val projects = mutableListOf<FrozenManifestProject>()
        for (i in 0 until projectsArray.length()) {
            val projectObj = projectsArray.getJSONObject(i)
            val volumesArray = projectObj.getJSONArray("volumes")
            val volumes = mutableListOf<FrozenManifestVolume>()
            for (j in 0 until volumesArray.length()) {
                val volumeObj = volumesArray.getJSONObject(j)
                val chaptersArray = volumeObj.getJSONArray("chapters")
                val chapters = mutableListOf<FrozenManifestChapter>()
                for (k in 0 until chaptersArray.length()) {
                    val chapterObj = chaptersArray.getJSONObject(k)
                    chapters.add(FrozenManifestChapter(
                        id = chapterObj.getString("id"),
                        title = chapterObj.getString("title"),
                        order = chapterObj.getInt("order"),
                        revision = chapterObj.getLong("revision"),
                        updatedAt = chapterObj.getString("updatedAt"),
                        contentFile = chapterObj.optString("contentFile", ""),
                        contentHash = chapterObj.optString("contentHash", ""),
                    ))
                }
                volumes.add(FrozenManifestVolume(
                    id = volumeObj.getString("id"),
                    title = volumeObj.getString("title"),
                    order = volumeObj.getInt("order"),
                    revision = volumeObj.getLong("revision"),
                    updatedAt = volumeObj.getString("updatedAt"),
                    chapters = chapters,
                ))
            }
            projects.add(FrozenManifestProject(
                id = projectObj.getString("id"),
                title = projectObj.getString("title"),
                order = projectObj.getInt("order"),
                revision = projectObj.getLong("revision"),
                updatedAt = projectObj.getString("updatedAt"),
                volumes = volumes,
            ))
        }

        FrozenManifestPlan(
            schemaVersion = schemaVersion,
            revision = revision,
            updatedAt = updatedAt,
            targetProjectId = targetProjectId,
            projects = projects,
        )
    } catch (_: Exception) {
        null
    }
}
